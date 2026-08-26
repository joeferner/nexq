//! SigV4 request verification.
//!
//! The client signs; we recompute the same signature from the request we actually
//! received and compare. NexQ is its own trust root, so the secret comes from the
//! configured credential registry rather than from AWS.
//!
//! What the client signed is a *canonical* form of the request:
//!
//! ```text
//! CanonicalRequest = METHOD \n URI \n QUERY \n CanonicalHeaders \n SignedHeaders \n SHA256(body)
//! StringToSign     = AWS4-HMAC-SHA256 \n amz-date \n date/region/service/aws4_request \n SHA256(CanonicalRequest)
//! SigningKey       = HMAC(HMAC(HMAC(HMAC("AWS4"+secret, date), region), service), "aws4_request")
//! Signature        = hex(HMAC(SigningKey, StringToSign))
//! ```
//!
//! The region is whatever the client used: signer and verifier only have to agree on
//! the string, and NexQ has no regions of its own. The service must be `sqs`, matching
//! what this facade serves.
//!
//! Not covered yet: the timestamp is used but not checked for freshness, so a captured
//! request stays replayable. Adding a clock-skew window is the natural next step, and
//! wants a configurable tolerance given air-gapped clocks.

use axum::http::{HeaderMap, Method, Uri, header};
// `KeyInit` provides `new_from_slice`; since hmac 0.13 it has to be imported
// explicitly rather than arriving with `Mac`.
use hmac::{Hmac, KeyInit, Mac};
use nexq_core::{AuthConfig, Credential};
use sha2::{Digest, Sha256};
use tracing::debug;

use crate::error::ApiError;

type HmacSha256 = Hmac<Sha256>;

/// The only signing algorithm accepted.
pub const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// Service name this facade expects in a credential scope.
pub const SERVICE: &str = "sqs";

/// Final element of a credential scope.
const SCOPE_TERMINATOR: &str = "aws4_request";

/// Header carrying the signing timestamp, in `YYYYMMDDTHHMMSSZ` form.
const AMZ_DATE_HEADER: &str = "x-amz-date";

/// When present, this header's value *is* the payload hash used in the canonical
/// request — that is the rule, so a client may declare `UNSIGNED-PAYLOAD` here rather
/// than hashing a body.
const CONTENT_SHA256_HEADER: &str = "x-amz-content-sha256";

/// Everything about a request that the signature covers.
#[derive(Debug, Clone, Copy)]
pub struct SigningContext<'a> {
    pub method: &'a Method,
    pub uri: &'a Uri,
    pub headers: &'a HeaderMap,
    pub body: &'a [u8],
}

/// The `date/region/service/aws4_request` part of a credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialScope {
    pub date: String,
    pub region: String,
    pub service: String,
}

impl CredentialScope {
    fn to_scope_string(&self) -> String {
        format!(
            "{}/{}/{}/{SCOPE_TERMINATOR}",
            self.date, self.region, self.service
        )
    }
}

/// A parsed `Authorization` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorization {
    pub key_id: String,
    pub scope: CredentialScope,
    pub signed_headers: Vec<String>,
    pub signature: String,
}

impl Authorization {
    /// Parse an `Authorization` header value.
    pub fn parse(value: &str) -> Result<Self, ApiError> {
        let fields = value
            .strip_prefix(ALGORITHM)
            .ok_or_else(|| ApiError::incomplete_signature(format!("expected {ALGORITHM}")))?;

        let mut credential = None;
        let mut signed_headers = None;
        let mut signature = None;

        for field in fields.split(',') {
            let field = field.trim();
            if let Some(value) = field.strip_prefix("Credential=") {
                credential = Some(value);
            } else if let Some(value) = field.strip_prefix("SignedHeaders=") {
                signed_headers = Some(value);
            } else if let Some(value) = field.strip_prefix("Signature=") {
                signature = Some(value);
            }
            // Unknown fields are ignored rather than rejected: the header is a
            // comma-separated list and a future addition should not break signing.
        }

        let credential = credential
            .ok_or_else(|| ApiError::incomplete_signature("Authorization has no Credential"))?;
        let signed_headers = signed_headers
            .ok_or_else(|| ApiError::incomplete_signature("Authorization has no SignedHeaders"))?;
        let signature = signature
            .ok_or_else(|| ApiError::incomplete_signature("Authorization has no Signature"))?;

        // key_id/date/region/service/aws4_request. An access key id never contains a
        // slash, so splitting on it is unambiguous.
        let parts: Vec<&str> = credential.split('/').collect();
        let [key_id, date, region, service, SCOPE_TERMINATOR] = parts.as_slice() else {
            return Err(ApiError::incomplete_signature(format!(
                "malformed credential scope: {credential}"
            )));
        };

        if key_id.is_empty() || signature.is_empty() {
            return Err(ApiError::incomplete_signature(
                "credential and signature must not be empty",
            ));
        }

        Ok(Self {
            key_id: (*key_id).to_owned(),
            scope: CredentialScope {
                date: (*date).to_owned(),
                region: (*region).to_owned(),
                service: (*service).to_owned(),
            },
            signed_headers: signed_headers
                .split(';')
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect(),
            signature: signature.to_owned(),
        })
    }
}

