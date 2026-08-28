//! The storage backend abstraction.
//!
//! A [`Store`] is where queues and messages actually live. Each backend crate
//! implements it — `nexq-store-memory`, `nexq-store-sql`, `nexq-store-search` — and
//! the engine only ever talks to the trait. No backend lives here, so this crate stays
//! the abstraction and nothing else.
//!
//! Backends are used behind `dyn Store`, so a queue can name its backend in config at
//! runtime rather than the choice being baked into a type. That costs a virtual call
//! and a boxed future per operation, which is nothing beside the storage round trip
//! it wraps.
//!
//! `async fn` in a trait is not `dyn`-compatible, so the methods here are written with
//! [`macro@async_trait`], which rewrites them to return boxed futures. That is why
//! implementations also carry the `#[async_trait]` attribute.
//!
//! Only queue lifecycle is here so far; sending, claiming, and acknowledging messages
//! arrive with the operations that need them.

use std::fmt;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use thiserror::Error;

use crate::model::{
    ClaimedMessage, Message, MessageCounts, MessageId, Queue, QueueAttributes, QueueName,
    QueuePosition, ReceiptHandle,
};

/// The result of a storage operation.
pub type Result<T> = std::result::Result<T, StoreError>;

/// Why a storage operation failed.
#[derive(Debug, Error)]
pub enum StoreError {
    /// No queue by that name.
    #[error("queue does not exist: {0}")]
    QueueNotFound(QueueName),

    /// A queue by that name already exists.
    #[error("queue already exists: {0}")]
    QueueAlreadyExists(QueueName),

    /// The receipt handle names no claim: it was never issued, its claim already
    /// ended, or the message has since been redelivered under a new handle.
    ///
    /// Carries no payload on purpose — a handle authorizes deleting a message, so it
    /// does not belong in an error string that will be logged.
    #[error("the receipt handle does not identify a current claim")]
    InvalidReceipt,

