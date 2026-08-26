//! NexQ core: domain model, engine, the `Store` trait, and leader-lease election.
//!
//! Every other crate depends on this one; this one depends on none of them.

pub mod config;
pub mod engine;
pub mod model;
pub mod store;
pub mod tls;
pub mod waiters;

#[cfg(test)]
mod test_support;

pub use config::{
    AuthConfig, AwsApiConfig, ClientTlsConfig, Config, Credential, Secret, ServerTlsConfig,
};
pub use engine::{Engine, EngineError};
pub use model::{
    ClaimedMessage, InvalidQueueName, Message, MessageId, Priority, Queue, QueueAttributes,
    QueueName, ReceiptHandle,
};
pub use store::{Store, StoreError};
