//! Store doubles for this crate's own tests.
//!
//! The real in-memory backend lives in `nexq-store-memory`, which depends on this
//! crate, so it cannot be used here. These stand-ins exist so the trait and the engine
//! can be tested without that cycle. They are deliberately dumb: `nexq-store-conformance`
//! is what holds real backends to their behavior.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::model::{Queue, QueueName};
use crate::store::{Result, Store, StoreError};

/// A store that works, backed by a map.
#[derive(Debug, Default)]
pub struct FakeStore {
    queues: Mutex<HashMap<QueueName, Queue>>,
}

impl FakeStore {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl Store for FakeStore {
    fn backend_name(&self) -> &'static str {
        "fake"
    }

    async fn create_queue(&self, queue: Queue) -> Result<()> {
        let mut queues = self.queues.lock().expect("lock");

        if queues.contains_key(&queue.name) {
            return Err(StoreError::QueueAlreadyExists(queue.name));
        }

        queues.insert(queue.name.clone(), queue);
        Ok(())
    }

    async fn get_queue(&self, name: &QueueName) -> Result<Queue> {
        self.queues
            .lock()
            .expect("lock")
            .get(name)
            .cloned()
            .ok_or_else(|| StoreError::QueueNotFound(name.clone()))
    }

    async fn delete_queue(&self, name: &QueueName) -> Result<()> {
        self.queues
            .lock()
            .expect("lock")
            .remove(name)
            .map(|_| ())
            .ok_or_else(|| StoreError::QueueNotFound(name.clone()))
    }

    async fn list_queues(&self) -> Result<Vec<Queue>> {
        Ok(self
            .queues
            .lock()
            .expect("lock")
            .values()
            .cloned()
            .collect())
    }
}

/// A store whose backend is down: every operation fails the same way.
#[derive(Debug, Default)]
pub struct BrokenStore;

impl BrokenStore {
    /// The message every failure carries, so tests can assert on it.
    pub const FAILURE: &'static str = "connection refused";

    fn failure() -> StoreError {
        StoreError::backend(Self::FAILURE)
    }
}

#[async_trait]
impl Store for BrokenStore {
    fn backend_name(&self) -> &'static str {
        "broken"
    }

    async fn create_queue(&self, _queue: Queue) -> Result<()> {
        Err(Self::failure())
    }

    async fn get_queue(&self, _name: &QueueName) -> Result<Queue> {
        Err(Self::failure())
    }

    async fn delete_queue(&self, _name: &QueueName) -> Result<()> {
        Err(Self::failure())
    }

    async fn list_queues(&self) -> Result<Vec<Queue>> {
        Err(Self::failure())
    }
}
