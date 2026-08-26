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
//! The timestamp is checked for freshness as well as signed, within a configurable
//! window — that is what stops a captured request being replayed indefinitely. The
//! window is configurable because an air-gapped deployment with no NTP can drift far
//! enough to refuse honest clients, and an operator has to be able to widen it.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

/// Seconds in a day, for turning a civil date into an instant.
const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

/// Parse a SigV4 timestamp: `YYYYMMDDTHHMMSSZ`, always UTC.
///
/// Strict about the shape, because a timestamp that parses loosely could be read
/// differently by the signer and the verifier, and the whole point is that both agree.
fn parse_amz_date(value: &str) -> Option<SystemTime> {
    let bytes = value.as_bytes();
    if bytes.len() != 16 || bytes[8] != b'T' || bytes[15] != b'Z' {
        return None;
    }

    let number = |range: std::ops::Range<usize>| -> Option<i64> {
        let text = value.get(range)?;
        if !text.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        text.parse().ok()
    };

    let (year, month, day) = (number(0..4)?, number(4..6)?, number(6..8)?);
    let (hour, minute, second) = (number(9..11)?, number(11..13)?, number(13..15)?);

    if !(1..=12).contains(&month)
        || day < 1
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }

    let seconds =
        days_from_civil(year, month, day) * SECONDS_PER_DAY + hour * 3600 + minute * 60 + second;

    Some(if seconds >= 0 {
        UNIX_EPOCH + Duration::from_secs(seconds.unsigned_abs())
    } else {
        UNIX_EPOCH - Duration::from_secs(seconds.unsigned_abs())
    })
}

fn is_leap_year(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

/// Days from 1970-01-01 to a civil date, by Howard Hinnant's `days_from_civil`.
///
/// Exact for the whole proleptic Gregorian calendar, which is why it is used here
/// rather than an approximation with leap-year special cases bolted on.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    // March-based year, so a leap day lands at the end and needs no special case.
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;

    // 719468 shifts the epoch from 0000-03-01 to 1970-01-01.
    era * 146_097 + day_of_era - 719_468
}

/// Reject a timestamp too far from this server's clock in either direction.
///
/// Both directions matter: a timestamp far in the past is a replayed request, and one
/// far in the future would otherwise stay replayable for as long as it takes the clock
/// to catch up.
fn check_skew(signed_at: SystemTime, now: SystemTime, tolerance: Duration) -> Result<(), ApiError> {
    let drift = now
        .duration_since(signed_at)
        .or_else(|_| signed_at.duration_since(now))
        .unwrap_or_default();

    if drift > tolerance {
        debug!(
            drift_secs = drift.as_secs(),
            tolerance_secs = tolerance.as_secs(),
            "request timestamp is outside the accepted window"
        );
        return Err(ApiError::request_time_too_skewed(drift, tolerance));
    }

    Ok(())
}

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
/// `max_clock_skew` bounds how old a request may be; `None` accepts any timestamp,
/// which leaves a captured request replayable forever.
///
/// Client-facing messages are deliberately the same ones real SQS sends and carry no
/// detail about why a signature failed; the specifics go to the debug log instead.
pub fn verify(
    context: &SigningContext<'_>,
    auth: &AuthConfig,
    max_clock_skew: Option<Duration>,
) -> Result<String, ApiError> {
    verify_at(context, auth, max_clock_skew, SystemTime::now())
}

