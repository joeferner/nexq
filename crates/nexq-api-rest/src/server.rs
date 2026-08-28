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
use aide::axum::routing::{delete_with, get_with, post_with, put_with};
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
use crate::docs::Docs;
use crate::error::ApiError;
use crate::messages;
use crate::queues;

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

/// Where the browsable documentation page is served. See [`crate::docs`].
pub const DOCS_PATH: &str = "/api/v1/docs";

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
        ApiRouter::new()
            // The collection, then the member, then what hangs off it — a queue is
            // addressed by its name rather than by a URL this server handed out.
            .api_route(
                "/queues",
                get_with(queues::list_queues, queues::list_queues_docs),
            )
            .api_route(
                "/queues/{queue}",
                put_with(queues::put_queue, queues::put_queue_docs)
                    .get_with(queues::get_queue, queues::get_queue_docs)
                    .patch_with(queues::patch_queue, queues::patch_queue_docs)
                    .delete_with(queues::delete_queue, queues::delete_queue_docs),
            )
            // The message collection, its sub-resource actions, and one claim. Receiving,
            // purging and the multi-entry forms are actions on the collection; deleting and
            // re-timing a single claim address it directly.
            .api_route(
                "/queues/{queue}/messages",
                post_with(messages::send, messages::send_docs)
                    .delete_with(messages::purge, messages::purge_docs),
            )
            .api_route(
                "/queues/{queue}/messages/receive",
                post_with(messages::receive, messages::receive_docs),
            )
            .api_route(
                "/queues/{queue}/messages/delete",
                post_with(messages::delete_batch, messages::delete_batch_docs),
            )
            .api_route(
                "/queues/{queue}/messages/visibility",
                post_with(messages::visibility_batch, messages::visibility_batch_docs),
            )
            // Position is addressed by *message id*, not by receipt handle: the producer
            // asking where its job got to has an id and never holds a claim.
            .api_route(
                "/queues/{queue}/messages/{messageId}/position",
                get_with(messages::position, messages::position_docs),
            )
            .api_route(
                "/queues/{queue}/messages/{receiptHandle}",
                delete_with(messages::delete_message, messages::delete_message_docs).patch_with(
                    messages::change_visibility,
                    messages::change_visibility_docs,
                ),
            ),
    )
}

