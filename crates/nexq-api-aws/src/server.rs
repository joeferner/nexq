//! The facade's HTTP listener.
//!
//! This facade owns its own socket rather than being mounted into a shared server, so
//! it can be enabled, disabled, and bound independently of the others.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use nexq_core::engine::Engine;
use nexq_core::{AuthConfig, AwsApiConfig};
use rustls::ServerConfig;
use serde_json::Value;
use tls_listener::TlsListener;
use tls_listener::rustls::TlsAcceptor;
use tokio::net::TcpListener;
use tracing::{debug, info, warn};

use crate::error::ApiError;
use crate::operations::Operations;
use crate::protocol::{JSON_CONTENT_TYPE, Operation, TARGET_HEADER, decode_input};
use crate::queue_url::QueueUrls;
use crate::sigv4::{self, SigningContext};

/// What every request needs: credentials to check the signature against, and the
/// operations to run once it passes.
#[derive(Debug)]
pub struct Facade {
    auth: Arc<AuthConfig>,
    operations: Operations,

    /// How stale a signed request may be. `None` accepts any timestamp.
    max_clock_skew: Option<Duration>,
}

/// Shared rather than cloned per request: everything in it is read-only while serving.
type FacadeState = Arc<Facade>;

/// A bound, not-yet-serving facade listener.
///
/// Binding is separate from serving so the caller can learn the real
/// [`Server::local_addr`] — which matters when the configured port is `0` — and so a
/// bind failure surfaces at startup rather than from inside a spawned task.
#[derive(Debug)]
pub struct Server {
    listener: TcpListener,
    local_addr: SocketAddr,
    router: Router,

    /// Built at bind time when the facade is configured for TLS, so a bad certificate
    /// stops the server coming up rather than being found by the first client to
    /// connect. `None` serves plain HTTP.
    tls: Option<Arc<ServerConfig>>,

    /// Held so [`Server::serve`] can release long polls when shutdown starts, rather
    /// than waiting for each of them to reach its own deadline.
    engine: Arc<Engine>,
}

impl Server {
    /// Bind the configured address.
    ///
    /// Whether this facade should run at all is [`AwsApiConfig::enabled`], which the
    /// caller checks; reaching here means it is meant to serve.
    ///
    /// `auth` is the registry every request is verified against — shared with the other
    /// facades, which present the same credentials differently. `engine` is the
    /// operation set this facade translates to.
    pub async fn bind(
        config: &AwsApiConfig,
        auth: Arc<AuthConfig>,
        engine: Arc<Engine>,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(config.bind_addr).await?;
        let local_addr = listener.local_addr()?;

        let tls = match &config.tls {
            Some(tls) => Some(
                nexq_core::tls::server_config(tls)
                    .map_err(|error| io::Error::other(error.to_string()))?,
            ),
            None => None,
        };

        // A client is handed queue URLs built from `public_base_url` and sends every
        // later request to them, so a scheme that disagrees with what is actually served
        // hands out URLs that cannot be used. Both mismatches are legitimate behind a
        // proxy that terminates or adds TLS, which is why this warns rather than
        // refusing.
        let base_url_is_https = config.base_url().starts_with("https://");
        if tls.is_some() != base_url_is_https {
            warn!(
                facade = "aws",
                serving = if tls.is_some() { "https" } else { "http" },
                public_base_url = config.base_url(),
                "the public base URL's scheme does not match what this facade serves, so \
                 queue URLs will name the other one. Correct unless a proxy in front is \
                 terminating or adding TLS."
            );
        }

        if config.max_clock_skew().is_none() {
            // Deliberate, but it means a captured request stays replayable forever, so
            // it should not be a silent setting.
            warn!(
                facade = "aws",
                "aws_api.max_clock_skew_secs is 0: signed requests are accepted \
                 whatever their timestamp, so a captured request can be replayed"
            );
        }

        Ok(Self {
            listener,
            local_addr,
            router: router(Arc::new(Facade {
                auth,
                operations: Operations::new(Arc::clone(&engine), QueueUrls::new(config)),
                max_clock_skew: config.max_clock_skew(),
            })),
            tls,
            engine,
        })
    }

    /// Whether this server will serve HTTPS.
    pub fn is_tls(&self) -> bool {
        self.tls.is_some()
    }

