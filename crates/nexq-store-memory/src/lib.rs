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
use nexq_core::model::{ClaimedMessage, Message, Queue, QueueName, ReceiptHandle};
use nexq_core::store::{Result, Store, StoreError};

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

    async fn delete_queue(&self, name: &QueueName) -> Result<()> {
        // Removing the queue takes its messages with it, as the trait promises.
        self.write()?
            .remove(name)
            .map(|_| ())
            .ok_or_else(|| StoreError::QueueNotFound(name.clone()))
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

    async fn claim_next(
        &self,
        queue: &QueueName,
        visibility_timeout: Option<Duration>,
    ) -> Result<Option<ClaimedMessage>> {
        let now = SystemTime::now();
        let mut queues = self.write()?;
        let held = queues
            .get_mut(queue)
            .ok_or_else(|| StoreError::QueueNotFound(queue.clone()))?;

        let timeout = visibility_timeout.unwrap_or(held.queue.attributes.visibility_timeout);

        // Anything whose visibility has come round is claimable, including a message
        // whose previous claim expired — that expiry is what makes delivery
        // at-least-once.
        let Some(next) = held
            .messages
            .iter_mut()
            .filter(|stored| stored.visible_at <= now)
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
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nexq_core::model::{Priority, QueueAttributes};

    use super::*;

    fn name(name: &str) -> QueueName {
        QueueName::new(name).expect("valid queue name")
    }

    fn queue(queue_name: &str) -> Queue {
        Queue::new(name(queue_name))
    }

    fn message(body: &str, priority: i32) -> Message {
        Message::new(body, Priority::new(priority))
    }

    /// A store with one empty queue named `jobs`.
    async fn store_with_queue() -> MemoryStore {
        let store = MemoryStore::new();
        store.create_queue(queue("jobs")).await.expect("create");
        store
    }

    async fn claim_body(store: &MemoryStore) -> Option<String> {
        store
            .claim_next(&name("jobs"), None)
            .await
            .expect("claim")
            .map(|claimed| claimed.message.body)
    }

    #[tokio::test]
    async fn a_created_queue_can_be_read_back() {
        let store = MemoryStore::new();

        store.create_queue(queue("jobs")).await.expect("create");

        let found = store.get_queue(&name("jobs")).await.expect("get");
        assert_eq!(found.name, name("jobs"));
        assert_eq!(found.attributes, QueueAttributes::default());
    }

    #[tokio::test]
    async fn attributes_survive_the_round_trip() {
        let store = MemoryStore::new();
        let mut expected = queue("jobs");
        expected.attributes = QueueAttributes {
            visibility_timeout: Duration::from_secs(120),
            delay: Duration::from_secs(5),
            receive_wait_time: Duration::from_secs(20),
            max_receive_count: Some(3),
            dead_letter_queue: Some(name("jobs_dlq")),
        };

        store.create_queue(expected.clone()).await.expect("create");

        let found = store.get_queue(&name("jobs")).await.expect("get");
        assert_eq!(found, expected);
    }

    #[tokio::test]
    async fn an_empty_store_has_no_queues() {
        let store = MemoryStore::new();

        assert!(store.list_queues().await.expect("list").is_empty());
        assert_eq!(store.queue_count().expect("count"), 0);
    }

    #[tokio::test]
    async fn every_queue_is_listed() {
        let store = MemoryStore::new();
        for queue_name in ["one", "two", "three"] {
            store.create_queue(queue(queue_name)).await.expect("create");
        }

        let mut listed: Vec<String> = store
            .list_queues()
            .await
            .expect("list")
            .into_iter()
            .map(|queue| queue.name.to_string())
            .collect();
        listed.sort();

        assert_eq!(listed, ["one", "three", "two"]);
        assert_eq!(store.queue_count().expect("count"), 3);
    }

    #[tokio::test]
    async fn creating_a_queue_twice_fails_without_disturbing_the_first() {
        let store = MemoryStore::new();
        store.create_queue(queue("jobs")).await.expect("create");
        let original = store.get_queue(&name("jobs")).await.expect("get");

        let mut different = queue("jobs");
        different.attributes.visibility_timeout = Duration::from_secs(600);
        let error = store
            .create_queue(different)
            .await
            .expect_err("already exists");

        assert!(
            matches!(&error, StoreError::QueueAlreadyExists(existing) if existing == &name("jobs")),
            "{error:?}"
        );
        assert_eq!(
            store.get_queue(&name("jobs")).await.expect("get"),
            original,
            "a rejected create must not have overwritten anything"
        );
    }

    #[tokio::test]
    async fn only_one_of_many_concurrent_creates_wins() {
        let store = Arc::new(MemoryStore::new());

        let attempts: Vec<_> = (0..16)
            .map(|_| {
                let store = Arc::clone(&store);
                tokio::spawn(async move { store.create_queue(queue("jobs")).await })
            })
            .collect();

        let mut created = 0;
        let mut rejected = 0;
        for attempt in attempts {
            match attempt.await.expect("task") {
                Ok(()) => created += 1,
                Err(StoreError::QueueAlreadyExists(_)) => rejected += 1,
                Err(other) => panic!("unexpected error: {other:?}"),
            }
        }

        assert_eq!(created, 1, "exactly one create should succeed");
        assert_eq!(rejected, 15);
        assert_eq!(store.queue_count().expect("count"), 1);
    }

    #[tokio::test]
    async fn a_deleted_queue_is_gone_and_the_name_is_reusable() {
        let store = MemoryStore::new();
        store.create_queue(queue("jobs")).await.expect("create");

        store.delete_queue(&name("jobs")).await.expect("delete");

        store.get_queue(&name("jobs")).await.expect_err("deleted");
        assert!(store.list_queues().await.expect("list").is_empty());
        store
            .create_queue(queue("jobs"))
            .await
            .expect("the name is free again");
    }

    #[tokio::test]
    async fn deleting_a_queue_discards_its_messages() {
        let store = store_with_queue().await;
        store
            .enqueue(&name("jobs"), message("hello", 0), None)
            .await
            .expect("enqueue");

        store.delete_queue(&name("jobs")).await.expect("delete");
        store.create_queue(queue("jobs")).await.expect("recreate");

        assert_eq!(store.message_count(&name("jobs")).expect("count"), 0);
        assert!(claim_body(&store).await.is_none());
    }

    #[tokio::test]
    async fn acting_on_a_missing_queue_says_which_one() {
        let store = MemoryStore::new();
        let missing = name("nope");

        let errors = [
            store.get_queue(&missing).await.expect_err("get"),
            store.delete_queue(&missing).await.expect_err("delete"),
            store
                .enqueue(&missing, message("hello", 0), None)
                .await
                .expect_err("enqueue"),
            store.claim_next(&missing, None).await.expect_err("claim"),
            store
                .ack(&missing, &ReceiptHandle::new())
                .await
                .expect_err("ack"),
        ];

        for error in errors {
            assert!(
                matches!(&error, StoreError::QueueNotFound(reported) if reported == &missing),
                "{error:?}"
            );
        }
    }

    #[tokio::test]
    async fn the_backend_names_itself_as_config_does() {
        assert_eq!(MemoryStore::new().backend_name(), "memory");
        assert_eq!(BACKEND_NAME, "memory");
    }

    #[tokio::test]
    async fn it_works_behind_dyn_store() {
        // The shape the engine holds a backend in.
        let store: Arc<dyn Store> = Arc::new(MemoryStore::new());

        store.create_queue(queue("jobs")).await.expect("create");

        assert_eq!(store.list_queues().await.expect("list").len(), 1);
    }

    #[tokio::test]
    async fn an_enqueued_message_can_be_claimed() {
        let store = store_with_queue().await;
        store
            .enqueue(&name("jobs"), message("hello", 0), None)
            .await
            .expect("enqueue");

        let claimed = store
            .claim_next(&name("jobs"), None)
            .await
            .expect("claim")
            .expect("a message is waiting");

        assert_eq!(claimed.message.body, "hello");
        assert_eq!(claimed.message.receive_count, 1);
        assert!(claimed.message.first_received_at.is_some());
    }

    #[tokio::test]
    async fn claiming_an_empty_queue_yields_nothing() {
        let store = store_with_queue().await;

        assert!(claim_body(&store).await.is_none());
    }

    #[tokio::test]
    async fn a_claimed_message_is_hidden_from_other_consumers() {
        let store = store_with_queue().await;
        store
            .enqueue(&name("jobs"), message("hello", 0), None)
            .await
            .expect("enqueue");

        store
            .claim_next(&name("jobs"), None)
            .await
            .expect("claim")
            .expect("a message");

        assert!(
            claim_body(&store).await.is_none(),
            "two consumers must not hold the same message"
        );
        assert_eq!(
            store.message_count(&name("jobs")).expect("count"),
            1,
            "claiming does not remove it — only acking does"
        );
    }

    #[tokio::test]
    async fn higher_priority_is_served_first() {
        let store = store_with_queue().await;
        for (body, priority) in [("low", 0), ("urgent", 10), ("medium", 5)] {
            store
                .enqueue(&name("jobs"), message(body, priority), None)
                .await
                .expect("enqueue");
        }

        assert_eq!(claim_body(&store).await.as_deref(), Some("urgent"));
        assert_eq!(claim_body(&store).await.as_deref(), Some("medium"));
        assert_eq!(claim_body(&store).await.as_deref(), Some("low"));
    }

    #[tokio::test]
    async fn equal_priority_is_served_in_order_of_arrival() {
        let store = store_with_queue().await;
        let mut first = message("first", 0);
        let mut second = message("second", 0);
        // Enqueued moments apart in reality; pinned here so the assertion is about
        // ordering rather than about how fast the test ran.
        first.enqueued_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
        second.enqueued_at = SystemTime::UNIX_EPOCH + Duration::from_secs(2);

        // Inserted in the opposite order, so arrival time is doing the work.
        store
            .enqueue(&name("jobs"), second, None)
            .await
            .expect("enqueue");
        store
            .enqueue(&name("jobs"), first, None)
            .await
            .expect("enqueue");

        assert_eq!(claim_body(&store).await.as_deref(), Some("first"));
        assert_eq!(claim_body(&store).await.as_deref(), Some("second"));
    }

    #[tokio::test]
    async fn priority_outranks_arrival_order() {
        let store = store_with_queue().await;
        store
            .enqueue(&name("jobs"), message("early_but_low", 0), None)
            .await
            .expect("enqueue");
        store
            .enqueue(&name("jobs"), message("late_but_high", 1), None)
            .await
            .expect("enqueue");

        assert_eq!(claim_body(&store).await.as_deref(), Some("late_but_high"));
    }

    #[tokio::test]
    async fn a_delayed_message_is_not_claimable_yet() {
        let store = store_with_queue().await;
        store
            .enqueue(
                &name("jobs"),
                message("later", 0),
                Some(Duration::from_millis(150)),
            )
            .await
            .expect("enqueue");

        assert!(claim_body(&store).await.is_none(), "still delayed");

        tokio::time::sleep(Duration::from_millis(250)).await;

        assert_eq!(claim_body(&store).await.as_deref(), Some("later"));
    }

    #[tokio::test]
    async fn the_queues_delay_applies_when_none_is_given() {
        let store = MemoryStore::new();
        let mut delayed = queue("jobs");
        delayed.attributes.delay = Duration::from_millis(150);
        store.create_queue(delayed).await.expect("create");

        store
            .enqueue(&name("jobs"), message("later", 0), None)
            .await
            .expect("enqueue");

        assert!(claim_body(&store).await.is_none(), "the queue's own delay");

        tokio::time::sleep(Duration::from_millis(250)).await;

        assert_eq!(claim_body(&store).await.as_deref(), Some("later"));
    }

    #[tokio::test]
    async fn a_claim_that_expires_is_redelivered_under_a_new_handle() {
        let store = store_with_queue().await;
        store
            .enqueue(&name("jobs"), message("hello", 0), None)
            .await
            .expect("enqueue");

        let first = store
            .claim_next(&name("jobs"), Some(Duration::from_millis(150)))
            .await
            .expect("claim")
            .expect("a message");

        tokio::time::sleep(Duration::from_millis(250)).await;

        let second = store
            .claim_next(&name("jobs"), None)
            .await
            .expect("claim")
            .expect("redelivered once the claim lapsed");

        assert_eq!(second.message.id, first.message.id, "the same message");
        assert_eq!(second.message.receive_count, 2, "a second delivery");
        assert_ne!(
            second.receipt, first.receipt,
            "a redelivery comes with a new handle"
        );
        assert_eq!(
            second.message.first_received_at, first.message.first_received_at,
            "first delivery time is not overwritten"
        );
    }

    #[tokio::test]
    async fn a_handle_from_a_lapsed_claim_cannot_delete_the_message() {
        let store = store_with_queue().await;
        store
            .enqueue(&name("jobs"), message("hello", 0), None)
            .await
            .expect("enqueue");
        let lapsed = store
            .claim_next(&name("jobs"), Some(Duration::from_millis(150)))
            .await
            .expect("claim")
            .expect("a message");

        tokio::time::sleep(Duration::from_millis(250)).await;
        // Someone else now holds it.
        store
            .claim_next(&name("jobs"), None)
            .await
            .expect("claim")
            .expect("redelivered");

        let error = store
            .ack(&name("jobs"), &lapsed.receipt)
            .await
            .expect_err("the old handle is spent");

        assert!(matches!(error, StoreError::InvalidReceipt), "{error:?}");
        assert_eq!(
            store.message_count(&name("jobs")).expect("count"),
            1,
            "and it must not have deleted the other consumer's message"
        );
    }

    #[tokio::test]
    async fn acking_removes_the_message_for_good() {
        let store = store_with_queue().await;
        store
            .enqueue(&name("jobs"), message("hello", 0), None)
            .await
            .expect("enqueue");
        let claimed = store
            .claim_next(&name("jobs"), Some(Duration::from_millis(100)))
            .await
            .expect("claim")
            .expect("a message");

        store
            .ack(&name("jobs"), &claimed.receipt)
            .await
            .expect("ack");

        assert_eq!(store.message_count(&name("jobs")).expect("count"), 0);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert!(
            claim_body(&store).await.is_none(),
            "an acked message does not come back when its claim would have expired"
        );
    }

    #[tokio::test]
    async fn acking_an_unissued_handle_is_refused() {
        let store = store_with_queue().await;
        store
            .enqueue(&name("jobs"), message("hello", 0), None)
            .await
            .expect("enqueue");

        let error = store
            .ack(&name("jobs"), &ReceiptHandle::new())
            .await
            .expect_err("not a handle this store issued");

        assert!(matches!(error, StoreError::InvalidReceipt), "{error:?}");
        assert_eq!(store.message_count(&name("jobs")).expect("count"), 1);
    }

    #[tokio::test]
    async fn acking_an_unclaimed_message_is_refused() {
        let store = store_with_queue().await;
        store
            .enqueue(&name("jobs"), message("hello", 0), None)
            .await
            .expect("enqueue");

        // Never claimed, so no handle exists for it.
        let error = store
            .ack(&name("jobs"), &ReceiptHandle::new())
            .await
            .expect_err("nothing to ack");

        assert!(matches!(error, StoreError::InvalidReceipt), "{error:?}");
    }

    #[tokio::test]
    async fn each_message_goes_to_exactly_one_of_many_concurrent_consumers() {
        let store = Arc::new(store_with_queue().await);
        for index in 0..8 {
            store
                .enqueue(&name("jobs"), message(&format!("m{index}"), 0), None)
                .await
                .expect("enqueue");
        }

        let consumers: Vec<_> = (0..8)
            .map(|_| {
                let store = Arc::clone(&store);
                tokio::spawn(async move { claim_body(&store).await })
            })
            .collect();

        let mut claimed = Vec::new();
        for consumer in consumers {
            claimed.push(consumer.await.expect("task").expect("a message each"));
        }
        claimed.sort();

        let expected: Vec<String> = (0..8).map(|index| format!("m{index}")).collect();
        assert_eq!(claimed, expected, "no message claimed twice, none skipped");
    }
}
