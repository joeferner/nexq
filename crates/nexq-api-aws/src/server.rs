//! The facade's HTTP listener.
//!
//! This facade owns its own socket rather than being mounted into a shared server, so
//! it can be enabled, disabled, and bound independently of the others.

use std::io;
use std::net::SocketAddr;

use axum::Router;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::routing::any;
use nexq_core::AwsApiConfig;
use tokio::net::TcpListener;
use tracing::info;

/// Header naming the operation to invoke, as sent by AWS JSON 1.0 clients:
/// `X-Amz-Target: AmazonSQS.ListQueues`.
pub const TARGET_HEADER: &str = "x-amz-target";

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
/// [`TARGET_HEADER`], so there is one route rather than one per operation. Dispatch on
/// that header, SigV4 verification, and the operations themselves come next.
pub fn router() -> Router {
    Router::new().route("/", any(dispatch))
}

async fn dispatch(request: Request) -> (StatusCode, String) {
    let target = request
        .headers()
        .get(TARGET_HEADER)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("<none>")
        .to_owned();

    (
        StatusCode::NOT_IMPLEMENTED,
        format!("aws facade reachable; operation {target} is not implemented yet\n"),
    )
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use axum::body::Body;
    use axum::http::Request as HttpRequest;
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

    #[tokio::test]
    async fn every_operation_routes_to_one_handler() {
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

        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);

        let body = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("AmazonSQS.ListQueues"), "{body}");
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
