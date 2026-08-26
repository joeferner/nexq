//! The facade's HTTP listener.
//!
//! This facade owns its own socket rather than being mounted into a shared server, so
//! it can be enabled, disabled, and bound independently of the others.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use nexq_core::engine::Engine;
use nexq_core::{AuthConfig, AwsApiConfig};
use serde_json::Value;
use tokio::net::TcpListener;
use tracing::{debug, info};

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

        Ok(Self {
            listener,
            local_addr,
            router: router(Arc::new(Facade {
                auth,
                operations: Operations::new(engine, QueueUrls::new(config)),
            })),
        })
    }

    /// The address actually bound, which differs from the configured one when the
    /// configured port was `0`.
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Serve until `shutdown` resolves, then let in-flight requests finish.
    pub async fn serve<S>(self, shutdown: S) -> io::Result<()>
    where
        S: Future<Output = ()> + Send + 'static,
    {
        info!(facade = "aws", address = %self.local_addr, "listening");

        axum::serve(self.listener, self.router)
            .with_graceful_shutdown(shutdown)
            .await
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
    let principal = sigv4::verify(context, &facade.auth)?;

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
    use std::time::Duration;

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
        Arc::new(Facade {
            auth,
            operations: Operations::new(test_engine(), crate::test_support::test_queue_urls()),
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
    async fn a_known_operation_without_a_handler_routes_but_is_not_implemented() {
        let (status, body) = send_signed(Some("AmazonSQS.SendMessage"), "{}").await;

        // Recognised, but not built yet — which is not the same as unknown.
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["__type"], "com.amazonaws.sqs#NotImplemented");
        assert!(
            body["message"]
                .as_str()
                .expect("message")
                .contains("SendMessage"),
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
        tokio::time::timeout(Duration::from_secs(5), serving)
            .await
            .expect("serve should stop once shutdown is signalled")
            .expect("serve task")
            .expect("serve");
    }
}
