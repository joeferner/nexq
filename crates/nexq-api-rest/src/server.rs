//! The facade's HTTP listener.
//!
//! Deliberately the same shape as `nexq_api_aws::server`: bind separately from serve, own
//! the socket, build TLS at bind time, and release long polls when shutdown starts. Two
//! listeners that behave differently under shutdown or a bad certificate would be two
//! things to reason about instead of one.

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use aide::axum::ApiRouter;
use aide::axum::routing::post_with;
use aide::openapi::{OpenApi, SecurityScheme};
use aide::transform::TransformOpenApi;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::HeaderMap;
use axum::http::header;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router};
use nexq_core::engine::Engine;
use nexq_core::{AuthConfig, RestApiConfig};
use rustls::ServerConfig;
use tls_listener::TlsListener;
use tls_listener::rustls::TlsAcceptor;
use tokio::net::TcpListener;
use tracing::{debug, info};

use crate::auth;
use crate::error::ApiError;
use crate::messages;

/// What every request needs: credentials to check the token against, and the engine to
/// run the operation on.
#[derive(Debug)]
pub struct Facade {
    pub(crate) auth: Arc<AuthConfig>,
    pub(crate) engine: Arc<Engine>,
}

/// Shared rather than cloned per request: everything in it is read-only while serving.
pub type FacadeState = Arc<Facade>;

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
    /// Whether this facade should run at all is [`RestApiConfig::enabled`], which the
    /// caller checks; reaching here means it is meant to serve.
    ///
    /// `auth` is the registry every request is checked against and `engine` is the
    /// operation set — both shared with the other facades, which is what makes a message
    /// sent through one receivable through the other.
    pub async fn bind(
        config: &RestApiConfig,
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

        Ok(Self {
            listener,
            local_addr,
            router: router(Arc::new(Facade {
                auth,
                engine: Arc::clone(&engine),
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
    /// A consumer long-polling for twenty seconds is an in-flight request, so waiting for
    /// it would make every shutdown as slow as the longest wait outstanding. The engine is
    /// told to stop waiting the moment shutdown starts, which turns those requests into
    /// ordinary empty responses instead of a delay or a dropped connection.
    ///
    /// Both facades share the engine, so either one entering its drain releases the
    /// other's waiters too. That is correct — a drain means the process is going away —
    /// and it is why this is safe to call from two servers at once.
    pub async fn serve<S>(self, shutdown: S) -> io::Result<()>
    where
        S: Future<Output = ()> + Send + 'static,
    {
        info!(
            facade = "rest",
            address = %self.local_addr,
            scheme = if self.tls.is_some() { "https" } else { "http" },
            "listening"
        );

        let engine = self.engine;
        let shutdown = async move {
            shutdown.await;
            debug!(facade = "rest", "shutting down: releasing long polls");
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

/// Every route lives under this prefix.
///
/// Versioned from the start because a generated client's base path should not have to
/// change the first time the API does. Under `/api` as well as `/v1` so that the API and
/// the web UI can be served from one origin without the two competing for paths — the SPA
/// will want `/` and its own asset routes, and a queue named `assets` should not be able
/// to collide with them.
pub const API_PREFIX: &str = "/api/v1";

/// Where the generated OpenAPI document is served.
///
/// Spelled out rather than built from [`API_PREFIX`] so it stays a `&'static str`; a test
/// asserts the two agree, which is cheaper than the formatting it saves.
pub const OPENAPI_PATH: &str = "/api/v1/openapi.json";

/// The name the spec gives this facade's security scheme, referenced by every operation.
const SECURITY_SCHEME: &str = "bearerAuth";

/// The documented routes, before any state is attached.
///
/// An [`ApiRouter`] rather than an [`axum::Router`], which is the whole of what `aide`
/// asks for: `api_route` records the operation while registering the handler, so the spec
/// is generated from *this* — the real routing table — and cannot describe an endpoint that
/// does not exist or miss one that does. Per Q18a, and the reason `utoipa` was not chosen:
/// its per-handler attribute would have this list restated by hand next to each function.
///
/// State-free so that [`openapi`] can generate the spec without an engine or a listener.
fn api_routes() -> ApiRouter<FacadeState> {
    ApiRouter::new().nest(
        // Nested rather than spelled into each path, so [`API_PREFIX`] is written once,
        // a route cannot be added outside it by accident, and the spec's paths carry it.
        API_PREFIX,
        ApiRouter::new().api_route(
            "/queues/{queue}/messages/receive",
            post_with(messages::receive, messages::receive_docs),
        ),
    )
}

/// Describe the API as a whole — everything not derivable from an individual route.
fn api_metadata(api: TransformOpenApi) -> TransformOpenApi {
    api.title("NexQ")
        .version(env!("CARGO_PKG_VERSION"))
        .description(
            "NexQ's native API: the complete operation set, including the extensions the \
             SQS-compatible facade cannot express. This document is generated from the \
             server's own routing table and is the source every client is generated from.",
        )
        .security_scheme(
            SECURITY_SCHEME,
            SecurityScheme::Http {
                scheme: "bearer".into(),
                bearer_format: None,
                description: Some(
                    "The token is `<key_id>.<secret>` for a credential in the server's \
                     registry. Presented in full on every request, so it should travel \
                     over TLS."
                        .into(),
                ),
                extensions: Default::default(),
            },
        )
        // Applied to every operation rather than named on each one, which matches the
        // server: authentication is a layer over all of them, not a per-route choice.
        .security_requirement(SECURITY_SCHEME)
}

/// The OpenAPI document as JSON — what is served, and what a committed copy would hold.
///
/// Pretty-printed so that a diff against the committed copy is readable line by line
/// rather than being one very long line that changed somewhere.
pub fn openapi_json() -> String {
    serde_json::to_string_pretty(&openapi())
        // An OpenAPI document is plain data with string keys throughout, so this can only
        // fail if `aide` produced something that is not representable as JSON — a bug
        // there, not a condition an operator can be in.
        .expect("an OpenAPI document always serializes")
}

/// The OpenAPI document describing this facade.
///
/// Generated from the same `api_routes` the server serves, so it cannot drift from what is
/// served — that routing table is the only definition either of them comes from.
pub fn openapi() -> OpenApi {
    // aide accumulates extracted schemas in a thread-local, so a second generation on the
    // same thread builds on the first one's leftovers. Defensive rather than proven: with
    // one route there is one set of types, so re-extracting produces the same components
    // and removing this changes nothing today — breaking it on purpose left every test
    // green. It is here for when that stops being true, since the failure it would cause —
    // a document carrying schemas from an unrelated generation — is one the committed-spec
    // check would report as a mystery diff.
    aide::generate::reset_context();

    let mut api = OpenApi::default();
    let _ = api_routes().finish_api_with(&mut api, api_metadata);

    api
}

/// The facade's routes.
///
/// One operation so far — see [`crate::messages`] for why.
pub fn router(facade: FacadeState) -> Router {
    aide::generate::reset_context();

    let mut api = OpenApi::default();
    let routes = api_routes().finish_api_with(&mut api, api_metadata);
    let spec = Bytes::from(
        serde_json::to_vec_pretty(&api).expect("an OpenAPI document always serializes"),
    );

    routes
        // Applied to the API routes only, so it runs *after* routing: a request to a
        // path that does not exist is a 404 without its credentials being looked at.
        // Deliberate — the paths are published in the OpenAPI spec, so refusing to admit
        // which ones exist would protect nothing — but it does mean a 404 is not proof
        // that a token was accepted.
        .layer(middleware::from_fn_with_state(
            Arc::clone(&facade),
            require_token,
        ))
        .with_state(facade)
        // Added after the auth layer, so the spec is readable without a token. It
        // describes the shape of the API and carries nothing deployment-specific — no
        // queue names, no data — and a client generator has to be able to fetch it.
        .route(OPENAPI_PATH, get(serve_openapi))
        .layer(Extension(spec))
        // Answers in this facade's own envelope rather than axum's empty body, so a
        // client parsing errors does not have to special-case a wrong URL.
        .fallback(async || ApiError::no_such_route())
}

/// Serve the document generated when this router was built.
///
/// Pre-serialized rather than rendered per request: the spec cannot change while the
/// process runs, and [`Bytes`] is refcounted so handing it out costs nothing.
async fn serve_openapi(Extension(spec): Extension<Bytes>) -> Response {
    ([(header::CONTENT_TYPE, "application/json")], spec).into_response()
}

/// Reject anything without a valid bearer token, before any handler runs.
///
/// A layer rather than a per-handler extractor so that adding a route cannot
/// accidentally add an unauthenticated one.
async fn require_token(
    State(facade): State<FacadeState>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Response {
    match auth::authenticate(&headers, &facade.auth) {
        Ok(principal) => {
            debug!(%principal, path = %request.uri().path(), "dispatching");
            next.run(request).await
        }
        Err(error) => {
            debug!(code = error.code(), "request rejected");
            error.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode, header};
    use http_body_util::BodyExt;
    use nexq_core::engine::Engine;
    use nexq_core::model::{MessageAttributes, Priority, QueueAttributes, QueueName};
    use nexq_core::{Credential, Secret};
    use nexq_store_memory::MemoryStore;
    use tower::ServiceExt;

    use super::*;

    const KEY_ID: &str = "AKIATESTKEY";
    const SECRET: &str = "test-secret";
    const QUEUE: &str = "jobs";

    /// Bind to port 0 so tests never collide with a real deployment or each other.
    fn test_config() -> RestApiConfig {
        RestApiConfig {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            ..RestApiConfig::default()
        }
    }

    fn test_auth() -> Arc<AuthConfig> {
        Arc::new(AuthConfig {
            credentials: vec![Credential {
                name: "dev".to_owned(),
                key_id: KEY_ID.to_owned(),
                secret: Secret::new(SECRET),
            }],
        })
    }

    fn token() -> String {
        format!("{KEY_ID}.{SECRET}")
    }

    fn test_engine() -> Arc<Engine> {
        let store: Arc<dyn nexq_core::store::Store> = Arc::new(MemoryStore::new());

        Arc::new(Engine::new(store))
    }

    async fn queue_with_a_message(engine: &Engine, body: &str) -> QueueName {
        let name = QueueName::new(QUEUE).expect("valid name");
        engine
            .create_queue(name.clone(), QueueAttributes::default())
            .await
            .expect("create");
        engine
            .enqueue(
                &name,
                body.to_owned(),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");

        name
    }

    fn receive_request(queue: &str, token: Option<&str>, body: &str) -> HttpRequest<Body> {
        let mut request = HttpRequest::builder()
            .method("POST")
            .uri(format!("/api/v1/queues/{queue}/messages/receive"))
            .header(header::CONTENT_TYPE, "application/json");

        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }

        request.body(Body::from(body.to_owned())).expect("request")
    }

    async fn json_of(response: Response) -> serde_json::Value {
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();

        serde_json::from_slice(&bytes).expect("json")
    }

    #[tokio::test]
    async fn a_message_comes_back_with_its_receipt_handle() {
        let engine = test_engine();
        queue_with_a_message(&engine, "hello over rest").await;

        let response = router(Arc::new(Facade {
            auth: test_auth(),
            engine,
        }))
        .oneshot(receive_request(QUEUE, Some(&token()), "{}"))
        .await
        .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let json = json_of(response).await;
        assert_eq!(json["messages"][0]["body"], "hello over rest");
        assert_eq!(json["messages"][0]["receive_count"], 1);
        assert!(
            json["messages"][0]["receipt_handle"]
                .as_str()
                .is_some_and(|handle| !handle.is_empty()),
            "a claim must come with a handle: {json}"
        );
    }

    /// An empty queue answers with an empty list, not an absent field, and not an error.
    #[tokio::test]
    async fn an_empty_queue_answers_with_an_empty_list() {
        let engine = test_engine();
        let name = QueueName::new(QUEUE).expect("valid name");
        engine
            .create_queue(name.clone(), QueueAttributes::default())
            .await
            .expect("create");

        let response = router(Arc::new(Facade {
            auth: test_auth(),
            engine,
        }))
        .oneshot(receive_request(QUEUE, Some(&token()), "{}"))
        .await
        .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(json_of(response).await["messages"], serde_json::json!([]));
    }

    /// The whole point of the layer: no token, no route runs.
    #[tokio::test]
    async fn an_unauthenticated_request_is_refused_before_the_handler() {
        let engine = test_engine();
        queue_with_a_message(&engine, "must not be handed out").await;

        let response = router(Arc::new(Facade {
            auth: test_auth(),
            engine: Arc::clone(&engine),
        }))
        .oneshot(receive_request(QUEUE, None, "{}"))
        .await
        .expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response
                .headers()
                .get(header::WWW_AUTHENTICATE)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer")
        );

        // And the message is still there, so the refusal happened before the claim.
        let name = QueueName::new(QUEUE).expect("valid name");
        assert_eq!(
            engine.message_counts(&name).await.expect("counts").visible,
            1
        );
    }

    #[tokio::test]
    async fn a_wrong_token_is_refused() {
        let response = router(Arc::new(Facade {
            auth: test_auth(),
            engine: test_engine(),
        }))
        .oneshot(receive_request(
            QUEUE,
            Some(&format!("{KEY_ID}.wrong")),
            "{}",
        ))
        .await
        .expect("response");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_missing_queue_is_a_404_in_this_facades_envelope() {
        let response = router(Arc::new(Facade {
            auth: test_auth(),
            engine: test_engine(),
        }))
        .oneshot(receive_request("no-such-queue", Some(&token()), "{}"))
        .await
        .expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            json_of(response).await["error"]["code"],
            "queue_not_found",
            "a client must be able to branch on the code"
        );
    }

    #[tokio::test]
    async fn an_unknown_path_answers_in_the_same_envelope() {
        let request = HttpRequest::builder()
            .method("GET")
            .uri("/api/v1/nothing-here")
            .header(header::AUTHORIZATION, format!("Bearer {}", token()))
            .body(Body::empty())
            .expect("request");

        let response = router(Arc::new(Facade {
            auth: test_auth(),
            engine: test_engine(),
        }))
        .oneshot(request)
        .await
        .expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(json_of(response).await["error"]["code"], "no_such_route");
    }

    /// `bad!name` is legal in a URI path and illegal as a queue name, which is the case
    /// that reaches the handler — a name with a space is rejected as a URI long before
    /// then, by the client's own request builder.
    #[tokio::test]
    async fn a_queue_name_the_model_refuses_is_a_400() {
        let response = router(Arc::new(Facade {
            auth: test_auth(),
            engine: test_engine(),
        }))
        .oneshot(receive_request("bad!name", Some(&token()), "{}"))
        .await
        .expect("response");

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            json_of(response).await["error"]["code"],
            "invalid_queue_name"
        );
    }

    /// Readable without a token, and by the same server that serves the routes it
    /// describes — a spec fetched from somewhere else could describe something else.
    #[tokio::test]
    async fn the_spec_is_served_unauthenticated_from_the_running_router() {
        let request = HttpRequest::builder()
            .method("GET")
            .uri(OPENAPI_PATH)
            .body(Body::empty())
            .expect("request");

        let response = router(Arc::new(Facade {
            auth: test_auth(),
            engine: test_engine(),
        }))
        .oneshot(request)
        .await
        .expect("response");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "a client generator has to be able to fetch this without credentials"
        );
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );

        let served = json_of(response).await;
        assert_eq!(
            served,
            serde_json::from_str::<serde_json::Value>(&openapi_json()).expect("json"),
            "the served document must be the generated one"
        );
    }

    #[tokio::test]
    async fn binding_reports_the_port_it_actually_got() {
        let server = Server::bind(&test_config(), test_auth(), test_engine())
            .await
            .expect("bind");

        assert_ne!(server.local_addr().port(), 0, "port 0 must be resolved");
        assert!(!server.is_tls(), "no [rest_api.tls] means plain HTTP");
    }
}
