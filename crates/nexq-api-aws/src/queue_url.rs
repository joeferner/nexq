//! Queue URLs: building them, and reading them back.
//!
//! Both directions matter, and the reverse direction is the one that carries risk. A
//! client calls `CreateQueue` or `GetQueueUrl` once, then sends the URL it was handed
//! back on *every* subsequent request — `SendMessage`, `ReceiveMessage`, and the rest
//! all identify their queue by `QueueUrl` rather than by name. So a URL this facade
//! emits must be one it can also parse, and the round trip is what the tests below
//! pin down.
//!
//! The shape matches SQS: `<base>/<account-id>/<queue-name>`.
//!
//! The format lives here rather than in `nexq-core` because it is an AWS-compatibility
//! detail. Config supplies the two pieces of data — the public base URL and the
//! account id — and this decides what to do with them.

use nexq_core::config::AwsApiConfig;
use nexq_core::model::QueueName;

use crate::error::ApiError;

/// Builds and parses queue URLs for one facade instance.
#[derive(Debug, Clone)]
pub struct QueueUrls {
    /// Externally reachable base, without a trailing slash. May include a path
    /// prefix, if NexQ is served under one.
    base_url: String,
    account_id: String,
}

impl QueueUrls {
    pub fn new(config: &AwsApiConfig) -> Self {
        Self {
            base_url: config.base_url().to_owned(),
            account_id: config.account_id.clone(),
        }
    }

    /// The URL to report for a queue, and the one clients will send back.
    pub fn for_queue(&self, name: &QueueName) -> String {
        format!("{}/{}/{}", self.base_url, self.account_id, name)
    }

    /// Read the queue name out of a URL a client sent.
    ///
    /// Only the tail of the path is interpreted: the last two segments must be the
    /// account id and the queue name. The scheme, host, and any leading path are
    /// ignored on purpose — behind an ingress or a port-forward, the URL a client holds
    /// legitimately differs from the one this facade would build, and rejecting it
    /// would break deployments that are working correctly. What is *not* ignored is the
    /// account id, since a URL carrying a different one belongs to some other
    /// deployment and honouring it would silently act on the wrong queue.
    pub fn queue_name(&self, url: &str) -> Result<QueueName, ApiError> {
        let segments: Vec<&str> = path_of(url).split('/').filter(|s| !s.is_empty()).collect();

        let [.., account_id, name] = segments.as_slice() else {
            return Err(ApiError::invalid_address(url));
        };

        if *account_id != self.account_id {
            return Err(ApiError::invalid_address(url));
        }

        QueueName::new(*name).map_err(|_| ApiError::invalid_address(url))
    }
}

/// The path part of a URL, with any query string or fragment removed.
///
/// Accepts a bare path as well as an absolute URL, since a client behind a proxy may
/// have been handed either.
fn path_of(url: &str) -> &str {
    let path = match url.split_once("://") {
        // Skip the authority: everything up to the first `/` after the scheme.
        Some((_scheme, rest)) => rest.split_once('/').map_or("", |(_authority, path)| path),
        None => url,
    };

    path.split(['?', '#']).next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(base_url: &str, account_id: &str) -> QueueUrls {
        QueueUrls::new(&AwsApiConfig {
            public_base_url: base_url.to_owned(),
            account_id: account_id.to_owned(),
            ..AwsApiConfig::default()
        })
    }

    fn default_urls() -> QueueUrls {
        urls("http://localhost:8080", "000000000000")
    }

    fn name(name: &str) -> QueueName {
        QueueName::new(name).expect("valid queue name")
    }

    #[test]
    fn a_queue_url_has_the_shape_sqs_uses() {
        assert_eq!(
            default_urls().for_queue(&name("jobs")),
            "http://localhost:8080/000000000000/jobs"
        );
    }

    #[test]
    fn a_trailing_slash_on_the_base_url_is_not_doubled() {
        assert_eq!(
            urls("http://localhost:8080/", "000000000000").for_queue(&name("jobs")),
            "http://localhost:8080/000000000000/jobs"
        );
    }

    #[test]
    fn every_url_this_facade_builds_can_be_read_back() {
        // The property that matters: clients only ever send back what we gave them.
        for (base, account) in [
            ("http://localhost:8080", "000000000000"),
            ("https://queue.example.com", "123456789012"),
            ("https://example.com/sqs", "000000000000"),
            ("http://10.0.0.5:9324/", "999999999999"),
        ] {
            let urls = urls(base, account);

            for queue in ["jobs", "a", "jobs_dlq", "Queue-42"] {
                let url = urls.for_queue(&name(queue));

                assert_eq!(
                    urls.queue_name(&url).expect(&url),
                    name(queue),
                    "round trip failed for {url}"
                );
            }
        }
    }

    #[test]
    fn a_url_under_a_path_prefix_is_understood() {
        let urls = urls("https://example.com/nexq/sqs", "000000000000");

        assert_eq!(
            urls.queue_name("https://example.com/nexq/sqs/000000000000/jobs")
                .expect("prefixed url"),
            name("jobs")
        );
    }

    #[test]
    fn the_host_is_not_checked() {
        // Behind an ingress or a port-forward the client's URL legitimately differs
        // from the one this facade would build for itself.
        let urls = default_urls();

        for url in [
            "http://localhost:8080/000000000000/jobs",
            "https://queue.internal.example.com/000000000000/jobs",
            "http://127.0.0.1:31234/000000000000/jobs",
            "/000000000000/jobs",
        ] {
            assert_eq!(urls.queue_name(url).expect(url), name("jobs"), "{url}");
        }
    }

    #[test]
    fn a_trailing_slash_or_query_string_does_not_confuse_it() {
        let urls = default_urls();

        for url in [
            "http://localhost:8080/000000000000/jobs/",
            "http://localhost:8080/000000000000/jobs?x=1",
            "http://localhost:8080/000000000000/jobs#frag",
        ] {
            assert_eq!(urls.queue_name(url).expect(url), name("jobs"), "{url}");
        }
    }

    #[test]
    fn a_url_for_another_account_is_refused() {
        // Another deployment's URL: acting on it would touch the wrong queue.
        let error = default_urls()
            .queue_name("http://localhost:8080/123456789012/jobs")
            .expect_err("wrong account");

        assert_eq!(error.code(), "InvalidAddress");
    }

    #[test]
    fn a_url_without_an_account_segment_is_refused() {
        let urls = default_urls();

        for url in [
            "http://localhost:8080/jobs",
            "http://localhost:8080/",
            "http://localhost:8080",
            "",
            "jobs",
        ] {
            let error = urls.queue_name(url).expect_err(url);
            assert_eq!(error.code(), "InvalidAddress", "{url}");
        }
    }

    #[test]
    fn a_url_naming_an_invalid_queue_is_refused() {
        let urls = default_urls();

        for url in [
            "http://localhost:8080/000000000000/not%20a%20name",
            "http://localhost:8080/000000000000/jobs.fifo",
        ] {
            let error = urls.queue_name(url).expect_err(url);
            assert_eq!(error.code(), "InvalidAddress", "{url}");
        }
    }
}