/// [`verify`] against a given notion of "now".
///
/// Split out so the skew window can be tested without waiting for real time to pass or
/// depending on when the test happens to run.
fn verify_at(
    context: &SigningContext<'_>,
    auth: &AuthConfig,
    max_clock_skew: Option<Duration>,
    now: SystemTime,
) -> Result<String, ApiError> {
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

    // Checked before the signature is recomputed: a stale request is refused whether or
    // not it was signed correctly, and this costs no HMAC work.
    if let Some(tolerance) = max_clock_skew {
        let signed_at = parse_amz_date(amz_date)
            .ok_or_else(|| ApiError::incomplete_signature("x-amz-date is not a valid timestamp"))?;

        check_skew(signed_at, now, tolerance)?;
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
    pub(super) mod captured {
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

    pub(super) fn captured_headers() -> HeaderMap {
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

    /// The captured request has a fixed timestamp, so cases about *signing* switch the
    /// freshness check off. The window has its own cases, further down.
    const SKEW_DISABLED: Option<Duration> = None;

    /// When the captured request was signed, as an instant.
    pub(super) fn captured_signing_time() -> SystemTime {
        parse_amz_date(captured::AMZ_DATE).expect("the captured timestamp should parse")
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
            SKEW_DISABLED,
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
            SKEW_DISABLED,
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
            SKEW_DISABLED,
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
            SKEW_DISABLED,
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
            SKEW_DISABLED,
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
            SKEW_DISABLED,
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

#[cfg(test)]
mod clock_skew_tests {
    use axum::http::HeaderValue;
    use nexq_core::Secret;

    use super::tests::{captured, captured_headers, captured_signing_time};
    use super::*;

    /// The window used by these cases, standing in for whatever config says.
    const TOLERANCE: Duration = Duration::from_secs(15 * 60);

    fn verify_captured_at(now: SystemTime) -> Result<String, ApiError> {
        let headers = captured_headers();
        let uri: Uri = "/".parse().expect("uri");
        let context = SigningContext {
            method: &Method::POST,
            uri: &uri,
            headers: &headers,
            body: captured::BODY,
        };

        verify_at(
            &context,
            &AuthConfig {
                credentials: vec![Credential {
                    name: "dev".to_owned(),
                    key_id: captured::KEY_ID.to_owned(),
                    secret: Secret::new(captured::SECRET),
                }],
            },
            Some(TOLERANCE),
            now,
        )
    }

    #[test]
    fn a_fresh_request_is_accepted() {
        let signed_at = captured_signing_time();

        for offset in [
            Duration::ZERO,
            Duration::from_secs(1),
            Duration::from_secs(14 * 60),
            TOLERANCE,
        ] {
            verify_captured_at(signed_at + offset)
                .unwrap_or_else(|error| panic!("{offset:?} after signing: {error}"));
        }
    }

    #[test]
    fn a_stale_request_is_refused() {
        // The replay case: a request captured off the wire and sent again later.
        let error =
            verify_captured_at(captured_signing_time() + TOLERANCE + Duration::from_secs(1))
                .expect_err("outside the window");

        assert_eq!(error.code(), "RequestTimeTooSkewed");
    }

    #[test]
    fn a_request_from_the_future_is_refused() {
        // Otherwise a request signed with a fast clock stays replayable until real time
        // catches up with its timestamp.
        let error =
            verify_captured_at(captured_signing_time() - TOLERANCE - Duration::from_secs(1))
                .expect_err("too far ahead");

        assert_eq!(error.code(), "RequestTimeTooSkewed");
    }

    #[test]
    fn the_refusal_says_how_far_off_the_clock_is() {
        // The usual cause is a clock that needs setting, which a bare refusal does not
        // help anyone work out.
        let error = verify_captured_at(captured_signing_time() + Duration::from_secs(3600))
            .expect_err("stale");

        assert!(error.message().contains("3600"), "{}", error.message());
        assert!(error.message().contains("900"), "{}", error.message());
    }

    #[test]
    fn skew_is_checked_before_the_signature_is_recomputed() {
        // A stale request is refused whether or not it was signed correctly, and
        // without spending HMAC work on it.
        let headers = captured_headers();
        let uri: Uri = "/".parse().expect("uri");
        let context = SigningContext {
            method: &Method::POST,
            uri: &uri,
            headers: &headers,
            // Tampered, so the signature cannot possibly match either.
            body: b"{\"tampered\":true}",
        };

        let error = verify_at(
            &context,
            &AuthConfig {
                credentials: vec![Credential {
                    name: "dev".to_owned(),
                    key_id: captured::KEY_ID.to_owned(),
                    secret: Secret::new(captured::SECRET),
                }],
            },
            Some(TOLERANCE),
            captured_signing_time() + Duration::from_secs(3600),
        )
        .expect_err("stale and tampered");

        assert_eq!(error.code(), "RequestTimeTooSkewed");
    }

    #[test]
    fn no_window_accepts_any_timestamp() {
        // What `max_clock_skew_secs = 0` buys, and what it costs.
        verify_captured_at_without_window(
            captured_signing_time() + Duration::from_secs(86_400 * 365),
        )
        .expect("with no window, age does not matter");
    }

    fn verify_captured_at_without_window(now: SystemTime) -> Result<String, ApiError> {
        let headers = captured_headers();
        let uri: Uri = "/".parse().expect("uri");
        let context = SigningContext {
            method: &Method::POST,
            uri: &uri,
            headers: &headers,
            body: captured::BODY,
        };

        verify_at(
            &context,
            &AuthConfig {
                credentials: vec![Credential {
                    name: "dev".to_owned(),
                    key_id: captured::KEY_ID.to_owned(),
                    secret: Secret::new(captured::SECRET),
                }],
            },
            None,
            now,
        )
    }

    #[test]
    fn a_timestamp_that_contradicts_the_credential_scope_is_a_signature_mismatch() {
        // Caught before the window is even consulted: the scope says one date and the
        // header says another, so the two cannot both have been signed.
        let mut headers = captured_headers();
        headers.insert(AMZ_DATE_HEADER, HeaderValue::from_static("yesterday"));
        let uri: Uri = "/".parse().expect("uri");
        let context = SigningContext {
            method: &Method::POST,
            uri: &uri,
            headers: &headers,
            body: captured::BODY,
        };

        let error = verify_at(
            &context,
            &AuthConfig {
                credentials: vec![Credential {
                    name: "dev".to_owned(),
                    key_id: captured::KEY_ID.to_owned(),
                    secret: Secret::new(captured::SECRET),
                }],
            },
            Some(TOLERANCE),
            SystemTime::now(),
        )
        .expect_err("date disagrees with the scope");

        assert_eq!(error.code(), "SignatureDoesNotMatch");
    }

    #[test]
    fn an_unparseable_timestamp_is_refused_when_a_window_is_set() {
        // Agrees with the credential scope's date, so it gets as far as being parsed —
        // and then cannot be, so there is no way to tell whether it is fresh.
        let mut headers = captured_headers();
        headers.insert(
            AMZ_DATE_HEADER,
            HeaderValue::from_static("20260826T0059XXZ"),
        );
        let uri: Uri = "/".parse().expect("uri");
        let context = SigningContext {
            method: &Method::POST,
            uri: &uri,
            headers: &headers,
            body: captured::BODY,
        };

        let error = verify_at(
            &context,
            &AuthConfig {
                credentials: vec![Credential {
                    name: "dev".to_owned(),
                    key_id: captured::KEY_ID.to_owned(),
                    secret: Secret::new(captured::SECRET),
                }],
            },
            Some(TOLERANCE),
            SystemTime::now(),
        )
        .expect_err("not a timestamp");

        assert_eq!(error.code(), "IncompleteSignature");
    }
}

#[cfg(test)]
mod timestamp_tests {
    use super::*;

    /// Epoch seconds for each timestamp, computed independently rather than by running
    /// this code — otherwise the test only proves the parser agrees with itself.
    const KNOWN: &[(&str, i64)] = &[
        ("19700101T000000Z", 0),
        ("20260826T005924Z", 1_787_705_964),
        ("20000229T120000Z", 951_825_600), // leap day, leap century
        ("21000301T000000Z", 4_107_542_400), // 2100 is not a leap year
        ("19991231T235959Z", 946_684_799),
        ("20240229T235959Z", 1_709_251_199),
        ("19691231T235959Z", -1), // before the epoch
    ];

    #[test]
    fn parses_timestamps_to_the_right_instant() {
        for (text, epoch_seconds) in KNOWN {
            let parsed = parse_amz_date(text).unwrap_or_else(|| panic!("{text} should parse"));

            let expected = if *epoch_seconds >= 0 {
                UNIX_EPOCH + Duration::from_secs(epoch_seconds.unsigned_abs())
            } else {
                UNIX_EPOCH - Duration::from_secs(epoch_seconds.unsigned_abs())
            };
            assert_eq!(parsed, expected, "{text}");
        }
    }

    #[test]
    fn refuses_anything_that_is_not_the_sigv4_shape() {
        for text in [
            "",
            "20260826",
            "20260826T005924",      // no trailing Z
            "20260826T005924z",     // lowercase
            "2026-08-26T00:59:24Z", // RFC 3339 rather than the basic format
            "20260826 005924Z",     // space instead of T
            "20260826T005924Z ",    // trailing space
            "+0260826T005924Z",     // sign where a digit belongs
        ] {
            assert!(parse_amz_date(text).is_none(), "{text:?} should not parse");
        }
    }

    #[test]
    fn refuses_dates_that_do_not_exist() {
        // Silently shifting Feb 30 to Mar 2 would mean signer and verifier disagree
        // about which instant was signed.
        for text in [
            "20260230T000000Z", // February 30
            "20260229T000000Z", // 2026 is not a leap year
            "21000229T000000Z", // nor is 2100
            "20260001T000000Z", // month zero
            "20261301T000000Z", // month thirteen
            "20260800T000000Z", // day zero
            "20260832T000000Z", // day thirty-two
            "20260826T240000Z", // hour twenty-four
            "20260826T006000Z", // minute sixty
            "20260826T005960Z", // second sixty
        ] {
            assert!(parse_amz_date(text).is_none(), "{text:?} should not parse");
        }
    }

    #[test]
    fn accepts_the_boundaries_that_do_exist() {
        for text in [
            "20240229T000000Z", // leap day in a leap year
            "20000229T000000Z", // leap day in a leap century
            "20261231T235959Z", // last second of a year
            "20260101T000000Z", // first second of a year
        ] {
            assert!(parse_amz_date(text).is_some(), "{text:?} should parse");
        }
    }

    #[test]
    fn drift_is_measured_in_both_directions() {
        let tolerance = Duration::from_secs(60);
        let now = UNIX_EPOCH + Duration::from_secs(1_000_000);

        check_skew(now, now, tolerance).expect("no drift");
        check_skew(now - tolerance, now, tolerance).expect("exactly at the limit, behind");
        check_skew(now + tolerance, now, tolerance).expect("exactly at the limit, ahead");

        check_skew(now - tolerance - Duration::from_secs(1), now, tolerance)
            .expect_err("too far behind");
        check_skew(now + tolerance + Duration::from_secs(1), now, tolerance)
            .expect_err("too far ahead");
    }
}
