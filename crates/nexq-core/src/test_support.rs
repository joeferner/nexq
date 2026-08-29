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

use crate::model::{
    ClaimedMessage, Message, MessageCounts, MessageId, MessageState, Queue, QueueAttributes,
    QueueName, QueuePosition, ReceiptHandle,
};
use crate::store::{Movable, Result, Store, StoreError};

/// A store that works, backed by a map.
///
/// Messages are handled first-in-first-out, with no attention to priority: ordering is a
/// backend behavior, and `nexq-store-conformance` is what holds real backends to it.
///
/// It *does* track visibility properly — delays, claim expiry, and hand-backs — because
/// whether a message is claimable is something the engine reasons about rather than just
/// passes along: the long-poll loop decides whether to keep waiting based on the answer.
/// A double that was loose about it would make those tests agree with a broken engine,
/// which has already happened twice while building this.
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

/// Which of the three states a message is in, the same way the real backends decide it.
fn state_of(held: &Claimable, now: SystemTime) -> MessageState {
    if held.visible_at <= now {
        MessageState::Visible
    } else if held.claim.is_some() {
        MessageState::NotVisible
    } else {
        MessageState::Delayed
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

    async fn set_queue_attributes(
        &self,
        name: &QueueName,
        attributes: QueueAttributes,
        modified_at: SystemTime,
    ) -> Result<()> {
        let mut queues = self.queues.lock().expect("lock");
        let held = queues
            .get_mut(name)
            .ok_or_else(|| StoreError::QueueNotFound(name.clone()))?;

        held.attributes = attributes;
        held.last_modified_at = modified_at;
        Ok(())
    }

    async fn message_counts(&self, name: &QueueName) -> Result<MessageCounts> {
        if !self.queues.lock().expect("lock").contains_key(name) {
            return Err(StoreError::QueueNotFound(name.clone()));
        }

        let now = SystemTime::now();
        let messages = self.messages.lock().expect("lock");

        let mut counts = MessageCounts::default();
        for held in messages.iter().filter(|held| &held.queue == name) {
            match state_of(held, now) {
                MessageState::Visible => counts.visible += 1,
                MessageState::NotVisible => counts.not_visible += 1,
                MessageState::Delayed => counts.delayed += 1,
            }
        }

        Ok(counts)
    }

    /// Position under this double's own order, which is arrival and nothing else.
    ///
    /// Priority is ignored here exactly as it is when claiming: a double that ordered one
    /// way and reported positions another would be a worse test than one that is simply
    /// first-in-first-out throughout.
    async fn position_of(
        &self,
        queue: &QueueName,
        message: &MessageId,
    ) -> Result<Option<QueuePosition>> {
        if !self.queues.lock().expect("lock").contains_key(queue) {
            return Err(StoreError::QueueNotFound(queue.clone()));
        }

        let now = SystemTime::now();
        let messages = self.messages.lock().expect("lock");
        let held: Vec<&Claimable> = messages
            .iter()
            .filter(|held| &held.queue == queue)
            .collect();

        let Some(at) = held.iter().position(|held| &held.message.id == message) else {
            return Ok(None);
        };

        let state = state_of(held[at], now);
        let claimable = |over: &[&Claimable]| {
            over.iter()
                .filter(|held| state_of(held, now) == MessageState::Visible)
                .count()
        };

        let ahead = match state {
            // Claimable, so what is in its way is what arrived before it.
            MessageState::Visible => claimable(&held[..at]),
            // Not claimable, so everything claimable is served first.
            MessageState::NotVisible | MessageState::Delayed => claimable(&held),
        };

        Ok(Some(QueuePosition {
            ahead: ahead as u64,
            state,
        }))
    }

    async fn delete_queue(&self, name: &QueueName) -> Result<()> {
        self.queues
            .lock()
            .expect("lock")
            .remove(name)
            .map(|_| ())
            .ok_or_else(|| StoreError::QueueNotFound(name.clone()))
    }

    async fn purge_queue(&self, name: &QueueName) -> Result<u64> {
        if !self.queues.lock().expect("lock").contains_key(name) {
            return Err(StoreError::QueueNotFound(name.clone()));
        }

        let mut messages = self.messages.lock().expect("lock");
        let before = messages.len();
        messages.retain(|held| &held.queue != name);

        Ok((before - messages.len()) as u64)
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

    async fn claim_next_skipping(
        &self,
        queue: &QueueName,
        visibility_timeout: Option<Duration>,
        skip: &[MessageId],
    ) -> Result<Option<ClaimedMessage>> {
        if !self.queues.lock().expect("lock").contains_key(queue) {
            return Err(StoreError::QueueNotFound(queue.clone()));
        }

        let (timeout, redrive) = {
            let queues = self.queues.lock().expect("lock");
            let Some(held) = queues.get(queue) else {
                return Err(StoreError::QueueNotFound(queue.clone()));
            };

            (
                visibility_timeout.unwrap_or(held.attributes.visibility_timeout),
                held.attributes.redrive.clone(),
            )
        };

        let now = SystemTime::now();
        let mut messages = self.messages.lock().expect("lock");

        // Visibility alone decides what is claimable — *not* whether a handle is
        // recorded. A message whose claim has lapsed, or whose holder handed it back,
        // still has its old handle on it and must be claimable all the same. Except one
        // the redrive policy has exhausted, which is out of deliveries and waiting to be
        // dead-lettered rather than tried again.
        let Some(claimable) = messages.iter_mut().find(|held| {
            &held.queue == queue
                && held.visible_at <= now
                && !skip.contains(&held.message.id)
                && !redrive
                    .as_ref()
                    .is_some_and(|policy| policy.is_exhausted(held.message.receive_count))
        }) else {
            return Ok(None);
        };

        let receipt = ReceiptHandle::new();
        let claim_expires_at = now + timeout;

        claimable.message.receive_count += 1;
        claimable.message.first_received_at.get_or_insert(now);
        // A new handle each delivery, which is what invalidates the previous holder's.
        claimable.claim = Some(receipt.clone());
        claimable.visible_at = claim_expires_at;

        Ok(Some(ClaimedMessage {
            message: claimable.message.clone(),
            receipt,
            claim_expires_at,
        }))
    }

    async fn claim_for_move(
        &self,
        queue: &QueueName,
        movable: Movable,
        hold: Duration,
        limit: usize,
    ) -> Result<Vec<ClaimedMessage>> {
        let redrive = {
            let queues = self.queues.lock().expect("lock");
            let Some(held) = queues.get(queue) else {
                return Err(StoreError::QueueNotFound(queue.clone()));
            };

            held.attributes.redrive.clone()
        };

        if movable == Movable::Exhausted && redrive.is_none() {
            return Ok(Vec::new());
        }

        let now = SystemTime::now();
        let claim_expires_at = now + hold;
        let mut messages = self.messages.lock().expect("lock");
        let mut taken = Vec::new();

        for held in messages.iter_mut() {
            if taken.len() >= limit {
                break;
            }

            let qualifies = &held.queue == queue
                && held.visible_at <= now
                && match movable {
                    Movable::Exhausted => redrive
                        .as_ref()
                        .is_some_and(|policy| policy.is_exhausted(held.message.receive_count)),
                    Movable::Everything => true,
                };
            if !qualifies {
                continue;
            }

            let receipt = ReceiptHandle::new();
            // Held like a delivery but not counted as one: the message is leaving, not
            // being tried again.
            held.claim = Some(receipt.clone());
            held.visible_at = claim_expires_at;

            taken.push(ClaimedMessage {
                message: held.message.clone(),
                receipt,
                claim_expires_at,
            });
        }

        Ok(taken)
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

    async fn change_visibility(
        &self,
        queue: &QueueName,
        receipt: &ReceiptHandle,
        visibility_timeout: Duration,
    ) -> Result<()> {
        let mut messages = self.messages.lock().expect("lock");

        let Some(held) = messages
            .iter_mut()
            .find(|held| &held.queue == queue && held.claim.as_ref() == Some(receipt))
        else {
            return Err(StoreError::InvalidReceipt);
        };

        held.visible_at = SystemTime::now() + visibility_timeout;
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

    async fn set_queue_attributes(
        &self,
        _name: &QueueName,
        _attributes: QueueAttributes,
        _modified_at: SystemTime,
    ) -> Result<()> {
        Err(Self::failure())
    }

    async fn message_counts(&self, _name: &QueueName) -> Result<MessageCounts> {
        Err(Self::failure())
    }

    async fn position_of(
        &self,
        _queue: &QueueName,
        _message: &MessageId,
    ) -> Result<Option<QueuePosition>> {
        Err(Self::failure())
    }

    async fn delete_queue(&self, _name: &QueueName) -> Result<()> {
        Err(Self::failure())
    }

    async fn purge_queue(&self, _name: &QueueName) -> Result<u64> {
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

    async fn claim_next_skipping(
        &self,
        _queue: &QueueName,
        _visibility_timeout: Option<Duration>,
        _skip: &[MessageId],
    ) -> Result<Option<ClaimedMessage>> {
        Err(Self::failure())
    }

    async fn claim_for_move(
        &self,
        _queue: &QueueName,
        _movable: Movable,
        _hold: Duration,
        _limit: usize,
    ) -> Result<Vec<ClaimedMessage>> {
        Err(Self::failure())
    }

    async fn ack(&self, _queue: &QueueName, _receipt: &ReceiptHandle) -> Result<()> {
        Err(Self::failure())
    }

    async fn change_visibility(
        &self,
        _queue: &QueueName,
        _receipt: &ReceiptHandle,
        _visibility_timeout: Duration,
    ) -> Result<()> {
        Err(Self::failure())
    }
}