    /// The backend itself failed — a connection dropped, a query was rejected, a
    /// response could not be decoded. Distinct from the cases above, which are normal
    /// answers a healthy backend gives.
    #[error("backend failure: {0}")]
    Backend(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl StoreError {
    /// Wrap a backend's own error.
    pub fn backend(error: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> Self {
        Self::Backend(error.into())
    }
}

/// Somewhere queues live.
///
/// Implementations must be safe to share across tasks — one store is used concurrently
/// by every request handler — hence `Send + Sync`. `Debug` is required so the types
/// that hold a store can derive it.
#[async_trait]
pub trait Store: fmt::Debug + Send + Sync + 'static {
    /// Backend name as it appears in config: `memory`, `postgres`, and so on.
    ///
    /// For diagnostics and metrics labels, not for behavior: nothing should branch on
    /// this, or the abstraction has leaked.
    fn backend_name(&self) -> &'static str;

    /// Create a queue.
    ///
    /// Fails with [`StoreError::QueueAlreadyExists`] rather than overwriting, since
    /// silently replacing a queue would discard whatever it held. Creating a queue
    /// that already exists is still an error here even if the attributes match;
    /// whether that counts as idempotent is the caller's call, because the answer
    /// differs per protocol.
    async fn create_queue(&self, queue: Queue) -> Result<()>;

    /// Look up a queue, attributes included.
    async fn get_queue(&self, name: &QueueName) -> Result<Queue>;

    /// Replace a queue's attributes, leaving its messages alone.
    ///
    /// `modified_at` is passed in rather than read from a clock here so the engine stays
    /// the one authority on server-set times. It matters once a backend is a database on
    /// another machine, whose clock need not agree with this one's.
    async fn set_queue_attributes(
        &self,
        name: &QueueName,
        attributes: QueueAttributes,
        modified_at: SystemTime,
    ) -> Result<()>;

    /// How many messages the queue holds, split by visibility.
    ///
    /// Separate from [`Store::list_queues`] and [`Store::get_queue`] because it is the
    /// expensive question: a backend may need to aggregate, where reading a queue's
    /// record does not. Callers should ask for it only when someone wants the numbers.
    async fn message_counts(&self, name: &QueueName) -> Result<MessageCounts>;

    /// Where one message sits in the line, or `None` if the queue does not hold it.
    ///
    /// `None` rather than an error because "no such message" is an ordinary answer: the
    /// message may have been received and deleted a moment ago, which is success from
    /// everyone's point of view. A missing *queue* is still
    /// [`StoreError::QueueNotFound`], since that one is a caller's mistake.
    ///
    /// **The contract is one sentence: how many messages claimable right now would be
    /// served before this one.** Everything else follows from it — messages that are
    /// delayed or in flight are not counted because a consumer polling now would not be
    /// given them, and when the named message is itself not claimable everything
    /// claimable is counted, because none of it can be served after something that cannot
    /// be served at all.
    ///
    /// Per-backend by construction, which is the point of stating the contract rather
    /// than an algorithm: an ordered structure indexes into it, while SQL and a search
    /// backend each answer with one count query over the ordering
    /// [`Store::claim_next_skipping`] already owes.
    ///
    /// Approximate the way SQS's counts are approximate, and rather more so: it is true
    /// at the instant it was computed, and a higher-priority arrival moves a message
    /// backwards. A caller treating it as a countdown will be wrong.
    async fn position_of(
        &self,
        queue: &QueueName,
        message: &MessageId,
    ) -> Result<Option<QueuePosition>>;

    /// Delete a queue and everything in it.
    async fn delete_queue(&self, name: &QueueName) -> Result<()>;

    /// Delete every message in a queue, keeping the queue itself.
    ///
    /// Returns how many were removed, which a caller can log — this throws away data
    /// irrecoverably, so how much it threw away is worth recording.
    ///
    /// Claimed messages go too. A consumer holding a receipt handle across a purge finds
    /// it invalid afterwards, which is correct: the message it named is gone.
    async fn purge_queue(&self, name: &QueueName) -> Result<u64>;

    /// Every queue, in no particular order.
    ///
    /// Unpaged and unfiltered on purpose while the surface stays small; prefix
    /// matching and paging come with the operations that need them, and belong here
    /// rather than in the caller so a backend can push them down to storage.
    async fn list_queues(&self) -> Result<Vec<Queue>>;

    /// Add a message to a queue.
    ///
    /// `delay` is how long before the message may be claimed; `None` means the queue's
    /// configured delay. The option is resolved here rather than by the caller because
    /// the backend already holds the queue record, so reading the default costs it
    /// nothing while the caller would need an extra round trip.
    async fn enqueue(
        &self,
        queue: &QueueName,
        message: Message,
        delay: Option<Duration>,
    ) -> Result<()>;

    /// Claim the next message, or `None` if there is nothing claimable.
    ///
    /// "Next" is the backend's judgement, but every backend owes the same contract:
    /// higher [`crate::model::Priority`] first, and within one priority the earliest
    /// enqueued first.
    ///
    /// A claim makes the message invisible to other consumers until it expires, at
    /// which point the message becomes claimable again — under a *new* receipt handle,
    /// which invalidates the old one. `visibility_timeout` of `None` means the queue's
    /// configured default.
    ///
    /// Claiming also increments the message's receive count and, on first delivery,
    /// records when that happened.
    ///
    /// Provided, not implemented: it is [`Store::claim_next_skipping`] with nothing to
    /// skip. A backend implements that one and gets this for free.
    async fn claim_next(
        &self,
        queue: &QueueName,
        visibility_timeout: Option<Duration>,
    ) -> Result<Option<ClaimedMessage>> {
        self.claim_next_skipping(queue, visibility_timeout, &[])
            .await
    }

    /// The same, passing over messages the caller already holds.
    ///
    /// This is the form a backend implements, and `skip` is not a convenience — it is
    /// what makes a batch of claims correct. A `visibility_timeout` of zero expires a
    /// claim the instant it is made, so the message it claimed is immediately claimable
    /// again *by the very loop assembling the batch*: without a way to say "not these",
    /// a receive asking for ten messages with a zero timeout gets one message ten times,
    /// each delivery incrementing its receive count and invalidating the handle handed
    /// out a moment earlier. Ordering makes it certain rather than likely, since the
    /// message that ranks first still ranks first the next time it is asked for.
    ///
    /// A skipped message is passed over, not treated as the end of the queue: the answer
    /// is the first claimable message that is not named, and `None` only when there is no
    /// such message. `skip` holds at most
    /// [`MAX_MESSAGES_PER_RECEIVE`](crate::engine::MAX_MESSAGES_PER_RECEIVE) ids, so a
    /// backend may push it down to storage as a literal list — `NOT IN` for SQL, a
    /// `must_not` clause for a search backend — rather than needing an anti-join.
    async fn claim_next_skipping(
        &self,
        queue: &QueueName,
        visibility_timeout: Option<Duration>,
        skip: &[MessageId],
    ) -> Result<Option<ClaimedMessage>>;

    /// Delete a claimed message, ending the claim for good.
    ///
    /// Fails with [`StoreError::InvalidReceipt`] unless the handle identifies a claim
    /// that is still current — so a consumer whose claim expired and was redelivered to
    /// someone else cannot delete the other consumer's message.
    async fn ack(&self, queue: &QueueName, receipt: &ReceiptHandle) -> Result<()>;

    /// Reset how long a claim has left to run.
    ///
    /// `visibility_timeout` is measured from *now*, not from when the message was
    /// claimed, so this both extends a claim that needs longer and shortens one that
    /// does not. Zero makes the message claimable immediately, which is how a consumer
    /// hands work back instead of sitting on it until the claim lapses.
    ///
    /// The handle stays valid: this changes when the claim ends, not whose it is. Fails
    /// with [`StoreError::InvalidReceipt`] on the same terms as
    /// [`Store::ack`] — a handle whose claim has already ended names nothing to change.
    async fn change_visibility(
        &self,
        queue: &QueueName,
        receipt: &ReceiptHandle,
        visibility_timeout: Duration,
    ) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::test_support::FakeStore;

    fn queue(name: &str) -> Queue {
        Queue::new(QueueName::new(name).expect("valid name"))
    }

    /// The shape the engine will hold a backend in.
    fn store() -> Arc<dyn Store> {
        Arc::new(FakeStore::default())
    }

    #[tokio::test]
    async fn a_store_is_usable_behind_dyn() {
        let store = store();

        assert_eq!(store.backend_name(), "fake");
        store.create_queue(queue("jobs")).await.expect("create");

        let found = store
            .get_queue(&QueueName::new("jobs").expect("valid"))
            .await
            .expect("get");
        assert_eq!(found.name.as_str(), "jobs");
    }

    #[tokio::test]
    async fn a_store_can_be_shared_across_tasks() {
        let store = store();

        // Two tasks writing through the same `Arc<dyn Store>` is the real usage
        // pattern, so it needs to compile and run without a lifetime in sight.
        let first = Arc::clone(&store);
        let second = Arc::clone(&store);

        let (left, right) = tokio::join!(
            tokio::spawn(async move { first.create_queue(queue("one")).await }),
            tokio::spawn(async move { second.create_queue(queue("two")).await }),
        );
        left.expect("task").expect("create");
        right.expect("task").expect("create");

        assert_eq!(store.list_queues().await.expect("list").len(), 2);
    }

    #[tokio::test]
    async fn creating_a_queue_twice_is_an_error_rather_than_an_overwrite() {
        let store = store();
        store.create_queue(queue("jobs")).await.expect("create");

        let error = store
            .create_queue(queue("jobs"))
            .await
            .expect_err("already exists");

        assert!(
            matches!(&error, StoreError::QueueAlreadyExists(name) if name.as_str() == "jobs"),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn acting_on_a_missing_queue_reports_which_one() {
        let store = store();
        let missing = QueueName::new("nope").expect("valid");

        for error in [
            store.get_queue(&missing).await.expect_err("get"),
            store.delete_queue(&missing).await.expect_err("delete"),
        ] {
            assert!(
                matches!(&error, StoreError::QueueNotFound(name) if name == &missing),
                "{error:?}"
            );
            assert_eq!(error.to_string(), "queue does not exist: nope");
        }
    }

    #[tokio::test]
    async fn a_deleted_queue_is_gone() {
        let store = store();
        let name = QueueName::new("jobs").expect("valid");
        store.create_queue(queue("jobs")).await.expect("create");

        store.delete_queue(&name).await.expect("delete");

        store.get_queue(&name).await.expect_err("deleted");
        assert!(store.list_queues().await.expect("list").is_empty());
    }

    #[tokio::test]
    async fn listing_an_empty_store_yields_nothing() {
        assert!(store().list_queues().await.expect("list").is_empty());
    }

    #[test]
    fn a_backend_failure_keeps_its_cause() {
        let cause = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "no route");
        let error = StoreError::backend(cause);

        assert_eq!(error.to_string(), "backend failure: no route");
        assert!(
            std::error::Error::source(&error).is_some(),
            "the underlying error must stay reachable for logging"
        );
    }
}
