//! A browsable documentation page for the generated OpenAPI document.
//!
//! [Scalar](https://github.com/scalar/scalar), served **entirely from this binary**. Every
//! documented way to use it — including `aide`'s own `scalar` feature — renders a page that
//! pulls its JavaScript from a CDN, which is exactly what the deployments NexQ targets
//! cannot do (plan Q21). A docs page that is blank in an air-gapped environment would be
//! worse than none, so the bundle is vendored: see
//! [`assets/scalar/PROVENANCE.md`](../../../assets/scalar/PROVENANCE.md) for its version,
//! licence, and how to refresh it.
//!
//! Three routes, all unauthenticated like the spec they render: it describes the shape of
//! the API and carries nothing deployment-specific — no queue names, no messages — and a
//! login wall in front of documentation is a login wall in front of nothing.
//!
//! The cost is honest and worth naming: the bundle is 3.8 MB of the binary. It lives in
//! read-only data, so it is paged in only when somebody opens the page, and conditional
//! requests mean a reload costs a 304 rather than 3.8 MB again.

use axum::Router;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;

/// The page shell. Substituted and served as-is.
const DOCS_HTML: &str = include_str!("../assets/docs.html");

/// Starts the bundle against this server's document.
const DOCS_BOOTSTRAP: &str = include_str!("../assets/docs-bootstrap.js");

/// The vendored Scalar build. See `assets/scalar/PROVENANCE.md`.
const SCALAR_BUNDLE: &[u8] = include_bytes!("../assets/scalar/standalone.js");

/// Replaced with the API prefix when the router is built, so the assets cannot name a path
/// the router does not serve.
const PREFIX_PLACEHOLDER: &str = "{{PREFIX}}";

/// What the browser is allowed to do on this page.
///
/// `connect-src 'self'` and `font-src` are the two doing the security work: between them
/// they mean this page cannot send a request — or a bearer token typed into it — anywhere
/// but back here, whatever the bundle's configuration says. That matters because the
/// bundle's own default is to route "try it" requests through `proxy.scalar.com`;
/// `docs-bootstrap.js` turns that off, and this makes the setting unnecessary rather than
/// load-bearing.
///
/// `script-src 'self'` with no `unsafe-inline` is why the bootstrap is a file rather than
/// an inline script, and `unsafe-eval` is deliberately absent — the vendored bundle
/// contains no `eval`, `new Function`, or `Worker`, which a test asserts so that a future
/// version needing one is a failure rather than a blank page.
///
/// `style-src` does allow `'unsafe-inline'`, which is not a preference: the bundle injects
/// its stylesheets by creating `<style>` elements at runtime, so a stricter value would
/// render the page unstyled.
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; \
     script-src 'self'; \
     style-src 'self' 'unsafe-inline'; \
     img-src 'self' data:; \
     font-src 'self' data:; \
     connect-src 'self'; \
     frame-ancestors 'none'; \
     base-uri 'none'; \
     form-action 'none'";

/// One served file, prepared once at startup.
#[derive(Debug, Clone)]
struct Asset {
    body: Bytes,
    content_type: &'static str,

    /// Identifies this build of this file, so a reload of a 3.8 MB script can be answered
    /// with a 304. Weak, since it is derived from the version and the length rather than
    /// from a hash of the content: two different bundles of the same length under the same
    /// NexQ version would collide, which cannot happen outside someone editing the vendored
    /// file by hand mid-session.
    etag: HeaderValue,
}

/// The three documentation assets, with `{{PREFIX}}` resolved.
#[derive(Debug, Clone)]
pub struct Docs {
    page: Asset,
    bootstrap: Asset,
    bundle: Asset,
}

impl Docs {
    /// Prepare the assets for an API mounted at `prefix`.
    pub fn new(prefix: &str) -> Self {
        Self {
            page: Asset::new(
                Bytes::from(DOCS_HTML.replace(PREFIX_PLACEHOLDER, prefix)),
                "text/html; charset=utf-8",
                "page",
            ),
            bootstrap: Asset::new(
                Bytes::from(DOCS_BOOTSTRAP.replace(PREFIX_PLACEHOLDER, prefix)),
                "text/javascript; charset=utf-8",
                "bootstrap",
            ),
            bundle: Asset::new(
                Bytes::from_static(SCALAR_BUNDLE),
                "text/javascript; charset=utf-8",
                "scalar",
            ),
        }
    }

    /// The documentation routes, relative to the API prefix they are nested under.
    ///
    /// Plain `axum` routes rather than `aide` ones: these serve the documentation, they are
    /// not part of the API being documented, and an operation for "fetch the docs page"
    /// would appear in every generated client as a method nobody wants.
    pub fn router(self) -> Router {
        Router::new()
            .route("/docs", get(page))
            .route("/docs/bootstrap.js", get(bootstrap))
            .route("/docs/scalar.js", get(bundle))
            .with_state(self)
    }
}

impl Asset {
    fn new(body: Bytes, content_type: &'static str, tag: &str) -> Self {
        let etag = format!("W/\"{}-{tag}-{}\"", env!("CARGO_PKG_VERSION"), body.len());

        Self {
            body,
            content_type,
            // Every byte is produced above from a `&'static str` and a version, so this
            // cannot contain anything a header value rejects.
            etag: HeaderValue::from_str(&etag).unwrap_or(HeaderValue::from_static("W/\"nexq\"")),
        }
    }