/// Verify a request's signature, returning the authenticated principal's name.
///
/// Client-facing messages are deliberately the same ones real SQS sends and carry no
/// detail about why a signature failed; the specifics go to the debug log instead.
pub fn verify(context: &SigningContext<'_>, auth: &AuthConfig) -> Result<String, ApiError> {
    let header = context
        .headers
        .get(header::AUTHORIZATION)
        .ok_or_else(ApiError::missing_authentication_token)?
        .to_str()
        .map_err(|_| ApiError::incomplete_signature("Authorization is not valid ASCII"))?;

    let authorization = Authorization::parse(header)?;

    let Some(credential) = auth.credential(&authorization.key_id) else {
        debug!(key_id = %authorization.key_id, "no such access key");
        return Err(ApiError::invalid_client_token_id());
    };

    if authorization.scope.service != SERVICE {
        debug!(
            service = %authorization.scope.service,
            "credential scoped to the wrong service"
        );
        return Err(ApiError::signature_does_not_match());
    }

    let amz_date = context
        .headers
        .get(AMZ_DATE_HEADER)
        .ok_or_else(|| ApiError::incomplete_signature("x-amz-date header is required"))?
        .to_str()
        .map_err(|_| ApiError::incomplete_signature("x-amz-date is not valid ASCII"))?;

    // The scope's date must be the date half of the timestamp, or the two could
    // disagree while both verifying.
    if !amz_date.starts_with(&authorization.scope.date) {
        debug!(
            amz_date,
            scope_date = %authorization.scope.date,
            "credential scope date does not match x-amz-date"
        );
        return Err(ApiError::signature_does_not_match());
    }

    let expected = signature(context, &authorization, amz_date, credential)?;
    let provided = hex::decode(&authorization.signature)
        .map_err(|_| ApiError::incomplete_signature("signature is not hex"))?;

    // Constant-time comparison, courtesy of `Mac::verify_slice`.
    expected.verify_slice(&provided).map_err(|_| {
        debug!(key_id = %authorization.key_id, "signature mismatch");
        ApiError::signature_does_not_match()
    })?;

    Ok(credential.name.clone())
}

/// Compute the signature for a request, as a MAC ready to be compared or finalized.
fn signature(
    context: &SigningContext<'_>,
    authorization: &Authorization,
    amz_date: &str,
    credential: &Credential,
) -> Result<HmacSha256, ApiError> {
    let canonical = canonical_request(context, &authorization.signed_headers)?;
    let to_sign = format!(
        "{ALGORITHM}\n{amz_date}\n{}\n{}",
        authorization.scope.to_scope_string(),
        hex::encode(Sha256::digest(canonical.as_bytes()))
    );

    let key = signing_key(credential.secret.expose(), &authorization.scope);
    let mut mac = hmac(&key);
    mac.update(to_sign.as_bytes());

    Ok(mac)
}

/// Produce the hex signature a client would send. The verification path recomputes the
/// same value; this is the form a caller needs when it has to *sign* a request, such as
/// a test, or a node forwarding a request onward.
pub fn sign(
    context: &SigningContext<'_>,
    authorization: &Authorization,
    amz_date: &str,
    credential: &Credential,
) -> Result<String, ApiError> {
    Ok(hex::encode(
        signature(context, authorization, amz_date, credential)?
            .finalize()
            .into_bytes(),
    ))
}

