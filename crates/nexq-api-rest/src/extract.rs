//! Extractors whose refusals arrive in this facade's error envelope.
//!
//! `axum`'s own answer a malformed request **themselves**, as plain text, before any
//! handler runs — so the one-envelope promise quietly has holes wherever a built-in
//! extractor is used directly:
//!
//! ```text
//! Expected request with `Content-Type: application/json`
//! Failed to deserialize query string: unknown field `limti`
//! ```
//!
//! No status to branch on, no `code`, and not JSON, so a client parsing errors has to
//! special-case them. Both were found the same way — by typing a request wrong while
//! writing documentation — and the second turned up *after* the first was fixed, which is
//! why these live together: the fix belongs to the extractor layer rather than to whichever
//! endpoint happened to be tested.

use aide::generate::GenContext;
use aide::openapi::Operation;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{FromRequest, FromRequestParts, Request};
use axum::http::header;
use axum::http::request::Parts;
use serde::de::DeserializeOwned;

use crate::error::ApiError;

/// An optional JSON request body.
///
/// - No `Content-Type` at all — an empty `POST` — is **absent**, and the handler decides
///   what that means.
/// - A `Content-Type` that is not JSON is `415`, since guessing at form-encoded bytes
///   would turn a proxy's misconfiguration into a confusing parse error.
/// - JSON that does not fit the type is `400`, carrying `serde`'s own explanation, which
///   names the field and the line.
#[derive(Debug)]
pub struct OptionalJson<T>(pub Option<T>);

impl<T> OptionalJson<T> {
    /// The body, or its default when none was sent.
    pub fn unwrap_or_default(self) -> T
    where
        T: Default,
    {
        self.0.unwrap_or_default()
    }
}

impl<T, S> FromRequest<S> for OptionalJson<T>
where
    T: DeserializeOwned + 'static,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(request: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Some(content_type) = request.headers().get(header::CONTENT_TYPE) else {
            // Nothing was sent, rather than something unreadable.
            return Ok(Self(None));
        };

        let content_type = content_type.to_str().unwrap_or_default();
        if !is_json(content_type) {
            return Err(ApiError::unsupported_media_type(format!(
                "this endpoint takes application/json; got {content_type:?}. Send no body at \
                 all to use the defaults."
            )));
        }

        match Json::<T>::from_request(request, state).await {
            Ok(Json(value)) => Ok(Self(Some(value))),
            // `serde`'s message names the offending field and where it was, which is the
            // useful part; the wrapper adds nothing, so the detail is passed through.
            Err(rejection) => Err(ApiError::bad_request(
                "invalid_request_body",
                rejection.body_text(),
            )),
        }
    }
}

/// Whether a `Content-Type` names JSON, ignoring parameters such as `; charset=utf-8` and
/// accepting the `+json` suffix convention.
fn is_json(content_type: &str) -> bool {
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    mime == "application/json" || mime.ends_with("+json")
}

/// Documented exactly as `Option<Json<T>>` would be, since that is what it behaves like
/// when the request is well formed. Delegated rather than restated so the spec cannot
/// describe a body shape the extractor does not accept.
impl<T> aide::OperationInput for OptionalJson<T>
where
    T: DeserializeOwned + schemars::JsonSchema,
{
    fn operation_input(context: &mut GenContext, operation: &mut Operation) {
        <Option<Json<T>> as aide::OperationInput>::operation_input(context, operation);
    }
}

/// Query-string parameters, refused in the envelope rather than in plain text.
///
/// The same problem as [`OptionalJson`] one layer along: `axum::extract::Query` answers a
/// bad query string itself. It matters most for `deny_unknown_fields`, where the refusal a
/// caller sees is the one telling them they misspelled a parameter.
#[derive(Debug)]
pub struct Query<T>(pub T);

impl<T, S> FromRequestParts<S> for Query<T>
where
    T: DeserializeOwned,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        match axum::extract::Query::<T>::from_request_parts(parts, state).await {
            Ok(axum::extract::Query(value)) => Ok(Self(value)),
            // `serde`'s message names the parameter, which is the useful half of it.
            Err(rejection) => Err(ApiError::bad_request(
                "invalid_query_parameter",
                query_detail(&rejection),
            )),
        }
    }
}

/// The readable part of a query rejection.
fn query_detail(rejection: &QueryRejection) -> String {
    rejection.body_text()
}

