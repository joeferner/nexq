//! Shared fixtures for this crate's tests.

use nexq_core::config::AwsApiConfig;

use crate::queue_url::QueueUrls;

/// Queue URLs built from the default config, so expected URLs in tests read as the
/// literal strings a client would receive.
pub fn test_queue_urls() -> QueueUrls {
    QueueUrls::new(&AwsApiConfig::default())
}
