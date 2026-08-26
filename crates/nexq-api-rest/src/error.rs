//! The one error shape this facade answers with.
//!
//! Deliberately not the SQS facade's `__type` envelope: that shape exists because AWS
//! SDKs parse it, and nothing here is one. What both facades *do* share is
//! [`EngineError`], which each maps to its own wire form — so the two can differ in
//! spelling without being able to disagree about what went wrong.
//!
//! ```json
//! { "error": { "code": "queue_not_found", "message": "no queue named jobs" } }
//! ```
//!
//! Codes are `snake_case`, matching the field naming everywhere else in this facade, and
//! are part of the contract: a client may branch on `code`, so one must not be renamed
//! without that being a breaking change. `message` is for a human and may change freely.

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use nexq_core::engine::EngineError;
use nexq_core::model::InvalidQueueName;
use serde::Serialize;
use tracing::error;

/// A refused request, as a status plus a machine-readable code.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

/// The JSON body of a refused request.
///
/// Nested under `error` rather than flattened so that a successful response and a failed
/// one can never be told apart only by which fields happen to be present.
#[derive(Debug, Serialize)]
struct ErrorBody<'a> {
    error: ErrorDetail<'a>,
}

#[derive(Debug, Serialize)]
struct ErrorDetail<'a> {
    code: &'a str,
    message: &'a str,
}

impl ApiError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    /// No credentials, or credentials that do not check out.
    ///
    /// One answer for both, unlike the SQS facade's `InvalidClientTokenId` versus
    /// `SignatureDoesNotMatch`. That facade has to tell them apart because AWS clients
    /// report on the distinction; this one does not, so it does not confirm to an
    /// unauthenticated caller that a key id exists.
    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "a valid bearer token is required",
        )
    }

    /// The request was understood and is wrong.
    pub fn bad_request(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, code, message)
    }

    pub fn not_found(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, code, message)
    }

    /// No route matches.
    pub fn no_such_route() -> Self {
        Self::not_found("no_such_route", "no route matches this method and path")
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl From<InvalidQueueName> for ApiError {
    fn from(error: InvalidQueueName) -> Self {
        Self::bad_request("invalid_queue_name", error.to_string())
    }
}

impl From<EngineError> for ApiError {
    fn from(error: EngineError) -> Self {
        match error {
            EngineError::QueueNotFound(name) => {
                Self::not_found("queue_not_found", format!("no queue named {name}"))
            }
            EngineError::QueueAlreadyExists(name) => Self::new(
                StatusCode::CONFLICT,
                "queue_already_exists",
                format!("a queue named {name} exists with different attributes"),
            ),
            // Retryable rather than wrong, so it is a 409 and says to try again: the
            // caller's request was fine and would succeed on its own next attempt.
            EngineError::Conflict(name) => Self::new(
                StatusCode::CONFLICT,
                "conflict",
                format!("queue {name} is being changed concurrently; retry"),
            ),
            EngineError::MessageTooLarge { .. } => Self::new(
                StatusCode::PAYLOAD_TOO_LARGE,
                "message_too_large",
                error.to_string(),
            ),
            EngineError::InvalidReceipt => Self::bad_request(
                "invalid_receipt_handle",
                "the receipt handle does not identify a current claim",
            ),
            // The only case that is this server's fault, so it is the only one whose
            // detail is logged and withheld: a backend error can name hosts, paths, or
            // credentials, none of which belong in a response to a client.
            EngineError::Backend(source) => {
                error!(error = %source, "storage backend failed");
                Self::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "the storage backend failed; see the server log",
                )
            }
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(ErrorBody {
            error: ErrorDetail {
                code: self.code,
                message: &self.message,
            },
        });

        let mut response = (self.status, body).into_response();

        // RFC 9110 requires this on a 401, and without it a client cannot tell which
        // scheme to present. Only the scheme: a realm would suggest scopes exist, and
        // every credential in the registry can currently do everything.
        if self.status == StatusCode::UNAUTHORIZED {
            response
                .headers_mut()
                .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        }

        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexq_core::QueueName;

    #[test]
    fn a_missing_queue_is_a_404_naming_the_queue() {
        let name = QueueName::new("jobs").expect("valid name");
        let error = ApiError::from(EngineError::QueueNotFound(name));

        assert_eq!(error.status(), StatusCode::NOT_FOUND);
        assert_eq!(error.code(), "queue_not_found");
        assert!(error.message().contains("jobs"), "{}", error.message());
    }

    /// A backend error can carry hosts, paths, or credentials, so the client gets the
    /// code and nothing else.
    #[test]
    fn a_backend_failure_does_not_reach_the_client() {
        let error = ApiError::from(EngineError::Backend(
            "connection to postgres://user:hunter2@db failed".into(),
        ));

        assert_eq!(error.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            !error.message().contains("hunter2"),
            "the detail must stay in the log: {}",
            error.message()
        );
    }

    #[tokio::test]
    async fn a_401_says_which_scheme_to_present() {
        let response = ApiError::unauthorized().into_response();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response
                .headers()
                .get(header::WWW_AUTHENTICATE)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer")
        );
    }

    #[tokio::test]
    async fn an_error_body_carries_the_code_under_error() {
        use http_body_util::BodyExt;

        let response =
            ApiError::not_found("queue_not_found", "no queue named jobs").into_response();
        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).expect("json");

        assert_eq!(json["error"]["code"], "queue_not_found");
        assert_eq!(json["error"]["message"], "no queue named jobs");
    }
}
