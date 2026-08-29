//! NexQ core: domain model, engine, the `Store` trait, and leader-lease election.
//!
//! Every other crate depends on this one; this one depends on none of them.

pub mod config;
pub mod dead_letter;
pub mod engine;
pub mod model;
pub mod move_task;
pub mod store;
pub mod tls;
pub mod waiters;

#[cfg(test)]
mod test_support;

pub use config::{
    AuthConfig, AwsApiConfig, BEARER_TOKEN_SEPARATOR, ClientTlsConfig, Config, Credential,
    RestApiConfig, Secret, ServerTlsConfig,
};
pub use dead_letter::Sweeper;
pub use engine::{Engine, EngineError};
pub use model::{
    ClaimedMessage, InvalidQueueName, InvalidRedrivePolicy, Message, MessageId, Priority, Queue,
    QueueAttributes, QueueName, ReceiptHandle, RedrivePolicy,
};
pub use move_task::{MoveTask, MoveTaskId, MoveTaskStatus};
pub use store::{Store, StoreError};