    /// The address actually bound, which differs from the configured one when the
    /// configured port was `0`.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Serve until `shutdown` resolves, then let in-flight requests finish.
    ///
    /// A consumer long-polling for twenty seconds is an in-flight request, so waiting
    /// for it would make every shutdown as slow as the longest wait outstanding. The
    /// engine is told to stop waiting the moment shutdown starts, which turns those
    /// requests into ordinary empty responses instead of a delay or a dropped
    /// connection.
    ///
    /// TLS, when configured, is a different listener and nothing else: handshakes happen
    /// off the accept path, so one client that opens a connection and then says nothing
    /// cannot stop the server accepting others, and graceful shutdown is unchanged from
    /// the plain-TCP case.
    pub async fn serve<S>(self, shutdown: S) -> io::Result<()>
    where
        S: Future<Output = ()> + Send + 'static,
    {
        info!(
            facade = "aws",
            address = %self.local_addr,
            scheme = if self.tls.is_some() { "https" } else { "http" },
            "listening"
        );

        let engine = self.engine;
        let shutdown = async move {
            shutdown.await;
            debug!(facade = "aws", "shutting down: releasing long polls");
            engine.begin_draining();
        };

        match self.tls {
            Some(tls) => {
                let listener = TlsListener::new(TlsAcceptor::from(tls), self.listener);

                axum::serve(listener, self.router)
                    .with_graceful_shutdown(shutdown)
                    .await
            }
            None => {
                axum::serve(self.listener, self.router)
                    .with_graceful_shutdown(shutdown)
                    .await
            }
        }
    }
}

/// The facade's routes.
///
/// AWS JSON clients send every operation as a `POST /` and name the operation in
/// `X-Amz-Target`, so there is one route rather than one per operation. Any method is
/// accepted so that a misdirected request is answered by the protocol layer, with an
/// error a client can read, rather than by a bare 405.
pub fn router(facade: FacadeState) -> Router {
    Router::new().route("/", any(handle)).with_state(facade)
}

async fn handle(
    State(facade): State<FacadeState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let context = SigningContext {
        method: &method,
        uri: &uri,
        headers: &headers,
        body: &body,
    };

    match route(&context, &facade).await {
        Ok(output) => json_response(&output),
        Err(error) => {
            // Not `message`: tracing renders a field by that name as the event body,
            // which swallows the key and reads as two messages run together.
            debug!(
                code = error.code(),
                detail = error.message(),
                "request rejected"
            );
            error.into_response()
        }
    }
}

/// Authenticate a request, then decode it far enough to know what it is asking for and
/// run it.
///
/// Signature first: an unauthenticated caller learns nothing about which operations
/// exist or whether its input parsed.
async fn route(context: &SigningContext<'_>, facade: &Facade) -> Result<Value, ApiError> {
    let principal = sigv4::verify(context, &facade.auth, facade.max_clock_skew)?;

    let target = context
        .headers
        .get(TARGET_HEADER)
        .ok_or_else(ApiError::missing_target)?
        .to_str()
        .map_err(|_| ApiError::unknown_operation("<non-ascii target>"))?;

    let operation = Operation::from_target(target)?;
    let input = decode_input(context.body)?;

    debug!(%operation, %principal, "dispatching");
    facade.operations.dispatch(operation, input).await
}

