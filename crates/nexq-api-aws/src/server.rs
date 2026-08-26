//! The facade's HTTP listener.
//!
//! This facade owns its own socket rather than being mounted into a shared server, so
//! it can be enabled, disabled, and bound independently of the others.

use std::io;
use std::net::SocketAddr;

use axum::Router;
use axum::body::Bytes;
use axum::http::{HeaderMap, header};
use axum::response::{IntoResponse, Response};
use axum::routing::any;
use nexq_core::AwsApiConfig;
use serde_json::Value;
use tokio::net::TcpListener;
use tracing::{debug, info};

use crate::error::ApiError;
use crate::operations;
use crate::protocol::{JSON_CONTENT_TYPE, Operation, TARGET_HEADER, decode_input};

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
    pub async fn bind(config: &AwsApiConfig) -> io::Result<Self> {
        let listener = TcpListener::bind(config.bind_addr).await?;
        let local_addr = listener.local_addr()?;

        Ok(Self {
            listener,
            local_addr,
            router: router(),
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
pub fn router() -> Router {
    Router::new().route("/", any(handle))
}

async fn handle(headers: HeaderMap, body: Bytes) -> Response {
    match route(&headers, &body).await {
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

/// Decode a request far enough to know what it is asking for, then run it.
async fn route(headers: &HeaderMap, body: &[u8]) -> Result<Value, ApiError> {
    let target = headers
        .get(TARGET_HEADER)
        .ok_or_else(ApiError::missing_target)?
        .to_str()
        .map_err(|_| ApiError::unknown_operation("<non-ascii target>"))?;

    let operation = Operation::from_target(target)?;
    let input = decode_input(body)?;

    debug!(%operation, "dispatching");
    operations::dispatch(operation, input).await
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
    use axum::http::{Request as HttpRequest, StatusCode};
    use http_body_util::BodyExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tower::ServiceExt;

    use super::*;

    /// Bind to port 0 so tests never collide with a real deployment or each other.
    fn test_config() -> AwsApiConfig {
        AwsApiConfig {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            ..AwsApiConfig::default()
        }
    }

    /// Send a request through the router, returning the status and decoded body.
    async fn send(target: Option<&str>, body: &'static str) -> (StatusCode, Value) {
        let mut request = HttpRequest::builder().method("POST").uri("/");
        if let Some(target) = target {
            request = request.header(TARGET_HEADER, target);
        }

        let response = router()
            .oneshot(request.body(Body::from(body)).expect("request"))
            .await
            .expect("response");

        let status = response.status();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();

        (status, serde_json::from_slice(&bytes).expect("json body"))
    }

    #[tokio::test]
    async fn a_known_operation_routes_to_its_handler() {
        let (status, body) = send(Some("AmazonSQS.ListQueues"), "{}").await;

        // Recognised, but not built yet — which is not the same as unknown.
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["__type"], "com.amazonaws.sqs#NotImplemented");
        assert!(
            body["message"]
                .as_str()
                .expect("message")
                .contains("ListQueues"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn an_unknown_operation_is_a_client_error() {
        let (status, body) = send(Some("AmazonSQS.Nope"), "{}").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body["__type"],
            "com.amazonaws.sqs#UnknownOperationException"
        );
    }

    #[tokio::test]
    async fn a_request_without_a_target_reports_the_query_protocol_gap() {
        let (status, body) = send(None, "Action=ListQueues&Version=2012-11-05").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["__type"], "com.amazonaws.sqs#MissingAction");
    }

    #[tokio::test]
    async fn a_malformed_body_is_rejected_before_the_operation_runs() {
        let (status, body) = send(Some("AmazonSQS.CreateQueue"), "{not json}").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["__type"], "com.amazonaws.sqs#SerializationException");
    }

    #[tokio::test]
    async fn an_empty_body_still_routes() {
        // Parameterless operations may send nothing rather than `{}`.
        let (status, _) = send(Some("AmazonSQS.ListQueues"), "").await;

        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    }

    #[tokio::test]
    async fn errors_are_returned_as_aws_json() {
        let response = router()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/")
                    .header(TARGET_HEADER, "AmazonSQS.ListQueues")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

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
        let server = Server::bind(&test_config()).await.expect("bind");

        assert_ne!(server.local_addr().port(), 0);
    }

    #[tokio::test]
    async fn serves_over_tcp_then_shuts_down_gracefully() {
        let server = Server::bind(&test_config()).await.expect("bind");
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
        assert!(response.starts_with("HTTP/1.1 501"), "{response}");

        shutdown_tx.send(()).expect("signal shutdown");
        tokio::time::timeout(Duration::from_secs(5), serving)
            .await
            .expect("serve should stop once shutdown is signalled")
            .expect("serve task")
            .expect("serve");
    }
}