/// Rebuild the canonical form of the request the client signed.
fn canonical_request(
    context: &SigningContext<'_>,
    signed_headers: &[String],
) -> Result<String, ApiError> {
    let mut canonical = String::new();

    canonical.push_str(context.method.as_str());
    canonical.push('\n');
    canonical.push_str(canonical_uri(context.uri));
    canonical.push('\n');
    canonical.push_str(&canonical_query(context.uri.query()));
    canonical.push('\n');

    for name in signed_headers {
        canonical.push_str(name);
        canonical.push(':');
        canonical.push_str(&canonical_header_value(context.headers, name)?);
        canonical.push('\n');
    }

    canonical.push('\n');
    canonical.push_str(&signed_headers.join(";"));
    canonical.push('\n');
    canonical.push_str(&payload_hash(context));

    Ok(canonical)
}

/// The path, as the client sent it.
///
/// It arrives already percent-encoded, and SQS's JSON protocol only ever uses `/`, so
/// it is used verbatim. Services other than S3 are supposed to encode each path
/// segment twice; that only matters for paths this facade does not serve.
fn canonical_uri(uri: &Uri) -> &str {
    match uri.path() {
        "" => "/",
        path => path,
    }
}

/// Query parameters sorted by name, then value.
///
/// Names and values are used as the client encoded them, since re-encoding could
/// disagree with what was signed.
fn canonical_query(query: Option<&str>) -> String {
    let Some(query) = query.filter(|query| !query.is_empty()) else {
        return String::new();
    };

    let mut pairs: Vec<(&str, &str)> = query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| pair.split_once('=').unwrap_or((pair, "")))
        .collect();
    pairs.sort_unstable();

    pairs
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// A signed header's value: trimmed, with runs of whitespace collapsed, and repeated
/// headers joined by commas.
fn canonical_header_value(headers: &HeaderMap, name: &str) -> Result<String, ApiError> {
    let mut values = Vec::new();

    for value in headers.get_all(name) {
        let value = value
            .to_str()
            .map_err(|_| ApiError::incomplete_signature(format!("{name} is not valid ASCII")))?;
        values.push(value.split_whitespace().collect::<Vec<_>>().join(" "));
    }

    if values.is_empty() {
        // Signed but absent: the canonical request cannot be rebuilt. `host` shows up
        // here if a client somehow omits it.
        return Err(ApiError::incomplete_signature(format!(
            "signed header {name} is missing from the request"
        )));
    }

    Ok(values.join(","))
}

/// Hash of the body, or the hash the client declared.
fn payload_hash(context: &SigningContext<'_>) -> String {
    if let Some(declared) = context
        .headers
        .get(CONTENT_SHA256_HEADER)
        .and_then(|value| value.to_str().ok())
    {
        return declared.to_owned();
    }

    hex::encode(Sha256::digest(context.body))
}

/// Derive the scope-specific signing key: one HMAC per scope element, so a leaked key
/// is only good for that date, region, and service.
fn signing_key(secret: &str, scope: &CredentialScope) -> Vec<u8> {
    let mut key = format!("AWS4{secret}").into_bytes();

    for element in [
        scope.date.as_str(),
        scope.region.as_str(),
        scope.service.as_str(),
        SCOPE_TERMINATOR,
    ] {
        let mut mac = hmac(&key);
        mac.update(element.as_bytes());
        key = mac.finalize().into_bytes().to_vec();
    }

    key
}