/// Describe the API as a whole — everything not derivable from an individual route.
fn api_metadata(api: TransformOpenApi) -> TransformOpenApi {
    api.title("NexQ")
        .version(env!("CARGO_PKG_VERSION"))
        .description(
            "The native REST API for NexQ, a multi-protocol queue server. It covers every \
             operation NexQ supports — including the features that go beyond what the \
             SQS-compatible endpoint can express — and is the API to reach for when you \
             are writing against NexQ directly rather than through an AWS SDK.",
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

/// The OpenAPI document as JSON — what is served, and what `openapi.json` holds.
///
/// Pretty-printed so a diff against the committed copy is readable line by line rather
/// than one very long line that changed somewhere, and **newline-terminated** so the two
/// are byte-identical: the file is a text file and wants the newline, and matching it here
/// means `curl <server>/api/v1/openapi.json | diff - openapi.json` is a valid check rather
/// than one that always reports a difference at the end.
pub fn openapi_json() -> String {
    to_json(&openapi())
}

/// Render a document the one way this facade renders documents.
///
/// Shared by [`openapi_json`] and [`router`] so the bytes served and the bytes compared
/// against the committed file come from the same code. Two call sites serializing
/// independently could differ in a setting and produce a mismatch with no cause to find.
fn to_json(api: &OpenApi) -> String {
    let mut json = serde_json::to_string_pretty(api)
        // An OpenAPI document is plain data with string keys throughout, so this can only
        // fail if `aide` produced something that is not representable as JSON — a bug
        // there, not a condition an operator can be in.
        .expect("an OpenAPI document always serializes");
    json.push('\n');

    json
}

/// Compare a committed copy of the document against what the code generates.
///
/// `Err` carries a report meant for a human: where the two first differ, and what to run.
/// Shared so the `openapi-check` task and
/// `the_committed_document_is_the_generated_one` report the same thing — two
/// implementations of "how do these differ" would drift, and a check that explains itself
/// differently depending on how you ran it is a check people learn to distrust.
pub fn check_openapi(committed: &str) -> Result<(), String> {
    let generated = openapi_json();
    if committed == generated {
        return Ok(());
    }

    let mut report = String::from(
        "the committed openapi.json is out of date — run `make openapi` (or \
         `cargo xtask openapi`) and review the diff, since it is a change to the published \
         contract.\n\n",
    );

    // The first differing line, rather than both documents in full: printing ten kilobytes
    // of JSON twice and leaving the reader to spot the change is the moment a useful
    // failure becomes a wall of text.
    for (number, (left, right)) in committed.lines().zip(generated.lines()).enumerate() {
        if left != right {
            report.push_str(&format!(
                "first difference at line {}:\n  committed: {left}\n  generated: {right}\n",
                number + 1
            ));
            break;
        }
    }

    let (committed_lines, generated_lines) = (committed.lines().count(), generated.lines().count());
    if committed_lines != generated_lines {
        report.push_str(&format!(
            "committed is {committed_lines} lines, generated is {generated_lines}\n"
        ));
    } else if report.ends_with("\n\n") {
        // Same length, same lines, still not equal: the difference is trailing bytes.
        report.push_str("the lines all agree; the documents differ in trailing whitespace\n");
    }

    Err(report)
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

    // The document this router just built, rendered by the same function `openapi_json`
    // uses — not a second generation of it, which would do the work twice for a result
    // that has to be identical anyway.
    let spec = Bytes::from(to_json(&api));

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
        // Added after the auth layer, so the spec and the page that renders it are
        // readable without a token. They describe the shape of the API and carry nothing
        // deployment-specific — no queue names, no data — and a client generator has to be
        // able to fetch the document.
        .route(OPENAPI_PATH, get(serve_openapi))
        .layer(Extension(spec))
        .nest(API_PREFIX, Docs::new(API_PREFIX).router())
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

    /// Drive one authenticated request, returning its status and parsed body.
    ///
    /// A `204` has no body, so it comes back as `null` rather than failing to parse.
    /// A facade over a fresh in-memory engine, with the test credential.
    pub(super) fn test_facade() -> FacadeState {
        Arc::new(Facade {
            auth: test_auth(),
            engine: test_engine(),
        })
    }

    /// The status of a request that presents no token.
    pub(super) async fn unauthenticated(
        facade: &FacadeState,
        method: &str,
        path: &str,
    ) -> StatusCode {
        let request = HttpRequest::builder()
            .method(method)
            .uri(path)
            .body(Body::empty())
            .expect("request");

        router(Arc::clone(facade))
            .oneshot(request)
            .await
            .expect("response")
            .status()
    }

    pub(super) async fn request(
        facade: &FacadeState,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = HttpRequest::builder()
            .method(method)
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {}", token()));

        if body.is_some() {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
        }

        let request = builder
            .body(body.map_or_else(Body::empty, |body| Body::from(body.to_owned())))
            .expect("request");

        let response = router(Arc::clone(facade))
            .oneshot(request)
            .await
            .expect("response");
        let status = response.status();

        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let json = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("json")
        };

        (status, json)
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
        assert_eq!(json["messages"][0]["receiveCount"], 1);
        assert!(
            json["messages"][0]["receiptHandle"]
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

    /// A queue's whole life over the resource routes: create, read, list, delete.
    #[tokio::test]
    async fn a_queue_is_created_read_listed_and_deleted_by_name() {
        let facade = Arc::new(Facade {
            auth: test_auth(),
            engine: test_engine(),
        });

        let created = request(
            &facade,
            "PUT",
            "/api/v1/queues/jobs",
            Some(r#"{"delaySeconds": 5}"#),
        )
        .await;
        assert_eq!(created.0, StatusCode::OK);
        assert_eq!(created.1["name"], "jobs");
        assert_eq!(created.1["attributes"]["delaySeconds"], 5);
        assert_eq!(
            created.1["attributes"]["visibilityTimeoutSeconds"], 30,
            "an unnamed attribute takes its default rather than zero"
        );

        // RFC 3339, which is what a generated client turns into a date type.
        let created_at = created.1["createdAt"].as_str().expect("a timestamp");
        assert!(
            created_at.contains('T') && created_at.ends_with('Z'),
            "expected RFC 3339: {created_at}"
        );

        // Idempotent: the same request again is the same queue, not a conflict.
        let again = request(
            &facade,
            "PUT",
            "/api/v1/queues/jobs",
            Some(r#"{"delaySeconds": 5}"#),
        )
        .await;
        assert_eq!(again.0, StatusCode::OK);
        assert_eq!(again.1["createdAt"], created.1["createdAt"]);

        // Different attributes are a conflict rather than a silent reconfiguration.
        let conflict = request(
            &facade,
            "PUT",
            "/api/v1/queues/jobs",
            Some(r#"{"delaySeconds": 6}"#),
        )
        .await;
        assert_eq!(conflict.0, StatusCode::CONFLICT);
        assert_eq!(conflict.1["error"]["code"], "queue_already_exists");

        let read = request(&facade, "GET", "/api/v1/queues/jobs", None).await;
        assert_eq!(read.0, StatusCode::OK);
        assert_eq!(read.1["attributes"]["delaySeconds"], 5);

        let listed = request(&facade, "GET", "/api/v1/queues", None).await;
        assert_eq!(listed.1["queues"][0]["name"], "jobs");
        assert_eq!(listed.1["nextCursor"], serde_json::Value::Null);

        let deleted = request(&facade, "DELETE", "/api/v1/queues/jobs", None).await;
        assert_eq!(deleted.0, StatusCode::NO_CONTENT);

        assert_eq!(
            request(&facade, "GET", "/api/v1/queues/jobs", None).await.0,
            StatusCode::NOT_FOUND
        );
    }

    /// An empty collection is an empty list, not a 404 and not an absent field.
    #[tokio::test]
    async fn listing_nothing_is_an_empty_list() {
        let facade = Arc::new(Facade {
            auth: test_auth(),
            engine: test_engine(),
        });

        let listed = request(&facade, "GET", "/api/v1/queues", None).await;

        assert_eq!(listed.0, StatusCode::OK);
        assert_eq!(listed.1["queues"], serde_json::json!([]));
        assert_eq!(listed.1["nextCursor"], serde_json::Value::Null);
    }

    /// The reason paging is by cursor rather than by offset: a queue deleted between
    /// pages must not make the caller skip the one that would have shifted into its place.
    #[tokio::test]
    async fn a_cursor_survives_churn_that_would_break_an_offset() {
        let facade = Arc::new(Facade {
            auth: test_auth(),
            engine: test_engine(),
        });

        for name in ["a", "b", "c", "d"] {
            request(&facade, "PUT", &format!("/api/v1/queues/{name}"), None).await;
        }

        let first = request(&facade, "GET", "/api/v1/queues?limit=2", None).await;
        assert_eq!(first.1["queues"][0]["name"], "a");
        assert_eq!(first.1["queues"][1]["name"], "b");
        let cursor = first.1["nextCursor"]
            .as_str()
            .expect("more remain")
            .to_owned();

        // Delete one from the page already read. An offset of 2 would now skip "c".
        request(&facade, "DELETE", "/api/v1/queues/a", None).await;

        let second = request(
            &facade,
            "GET",
            &format!("/api/v1/queues?cursor={cursor}"),
            None,
        )
        .await;
        let names: Vec<&str> = second.1["queues"]
            .as_array()
            .expect("a page")
            .iter()
            .map(|queue| queue["name"].as_str().expect("a name"))
            .collect();

        assert_eq!(names, ["c", "d"], "nothing skipped and nothing repeated");
        assert_eq!(second.1["nextCursor"], serde_json::Value::Null);
    }

    #[tokio::test]
    async fn listing_can_be_filtered_by_prefix() {
        let facade = Arc::new(Facade {
            auth: test_auth(),
            engine: test_engine(),
        });
        for name in ["job-one", "job-two", "email"] {
            request(&facade, "PUT", &format!("/api/v1/queues/{name}"), None).await;
        }

        let listed = request(&facade, "GET", "/api/v1/queues?prefix=job", None).await;

        assert_eq!(listed.1["queues"].as_array().map(Vec::len), Some(2));
    }

    /// A cursor is this server's to issue, so a nonsense one says that rather than
    /// blaming the caller's queue name.
    #[tokio::test]
    async fn a_cursor_we_did_not_issue_is_refused_as_a_cursor() {
        let facade = Arc::new(Facade {
            auth: test_auth(),
            engine: test_engine(),
        });

        let refused = request(&facade, "GET", "/api/v1/queues?cursor=not%20a%20name", None).await;

        assert_eq!(refused.0, StatusCode::BAD_REQUEST);
        assert_eq!(refused.1["error"]["code"], "invalid_cursor");
    }

    #[tokio::test]
    async fn an_unknown_query_parameter_is_refused() {
        let facade = Arc::new(Facade {
            auth: test_auth(),
            engine: test_engine(),
        });

        // `limit` misspelled: silently listing everything would be worse than saying no.
        let refused = request(&facade, "GET", "/api/v1/queues?limti=2", None).await;

        assert_eq!(refused.0, StatusCode::BAD_REQUEST);
    }

    /// Every queue route needs a token, like every other route.
    #[tokio::test]
    async fn the_queue_routes_are_behind_the_same_auth_layer() {
        let facade = Arc::new(Facade {
            auth: test_auth(),
            engine: test_engine(),
        });

        for (method, path) in [
            ("GET", "/api/v1/queues"),
            ("PUT", "/api/v1/queues/jobs"),
            ("GET", "/api/v1/queues/jobs"),
            ("DELETE", "/api/v1/queues/jobs"),
        ] {
            let request = HttpRequest::builder()
                .method(method)
                .uri(path)
                .body(Body::empty())
                .expect("request");

            let response = router(Arc::clone(&facade))
                .oneshot(request)
                .await
                .expect("response");

            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "{method} {path} must not be reachable without a token"
            );
        }
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

    /// The docs page and its assets, all readable without a token like the spec.
    #[tokio::test]
    async fn the_docs_page_and_its_assets_are_served_from_this_binary() {
        for (path, expected_type) in [
            (DOCS_PATH, "text/html; charset=utf-8"),
            (
                "/api/v1/docs/bootstrap.js",
                "text/javascript; charset=utf-8",
            ),
            ("/api/v1/docs/scalar.js", "text/javascript; charset=utf-8"),
        ] {
            let request = HttpRequest::builder()
                .method("GET")
                .uri(path)
                .body(Body::empty())
                .expect("request");

            let response = router(Arc::new(Facade {
                auth: test_auth(),
                engine: test_engine(),
            }))
            .oneshot(request)
            .await
            .expect("response");

            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(
                response
                    .headers()
                    .get(header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok()),
                Some(expected_type),
                "{path}"
            );

            // The header that stops a pasted token reaching anywhere but this origin.
            let policy = response
                .headers()
                .get("content-security-policy")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default();
            assert!(policy.contains("connect-src 'self'"), "{path}: {policy}");
        }
    }

    /// A 3.8 MB script should not come down again on every reload.
    #[tokio::test]
    async fn a_client_that_already_has_an_asset_gets_a_304() {
        let path = "/api/v1/docs/scalar.js";

        let first = router(Arc::new(Facade {
            auth: test_auth(),
            engine: test_engine(),
        }))
        .oneshot(
            HttpRequest::builder()
                .method("GET")
                .uri(path)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

        let etag = first
            .headers()
            .get(header::ETAG)
            .expect("an asset must be cacheable")
            .clone();

        let again = router(Arc::new(Facade {
            auth: test_auth(),
            engine: test_engine(),
        }))
        .oneshot(
            HttpRequest::builder()
                .method("GET")
                .uri(path)
                .header(header::IF_NONE_MATCH, &etag)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

        assert_eq!(again.status(), StatusCode::NOT_MODIFIED);
        assert!(
            again
                .into_body()
                .collect()
                .await
                .expect("body")
                .to_bytes()
                .is_empty(),
            "a 304 carries no body"
        );
    }

    /// The page is documentation, not an operation — a generated client should have no
    /// `getDocsPage` method.
    #[tokio::test]
    async fn the_docs_routes_are_not_in_the_spec() {
        let paths =
            serde_json::from_str::<serde_json::Value>(&openapi_json()).expect("json")["paths"]
                .clone();

        for path in paths.as_object().expect("an object").keys() {
            assert!(!path.contains("/docs"), "{path} should not be documented");
        }
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

/// The rest of the surface, driven through the router.
///
/// Its own module because [`tests`] was already long, and because these are about what the
/// API *does* rather than about how it is wired — they read as a sequence of requests a
/// client would actually make.
#[cfg(test)]
mod parity_tests {
    use axum::http::StatusCode;

    use super::tests::{request, test_facade, unauthenticated};

    /// Produce and consume entirely over REST, which until now was impossible: there was
    /// no way to send, and no way to say a message was done with.
    #[tokio::test]
    async fn a_message_can_be_sent_received_and_deleted_over_rest_alone() {
        let facade = test_facade();
        request(&facade, "PUT", "/api/v1/queues/jobs", None).await;

        let sent = request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages",
            Some(r#"{"messages":[{"body":"work","priority":5}]}"#),
        )
        .await;
        assert_eq!(sent.0, StatusCode::OK);
        assert_eq!(sent.1["results"][0]["status"], "accepted");
        let id = sent.1["results"][0]["messageId"]
            .as_str()
            .expect("an id")
            .to_owned();

        let received = request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages/receive",
            Some("{}"),
        )
        .await;
        let message = &received.1["messages"][0];
        assert_eq!(message["id"], id, "the message that was just sent");
        assert_eq!(message["body"], "work");
        assert_eq!(
            message["priority"], 5,
            "priority is settable here, unlike through the SQS facade"
        );
        let handle = message["receiptHandle"].as_str().expect("a handle");

        let deleted = request(
            &facade,
            "DELETE",
            &format!("/api/v1/queues/jobs/messages/{handle}"),
            None,
        )
        .await;
        assert_eq!(deleted.0, StatusCode::NO_CONTENT);

        // Gone for good: a spent handle is refused rather than quietly accepted.
        let again = request(
            &facade,
            "DELETE",
            &format!("/api/v1/queues/jobs/messages/{handle}"),
            None,
        )
        .await;
        assert_eq!(again.0, StatusCode::BAD_REQUEST);
        assert_eq!(again.1["error"]["code"], "invalid_receipt_handle");
    }

    /// The point of per-entry results: one bad message must not sink the others.
    #[tokio::test]
    async fn a_send_reports_each_message_separately() {
        let facade = test_facade();
        request(&facade, "PUT", "/api/v1/queues/jobs", None).await;

        let sent = request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages",
            Some(
                r#"{"messages":[
                     {"body":"fine"},
                     {"body":"bad delay","delaySeconds":901},
                     {"body":"also fine"}
                   ]}"#,
            ),
        )
        .await;

        assert_eq!(sent.0, StatusCode::OK, "the request itself succeeded");
        let results = sent.1["results"].as_array().expect("results");
        assert_eq!(results.len(), 3);
        assert_eq!(results[0]["status"], "accepted");
        assert_eq!(results[1]["status"], "refused");
        assert_eq!(results[1]["index"], 1, "identified by position");
        assert_eq!(results[1]["error"]["code"], "invalid_delay");
        assert_eq!(results[2]["status"], "accepted");

        // Two arrived, not zero and not three.
        let counted = request(&facade, "GET", "/api/v1/queues/jobs?counts=true", None).await;
        assert_eq!(counted.1["counts"]["visible"], 2);
        assert_eq!(counted.1["counts"]["total"], 2);
    }

    /// Message attributes must survive the round trip, binary included.
    #[tokio::test]
    async fn attributes_come_back_as_they_went_in() {
        let facade = test_facade();
        request(&facade, "PUT", "/api/v1/queues/jobs", None).await;

        request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages",
            Some(
                r#"{"messages":[{"body":"hi","attributes":{
                     "City":  {"type":"string","value":"Any City"},
                     "Count": {"type":"number","value":"1250800"},
                     "Label": {"type":"string","label":"uuid","value":"3f2b1c"},
                     "Thumb": {"type":"binary","value":"aGVsbG8="}
                   }}]}"#,
            ),
        )
        .await;

        let received = request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages/receive",
            Some("{}"),
        )
        .await;
        let attributes = &received.1["messages"][0]["attributes"];

        assert_eq!(attributes["City"]["type"], "string");
        assert_eq!(attributes["City"]["value"], "Any City");
        assert_eq!(attributes["Count"]["type"], "number");
        assert_eq!(attributes["Count"]["value"], "1250800");
        assert_eq!(attributes["Label"]["type"], "string");
        assert_eq!(
            attributes["Label"]["label"], "uuid",
            "a producer's own label is carried through untouched"
        );
        assert_eq!(attributes["Thumb"]["type"], "binary");
        assert_eq!(
            attributes["Thumb"]["value"], "aGVsbG8=",
            "binary comes back base64, of the bytes that were stored"
        );
    }

    #[tokio::test]
    async fn a_binary_attribute_that_is_not_base64_is_refused() {
        let facade = test_facade();
        request(&facade, "PUT", "/api/v1/queues/jobs", None).await;

        let sent = request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages",
            Some(r#"{"messages":[{"body":"hi","attributes":{"T":{"type":"binary","value":"!!!"}}}]}"#),
        )
        .await;

        assert_eq!(sent.1["results"][0]["status"], "refused");
        assert_eq!(
            sent.1["results"][0]["error"]["code"],
            "invalid_message_attribute"
        );
    }

    /// A number stored as text still has to be a number, or the next reader is handed a lie.
    #[tokio::test]
    async fn a_number_attribute_that_is_not_a_number_is_refused() {
        let facade = test_facade();
        request(&facade, "PUT", "/api/v1/queues/jobs", None).await;

        let sent = request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages",
            Some(r#"{"messages":[{"body":"hi","attributes":{"N":{"type":"number","value":"banana"}}}]}"#),
        )
        .await;

        assert_eq!(sent.1["results"][0]["status"], "refused");
    }

    /// `PATCH` keeps what it does not name; `PUT` resets it. That difference is the reason
    /// both verbs exist on the member.
    #[tokio::test]
    async fn patch_changes_one_attribute_and_put_resets_the_rest() {
        let facade = test_facade();
        request(
            &facade,
            "PUT",
            "/api/v1/queues/jobs",
            Some(r#"{"delaySeconds":5,"visibilityTimeoutSeconds":120}"#),
        )
        .await;

        let patched = request(
            &facade,
            "PATCH",
            "/api/v1/queues/jobs",
            Some(r#"{"delaySeconds":7}"#),
        )
        .await;

        assert_eq!(patched.0, StatusCode::OK);
        assert_eq!(patched.1["attributes"]["delaySeconds"], 7);
        assert_eq!(
            patched.1["attributes"]["visibilityTimeoutSeconds"], 120,
            "PATCH must leave an attribute it was not told about alone"
        );

        // And a PATCH that names nothing is refused rather than moving last_modified_at
        // while changing nothing.
        let empty = request(&facade, "PATCH", "/api/v1/queues/jobs", Some("{}")).await;
        assert_eq!(empty.0, StatusCode::BAD_REQUEST);
        assert_eq!(empty.1["error"]["code"], "empty_update");
    }

    #[tokio::test]
    async fn a_patch_with_a_bad_attribute_changes_nothing() {
        let facade = test_facade();
        request(
            &facade,
            "PUT",
            "/api/v1/queues/jobs",
            Some(r#"{"delaySeconds":5}"#),
        )
        .await;

        let refused = request(
            &facade,
            "PATCH",
            "/api/v1/queues/jobs",
            Some(r#"{"delaySeconds":9,"visibilityTimeoutSeconds":99999}"#),
        )
        .await;
        assert_eq!(refused.0, StatusCode::BAD_REQUEST);

        let read = request(&facade, "GET", "/api/v1/queues/jobs", None).await;
        assert_eq!(
            read.1["attributes"]["delaySeconds"], 5,
            "all-or-nothing: the good attribute must not have been applied either"
        );
    }

    /// Purge empties the queue and keeps it, taking claimed messages too.
    #[tokio::test]
    async fn purging_takes_in_flight_messages_with_it() {
        let facade = test_facade();
        request(&facade, "PUT", "/api/v1/queues/jobs", None).await;
        request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages",
            Some(r#"{"messages":[{"body":"a"},{"body":"b"},{"body":"c"}]}"#),
        )
        .await;

        // Claim one, so a purge has an in-flight message to deal with.
        let received = request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages/receive",
            Some("{}"),
        )
        .await;
        let handle = received.1["messages"][0]["receiptHandle"]
            .as_str()
            .expect("a handle")
            .to_owned();

        let purged = request(&facade, "DELETE", "/api/v1/queues/jobs/messages", None).await;
        assert_eq!(purged.0, StatusCode::OK);
        assert_eq!(
            purged.1["purged"], 3,
            "all three, including the one being worked on"
        );

        // The queue survives, empty.
        let counted = request(&facade, "GET", "/api/v1/queues/jobs?counts=true", None).await;
        assert_eq!(counted.0, StatusCode::OK);
        assert_eq!(counted.1["counts"]["total"], 0);

        // And the handle taken across the purge no longer refers to anything.
        let stale = request(
            &facade,
            "DELETE",
            &format!("/api/v1/queues/jobs/messages/{handle}"),
            None,
        )
        .await;
        assert_eq!(stale.0, StatusCode::BAD_REQUEST);
    }

    /// Handing a message back with `0` is the useful edge, and it must make the message
    /// claimable again straight away.
    #[tokio::test]
    async fn re_timing_a_claim_to_zero_hands_the_message_back() {
        let facade = test_facade();
        request(&facade, "PUT", "/api/v1/queues/jobs", None).await;
        request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages",
            Some(r#"{"messages":[{"body":"work"}]}"#),
        )
        .await;

        let received = request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages/receive",
            Some("{}"),
        )
        .await;
        let handle = received.1["messages"][0]["receiptHandle"]
            .as_str()
            .expect("a handle")
            .to_owned();

        // Nobody else can have it while the claim stands.
        let blocked = request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages/receive",
            Some("{}"),
        )
        .await;
        assert_eq!(blocked.1["messages"], serde_json::json!([]));

        let handed_back = request(
            &facade,
            "PATCH",
            &format!("/api/v1/queues/jobs/messages/{handle}"),
            Some(r#"{"visibilityTimeoutSeconds":0}"#),
        )
        .await;
        assert_eq!(handed_back.0, StatusCode::NO_CONTENT);

        let again = request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages/receive",
            Some("{}"),
        )
        .await;
        assert_eq!(
            again.1["messages"][0]["body"], "work",
            "a message handed back is available to the next consumer at once"
        );
        assert_eq!(
            again.1["messages"][0]["receiveCount"], 2,
            "and it counts as a second delivery"
        );
    }

    #[tokio::test]
    async fn the_batch_forms_report_per_entry_outcomes() {
        let facade = test_facade();
        request(&facade, "PUT", "/api/v1/queues/jobs", None).await;
        request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages",
            Some(r#"{"messages":[{"body":"a"},{"body":"b"}]}"#),
        )
        .await;

        let received = request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages/receive",
            Some(r#"{"maxMessages":2}"#),
        )
        .await;
        let handles: Vec<String> = received.1["messages"]
            .as_array()
            .expect("messages")
            .iter()
            .map(|message| {
                message["receiptHandle"]
                    .as_str()
                    .expect("a handle")
                    .to_owned()
            })
            .collect();
        assert_eq!(handles.len(), 2);

        // Re-time both, one of them to zero.
        let retimed = request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages/visibility",
            Some(&format!(
                r#"{{"changes":[
                     {{"receiptHandle":"{}","visibilityTimeoutSeconds":300}},
                     {{"receiptHandle":"{}","visibilityTimeoutSeconds":0}}
                   ]}}"#,
                handles[0], handles[1]
            )),
        )
        .await;
        assert_eq!(retimed.0, StatusCode::OK);
        assert_eq!(retimed.1["results"][0]["status"], "accepted");
        assert_eq!(retimed.1["results"][1]["status"], "accepted");

        // Delete a real handle alongside one that was never issued, which is the mix the
        // per-entry results exist for.
        //
        // Note what is *not* used here: the handle handed back with `0` above still deletes
        // its message, because handing a message back changes when its claim ends and not
        // whose it is — the handle is spent only once someone else claims the message and
        // is given a new one. That matches SQS, and it is worth knowing, because assuming
        // otherwise is what this test first got wrong.
        let deleted = request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages/delete",
            Some(&format!(
                r#"{{"receiptHandles":["{}","never-issued"]}}"#,
                handles[0]
            )),
        )
        .await;

        assert_eq!(deleted.0, StatusCode::OK);
        assert_eq!(deleted.1["results"][0]["status"], "accepted");
        assert_eq!(
            deleted.1["results"][1]["status"], "refused",
            "one bad handle is refused as itself rather than sinking the good one"
        );
        assert_eq!(
            deleted.1["results"][1]["error"]["code"],
            "invalid_receipt_handle"
        );
    }

    /// The whole-request failures that survive positional entries.
    #[tokio::test]
    async fn an_empty_or_oversized_list_is_refused_whole() {
        let facade = test_facade();
        request(&facade, "PUT", "/api/v1/queues/jobs", None).await;

        let empty = request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages",
            Some(r#"{"messages":[]}"#),
        )
        .await;
        assert_eq!(empty.0, StatusCode::BAD_REQUEST);
        assert_eq!(empty.1["error"]["code"], "empty_request");

        let entries: Vec<String> = (0..11).map(|n| format!(r#"{{"body":"{n}"}}"#)).collect();
        let too_many = request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages",
            Some(&format!(r#"{{"messages":[{}]}}"#, entries.join(","))),
        )
        .await;
        assert_eq!(too_many.0, StatusCode::BAD_REQUEST);
        assert_eq!(too_many.1["error"]["code"], "too_many_entries");

        // Nothing was sent by either attempt.
        let counted = request(&facade, "GET", "/api/v1/queues/jobs?counts=true", None).await;
        assert_eq!(counted.1["counts"]["total"], 0);
    }

    /// A missing queue is one raised error, not the same failure repeated per entry.
    #[tokio::test]
    async fn sending_to_a_missing_queue_is_one_error() {
        let facade = test_facade();

        let sent = request(
            &facade,
            "POST",
            "/api/v1/queues/nope/messages",
            Some(r#"{"messages":[{"body":"a"},{"body":"b"}]}"#),
        )
        .await;

        assert_eq!(sent.0, StatusCode::NOT_FOUND);
        assert_eq!(sent.1["error"]["code"], "queue_not_found");
        assert!(
            sent.1["results"].is_null(),
            "no per-entry list at all: {}",
            sent.1
        );
    }

    /// Counts are off by default, because asking for them costs an aggregate per queue.
    #[tokio::test]
    async fn counts_are_absent_unless_asked_for() {
        let facade = test_facade();
        request(&facade, "PUT", "/api/v1/queues/jobs", None).await;

        let without = request(&facade, "GET", "/api/v1/queues/jobs", None).await;
        assert!(without.1["counts"].is_null(), "{}", without.1);

        let with = request(&facade, "GET", "/api/v1/queues/jobs?counts=true", None).await;
        assert_eq!(with.1["counts"]["visible"], 0);

        let listed = request(&facade, "GET", "/api/v1/queues?counts=true", None).await;
        assert_eq!(listed.1["queues"][0]["counts"]["total"], 0);

        let listed_without = request(&facade, "GET", "/api/v1/queues", None).await;
        assert!(listed_without.1["queues"][0]["counts"].is_null());
    }

    /// Position over the wire: a producer asks where its job is, by the id it was given
    /// when it sent it, and never has to claim anything to find out.
    #[tokio::test]
    async fn a_producer_can_ask_where_its_message_is_in_the_line() {
        let facade = test_facade();
        request(&facade, "PUT", "/api/v1/queues/jobs", None).await;

        let sent = request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages",
            Some(r#"{"messages":[{"body":"first"},{"body":"second"}]}"#),
        )
        .await;
        let ids: Vec<String> = sent.1["results"]
            .as_array()
            .expect("results")
            .iter()
            .map(|result| result["messageId"].as_str().expect("an id").to_owned())
            .collect();

        let position = |id: &str| format!("/api/v1/queues/jobs/messages/{id}/position");

        let first = request(&facade, "GET", &position(&ids[0]), None).await;
        assert_eq!(first.0, StatusCode::OK);
        assert_eq!(first.1["messageId"], ids[0]);
        assert_eq!(
            first.1["approximatePosition"], 1,
            "one-based, so the next message to be served is first: {}",
            first.1
        );
        assert_eq!(first.1["state"], "visible");

        let second = request(&facade, "GET", &position(&ids[1]), None).await;
        assert_eq!(second.1["approximatePosition"], 2);

        // The surprising half of the contract, and the reason it is called approximate:
        // an urgent arrival pushes both of these back.
        request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages",
            Some(r#"{"messages":[{"body":"urgent","priority":10}]}"#),
        )
        .await;
        let moved = request(&facade, "GET", &position(&ids[0]), None).await;
        assert_eq!(
            moved.1["approximatePosition"], 2,
            "a higher-priority message moves an existing one backwards: {}",
            moved.1
        );

        // And a consumer taking that urgent message off the queue moves them up again:
        // only what is claimable now counts, and a message in flight is not.
        let received = request(
            &facade,
            "POST",
            "/api/v1/queues/jobs/messages/receive",
            Some("{}"),
        )
        .await;
        assert_eq!(received.1["messages"][0]["body"], "urgent");
        let urgent_id = received.1["messages"][0]["id"].as_str().expect("an id");

        let back_in_front = request(&facade, "GET", &position(&ids[0]), None).await;
        assert_eq!(back_in_front.1["approximatePosition"], 1);

        let in_flight = request(&facade, "GET", &position(urgent_id), None).await;
        assert_eq!(
            in_flight.1["state"], "notVisible",
            "a message being worked on says so: {}",
            in_flight.1
        );
        assert_eq!(
            in_flight.1["approximatePosition"], 3,
            "and is behind everything claimable rather than still at the front: {}",
            in_flight.1
        );
    }

    /// A message the queue does not hold is a `404` in this facade's own envelope, and
    /// says it is the *message* that is missing rather than the queue.
    #[tokio::test]
    async fn asking_where_a_message_that_is_gone_is_answers_404() {
        let facade = test_facade();
        request(&facade, "PUT", "/api/v1/queues/jobs", None).await;

        let unknown = request(
            &facade,
            "GET",
            "/api/v1/queues/jobs/messages/no-such-id/position",
            None,
        )
        .await;
        assert_eq!(unknown.0, StatusCode::NOT_FOUND);
        assert_eq!(unknown.1["error"]["code"], "message_not_found");

        let no_queue = request(
            &facade,
            "GET",
            "/api/v1/queues/nowhere/messages/no-such-id/position",
            None,
        )
        .await;
        assert_eq!(no_queue.0, StatusCode::NOT_FOUND);
        assert_eq!(
            no_queue.1["error"]["code"], "queue_not_found",
            "a missing queue is a different problem from a missing message"
        );
    }

    /// Every new route is behind the same layer as the old ones.
    #[tokio::test]
    async fn the_new_routes_need_a_token_too() {
        let facade = test_facade();

        for (method, path) in [
            ("PATCH", "/api/v1/queues/jobs"),
            ("GET", "/api/v1/queues/jobs/messages/some-id/position"),
            ("POST", "/api/v1/queues/jobs/messages"),
            ("DELETE", "/api/v1/queues/jobs/messages"),
            ("POST", "/api/v1/queues/jobs/messages/delete"),
            ("POST", "/api/v1/queues/jobs/messages/visibility"),
            ("DELETE", "/api/v1/queues/jobs/messages/handle"),
            ("PATCH", "/api/v1/queues/jobs/messages/handle"),
        ] {
            let response = unauthenticated(&facade, method, path).await;

            assert_eq!(
                response,
                StatusCode::UNAUTHORIZED,
                "{method} {path} must not be reachable without a token"
            );
        }
    }
}
