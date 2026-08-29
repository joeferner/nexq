//! In-memory storage backend.
//!
//! Everything lives in process and nothing survives a restart, so this backend cannot
//! support multi-node HA — a second node could never observe the same state. It is
//! still a first-class choice for single-node deployments, and the default one.
//!
//! Its other job is to be the reference implementation: it has no external
//! dependencies, so it is the backend the `Store` trait is designed against and the
//! one the conformance suite is written for first.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use nexq_core::model::{
    ClaimedMessage, Message, MessageCounts, MessageId, MessageState, Queue, QueueAttributes,
    QueueName, QueuePosition, ReceiptHandle, RedrivePolicy,
};
use nexq_core::store::{Movable, Result, Store, StoreError};

/// This backend's name in config.
pub const BACKEND_NAME: &str = "memory";

/// Queues held in process.
///
/// Guarded by a [`std::sync::RwLock`] rather than an async one on purpose: every
/// critical section here is a map or vector operation with no `await` inside it, so an
/// async lock would add machinery to protect against a suspension that cannot happen.
#[derive(Debug, Default)]
pub struct MemoryStore {
    queues: RwLock<HashMap<QueueName, StoredQueue>>,
}

/// A queue and its messages.
#[derive(Debug)]
struct StoredQueue {
    queue: Queue,
    messages: Vec<StoredMessage>,
}

/// A message, plus the two pieces of state that decide who may see it.
#[derive(Debug)]
struct StoredMessage {
    message: Message,

    /// When this becomes claimable. In the future means invisible: either it is
    /// waiting out a delay, or a consumer holds a claim that has not expired.
    visible_at: SystemTime,

    /// The handle issued for the current claim, if any. Replaced on each delivery,
    /// which is what invalidates the previous consumer's handle.
    claim: Option<ReceiptHandle>,
}

impl MemoryStore {
    /// An empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many queues exist. Cheap, and handy for tests and metrics.
    pub fn queue_count(&self) -> Result<usize> {
        Ok(self.read()?.len())
    }

    /// How many messages a queue holds, claimed or not.
    pub fn message_count(&self, queue: &QueueName) -> Result<usize> {
        Ok(self
            .read()?
            .get(queue)
            .map_or(0, |held| held.messages.len()))
    }

    fn read(&self) -> Result<RwLockReadGuard<'_, HashMap<QueueName, StoredQueue>>> {
        self.queues.read().map_err(|_| Self::poisoned())
    }

    fn write(&self) -> Result<RwLockWriteGuard<'_, HashMap<QueueName, StoredQueue>>> {
        self.queues.write().map_err(|_| Self::poisoned())
    }

    /// A panic while the lock was held leaves it poisoned.
    ///
    /// Reported as a backend failure rather than recovered from: the state may be
    /// half-written, and a caller deciding what to do about that is better than this
    /// store pretending nothing happened. It also cannot be signalled by panicking
    /// here, since that would take down whatever request touched it next.
    fn poisoned() -> StoreError {
        StoreError::backend("the in-memory store's lock is poisoned")
    }
}

/// Which of two claimable messages should be served first.
///
/// Higher priority wins; within one priority the earliest enqueued wins; the id breaks
/// remaining ties so the order is total rather than dependent on insertion. Written as
/// a comparator on `Ordering::Less` meaning "serve this one first".
fn serve_first(left: &StoredMessage, right: &StoredMessage) -> Ordering {
    right
        .message
        .priority
        .cmp(&left.message.priority)
        .then_with(|| left.message.enqueued_at.cmp(&right.message.enqueued_at))
        .then_with(|| left.message.id.cmp(&right.message.id))
}

/// Which of the three states a message is in.
///
/// One function rather than the same three-way test written wherever it is needed: the
/// cases are disjoint by construction — either it is claimable now, or it is not, and if
/// it is not then either someone holds it or it is waiting out a delay — and writing that
/// twice is how two callers come to disagree about a message nobody has claimed yet.
fn state_of(stored: &StoredMessage, now: SystemTime) -> MessageState {
    if stored.visible_at <= now {
        MessageState::Visible
    } else if stored.claim.is_some() {
        MessageState::NotVisible
    } else {
        MessageState::Delayed
    }
}

