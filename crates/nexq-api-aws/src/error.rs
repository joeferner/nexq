//! Errors in the shape AWS clients expect.
//!
//! Enough to route requests correctly; the full catalogue of SQS error codes, and the
//! Query-shaped rendering that `x-amzn-query-mode: true` asks for, come with the
//! operations themselves.
//!
//! Two things are set on every error because clients look in different places: the
//! `x-amzn-errortype` header, and `__type` in the body. Both carry the same code.

use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::protocol::JSON_CONTENT_TYPE;

/// Namespace prefixed to an error code in `__type`, matching what real SQS sends.
const ERROR_TYPE_NAMESPACE: &str = "com.amazonaws.sqs";

/// Header naming the error code, which some SDKs read in preference to the body.
const ERROR_TYPE_HEADER: &str = "x-amzn-errortype";

/// A failure to report to the client.
#[derive(Debug, Clone)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    /// No `X-Amz-Target`, so this is either a Query-protocol client or not an SQS
    /// client at all. Named the way the Query protocol names it, since that is what
    /// such a client is expecting to hear.
    pub fn missing_target() -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "MissingAction",
            "no X-Amz-Target header; the AWS Query protocol is not supported yet, \
             so this request could not be routed",
        )
    }

    /// The target named an operation this facade does not have.
    pub fn unknown_operation(target: &str) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "UnknownOperationException",
            format!("unknown operation target: {target}"),
        )
    }

    /// The body was not a JSON object.
    pub fn malformed_body(detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "SerializationException",
            format!("could not decode the request body: {}", detail.into()),
        )
    }

    /// The request carried no `Authorization` header at all.
    pub fn missing_authentication_token() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "MissingAuthenticationToken",
            "Request is missing Authentication Token",
        )
    }

    /// The `Authorization` header is present but cannot be understood. The detail is
    /// safe to return: it describes the header's own structure, not the secret.
    pub fn incomplete_signature(detail: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "IncompleteSignature",
            detail.into(),
        )
    }

    /// The access key id is not in the credential registry.
    ///
    /// Same wording as real SQS, and deliberately identical for every unknown key so
    /// it cannot be used to probe which ids exist.
    pub fn invalid_client_token_id() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "InvalidClientTokenId",
            "The security token included in the request is invalid.",
        )
    }

    /// The signature did not match the one recomputed here.
    ///
    /// Carries no detail about why: what differed is a signal worth withholding, and
    /// the specifics are logged instead.
    pub fn signature_does_not_match() -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "SignatureDoesNotMatch",
            "The request signature we calculated does not match the signature you \
             provided. Check your AWS Secret Access Key and signing method.",
        )
    }

    /// The `QueueUrl` a client sent is not one this facade can act on.
    ///
    /// SQS reports a bad queue URL as a 404, which is what an SDK expects to see.
    pub fn invalid_address(url: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "InvalidAddress",
            format!("The address {url} is not valid for this endpoint."),
        )
    }

    /// A real operation that is not built yet. Temporary, and not an AWS error code —
    /// it should disappear as the operations land.
    pub fn not_implemented(operation: impl std::fmt::Display) -> Self {
        Self::new(
            StatusCode::NOT_IMPLEMENTED,
            "NotImplemented",
            format!("operation {operation} is not implemented yet"),
        )
    }

    pub fn status(&self) -> StatusCode {
        self.status
    }

    pub fn code(&self) -> &str {
        self.code
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({
            "__type": format!("{ERROR_TYPE_NAMESPACE}#{}", self.code),
            "message": self.message,
        });

        let mut response = (self.status, body.to_string()).into_response();
        let headers = response.headers_mut();

        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(JSON_CONTENT_TYPE),
        );
        // The code is always a bare identifier, so this cannot fail; fall back rather
        // than panic if that ever stops being true.
        if let Ok(value) = HeaderValue::from_str(self.code) {
            headers.insert(ERROR_TYPE_HEADER, value);
        }

        response
    }
}

#[cfg(test)]
mod tests {
    use http_body_util::BodyExt;
    use serde_json::Value;

    use super::*;

    async fn body_of(error: ApiError) -> (StatusCode, Value, String) {
        let response = error.into_response();
        let status = response.status();
        let error_type = response
            .headers()
            .get(ERROR_TYPE_HEADER)
            .expect("error type header")
            .to_str()
            .expect("ascii")
            .to_owned();

        let bytes = response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes();

        (
            status,
            serde_json::from_slice(&bytes).expect("json body"),
            error_type,
        )
    }

    #[tokio::test]
    async fn an_error_carries_its_code_in_the_body_and_the_header() {
        let (status, body, error_type) =
            body_of(ApiError::unknown_operation("AmazonSQS.Nope")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(
            body["__type"],
            "com.amazonaws.sqs#UnknownOperationException"
        );
        assert!(
            body["message"]
                .as_str()
                .expect("message")
                .contains("AmazonSQS.Nope")
        );
        assert_eq!(error_type, "UnknownOperationException");
    }

    #[tokio::test]
    async fn an_unimplemented_operation_is_not_a_client_error() {
        let (status, body, _) = body_of(ApiError::not_implemented("ListQueues")).await;

        assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(body["__type"], "com.amazonaws.sqs#NotImplemented");
    }

    #[tokio::test]
    async fn a_missing_target_explains_the_query_protocol_gap() {
        let (status, body, error_type) = body_of(ApiError::missing_target()).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(error_type, "MissingAction");
        assert!(
            body["message"]
                .as_str()
                .expect("message")
                .contains("Query protocol")
        );
    }
}
