//! SQS- and SNS-compatible facades: compatibility-only translation layers over the
//! core operation set. One crate because both need SigV4 verification and the same AWS
//! wire encoding and error shapes.
//!
//! The facade owns its own listener — see [`Server`] — so `nexq-server` only decides
//! whether to run it.

pub mod attributes;
pub mod checksum;
pub mod error;
pub mod operations;
pub mod protocol;
pub mod queue_url;
pub mod server;
pub mod sigv4;
pub mod system_attributes;

#[cfg(test)]
mod test_support;

pub use error::ApiError;
pub use operations::Operations;
pub use protocol::{JSON_CONTENT_TYPE, Operation, TARGET_HEADER};
pub use queue_url::QueueUrls;
pub use server::Server;
pub use sigv4::SigningContext;