fn json_response(output: &Value) -> Response {
    (
        [(header::CONTENT_TYPE, JSON_CONTENT_TYPE)],
        output.to_string(),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{HeaderValue, Request as HttpRequest, StatusCode};
    use http_body_util::BodyExt;
    use nexq_core::{Credential, Secret};
    use nexq_store_memory::MemoryStore;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tower::ServiceExt;

    use super::*;
    use crate::sigv4::{ALGORITHM, Authorization, CredentialScope};

    /// How long a test waits on the server before calling it hung.
    ///
    /// Generous on purpose. Nothing here measures how *fast* anything is — these deadlines
    /// exist to turn a hang into a failure, which a hang reaches whatever the bound. A tight
    /// one instead fails when the machine is busy running the rest of the suite in parallel,
    /// which is a failure about the machine rather than about the code.
    ///
    /// Raised from five seconds after two of these tests failed together in a full
    /// `make pre-commit` and then passed in twelve consecutive runs. That was not proven to
    /// be the cause — the failure's own message was lost — but a five-second wall-clock
    /// assertion nothing needs is the plausible candidate, and widening it costs no
    /// assertion strength.
    const HUNG: Duration = Duration::from_secs(30);

    const KEY_ID: &str = "AKIATESTKEY";
    const SECRET: &str = "test-secret";
    const AMZ_DATE: &str = "20260826T005924Z";
    const SCOPE_DATE: &str = "20260826";
    const REGION: &str = "us-east-1";

    /// Bind to port 0 so tests never collide with a real deployment or each other.
    fn test_config() -> AwsApiConfig {
        AwsApiConfig {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            ..AwsApiConfig::default()
        }
    }

    fn credential(secret: &str) -> Credential {
        Credential {
            name: "dev".to_owned(),
            key_id: KEY_ID.to_owned(),
            secret: Secret::new(secret),
        }
    }

    fn test_auth() -> Arc<AuthConfig> {
        Arc::new(AuthConfig {
            credentials: vec![credential(SECRET)],
        })
    }

    /// An engine over an in-memory backend, so these tests exercise the whole path
    /// rather than stopping at the protocol layer.
    fn test_engine() -> Arc<Engine> {
        let store: Arc<dyn nexq_core::store::Store> = Arc::new(MemoryStore::new());

        Arc::new(Engine::new(store))
    }

    fn facade_with(auth: Arc<AuthConfig>) -> FacadeState {
        // These cases sign with a fixed timestamp, so freshness is switched off; the
        // window itself is covered in `sigv4`, plus one end-to-end case below.
        facade_with_skew(auth, None)
    }

    fn facade_with_skew(auth: Arc<AuthConfig>, max_clock_skew: Option<Duration>) -> FacadeState {
        Arc::new(Facade {
            auth,
            operations: Operations::new(test_engine(), crate::test_support::test_queue_urls()),
            max_clock_skew,
        })
    }

    fn facade() -> FacadeState {
        facade_with(test_auth())
    }

    /// Sign a set of headers the way botocore does, returning them with an
    /// `Authorization` header added.
    fn sign(headers: HeaderMap, body: &[u8], signing_secret: &str) -> HeaderMap {
        let mut headers = headers;
        let mut names: Vec<String> = headers
            .keys()
            .map(|name| name.as_str().to_owned())
            .collect();
        names.sort();

        let authorization = Authorization {
            key_id: KEY_ID.to_owned(),
            scope: CredentialScope {
                date: SCOPE_DATE.to_owned(),
                region: REGION.to_owned(),
                service: crate::sigv4::SERVICE.to_owned(),
            },
            signed_headers: names.clone(),
            signature: String::new(),
        };

        let uri: Uri = "/".parse().expect("uri");
        let context = SigningContext {
            method: &Method::POST,
            uri: &uri,
            headers: &headers,
            body,
        };
        let signature = crate::sigv4::sign(
            &context,
            &authorization,
            AMZ_DATE,
            &credential(signing_secret),
        )
        .expect("sign");

        let value = format!(
            "{ALGORITHM} Credential={KEY_ID}/{SCOPE_DATE}/{REGION}/{}/aws4_request, \
             SignedHeaders={}, Signature={signature}",
            crate::sigv4::SERVICE,
            names.join(";")
        );
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(&value).expect("authorization value"),
        );

        headers
    }

    /// The headers `aws-cli` sends, minus the signature.
    fn unsigned_headers(target: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("nexq.test"));
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(JSON_CONTENT_TYPE),
        );
        headers.insert("x-amz-date", HeaderValue::from_static(AMZ_DATE));
        if let Some(target) = target {
            headers.insert(
                TARGET_HEADER,
                HeaderValue::from_str(target).expect("target"),
            );
        }
        headers
    }

    fn request_with(headers: HeaderMap, body: &str) -> HttpRequest<Body> {
        let mut request = HttpRequest::builder().method("POST").uri("/");
        for (name, value) in &headers {
            request = request.header(name, value);
        }
        request.body(Body::from(body.to_owned())).expect("request")
    }

    async fn send_request(request: HttpRequest<Body>) -> (StatusCode, Value) {
        let response = router(facade()).oneshot(request).await.expect("response");

        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();

        (status, serde_json::from_slice(&bytes).expect("json body"))
    }

    /// Generate a test certificate authority and a `nexq.test` certificate it signed.
    ///
    /// A chain rather than one self-signed certificate, because rustls refuses to use a
    /// certificate marked as a CA as an end entity — which is what `openssl req -x509`
    /// produces, and what a first attempt at this used. It is also the realistic shape:
    /// a client trusts the authority, and the server presents a leaf it signed.
    ///
    /// Generated per run rather than committed, so nothing starts failing on the day a
    /// checked-in certificate would have expired.
    ///
    /// The files a TLS test needs, all signed by the one authority.
    struct TestChain {
        authority: std::path::PathBuf,
        certificate: std::path::PathBuf,
        private_key: std::path::PathBuf,
        client_certificate: std::path::PathBuf,
        client_key: std::path::PathBuf,
    }

    fn test_chain(name: &str) -> TestChain {
        use std::path::PathBuf;
        use std::process::Command;

        let directory = std::env::temp_dir().join(format!("nexq-server-tls-{name}"));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create temp dir");

        let openssl = |arguments: Vec<&str>| {
            let output = Command::new("openssl")
                .args(&arguments)
                .output()
                .expect("openssl should be installed");
            assert!(
                output.status.success(),
                "openssl {}: {}",
                arguments.join(" "),
                String::from_utf8_lossy(&output.stderr)
            );
        };

        let path = |file: &str| -> PathBuf { directory.join(file) };
        let text = |file: &str| -> String { path(file).display().to_string() };

        // The authority a client will trust.
        openssl(vec![
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "1",
            "-subj",
            "/CN=NexQ Test CA",
            "-keyout",
            &text("ca.key"),
            "-out",
            &text("ca.pem"),
        ]);

        // A leaf for `nexq.test`, with the name in a SAN because that is where a modern
        // client looks rather than at the common name.
        openssl(vec![
            "req",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-subj",
            "/CN=nexq.test",
            "-keyout",
            &text("server.key"),
            "-out",
            &text("server.csr"),
        ]);

        std::fs::write(
            path("server.ext"),
            "subjectAltName=DNS:nexq.test\nbasicConstraints=critical,CA:FALSE\n",
        )
        .expect("write extensions");

        openssl(vec![
            "x509",
            "-req",
            "-days",
            "1",
            "-in",
            &text("server.csr"),
            "-CA",
            &text("ca.pem"),
            "-CAkey",
            &text("ca.key"),
            "-extfile",
            &text("server.ext"),
            "-out",
            &text("server.pem"),
        ]);

        // A client certificate from the same authority, for the mutual-TLS test. The
        // `aws` CLI cannot present one, so proving that gate works needs a Rust client.
        openssl(vec![
            "req",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-subj",
            "/CN=a-client",
            "-keyout",
            &text("client.key"),
            "-out",
            &text("client.csr"),
        ]);
        std::fs::write(path("client.ext"), "basicConstraints=critical,CA:FALSE\n")
            .expect("write extensions");
        openssl(vec![
            "x509",
            "-req",
            "-days",
            "1",
            "-in",
            &text("client.csr"),
            "-CA",
            &text("ca.pem"),
            "-CAkey",
            &text("ca.key"),
            "-extfile",
            &text("client.ext"),
            "-out",
            &text("client.pem"),
        ]);

        TestChain {
            authority: path("ca.pem"),
            certificate: path("server.pem"),
            private_key: path("server.key"),
            client_certificate: path("client.pem"),
            client_key: path("client.key"),
        }
    }

    /// Read a PEM file's certificates.
    fn pem_certificates(path: &std::path::Path) -> Vec<rustls::pki_types::CertificateDer<'static>> {
        rustls_pemfile::certs(&mut std::io::BufReader::new(
            std::fs::File::open(path).expect("open"),
        ))
        .map(|certificate| certificate.expect("certificate"))
        .collect()
    }

    /// Read a PEM file's single private key.
    fn private_key_of(path: &std::path::Path) -> rustls::pki_types::PrivateKeyDer<'static> {
        rustls_pemfile::private_key(&mut std::io::BufReader::new(
            std::fs::File::open(path).expect("open"),
        ))
        .expect("read")
        .expect("a private key")
    }

    /// A root store trusting one authority.
    fn roots_trusting(authority: &std::path::Path) -> rustls::RootCertStore {
        let mut roots = rustls::RootCertStore::empty();
        for certificate in pem_certificates(authority) {
            roots.add(certificate).expect("add");
        }

        roots
    }

    /// A correctly signed request.
    async fn send_signed(target: Option<&str>, body: &str) -> (StatusCode, Value) {
        let headers = sign(unsigned_headers(target), body.as_bytes(), SECRET);
        send_request(request_with(headers, body)).await
    }

    #[tokio::test]
    async fn list_queues_succeeds_and_reports_no_queues() {
        let (status, body) = send_signed(Some("AmazonSQS.ListQueues"), "{}").await;

        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, serde_json::json!({}));
    }

    #[tokio::test]
    async fn an_unsigned_request_is_rejected() {
        let (status, body) = send_request(request_with(
            unsigned_headers(Some("AmazonSQS.ListQueues")),
            "{}",
        ))
        .await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body["__type"],
            "com.amazonaws.sqs#MissingAuthenticationToken"
        );
    }

    #[tokio::test]
    async fn a_request_signed_with_the_wrong_secret_is_rejected() {
        let headers = sign(
            unsigned_headers(Some("AmazonSQS.ListQueues")),
            b"{}",
            "not-the-secret",
        );
        let (status, body) = send_request(request_with(headers, "{}")).await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["__type"], "com.amazonaws.sqs#SignatureDoesNotMatch");
    }

    #[tokio::test]
    async fn a_body_swapped_after_signing_is_rejected() {
        // Sign one body, send another: the payload hash is part of the signature.
        let headers = sign(
            unsigned_headers(Some("AmazonSQS.ListQueues")),
            b"{}",
            SECRET,
        );
        let (status, body) =
            send_request(request_with(headers, r#"{"QueueNamePrefix":"x"}"#)).await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["__type"], "com.amazonaws.sqs#SignatureDoesNotMatch");
    }

    #[tokio::test]
    async fn an_unknown_key_id_is_rejected_without_revealing_which_part_was_wrong() {
        let headers = sign(
            unsigned_headers(Some("AmazonSQS.ListQueues")),
            b"{}",
            SECRET,
        );
        let auth = Arc::new(AuthConfig {
            credentials: vec![Credential {
                name: "someone-else".to_owned(),
                key_id: "AKIASOMEONEELSE".to_owned(),
                secret: Secret::new(SECRET),
            }],
        });

        let response = router(facade_with(auth))
            .oneshot(request_with(headers, "{}"))
            .await
            .expect("response");
        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let body: Value = serde_json::from_slice(&bytes).expect("json");

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(body["__type"], "com.amazonaws.sqs#InvalidClientTokenId");
        assert_eq!(
            body["message"], "The security token included in the request is invalid.",
            "the message must not say whether the key or the secret was the problem"
        );
    }

    #[tokio::test]
    async fn authentication_happens_before_routing() {
        // An unsigned request for a nonexistent operation reports the signature
        // problem, so an anonymous caller cannot probe which operations exist.
        let (status, body) =
            send_request(request_with(unsigned_headers(Some("AmazonSQS.Nope")), "{}")).await;

        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            body["__type"],
            "com.amazonaws.sqs#MissingAuthenticationToken"
        );
    }

    #[tokio::test]
    async fn a_stale_request_is_refused_end_to_end() {
        // The fixed test timestamp is long past by the time this runs, so a real window
        // must refuse it — this is the replay case, checked through the router.
        let headers = sign(
            unsigned_headers(Some("AmazonSQS.ListQueues")),
            b"{}",
            SECRET,
        );
        let response = router(facade_with_skew(
            test_auth(),
            Some(Duration::from_secs(900)),
        ))
        .oneshot(request_with(headers, "{}"))
        .await
        .expect("response");

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let body: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(body["__type"], "com.amazonaws.sqs#RequestTimeTooSkewed");
    }

    #[tokio::test]
    async fn a_known_operation_without_a_handler_routes_but_is_not_implemented() {
        let (status, body) = send_signed(Some("AmazonSQS.TagQueue"), "{}").await;

        // Recognised, but not built yet — which is not the same as unknown.
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["__type"], "com.amazonaws.sqs#NotImplemented");
        assert!(
            body["message"]
                .as_str()
                .expect("message")
                .contains("TagQueue"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn an_unknown_operation_is_a_client_error() {
        let (status, body) = send_signed(Some("AmazonSQS.Nope"), "{}").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body["__type"],
            "com.amazonaws.sqs#UnknownOperationException"
        );
    }

    #[tokio::test]
    async fn a_request_without_a_target_reports_the_query_protocol_gap() {
        let body = "Action=ListQueues&Version=2012-11-05";
        let (status, response) = send_signed(None, body).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(response["__type"], "com.amazonaws.sqs#MissingAction");
    }

    #[tokio::test]
    async fn a_malformed_body_is_rejected_before_the_operation_runs() {
        let (status, body) = send_signed(Some("AmazonSQS.CreateQueue"), "{not json}").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["__type"], "com.amazonaws.sqs#SerializationException");
    }

    #[tokio::test]
    async fn an_empty_body_still_routes() {
        // Parameterless operations may send nothing rather than `{}`.
        let (status, _) = send_signed(Some("AmazonSQS.ListQueues"), "").await;

        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn responses_are_aws_json() {
        let headers = sign(
            unsigned_headers(Some("AmazonSQS.ListQueues")),
            b"{}",
            SECRET,
        );
        let response = router(facade())
            .oneshot(request_with(headers, "{}"))
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .expect("content type"),
            JSON_CONTENT_TYPE
        );
    }

    #[tokio::test]
    async fn binding_port_zero_reports_the_real_port() {
        let server = Server::bind(&test_config(), test_auth(), test_engine())
            .await
            .expect("bind");

        assert_ne!(server.local_addr().port(), 0);
    }

    #[tokio::test]
    async fn without_a_certificate_the_facade_serves_plain_http() {
        let server = Server::bind(&test_config(), test_auth(), test_engine())
            .await
            .expect("bind");

        assert!(!server.is_tls(), "no [aws_api.tls] means plain HTTP");
    }

    #[tokio::test]
    async fn a_certificate_makes_the_facade_serve_https() {
        let chain = test_chain("bind");
        let config = AwsApiConfig {
            tls: Some(nexq_core::ServerTlsConfig {
                certificate: chain.certificate,
                private_key: chain.private_key,
                client_ca: None,
            }),
            ..test_config()
        };

        let server = Server::bind(&config, test_auth(), test_engine())
            .await
            .expect("bind");

        assert!(server.is_tls());
    }

    #[tokio::test]
    async fn a_bad_certificate_stops_the_server_binding() {
        // Rather than being discovered by whoever connects first, when the only symptom
        // a client sees is a handshake that failed.
        let config = AwsApiConfig {
            tls: Some(nexq_core::ServerTlsConfig {
                certificate: std::path::PathBuf::from("/nonexistent/cert.pem"),
                private_key: std::path::PathBuf::from("/nonexistent/key.pem"),
                client_ca: None,
            }),
            ..test_config()
        };

        let error = Server::bind(&config, test_auth(), test_engine())
            .await
            .expect_err("there is no such certificate");

        assert!(
            error.to_string().contains("cert.pem"),
            "the failure should name the file: {error}"
        );
    }

    #[tokio::test]
    async fn a_tls_facade_serves_a_real_handshake_over_tcp() {
        // The end-to-end shape: a genuine TLS client, a genuine handshake, and a request
        // answered inside it. Unit tests through the router cannot show any of that,
        // since they never reach a socket.
        let chain = test_chain("serve");
        let config = AwsApiConfig {
            tls: Some(nexq_core::ServerTlsConfig {
                certificate: chain.certificate,
                private_key: chain.private_key,
                client_ca: None,
            }),
            ..test_config()
        };

        let server = Server::bind(&config, test_auth(), test_engine())
            .await
            .expect("bind");
        let address = server.local_addr();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let serving = tokio::spawn(server.serve(async move {
            let _ = shutdown_rx.await;
        }));

        // A client that trusts only this authority, so a successful handshake proves
        // the server presented a certificate that authority signed.
        let client = rustls::ClientConfig::builder_with_provider(
            rustls::crypto::ring::default_provider().into(),
        )
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_root_certificates(roots_trusting(&chain.authority))
        .with_no_client_auth();

        let stream = TcpStream::connect(address).await.expect("connect");
        let mut tls = tokio_rustls::TlsConnector::from(Arc::new(client))
            .connect(
                rustls::pki_types::ServerName::try_from("nexq.test").expect("server name"),
                stream,
            )
            .await
            .expect("the handshake should succeed");

        // Unsigned on purpose: what matters here is that a request crossed the TLS
        // connection and came back answered, not which answer it got.
        tls.write_all(
            b"POST / HTTP/1.1\r\n\
              Host: nexq.test\r\n\
              X-Amz-Target: AmazonSQS.ListQueues\r\n\
              Content-Length: 0\r\n\
              Connection: close\r\n\r\n",
        )
        .await
        .expect("write request");

        let mut response = String::new();
        tls.read_to_string(&mut response)
            .await
            .expect("read response");
        assert!(
            response.starts_with("HTTP/1.1 403"),
            "an unsigned request should be refused, over TLS like anywhere else: {response}"
        );

        shutdown_tx.send(()).expect("signal shutdown");
        tokio::time::timeout(HUNG, serving)
            .await
            .expect("serve should stop")
            .expect("serve task")
            .expect("serve");
    }

    #[tokio::test]
    async fn mutual_tls_refuses_a_client_with_no_certificate_and_admits_one_with() {
        // The whole point of `client_ca`, and the only place it can be shown: the `aws`
        // CLI cannot present a client certificate, so the acceptance suite cannot cover
        // this and a Rust client has to.
        let chain = test_chain("mtls");
        let config = AwsApiConfig {
            tls: Some(nexq_core::ServerTlsConfig {
                certificate: chain.certificate,
                private_key: chain.private_key,
                client_ca: Some(chain.authority.clone()),
            }),
            ..test_config()
        };

        let server = Server::bind(&config, test_auth(), test_engine())
            .await
            .expect("bind");
        let address = server.local_addr();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let serving = tokio::spawn(server.serve(async move {
            let _ = shutdown_rx.await;
        }));

        let name = rustls::pki_types::ServerName::try_from("nexq.test").expect("server name");

        // No client certificate: the server should refuse during the handshake, before
        // any request is read.
        let anonymous = rustls::ClientConfig::builder_with_provider(
            rustls::crypto::ring::default_provider().into(),
        )
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_root_certificates(roots_trusting(&chain.authority))
        .with_no_client_auth();

        let stream = TcpStream::connect(address).await.expect("connect");
        let refused = tokio_rustls::TlsConnector::from(Arc::new(anonymous))
            .connect(name.clone(), stream)
            .await;

        // The refusal can surface on the handshake or on the first read, depending on
        // how far TLS 1.3 has got before the server objects — so a write and a read
        // follow, and what matters is that no response comes back.
        // Under a timeout, and asking the server to close, because both are needed for
        // this to *fail* rather than hang if the gate ever stops being enforced: a served
        // keep-alive connection would leave the read waiting for a close that never
        // comes. Found by breaking the gate on purpose and watching the test hang.
        let served = match refused {
            Err(_) => false,
            Ok(mut tls) => {
                let exchange = async {
                    tls.write_all(
                        b"POST / HTTP/1.1\r\n\
                          Host: nexq.test\r\n\
                          Content-Length: 0\r\n\
                          Connection: close\r\n\r\n",
                    )
                    .await
                    .ok()?;

                    let mut response = String::new();
                    tls.read_to_string(&mut response).await.ok()?;

                    Some(response)
                };

                tokio::time::timeout(HUNG, exchange)
                    .await
                    .ok()
                    .flatten()
                    .is_some_and(|response| response.starts_with("HTTP/"))
            }
        };
        assert!(
            !served,
            "a client with no certificate must not be served when client_ca is set"
        );

        // With a certificate that authority signed, the same connection works.
        let certified = rustls::ClientConfig::builder_with_provider(
            rustls::crypto::ring::default_provider().into(),
        )
        .with_safe_default_protocol_versions()
        .expect("versions")
        .with_root_certificates(roots_trusting(&chain.authority))
        .with_client_auth_cert(
            pem_certificates(&chain.client_certificate),
            private_key_of(&chain.client_key),
        )
        .expect("a client certificate and key");

        let stream = TcpStream::connect(address).await.expect("connect");
        let mut tls = tokio_rustls::TlsConnector::from(Arc::new(certified))
            .connect(name, stream)
            .await
            .expect("a certified client should be admitted");

        tls.write_all(
            b"POST / HTTP/1.1\r\n\
              Host: nexq.test\r\n\
              X-Amz-Target: AmazonSQS.ListQueues\r\n\
              Content-Length: 0\r\n\
              Connection: close\r\n\r\n",
        )
        .await
        .expect("write request");

        let mut response = String::new();
        tls.read_to_string(&mut response)
            .await
            .expect("read response");
        assert!(
            response.starts_with("HTTP/1.1 403"),
            "the request is unsigned, so SigV4 still refuses it — the certificate is a \
             gate, not an identity: {response}"
        );

        shutdown_tx.send(()).expect("signal shutdown");
        tokio::time::timeout(HUNG, serving)
            .await
            .expect("serve should stop")
            .expect("serve task")
            .expect("serve");
    }

    #[tokio::test]
    async fn a_plain_http_client_against_a_tls_facade_is_refused_not_served() {
        // The mistake everyone makes once. It must fail as a failed connection rather
        // than by the server accidentally answering HTTP on a TLS port.
        let chain = test_chain("plain-to-tls");
        let config = AwsApiConfig {
            tls: Some(nexq_core::ServerTlsConfig {
                certificate: chain.certificate,
                private_key: chain.private_key,
                client_ca: None,
            }),
            ..test_config()
        };

        let server = Server::bind(&config, test_auth(), test_engine())
            .await
            .expect("bind");
        let address = server.local_addr();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let serving = tokio::spawn(server.serve(async move {
            let _ = shutdown_rx.await;
        }));

        let mut stream = TcpStream::connect(address).await.expect("connect");
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: nexq.test\r\n\r\n")
            .await
            .expect("write");

        let mut response = String::new();
        // Either nothing comes back or the connection is reset; what must not happen is
        // an HTTP response.
        let _ = stream.read_to_string(&mut response).await;
        assert!(
            !response.starts_with("HTTP/"),
            "a TLS listener must not answer plain HTTP: {response:?}"
        );

        shutdown_tx.send(()).expect("signal shutdown");
        tokio::time::timeout(HUNG, serving)
            .await
            .expect("serve should stop")
            .expect("serve task")
            .expect("serve");
    }

    #[tokio::test]
    async fn shutting_down_releases_long_polls_instead_of_waiting_for_them() {
        // A consumer parked on a twenty-second wait is an in-flight request, so a
        // server that simply waited for in-flight requests would take twenty seconds to
        // stop. The link between the shutdown signal and the engine is what avoids that.
        let engine = test_engine();
        let server = Server::bind(&test_config(), test_auth(), Arc::clone(&engine))
            .await
            .expect("bind");
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let serving = tokio::spawn(server.serve(async move {
            let _ = shutdown_rx.await;
        }));
        assert!(!engine.is_draining(), "not while it is serving");

        shutdown_tx.send(()).expect("signal shutdown");
        tokio::time::timeout(HUNG, serving)
            .await
            .expect("serve should stop promptly")
            .expect("serve task")
            .expect("serve");

        assert!(
            engine.is_draining(),
            "shutdown must tell the engine to stop waiting"
        );
    }

    #[tokio::test]
    async fn serves_over_tcp_then_shuts_down_gracefully() {
        let server = Server::bind(&test_config(), test_auth(), test_engine())
            .await
            .expect("bind");
        let address = server.local_addr();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let serving = tokio::spawn(server.serve(async move {
            let _ = shutdown_rx.await;
        }));

        let mut stream = TcpStream::connect(address).await.expect("connect");
        stream
            .write_all(
                b"POST / HTTP/1.1\r\n\
                  Host: nexq.test\r\n\
                  X-Amz-Target: AmazonSQS.ListQueues\r\n\
                  Content-Length: 0\r\n\
                  Connection: close\r\n\r\n",
            )
            .await
            .expect("write request");

        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .await
            .expect("read response");
        // Unsigned over the wire, so this exercises the reject path end to end.
        assert!(response.starts_with("HTTP/1.1 403"), "{response}");

        shutdown_tx.send(()).expect("signal shutdown");
        tokio::time::timeout(HUNG, serving)
            .await
            .expect("serve should stop once shutdown is signalled")
            .expect("serve task")
            .expect("serve");
    }
}
