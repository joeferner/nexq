//! SQS- and SNS-compatible facades: compatibility-only translation layers over the
//! core operation set. One crate because both need SigV4 verification and the same AWS
//! wire encoding and error shapes.
//!
//! The facade owns its own listener — see [`Server`] — so `nexq-server` only decides
//! whether to run it.

pub mod error;
pub mod operations;
pub mod protocol;
pub mod server;

pub use error::ApiError;
pub use protocol::{JSON_CONTENT_TYPE, Operation, TARGET_HEADER};
pub use server::Server;
