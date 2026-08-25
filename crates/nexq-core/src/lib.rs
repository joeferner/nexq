//! NexQ core: domain model, engine, the `Store` trait, and leader-lease election.
//!
//! Every other crate depends on this one; this one depends on none of them.

pub mod config;

pub use config::{AuthConfig, AwsApiConfig, Config, Credential, Secret};