    /// Serve this asset, or a 304 when the client already has it.
    fn respond(&self, if_none_match: Option<&HeaderValue>) -> Response {
        let headers = [
            (header::CONTENT_TYPE, self.content_type),
            (header::CACHE_CONTROL, "public, max-age=3600"),
            (
                HeaderName::from_static("content-security-policy"),
                CONTENT_SECURITY_POLICY,
            ),
            // The page names no other origin, so this is belt and braces — but a
            // documentation page is the sort of thing that ends up behind a proxy that
            // adds framing, and it costs a header.
            (HeaderName::from_static("x-content-type-options"), "nosniff"),
        ];

        if if_none_match.is_some_and(|value| value == self.etag) {
            return (
                StatusCode::NOT_MODIFIED,
                headers,
                [(header::ETAG, &self.etag)],
            )
                .into_response();
        }

        (headers, [(header::ETAG, &self.etag)], self.body.clone()).into_response()
    }
}

async fn page(State(docs): State<Docs>, headers: axum::http::HeaderMap) -> Response {
    docs.page.respond(headers.get(header::IF_NONE_MATCH))
}

async fn bootstrap(State(docs): State<Docs>, headers: axum::http::HeaderMap) -> Response {
    docs.bootstrap.respond(headers.get(header::IF_NONE_MATCH))
}

async fn bundle(State(docs): State<Docs>, headers: axum::http::HeaderMap) -> Response {
    docs.bundle.respond(headers.get(header::IF_NONE_MATCH))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bundle must not split itself into chunks it fetches at runtime: nothing serves
    /// `chunks/`, so half the page would 404. Checked here rather than trusted, because
    /// refreshing the vendored file is exactly when it could change.
    #[test]
    fn the_vendored_bundle_is_self_contained() {
        let source = String::from_utf8_lossy(SCALAR_BUNDLE);

        assert!(
            !source.contains("chunks/"),
            "the bundle references sibling chunks, which this server does not serve"
        );
    }

    /// What lets the Content-Security-Policy omit `unsafe-eval`. If a future bundle needs
    /// it, that should be a decision taken deliberately rather than found as a blank page.
    #[test]
    fn the_vendored_bundle_needs_no_eval() {
        let source = String::from_utf8_lossy(SCALAR_BUNDLE);

        for forbidden in ["new Function", "new Worker"] {
            assert!(
                !source.contains(forbidden),
                "the bundle uses {forbidden}, which the page's CSP forbids"
            );
        }
    }

    #[test]
    fn the_bundle_exposes_the_api_the_page_calls() {
        let source = String::from_utf8_lossy(SCALAR_BUNDLE);

        assert!(source.contains("createApiReference"));
        assert!(
            DOCS_BOOTSTRAP.contains("Scalar.createApiReference"),
            "the bootstrap must call what the bundle exposes"
        );
    }

    /// The reason `proxyUrl` is set at all. A bundle that stopped defaulting to the hosted
    /// proxy would make this test's premise stale, which is worth noticing.
    #[test]
    fn the_bundle_would_otherwise_use_a_third_party_proxy() {
        let source = String::from_utf8_lossy(SCALAR_BUNDLE);

        assert!(
            source.contains("proxy.scalar.com"),
            "if the bundle no longer defaults to a hosted proxy, `proxyUrl: ''` and the \
             comments explaining it can go"
        );
        assert!(
            DOCS_BOOTSTRAP.contains("proxyUrl: ''"),
            "the hosted proxy must be switched off"
        );
        assert!(
            DOCS_BOOTSTRAP.contains("withDefaultFonts: false"),
            "the hosted webfonts must be switched off"
        );
    }

    #[test]
    fn the_policy_confines_the_page_to_this_origin() {
        for directive in [
            "connect-src 'self'",
            "font-src 'self' data:",
            "script-src 'self'",
        ] {
            assert!(
                CONTENT_SECURITY_POLICY.contains(directive),
                "{directive} is what keeps a pasted token from leaving"
            );
        }

        assert!(
            !CONTENT_SECURITY_POLICY.contains("unsafe-eval"),
            "the bundle does not need it, so it should not be granted"
        );
        assert!(
            !CONTENT_SECURITY_POLICY.contains("script-src 'self' 'unsafe-inline'"),
            "the bootstrap is a file precisely so this is not needed"
        );
    }

    #[test]
    fn the_prefix_is_substituted_into_every_asset_that_names_a_path() {
        let docs = Docs::new("/api/v9");

        let page = String::from_utf8(docs.page.body.to_vec()).expect("utf-8");
        let bootstrap = String::from_utf8(docs.bootstrap.body.to_vec()).expect("utf-8");

        assert!(page.contains("/api/v9/docs/scalar.js"), "{page}");
        assert!(page.contains("/api/v9/docs/bootstrap.js"), "{page}");
        assert!(bootstrap.contains("/api/v9/openapi.json"), "{bootstrap}");

        for (name, asset) in [("page", &page), ("bootstrap", &bootstrap)] {
            assert!(
                !asset.contains(PREFIX_PLACEHOLDER),
                "{name} still carries an unsubstituted placeholder"
            );
        }
    }

    /// Distinct tags, or a browser would serve one asset's bytes for another's URL.
    #[test]
    fn each_asset_has_its_own_etag() {
        let docs = Docs::new("/api/v1");

        let tags = [&docs.page.etag, &docs.bootstrap.etag, &docs.bundle.etag];
        for (index, tag) in tags.iter().enumerate() {
            for other in &tags[index + 1..] {
                assert_ne!(tag, other, "two assets share an ETag");
            }
        }
    }
}
