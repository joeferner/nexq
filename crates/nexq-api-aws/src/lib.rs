//! SQS- and SNS-compatible facades: compatibility-only translation layers over the
//! core operation set. One crate because both need SigV4 verification and the same AWS
//! wire encoding and error shapes.
//!
//! The facade owns its own listener — see [`Server`] — so `nexq-server` only decides
//! whether to run it.

pub mod server;

pub use server::{Server, TARGET_HEADER};