fn hmac(key: &[u8]) -> HmacSha256 {
    // HMAC accepts a key of any length, so this cannot fail.
    HmacSha256::new_from_slice(key).expect("HMAC accepts keys of any size")
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;
    use nexq_core::Secret;

    use super::*;

    /// A request `aws-cli` 2.36.30 really sent, captured with `--debug`, together with
    /// the signature botocore computed for it. Reproducing this signature is what
    /// proves the implementation agrees with real AWS tooling rather than only with
    /// itself.
    mod captured {
        pub const SECRET: &str = "change-me";
        pub const KEY_ID: &str = "AKIANEXQDEV";
        pub const AMZ_DATE: &str = "20260826T005924Z";
        pub const HOST: &str = "127.0.0.1:18100";
        pub const BODY: &[u8] = b"{}";
        pub const AUTHORIZATION: &str = "AWS4-HMAC-SHA256 \
            Credential=AKIANEXQDEV/20260826/us-east-1/sqs/aws4_request, \
            SignedHeaders=content-type;host;x-amz-date;x-amz-target;x-amzn-query-mode, \
            Signature=3ab471d595641719c34224df0512225be69ddb1d98d792feb3e09bc5f95c6a7f";
        pub const SIGNATURE: &str =
            "3ab471d595641719c34224df0512225be69ddb1d98d792feb3e09bc5f95c6a7f";
    }

    fn captured_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static(captured::AUTHORIZATION),
        );
        headers.insert(header::HOST, HeaderValue::from_static(captured::HOST));
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/x-amz-json-1.0"),
        );
        headers.insert(
            AMZ_DATE_HEADER,
            HeaderValue::from_static(captured::AMZ_DATE),
        );
        headers.insert(
            "x-amz-target",
            HeaderValue::from_static("AmazonSQS.ListQueues"),
        );
        headers.insert("x-amzn-query-mode", HeaderValue::from_static("true"));
        headers
    }

    fn credential(key_id: &str, secret: &str) -> Credential {
        Credential {
            name: "dev".to_owned(),
            key_id: key_id.to_owned(),
            secret: Secret::new(secret),
        }
    }

    fn auth_config(credential: Credential) -> AuthConfig {
        AuthConfig {
            credentials: vec![credential],
        }
    }

    #[test]
    fn reproduces_a_signature_that_botocore_computed() {
        let headers = captured_headers();
        let uri: Uri = "/".parse().expect("uri");
        let context = SigningContext {
            method: &Method::POST,
            uri: &uri,
            headers: &headers,
            body: captured::BODY,
        };
        let authorization =
            Authorization::parse(captured::AUTHORIZATION).expect("parse authorization");

        let computed = sign(
            &context,
            &authorization,
            captured::AMZ_DATE,
            &credential(captured::KEY_ID, captured::SECRET),
        )
        .expect("sign");

        assert_eq!(computed, captured::SIGNATURE);
    }

    #[test]
    fn accepts_the_captured_request() {
        let headers = captured_headers();
        let uri: Uri = "/".parse().expect("uri");
        let context = SigningContext {
            method: &Method::POST,
            uri: &uri,
            headers: &headers,
            body: captured::BODY,
        };

        let principal = verify(
            &context,
            &auth_config(credential(captured::KEY_ID, captured::SECRET)),
        )
        .expect("verify");

        assert_eq!(principal, "dev");
    }

    #[test]
    fn rejects_the_wrong_secret() {
        let headers = captured_headers();
        let uri: Uri = "/".parse().expect("uri");
        let context = SigningContext {
            method: &Method::POST,
            uri: &uri,
            headers: &headers,
            body: captured::BODY,
        };

        let error = verify(
            &context,
            &auth_config(credential(captured::KEY_ID, "not-the-secret")),
        )
        .expect_err("wrong secret");

        assert_eq!(error.code(), "SignatureDoesNotMatch");
    }

    #[test]
    fn rejects_an_unknown_key_id() {
        let headers = captured_headers();
        let uri: Uri = "/".parse().expect("uri");
        let context = SigningContext {
            method: &Method::POST,
            uri: &uri,
            headers: &headers,
            body: captured::BODY,
        };

        let error = verify(
            &context,
            &auth_config(credential("AKIASOMEONEELSE", captured::SECRET)),
        )
        .expect_err("unknown key");

        assert_eq!(error.code(), "InvalidClientTokenId");
    }

    #[test]
    fn rejects_a_tampered_body() {
        let headers = captured_headers();
        let uri: Uri = "/".parse().expect("uri");
        let context = SigningContext {
            method: &Method::POST,
            uri: &uri,
            headers: &headers,
            // Same signature, different payload: the body is covered by the signature.
            body: b"{\"QueueNamePrefix\":\"x\"}",
        };

        let error = verify(
            &context,
            &auth_config(credential(captured::KEY_ID, captured::SECRET)),
        )
        .expect_err("tampered body");

        assert_eq!(error.code(), "SignatureDoesNotMatch");
    }

    #[test]
    fn rejects_a_tampered_header() {
        let mut headers = captured_headers();
        // A signed header, so changing the operation invalidates the signature.
        headers.insert(
            "x-amz-target",
            HeaderValue::from_static("AmazonSQS.PurgeQueue"),
        );
        let uri: Uri = "/".parse().expect("uri");
        let context = SigningContext {
            method: &Method::POST,
            uri: &uri,
            headers: &headers,
            body: captured::BODY,
        };

        let error = verify(
            &context,
            &auth_config(credential(captured::KEY_ID, captured::SECRET)),
        )
        .expect_err("tampered header");

        assert_eq!(error.code(), "SignatureDoesNotMatch");
    }

    #[test]
    fn requires_an_authorization_header() {
        let headers = HeaderMap::new();
        let uri: Uri = "/".parse().expect("uri");
        let context = SigningContext {
            method: &Method::POST,
            uri: &uri,
            headers: &headers,
            body: b"{}",
        };

        let error = verify(
            &context,
            &auth_config(credential(captured::KEY_ID, captured::SECRET)),
        )
        .expect_err("unsigned");

        assert_eq!(error.code(), "MissingAuthenticationToken");
    }

    #[test]
    fn parses_a_credential_scope() {
        let authorization =
            Authorization::parse(captured::AUTHORIZATION).expect("parse authorization");

        assert_eq!(authorization.key_id, captured::KEY_ID);
        assert_eq!(authorization.scope.date, "20260826");
        assert_eq!(authorization.scope.region, "us-east-1");
        assert_eq!(authorization.scope.service, SERVICE);
        assert_eq!(authorization.signature, captured::SIGNATURE);
        assert_eq!(
            authorization.signed_headers,
            [
                "content-type",
                "host",
                "x-amz-date",
                "x-amz-target",
                "x-amzn-query-mode"
            ]
        );
    }

    #[test]
    fn rejects_a_malformed_authorization_header() {
        for header in [
            "Basic dXNlcjpwYXNz",
            "AWS4-HMAC-SHA256 SignedHeaders=host, Signature=abc",
            "AWS4-HMAC-SHA256 Credential=key/20260826/us-east-1/sqs, SignedHeaders=host, Signature=abc",
            "AWS4-HMAC-SHA256 Credential=key/20260826/us-east-1/sqs/aws4_request, Signature=abc",
            "AWS4-HMAC-SHA256 Credential=key/20260826/us-east-1/sqs/aws4_request, SignedHeaders=host",
        ] {
            let error = Authorization::parse(header).expect_err(header);
            assert_eq!(error.code(), "IncompleteSignature", "{header}");
        }
    }

    #[test]
    fn a_query_string_is_canonicalized_by_sorting() {
        assert_eq!(canonical_query(None), "");
        assert_eq!(canonical_query(Some("")), "");
        assert_eq!(canonical_query(Some("b=2&a=1")), "a=1&b=2");
        assert_eq!(canonical_query(Some("a=2&a=1")), "a=1&a=2");
        assert_eq!(canonical_query(Some("flag")), "flag=");
    }

    #[test]
    fn a_header_value_is_trimmed_and_its_whitespace_collapsed() {
        let mut headers = HeaderMap::new();
        headers.insert("x-test", HeaderValue::from_static("  spaced   out  "));

        assert_eq!(
            canonical_header_value(&headers, "x-test").expect("value"),
            "spaced out"
        );
    }

    #[test]
    fn repeated_headers_are_joined() {
        let mut headers = HeaderMap::new();
        headers.append("x-test", HeaderValue::from_static("one"));
        headers.append("x-test", HeaderValue::from_static("two"));

        assert_eq!(
            canonical_header_value(&headers, "x-test").expect("value"),
            "one,two"
        );
    }

    #[test]
    fn a_signed_but_absent_header_is_an_incomplete_signature() {
        let headers = HeaderMap::new();
        let error = canonical_header_value(&headers, "host").expect_err("missing");

        assert_eq!(error.code(), "IncompleteSignature");
    }

    #[test]
    fn a_declared_payload_hash_is_used_verbatim() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_SHA256_HEADER,
            HeaderValue::from_static("UNSIGNED-PAYLOAD"),
        );
        let uri: Uri = "/".parse().expect("uri");

        let hash = payload_hash(&SigningContext {
            method: &Method::POST,
            uri: &uri,
            headers: &headers,
            body: b"ignored",
        });

        assert_eq!(hash, "UNSIGNED-PAYLOAD");
    }
}
