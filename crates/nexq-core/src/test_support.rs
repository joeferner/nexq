//! Store doubles for this crate's own tests.
//!
//! The real in-memory backend lives in `nexq-store-memory`, which depends on this
//! crate, so it cannot be used here. These stand-ins exist so the trait and the engine
//! can be tested without that cycle. They are deliberately dumb: `nexq-store-conformance`
//! is what holds real backends to their behavior.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;

use crate::model::{ClaimedMessage, Message, Queue, QueueName, ReceiptHandle};
use crate::store::{Result, Store, StoreError};

/// A store that works, backed by a map.
///
/// Messages are handled first-in-first-out, with no attention to priority: ordering is a
/// backend behavior, and `nexq-store-conformance` is what holds real backends to it.
///
/// It *does* honour a delay, because whether a message is claimable at all is something
/// the engine reasons about — the long-poll loop decides whether to keep waiting on the
/// answer — so a double that handed out delayed messages would make those tests agree
/// with a broken engine.
#[derive(Debug, Default)]
pub struct FakeStore {
    queues: Mutex<HashMap<QueueName, Queue>>,
    messages: Mutex<Vec<Claimable>>,
}

/// A message and, while a consumer holds it, the handle it was given.
#[derive(Debug)]
struct Claimable {
    queue: QueueName,
    message: Message,
    claim: Option<ReceiptHandle>,

    /// When this becomes claimable. In the future means it is still waiting out a delay.
    visible_at: SystemTime,
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

    async fn enqueue(
        &self,
        queue: &QueueName,
        message: Message,
        delay: Option<Duration>,
    ) -> Result<()> {
        let delay = {
            let queues = self.queues.lock().expect("lock");
            let Some(held) = queues.get(queue) else {
                return Err(StoreError::QueueNotFound(queue.clone()));
            };

            delay.unwrap_or(held.attributes.delay)
        };

        self.messages.lock().expect("lock").push(Claimable {
            queue: queue.clone(),
            message,
            claim: None,
            visible_at: SystemTime::now() + delay,
        });
        Ok(())
    }

    async fn claim_next(
        &self,
        queue: &QueueName,
        visibility_timeout: Option<Duration>,
    ) -> Result<Option<ClaimedMessage>> {
        if !self.queues.lock().expect("lock").contains_key(queue) {
            return Err(StoreError::QueueNotFound(queue.clone()));
        }

        let now = SystemTime::now();
        let mut messages = self.messages.lock().expect("lock");
        let Some(claimable) = messages
            .iter_mut()
            .find(|held| &held.queue == queue && held.claim.is_none() && held.visible_at <= now)
        else {
            return Ok(None);
        };

        let receipt = ReceiptHandle::new();
        claimable.claim = Some(receipt.clone());
        claimable.message.receive_count += 1;
        claimable.message.first_received_at.get_or_insert(now);

        Ok(Some(ClaimedMessage {
            message: claimable.message.clone(),
            receipt,
            claim_expires_at: now + visibility_timeout.unwrap_or(Duration::from_secs(30)),
        }))
    }

    async fn ack(&self, queue: &QueueName, receipt: &ReceiptHandle) -> Result<()> {
        let mut messages = self.messages.lock().expect("lock");

        let Some(index) = messages
            .iter()
            .position(|held| &held.queue == queue && held.claim.as_ref() == Some(receipt))
        else {
            return Err(StoreError::InvalidReceipt);
        };

        messages.remove(index);
        Ok(())
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

    async fn enqueue(
        &self,
        _queue: &QueueName,
        _message: Message,
        _delay: Option<Duration>,
    ) -> Result<()> {
        Err(Self::failure())
    }

    async fn claim_next(
        &self,
        _queue: &QueueName,
        _visibility_timeout: Option<Duration>,
    ) -> Result<Option<ClaimedMessage>> {
        Err(Self::failure())
    }

    async fn ack(&self, _queue: &QueueName, _receipt: &ReceiptHandle) -> Result<()> {
        Err(Self::failure())
    }
}