/// Whether a redrive policy has run out of patience with a message.
///
/// A queue with no policy exhausts nothing, which is why this takes the option rather
/// than making every caller unwrap it: "no limit" and "under the limit" lead to the same
/// answer everywhere, and writing that branch twice is how they come apart.
fn is_exhausted(redrive: Option<&RedrivePolicy>, message: &Message) -> bool {
    redrive.is_some_and(|policy| policy.is_exhausted(message.receive_count))
}

/// `now + duration`, or `now` if that would overflow the clock.
///
/// Overflow needs a duration far beyond anything a facade will accept, and treating it
/// as "visible now" is the safe direction: a message delivered too eagerly is a
/// duplicate, which consumers must already tolerate, while one hidden forever is lost.
fn visible_after(now: SystemTime, duration: Duration) -> SystemTime {
    now.checked_add(duration).unwrap_or(now)
}

#[async_trait]
impl Store for MemoryStore {
    fn backend_name(&self) -> &'static str {
        BACKEND_NAME
    }

    async fn create_queue(&self, queue: Queue) -> Result<()> {
        match self.write()?.entry(queue.name.clone()) {
            // Checked and inserted under one write lock, so two concurrent creates of
            // the same name cannot both succeed.
            Entry::Occupied(existing) => {
                Err(StoreError::QueueAlreadyExists(existing.key().clone()))
            }
            Entry::Vacant(slot) => {
                slot.insert(StoredQueue {
                    queue,
                    messages: Vec::new(),
                });
                Ok(())
            }
        }
    }

    async fn get_queue(&self, name: &QueueName) -> Result<Queue> {
        self.read()?
            .get(name)
            .map(|held| held.queue.clone())
            .ok_or_else(|| StoreError::QueueNotFound(name.clone()))
    }

    async fn set_queue_attributes(
        &self,
        name: &QueueName,
        attributes: QueueAttributes,
        modified_at: SystemTime,
    ) -> Result<()> {
        let mut queues = self.write()?;
        let held = queues
            .get_mut(name)
            .ok_or_else(|| StoreError::QueueNotFound(name.clone()))?;

        held.queue.attributes = attributes;
        held.queue.last_modified_at = modified_at;

        Ok(())
    }

    async fn message_counts(&self, name: &QueueName) -> Result<MessageCounts> {
        let now = SystemTime::now();
        let queues = self.read()?;
        let held = queues
            .get(name)
            .ok_or_else(|| StoreError::QueueNotFound(name.clone()))?;

        let mut counts = MessageCounts::default();
        for stored in &held.messages {
            match state_of(stored, now) {
                MessageState::Visible => counts.visible += 1,
                MessageState::NotVisible => counts.not_visible += 1,
                MessageState::Delayed => counts.delayed += 1,
            }
        }

        Ok(counts)
    }

    async fn position_of(
        &self,
        queue: &QueueName,
        message: &MessageId,
    ) -> Result<Option<QueuePosition>> {
        let now = SystemTime::now();
        let queues = self.read()?;
        let held = queues
            .get(queue)
            .ok_or_else(|| StoreError::QueueNotFound(queue.clone()))?;

        let Some(target) = held
            .messages
            .iter()
            .find(|stored| &stored.message.id == message)
        else {
            return Ok(None);
        };

        let state = state_of(target, now);
        let claimable = held
            .messages
            .iter()
            .filter(|stored| state_of(stored, now) == MessageState::Visible);

        // A linear scan, where a store built for this would keep an ordered index — the
        // trait asks for the answer rather than the method precisely so that a backend
        // can use one. This backend holds a `Vec` and the claim it serves every receive
        // from is a scan already, so a second structure to maintain would cost more than
        // it saves.
        let ahead = match state {
            // Claimable now, so the ones in its way are the ones that outrank it. Its own
            // comparison is `Equal`, so it does not count itself.
            MessageState::Visible => claimable
                .filter(|stored| serve_first(stored, target) == Ordering::Less)
                .count(),

            // Not claimable, so everything claimable goes first: none of it is waiting on
            // a message that cannot be served at all.
            MessageState::NotVisible | MessageState::Delayed => claimable.count(),
        };

        Ok(Some(QueuePosition {
            ahead: ahead as u64,
            state,
        }))
    }

    async fn delete_queue(&self, name: &QueueName) -> Result<()> {
        // Removing the queue takes its messages with it, as the trait promises.
        self.write()?
            .remove(name)
            .map(|_| ())
            .ok_or_else(|| StoreError::QueueNotFound(name.clone()))
    }

    async fn purge_queue(&self, name: &QueueName) -> Result<u64> {
        let mut queues = self.write()?;
        let held = queues
            .get_mut(name)
            .ok_or_else(|| StoreError::QueueNotFound(name.clone()))?;

        // Everything, claimed included: the queue stays, its contents do not.
        let purged = held.messages.len() as u64;
        held.messages.clear();

        Ok(purged)
    }

    async fn list_queues(&self) -> Result<Vec<Queue>> {
        Ok(self
            .read()?
            .values()
            .map(|held| held.queue.clone())
            .collect())
    }

    async fn enqueue(
        &self,
        queue: &QueueName,
        message: Message,
        delay: Option<Duration>,
    ) -> Result<()> {
        let mut queues = self.write()?;
        let held = queues
            .get_mut(queue)
            .ok_or_else(|| StoreError::QueueNotFound(queue.clone()))?;

        let delay = delay.unwrap_or(held.queue.attributes.delay);

        held.messages.push(StoredMessage {
            message,
            visible_at: visible_after(SystemTime::now(), delay),
            claim: None,
        });

        Ok(())
    }

    async fn claim_next_skipping(
        &self,
        queue: &QueueName,
        visibility_timeout: Option<Duration>,
        skip: &[MessageId],
    ) -> Result<Option<ClaimedMessage>> {
        let now = SystemTime::now();
        let mut queues = self.write()?;
        let held = queues
            .get_mut(queue)
            .ok_or_else(|| StoreError::QueueNotFound(queue.clone()))?;

        let timeout = visibility_timeout.unwrap_or(held.queue.attributes.visibility_timeout);
        // Destructured so the attributes can be read while the messages are borrowed
        // mutably: they are separate fields, and the borrow checker can see that here
        // where it cannot through `held.`.
        let StoredQueue { queue: stored_queue, messages } = held;
        let redrive = stored_queue.attributes.redrive.as_ref();

        // Anything whose visibility has come round is claimable, including a message
        // whose previous claim expired — that expiry is what makes delivery
        // at-least-once. Two exceptions:
        //
        // - the ones the caller already holds, since a claim taken with a zero timeout is
        //   visible again immediately and without this a batch would keep being handed its
        //   own first message;
        // - the ones the redrive policy has exhausted, whose deliveries are spent. Handing
        //   one out would be the delivery the policy exists to prevent, and it stays put
        //   until `claim_exhausted` moves it out.
        let Some(next) = messages
            .iter_mut()
            .filter(|stored| {
                stored.visible_at <= now
                    && !skip.contains(&stored.message.id)
                    && !is_exhausted(redrive, &stored.message)
            })
            .min_by(|left, right| serve_first(left, right))
        else {
            return Ok(None);
        };

        let receipt = ReceiptHandle::new();
        let claim_expires_at = visible_after(now, timeout);

        next.message.receive_count += 1;
        next.message.first_received_at.get_or_insert(now);
        // A new handle each delivery, so the previous consumer's handle stops working.
        next.claim = Some(receipt.clone());
        next.visible_at = claim_expires_at;

        Ok(Some(ClaimedMessage {
            message: next.message.clone(),
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
        let now = SystemTime::now();
        let mut queues = self.write()?;
        let held = queues
            .get_mut(queue)
            .ok_or_else(|| StoreError::QueueNotFound(queue.clone()))?;

        let redrive = held.queue.attributes.redrive.clone();

        // No policy, nothing exhausted. Checked before anything is scanned, since this is
        // the case for most queues and the dead-letter sweep asks every one of them.
        if movable == Movable::Exhausted && redrive.is_none() {
            return Ok(Vec::new());
        }

        // Indices rather than `iter_mut`, because the order has to be decided across the
        // whole set before any of it is mutated.
        let mut movable_indices: Vec<usize> = held
            .messages
            .iter()
            .enumerate()
            .filter(|(_, stored)| {
                // Claimable, always. Without this a move would take a message away from a
                // consumer that is still working on it and may yet acknowledge it.
                stored.visible_at <= now
                    && match movable {
                        Movable::Exhausted => is_exhausted(redrive.as_ref(), &stored.message),
                        Movable::Everything => true,
                    }
            })
            .map(|(index, _)| index)
            .collect();

        // In the order they would have been served, so which messages a limited move takes
        // is the same rule as everywhere else rather than an accident of storage.
        movable_indices
            .sort_by(|&left, &right| serve_first(&held.messages[left], &held.messages[right]));
        movable_indices.truncate(limit);

        let claim_expires_at = visible_after(now, hold);
        let mut taken = Vec::with_capacity(movable_indices.len());

        for index in movable_indices {
            let stored = &mut held.messages[index];
            let receipt = ReceiptHandle::new();

            // Held exactly as a delivery holds it — a fresh handle, invisible until the
            // hold lapses — but *not* counted as one: the message is leaving, not being
            // tried again.
            stored.claim = Some(receipt.clone());
            stored.visible_at = claim_expires_at;

            taken.push(ClaimedMessage {
                message: stored.message.clone(),
                receipt,
                claim_expires_at,
            });
        }

        Ok(taken)
    }

    async fn ack(&self, queue: &QueueName, receipt: &ReceiptHandle) -> Result<()> {
        let mut queues = self.write()?;
        let held = queues
            .get_mut(queue)
            .ok_or_else(|| StoreError::QueueNotFound(queue.clone()))?;

        // Matching on the stored handle is what rejects a handle from a claim that has
        // since expired and been redelivered to someone else.
        let Some(index) = held
            .messages
            .iter()
            .position(|stored| stored.claim.as_ref() == Some(receipt))
        else {
            return Err(StoreError::InvalidReceipt);
        };

        held.messages.remove(index);
        Ok(())
    }

    async fn change_visibility(
        &self,
        queue: &QueueName,
        receipt: &ReceiptHandle,
        visibility_timeout: Duration,
    ) -> Result<()> {
        let now = SystemTime::now();
        let mut queues = self.write()?;
        let held = queues
            .get_mut(queue)
            .ok_or_else(|| StoreError::QueueNotFound(queue.clone()))?;

        let Some(stored) = held
            .messages
            .iter_mut()
            .find(|stored| stored.claim.as_ref() == Some(receipt))
        else {
            return Err(StoreError::InvalidReceipt);
        };

        // From now rather than from the original claim, so this shortens as well as
        // extends. The handle is left in place: the claim's owner has not changed, only
        // how long it lasts.
        stored.visible_at = visible_after(now, visibility_timeout);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The shared contract — ordering, claim expiry, acknowledgement, concurrency — is
    // asserted by `nexq-store-conformance` in `tests/conformance.rs`, which every
    // backend runs. What is left here is what belongs to this backend alone.

    use nexq_core::model::Priority;

    fn name(name: &str) -> QueueName {
        QueueName::new(name).expect("valid queue name")
    }

    #[tokio::test]
    async fn it_names_itself_the_way_config_does() {
        // `memory` is what an operator writes in config, so the string matters.
        assert_eq!(MemoryStore::new().backend_name(), "memory");
        assert_eq!(BACKEND_NAME, "memory");
    }

    #[tokio::test]
    async fn the_counters_track_what_the_store_holds() {
        let store = MemoryStore::new();
        assert_eq!(store.queue_count().expect("count"), 0);
        assert_eq!(store.message_count(&name("jobs")).expect("count"), 0);

        store
            .create_queue(Queue::new(name("jobs")))
            .await
            .expect("create");
        store
            .enqueue(
                &name("jobs"),
                Message::new("hello", Priority::DEFAULT),
                None,
            )
            .await
            .expect("enqueue");

        assert_eq!(store.queue_count().expect("count"), 1);
        assert_eq!(store.message_count(&name("jobs")).expect("count"), 1);
    }

    #[tokio::test]
    async fn a_claimed_message_still_counts_until_it_is_acked() {
        // The counters report what is stored, not what is visible — a claimed message
        // is still occupying memory, which is the point of counting it.
        let store = MemoryStore::new();
        store
            .create_queue(Queue::new(name("jobs")))
            .await
            .expect("create");
        store
            .enqueue(
                &name("jobs"),
                Message::new("hello", Priority::DEFAULT),
                None,
            )
            .await
            .expect("enqueue");

        let claimed = store
            .claim_next(&name("jobs"), None)
            .await
            .expect("claim")
            .expect("a message");
        assert_eq!(store.message_count(&name("jobs")).expect("count"), 1);

        store
            .ack(&name("jobs"), &claimed.receipt)
            .await
            .expect("ack");
        assert_eq!(store.message_count(&name("jobs")).expect("count"), 0);
    }

    #[tokio::test]
    async fn the_counters_ignore_queues_that_do_not_exist() {
        assert_eq!(
            MemoryStore::new()
                .message_count(&name("nope"))
                .expect("count"),
            0,
            "counting a missing queue is a question, not an error"
        );
    }
}