/// Documented as `axum`'s own would be, so the spec still lists the parameters.
impl<T> aide::OperationInput for Query<T>
where
    T: DeserializeOwned + schemars::JsonSchema,
{
    fn operation_input(context: &mut GenContext, operation: &mut Operation) {
        <axum::extract::Query<T> as aide::OperationInput>::operation_input(context, operation);
    }
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
    #[serde(deny_unknown_fields, default)]
    struct Probe {
        count: Option<u32>,
    }

    async fn extract(
        content_type: Option<&str>,
        body: &'static str,
    ) -> Result<Option<u32>, ApiError> {
        let mut request = HttpRequest::builder().method("POST").uri("/");
        if let Some(content_type) = content_type {
            request = request.header(header::CONTENT_TYPE, content_type);
        }
        let request = request.body(Body::from(body)).expect("request");

        OptionalJson::<Probe>::from_request(request, &())
            .await
            .map(|OptionalJson(parsed)| parsed.and_then(|parsed| parsed.count))
    }

    #[tokio::test]
    async fn no_content_type_means_no_body() {
        assert_eq!(extract(None, "").await.expect("absent is fine"), None);
    }

    #[tokio::test]
    async fn json_is_parsed() {
        assert_eq!(
            extract(Some("application/json"), r#"{"count": 3}"#)
                .await
                .expect("valid"),
            Some(3)
        );
    }

    /// Real clients send parameters, and `application/…+json` is a convention worth
    /// honouring rather than refusing on a technicality.
    #[tokio::test]
    async fn json_with_parameters_or_a_suffix_is_still_json() {
        for content_type in [
            "application/json; charset=utf-8",
            "APPLICATION/JSON",
            "application/merge-patch+json",
        ] {
            extract(Some(content_type), r#"{"count": 1}"#)
                .await
                .unwrap_or_else(|error| {
                    panic!("{content_type} should be accepted: {}", error.message())
                });
        }
    }

    /// What `curl -d '{}'` sends, and the case that started this module.
    #[tokio::test]
    async fn form_encoding_is_refused_in_the_envelope() {
        let error = extract(Some("application/x-www-form-urlencoded"), "{}")
            .await
            .expect_err("a form body is not a JSON body");

        assert_eq!(error.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
        assert_eq!(error.code(), "unsupported_media_type");
        assert!(
            error.message().contains("application/json"),
            "the message should say what to send: {}",
            error.message()
        );
    }

    /// The second hole, found after the first was fixed: a misspelled parameter must be
    /// refused in the envelope, naming what was misspelled.
    #[tokio::test]
    async fn a_bad_query_string_is_refused_in_the_envelope() {
        #[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
        #[serde(deny_unknown_fields, default)]
        struct Params {
            limit: Option<usize>,
        }

        let mut parts = HttpRequest::builder()
            .uri("/?limti=2")
            .body(())
            .expect("request")
            .into_parts()
            .0;

        let error = Query::<Params>::from_request_parts(&mut parts, &())
            .await
            .expect_err("a misspelled parameter must not be ignored");

        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
        assert_eq!(error.code(), "invalid_query_parameter");
        assert!(
            error.message().contains("limti"),
            "the message should name it: {}",
            error.message()
        );
    }

    #[tokio::test]
    async fn a_good_query_string_is_parsed() {
        #[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
        #[serde(deny_unknown_fields, default)]
        struct Params {
            limit: Option<usize>,
        }

        let mut parts = HttpRequest::builder()
            .uri("/?limit=7")
            .body(())
            .expect("request")
            .into_parts()
            .0;

        let Query(params) = Query::<Params>::from_request_parts(&mut parts, &())
            .await
            .expect("valid");

        assert_eq!(params.limit, Some(7));
    }

    #[tokio::test]
    async fn malformed_json_is_a_400_that_explains_itself() {
        let error = extract(Some("application/json"), "{not json")
            .await
            .expect_err("malformed");

        assert_eq!(error.status(), StatusCode::BAD_REQUEST);
        assert_eq!(error.code(), "invalid_request_body");
        assert!(!error.message().is_empty());
    }

    #[tokio::test]
    async fn an_unknown_field_is_a_400_naming_it() {
        let error = extract(Some("application/json"), r#"{"cuont": 3}"#)
            .await
            .expect_err("a typo must not be accepted as a default");

        assert_eq!(error.code(), "invalid_request_body");
        assert!(
            error.message().contains("cuont"),
            "serde names the field, which is the useful part: {}",
            error.message()
        );
    }
}
