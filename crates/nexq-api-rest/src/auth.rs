//! Bearer-token authentication.
//!
//! REST presents the same credentials the SQS facade does, in the way its own protocol
//! expects: one opaque string rather than a signing procedure. The token is
//! `<key_id>.<secret>` — see [`nexq_core::Credential::bearer_token`] — so there is
//! nothing extra to issue, rotate, or leak, and one registry serves both facades.
//!
//! What this is *not* is a substitute for TLS. The token is presented in full on every
//! request, so anyone who can read the traffic can replay it — unlike SigV4, where a
//! signature covers one request and the secret never crosses the wire. That is the trade
//! Q10b makes for not asking REST clients to implement a signing procedure, and it is why
//! `[rest_api.tls]` exists.

use axum::http::{HeaderMap, header};
use nexq_core::{AuthConfig, BEARER_TOKEN_SEPARATOR, Credential};

use crate::error::ApiError;

/// The `Bearer` scheme name, matched case-insensitively as RFC 9110 requires.
const SCHEME: &str = "bearer";

/// Check the `Authorization` header, returning the principal's name.
///
/// The name is for logs and metrics only. Every authenticated principal can currently do
/// everything, so nothing branches on *who* this is — see the authorization item in
/// `todo.md`, which is the reason per-principal keys are the recommended posture already.
pub fn authenticate(headers: &HeaderMap, auth: &AuthConfig) -> Result<String, ApiError> {
    let token = bearer_token(headers).ok_or_else(ApiError::unauthorized)?;

    // The identifier comes first so it can be split off and looked up; a secret
    // containing the separator still works, since only the first one splits.
    let (key_id, secret) = token
        .split_once(BEARER_TOKEN_SEPARATOR)
        .ok_or_else(ApiError::unauthorized)?;

    let credential = auth.credential(key_id).ok_or_else(ApiError::unauthorized)?;

    if !secret_matches(credential, secret) {
        return Err(ApiError::unauthorized());
    }

    Ok(credential.name.clone())
}

/// The token from an `Authorization: Bearer <token>` header.
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;

    if !scheme.eq_ignore_ascii_case(SCHEME) {
        return None;
    }

    // A header arrives as `Bearer <token>`, but tolerating extra space costs nothing and
    // an empty token must not compare equal to an empty secret.
    let token = token.trim();
    (!token.is_empty()).then_some(token)
}

/// Compare a presented secret against a stored one without returning early.
///
/// A `==` on strings stops at the first differing byte, which makes the time it takes
/// depend on how much of the secret was right — enough, in principle, to recover one
/// byte at a time. This compares every byte regardless.
///
/// Honest about its limits: the *length* is still compared first and short-circuits, and
/// a sufficiently determined optimizer could in theory undo the loop. Both are the usual
/// trade for not taking a dependency on a crate that pins the guarantee down. The length
/// of a secret is not the secret, and this removes the leak that actually matters.
fn secret_matches(credential: &Credential, presented: &str) -> bool {
    let expected = credential.secret.expose().as_bytes();
    let presented = presented.as_bytes();

    if expected.len() != presented.len() {
        return false;
    }

    let mut difference = 0u8;
    for (left, right) in expected.iter().zip(presented) {
        difference |= left ^ right;
    }

    difference == 0
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;
    use nexq_core::Secret;

    use super::*;

    const KEY_ID: &str = "AKIATESTKEY";
    const SECRET: &str = "test-secret";

    fn registry() -> AuthConfig {
        AuthConfig {
            credentials: vec![Credential {
                name: "dev".to_owned(),
                key_id: KEY_ID.to_owned(),
                secret: Secret::new(SECRET),
            }],
        }
    }

    fn headers_with(authorization: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(authorization).expect("header value"),
        );
        headers
    }

    #[test]
    fn a_correct_token_names_the_principal() {
        let token = registry().credentials[0].bearer_token();

        let principal = authenticate(&headers_with(&format!("Bearer {token}")), &registry())
            .expect("the registry's own token must be accepted");

        assert_eq!(principal, "dev");
    }

    /// RFC 9110 makes the scheme case-insensitive, and clients do vary.
    #[test]
    fn the_scheme_is_case_insensitive() {
        let token = registry().credentials[0].bearer_token();

        for scheme in ["Bearer", "bearer", "BEARER"] {
            authenticate(&headers_with(&format!("{scheme} {token}")), &registry())
                .unwrap_or_else(|_| panic!("{scheme} should be accepted"));
        }
    }

    #[test]
    fn a_wrong_secret_is_refused() {
        let error = authenticate(
            &headers_with(&format!("Bearer {KEY_ID}.wrong")),
            &registry(),
        )
        .expect_err("a wrong secret must not authenticate");

        assert_eq!(error.code(), "unauthorized");
    }

    /// An unknown key id and a wrong secret must be indistinguishable, so a caller
    /// cannot enumerate which key ids exist.
    #[test]
    fn an_unknown_key_id_is_refused_the_same_way_as_a_wrong_secret() {
        let unknown = authenticate(
            &headers_with("Bearer AKIANOSUCHKEY.test-secret"),
            &registry(),
        )
        .expect_err("unknown key id");
        let wrong = authenticate(
            &headers_with(&format!("Bearer {KEY_ID}.wrong-secret")),
            &registry(),
        )
        .expect_err("wrong secret");

        assert_eq!(unknown.code(), wrong.code());
        assert_eq!(unknown.status(), wrong.status());
        assert_eq!(
            unknown.message(),
            wrong.message(),
            "the two must not be distinguishable"
        );
    }

    #[test]
    fn a_malformed_header_is_refused() {
        for value in [
            "",
            "Bearer",
            "Bearer ",
            // No separator, so there is no key id to look up.
            "Bearer justonestring",
            // The right token under the wrong scheme.
            "Basic AKIATESTKEY.test-secret",
            // Empty halves must not match an empty anything.
            "Bearer .",
        ] {
            let headers = HeaderValue::from_str(value)
                .map(|header| {
                    let mut headers = HeaderMap::new();
                    headers.insert(header::AUTHORIZATION, header);
                    headers
                })
                .expect("header value");

            authenticate(&headers, &registry())
                .expect_err(&format!("{value:?} must not authenticate"));
        }
    }

    #[test]
    fn no_authorization_header_is_refused() {
        authenticate(&HeaderMap::new(), &registry()).expect_err("anonymous must be refused");
    }

    #[test]
    fn a_secret_containing_the_separator_still_works() {
        let auth = AuthConfig {
            credentials: vec![Credential {
                name: "dotted".to_owned(),
                key_id: KEY_ID.to_owned(),
                secret: Secret::new("has.dots.in.it"),
            }],
        };
        let token = auth.credentials[0].bearer_token();

        assert_eq!(
            authenticate(&headers_with(&format!("Bearer {token}")), &auth).expect("accepted"),
            "dotted"
        );
    }

    #[test]
    fn a_secret_of_a_different_length_does_not_match() {
        let credential = &registry().credentials[0];

        assert!(!secret_matches(credential, ""));
        assert!(!secret_matches(credential, "test-secretx"));
        assert!(!secret_matches(credential, "test-secre"));
        assert!(secret_matches(credential, SECRET));
    }
}
