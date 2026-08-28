//! The engine: the operation set NexQ actually offers.
//!
//! Every facade is a translation layer over this — the SQS and SNS facades expose a
//! subset in AWS's wire format, REST exposes all of it natively — so behavior that
//! should be the same whichever protocol asked for it belongs here, not in a facade.
//! Queue creation being idempotent is the first example: it is decided once, and both
//! facades inherit it.
//!
//! What the engine adds over a bare [`Store`]:
//!
//! - server-owned fields, so a client cannot dictate a queue's creation time
//! - semantics a single storage call cannot express, like idempotent creation
//! - one error type describing outcomes a caller can act on
//! - waiting for a message to arrive, which no single storage call can express
//!
//! That last one is why the engine holds a [`Waiters`] registry as well as a store: long
//! polling is about holding a request open until an enqueue happens, so it belongs
//! wherever both the enqueue and the waiting consumer can be seen at once.
//!
//! One store is held for now; per-queue backend selection arrives with the config that
//! describes it, and belongs here since routing a queue to its backend is exactly this
//! layer's job.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use thiserror::Error;
use tokio::time::{Instant, timeout};

use crate::model::{
    ClaimedMessage, MAX_BODY_BYTES, Message, MessageAttributes, MessageCounts, MessageId, Priority,
    Queue, QueueAttributes, QueueName, ReceiptHandle,
};
use crate::store::{Store, StoreError};
use crate::waiters::Waiters;

/// The result of an engine operation.
pub type Result<T> = std::result::Result<T, EngineError>;

/// How many times [`Engine::create_queue`] will retry a create that lost a race.
const CREATE_ATTEMPTS: usize = 2;

/// Most queues returned by one [`Engine::list_queues`] call, matching SQS's own cap.
///
/// Applied whether or not a caller asked for a limit, so a deployment with more queues
/// than this is paged rather than silently truncated.
pub const MAX_QUEUES_PER_PAGE: usize = 1000;

/// Which queues to list, and how many.
#[derive(Debug, Clone, Default)]
pub struct QueueQuery {
    /// Only queues whose name starts with this.
    pub prefix: Option<String>,

    /// How many to return, capped at [`MAX_QUEUES_PER_PAGE`].
    pub limit: Option<usize>,

    /// Resume after this name — the cursor from a previous page.
    pub after: Option<QueueName>,
}

/// Longest a receive will wait for a message, matching SQS's cap on `WaitTimeSeconds`.
///
/// Applied here as well as in a facade so a caller cannot hold a request open
/// indefinitely by asking for a longer wait than the protocol allows.
pub const MAX_RECEIVE_WAIT: Duration = Duration::from_secs(20);

/// Most messages one receive will return, matching SQS's `MaxNumberOfMessages` cap.
pub const MAX_MESSAGES_PER_RECEIVE: usize = 10;

/// What a consumer is asking for.
#[derive(Debug, Clone, Default)]
pub struct ReceiveRequest {
    /// How many messages to return at most, capped at [`MAX_MESSAGES_PER_RECEIVE`]. A
    /// receive may return fewer, including none.
    pub max_messages: usize,

    /// How long a returned message stays invisible to other consumers. `None` means the
    /// queue's configured default.
    pub visibility_timeout: Option<Duration>,

    /// How long to wait for a message when the queue has none — long polling. `None`
    /// means the queue's configured `receive_wait_time`, and zero means do not wait.
    /// Capped at [`MAX_RECEIVE_WAIT`].
    pub wait: Option<Duration>,
}

/// One page of queues.
#[derive(Debug, Clone)]
pub struct QueuePage {
    pub queues: Vec<Queue>,

    /// Cursor to pass as [`QueueQuery::after`] for the next page, or `None` when this
    /// is the last one.
    pub next: Option<QueueName>,
}

/// Why an engine operation failed.
///
/// Flat on purpose: a facade maps these to its own wire errors, and should not have to
/// dig through nested storage errors to do it.
#[derive(Debug, Error)]
pub enum EngineError {
    /// No queue by that name.
    #[error("queue does not exist: {0}")]
    QueueNotFound(QueueName),

    /// A queue by that name exists with *different* attributes. Creating one that
    /// already exists with matching attributes is not an error — see
    /// [`Engine::create_queue`].
    #[error("queue already exists with different attributes: {0}")]
    QueueAlreadyExists(QueueName),

    /// The queue was created and deleted by someone else while this operation was
    /// deciding what to do, more than once running. Retrying is the right response.
    #[error("queue {0} is being created and deleted concurrently; retry")]
    Conflict(QueueName),

    /// The message body is over [`MAX_BODY_BYTES`].
    #[error("message body is {bytes} bytes, over the {MAX_BODY_BYTES} byte limit")]
    MessageTooLarge { bytes: usize },

    /// The receipt handle does not identify a claim that is still current.
    #[error("the receipt handle does not identify a current claim")]
    InvalidReceipt,

    /// The storage backend failed.
    #[error("backend failure: {0}")]
    Backend(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<StoreError> for EngineError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::QueueNotFound(name) => Self::QueueNotFound(name),
            StoreError::QueueAlreadyExists(name) => Self::QueueAlreadyExists(name),
            StoreError::InvalidReceipt => Self::InvalidReceipt,
            StoreError::Backend(source) => Self::Backend(source),
        }
    }
}

/// The queueing operations, over whatever backend holds the data.
#[derive(Debug)]
pub struct Engine {
    store: Arc<dyn Store>,

    /// Consumers waiting for a message, so an enqueue can wake one instead of leaving
    /// it to poll. In process, which is why a queue's traffic belongs on one node.
    waiters: Waiters,

    /// Set once the process is going away, after which nothing waits. See
    /// [`Engine::begin_draining`].
    draining: AtomicBool,
}

impl Engine {
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self {
            store,
            waiters: Waiters::new(),
            draining: AtomicBool::new(false),
        }
    }

    /// Stop long polls from waiting, because the process is shutting down.
    ///
    /// Without this a graceful shutdown would take as long as the longest wait in
    /// flight, since a consumer parked for twenty seconds is an in-flight request and a
    /// server that waits for those would wait for that. Waiters are released to return
    /// their normal empty answer, which is a thing consumers already handle, rather than
    /// having their connections dropped from under them.
    ///
    /// Only *waiting* stops. Requests still run, so a receive during the drain behaves
    /// like a plain poll and anything already claimable still comes back.
    ///
    /// One way, and deliberately: a draining engine is one whose process is going away,
    /// so there is nothing to undo. Calling it twice is harmless.
    pub fn begin_draining(&self) {
        self.draining.store(true, Ordering::SeqCst);
        self.waiters.notify_everything();
    }

    /// Whether waiting has been switched off.
    pub fn is_draining(&self) -> bool {
        self.draining.load(Ordering::SeqCst)
    }

    /// Create a queue, returning it as stored.
    ///
    /// **Idempotent when the attributes match.** Asking for a queue that already
    /// exists with the same attributes returns the existing queue rather than failing,
    /// because the caller's intent — "there should be a queue named this, configured
    /// like this" — is already satisfied. Asking with *different* attributes is an
    /// error, since honouring it would mean either silently ignoring the new
    /// attributes or silently reconfiguring a live queue. This matches how SQS
    /// behaves, and applies to every facade because it is decided here.
    ///
    /// `created_at` is set here, not taken from the caller: it records when the server
    /// accepted the queue, which is not a client's to assert.
    pub async fn create_queue(
        &self,
        name: QueueName,
        attributes: QueueAttributes,
    ) -> Result<Queue> {
        for _ in 0..CREATE_ATTEMPTS {
            let created_at = SystemTime::now();
            let queue = Queue {
                name: name.clone(),
                created_at,
                // Never modified, so the two agree until something changes it.
                last_modified_at: created_at,
                attributes: attributes.clone(),
            };

            match self.store.create_queue(queue.clone()).await {
                Ok(()) => return Ok(queue),

                Err(StoreError::QueueAlreadyExists(existing_name)) => {
                    match self.store.get_queue(&existing_name).await {
                        // Already as the caller wants it, so their intent holds.
                        Ok(existing) if existing.attributes == attributes => return Ok(existing),
                        Ok(_) => return Err(EngineError::QueueAlreadyExists(existing_name)),

                        // It existed a moment ago and is gone now, so someone deleted
                        // it in between. Nothing is wrong; try to create it again.
                        Err(StoreError::QueueNotFound(_)) => continue,

                        Err(other) => return Err(other.into()),
                    }
                }

                Err(other) => return Err(other.into()),
            }
        }

        // Losing the race twice means real churn on this name rather than an unlucky
        // interleaving, and the caller is better placed to decide whether to keep
        // trying than a loop here is.
        Err(EngineError::Conflict(name))
    }

    /// Look up a queue.
    pub async fn get_queue(&self, name: &QueueName) -> Result<Queue> {
        Ok(self.store.get_queue(name).await?)
    }

    /// Change a queue's attributes, returning it as stored.
    ///
    /// `change` is handed the queue's current attributes and returns what they should
    /// become, which is how a caller applies a *partial* update — SQS's
    /// `SetQueueAttributes` names only the attributes it wants changed, and the rest
    /// have to be left as they are rather than reset to defaults.
    ///
    /// `last_modified_at` is stamped here, not taken from the caller, for the same
    /// reason a queue's creation time is: it records when the server accepted the
    /// change.
    ///
    /// Read-modify-write, so two changes racing on one queue end with one of them
    /// winning whole rather than the two being merged. Reconfiguring a queue is a rare,
    /// deliberate act, and the machinery to do better — a compare-and-swap through every
    /// backend — would cost more than the problem is worth.
    ///
    /// `change` may fail with an error of its own — a facade rejecting an attribute it
    /// does not support, say — so the error type is the caller's as long as engine
    /// failures can convert into it. That is what stops a facade having to smuggle its
    /// own error out through a storage-shaped one.
    pub async fn set_queue_attributes<F, E>(
        &self,
        name: &QueueName,
        change: F,
    ) -> std::result::Result<Queue, E>
    where
        F: FnOnce(QueueAttributes) -> std::result::Result<QueueAttributes, E>,
        E: From<EngineError>,
    {
        let existing = self.get_queue(name).await.map_err(E::from)?;
        let attributes = change(existing.attributes)?;
        let modified_at = SystemTime::now();

        self.store
            .set_queue_attributes(name, attributes.clone(), modified_at)
            .await
            .map_err(EngineError::from)
            .map_err(E::from)?;

        Ok(Queue {
            attributes,
            last_modified_at: modified_at,
            ..existing
        })
    }

    /// How many messages a queue holds, split by visibility.
    pub async fn message_counts(&self, name: &QueueName) -> Result<MessageCounts> {
        Ok(self.store.message_counts(name).await?)
    }

    /// Delete a queue and everything in it.
    ///
    /// Deleting a queue that is not there is an error rather than a no-op: a client
    /// that deletes an unknown name has misunderstood something, and SQS reports it.
    pub async fn delete_queue(&self, name: &QueueName) -> Result<()> {
        self.store.delete_queue(name).await?;

        // Anyone long-polling this queue should find out now rather than wait out a
        // timeout on a queue that no longer exists. Woken before the entry goes, since
        // forgetting it first would leave them with nothing to wake them.
        self.waiters.notify_all(name);
        self.waiters.forget(name);

        Ok(())
    }

    /// Delete every message in a queue, keeping the queue.
    ///
    /// Returns how many were removed. **Irreversible, and it takes in-flight messages
    /// with it** — a consumer that is working on a message right now will find its
    /// receipt handle invalid, because the message is gone.
    ///
    /// No rate limit, unlike SQS, which refuses a second purge within sixty seconds with
    /// `PurgeQueueInProgress`. That limit exists because SQS's purge is asynchronous and
    /// takes up to a minute to finish; this one has finished when it returns, so there is
    /// no window to protect and refusing would be a limitation invented for its own sake.
    pub async fn purge_queue(&self, name: &QueueName) -> Result<u64> {
        Ok(self.store.purge_queue(name).await?)
    }

    /// One page of queues, in name order.
    ///
    /// Filtering and paging live here rather than in a facade so every protocol gets
    /// the same answer for the same question. Both are applied after loading for now;
    /// when the store learns to do them, this pushes down into it so a backend with
    /// many queues need not send them all back.
    ///
    /// **Paging is by cursor, not by offset.** The cursor is the last name returned, so
    /// the next page is "everything after that name". Queues created or deleted between
    /// pages therefore cannot make a caller skip or repeat one that was present
    /// throughout, which an offset would. Ordering by name is what makes that work, so
    /// it is part of the contract rather than incidental.
    pub async fn list_queues(&self, query: &QueueQuery) -> Result<QueuePage> {
        let mut queues = self.store.list_queues().await?;

        if let Some(prefix) = &query.prefix {
            queues.retain(|queue| queue.name.as_str().starts_with(prefix));
        }

        queues.sort_by(|left, right| left.name.cmp(&right.name));

        if let Some(after) = &query.after {
            queues.retain(|queue| &queue.name > after);
        }

        let limit = query
            .limit
            .unwrap_or(MAX_QUEUES_PER_PAGE)
            .clamp(1, MAX_QUEUES_PER_PAGE);
        let has_more = queues.len() > limit;
        queues.truncate(limit);

        // The cursor is the last name on this page, so the next page starts after it.
        let next = has_more
            .then(|| queues.last().map(|queue| queue.name.clone()))
            .flatten();

        Ok(QueuePage { queues, next })
    }

    /// Add a message to a queue, returning it as stored.
    ///
    /// The identifier and enqueue time are minted here, not accepted from the caller,
    /// for the same reason a queue's creation time is: they record what the server did.
    /// The returned message is what a caller needs to answer with — an id to report, a
    /// body to checksum.
    ///
    /// `delay` of `None` means the queue's configured delay.
    pub async fn enqueue(
        &self,
        queue: &QueueName,
        body: String,
        priority: Priority,
        attributes: MessageAttributes,
        delay: Option<Duration>,
    ) -> Result<Message> {
        let message = Message::new(body, priority).with_attributes(attributes);

        // Checked before the store is touched: a message over the limit is the caller's
        // mistake, and no backend should have to decide what to do about it. Attributes
        // count towards the limit, so this is not the body's length alone.
        if !message.within_size_limit() {
            return Err(EngineError::MessageTooLarge {
                bytes: message.size_bytes(),
            });
        }

        self.store.enqueue(queue, message.clone(), delay).await?;

        // One message, so one waiter. Done unconditionally rather than only for a
        // message that is visible immediately: a delayed one wakes a consumer that finds
        // nothing and goes back to waiting, which costs one claim, while working out
        // whether it *would* be visible costs a read of the queue on every send. What
        // this does not do is wake anyone when a delay elapses or a claim expires —
        // those need a timer, not an event, and until there is one a consumer learns
        // about them on its next receive.
        self.waiters.notify_one(queue);

        Ok(message)
    }

    /// Claim up to `request.max_messages` messages, waiting for the first if asked to.
    ///
    /// This is long polling, and it is the reason the engine holds a waiter registry.
    /// The wait applies only to the *first* message: once something is available the
    /// rest of the batch is whatever else can be claimed right now, so a consumer
    /// asking for ten is not held open until ten exist. That is SQS's behaviour, and it
    /// is the useful one — a batch that waits to fill trades latency for nothing.
    ///
    /// A wait of zero, or a queue configured with none, makes this a plain poll.
    /// Returning empty is a normal answer either way.
    pub async fn receive(
        &self,
        queue: &QueueName,
        request: &ReceiveRequest,
    ) -> Result<Vec<ClaimedMessage>> {
        let wanted = request.max_messages.clamp(1, MAX_MESSAGES_PER_RECEIVE);

        let Some(first) = self.claim_waiting(queue, request).await? else {
            return Ok(Vec::new());
        };

        let mut claimed = Vec::with_capacity(wanted);
        // What has already been handed to this consumer, so the store passes over it. A
        // `visibility_timeout` of zero expires each claim as it is made, which would
        // otherwise leave the highest-ranked message ranked first again on the next
        // iteration — a batch of ten copies of one message, nine of them under receipt
        // handles the tenth claim had already invalidated.
        let mut held: Vec<MessageId> = Vec::with_capacity(wanted);

        held.push(first.message.id.clone());
        claimed.push(first);

        while claimed.len() < wanted {
            match self
                .store
                .claim_next_skipping(queue, request.visibility_timeout, &held)
                .await?
            {
                Some(message) => {
                    held.push(message.message.id.clone());
                    claimed.push(message);
                }
                // Nothing more right now, which is a normal short batch.
                None => break,
            }
        }

        Ok(claimed)
    }

    /// The first message of a receive, waiting for one to arrive if the queue is empty.
    ///
    /// The ordering here is what makes the wait reliable, and it is not incidental: the
    /// waiter is armed *before* the queue is looked at, so a message enqueued between
    /// the look and the wait wakes it rather than being missed. Every later iteration
    /// re-arms before re-checking for the same reason.
    async fn claim_waiting(
        &self,
        queue: &QueueName,
        request: &ReceiveRequest,
    ) -> Result<Option<ClaimedMessage>> {
        let notify = self.waiters.register(queue);
        let notified = notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        if let Some(message) = self.claim_next(queue, request.visibility_timeout).await? {
            return Ok(Some(message));
        }

        // Checked before the queue is read for its wait, so a drain costs nothing.
        if self.is_draining() {
            return Ok(None);
        }

        // Only worth resolving now: a receive that found a message never pays to read
        // the queue's configured wait.
        let wait = self.resolve_wait(queue, request).await?;
        if wait.is_zero() {
            return Ok(None);
        }

        let deadline = Instant::now() + wait;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }

            // The waiter armed above covers anything enqueued since the last check, so
            // this cannot sleep through a message that is already there.
            if timeout(remaining, notified.as_mut()).await.is_err() {
                return Ok(None);
            }

            // Re-armed before looking, so a message arriving during the look is not lost.
            notified.set(notify.notified());
            notified.as_mut().enable();

            // The wake may have been a drain rather than a message. Checked after
            // looking would work too, but there is no reason to touch the store.
            if self.is_draining() {
                return Ok(None);
            }

            // Deciding which message only now is the point: what a woken consumer gets
            // is whatever ranks first at this instant, not whatever ranked first when it
            // started waiting.
            if let Some(message) = self.claim_next(queue, request.visibility_timeout).await? {
                return Ok(Some(message));
            }

            // Another consumer got there first. Keep waiting out the deadline.
        }
    }

    /// How long a receive should wait, honouring the queue's default and SQS's cap.
    async fn resolve_wait(&self, queue: &QueueName, request: &ReceiveRequest) -> Result<Duration> {
        let wait = match request.wait {
            Some(wait) => wait,
            None => self.get_queue(queue).await?.attributes.receive_wait_time,
        };

        Ok(wait.min(MAX_RECEIVE_WAIT))
    }

    /// Claim the next message for a consumer, or `None` if the queue has nothing
    /// claimable right now.
    ///
    /// This returns immediately either way. Waiting for a message to arrive — long
    /// polling — is a separate concern that belongs above this call, since it is about
    /// holding a request open rather than about storage.
    ///
    /// `visibility_timeout` of `None` means the queue's configured default.
    pub async fn claim_next(
        &self,
        queue: &QueueName,
        visibility_timeout: Option<Duration>,
    ) -> Result<Option<ClaimedMessage>> {
        Ok(self.store.claim_next(queue, visibility_timeout).await?)
    }

    /// Delete a claimed message.
    ///
    /// A message is only gone once this is called: until then an expired claim means
    /// redelivery, which is what makes delivery at-least-once rather than at-most-once.
    pub async fn ack(&self, queue: &QueueName, receipt: &ReceiptHandle) -> Result<()> {
        Ok(self.store.ack(queue, receipt).await?)
    }

    /// Reset how long a claim has left, counted from now.
    ///
    /// Two jobs in one operation, which is SQS's shape rather than a choice made here.
    /// A consumer that needs longer than it asked for extends its claim instead of
    /// letting the message be handed to someone else mid-work. A consumer that cannot
    /// do the work sets zero, which puts the message back immediately rather than
    /// leaving the queue to wait out a timeout for work nobody is doing.
    ///
    /// That second case is a client action making a message claimable, so it wakes a
    /// waiting consumer exactly as an enqueue does — a message handed back is available
    /// now, and nobody should sit through a long poll next to it.
    pub async fn change_visibility(
        &self,
        queue: &QueueName,
        receipt: &ReceiptHandle,
        visibility_timeout: Duration,
    ) -> Result<()> {
        self.store
            .change_visibility(queue, receipt, visibility_timeout)
            .await?;

        // Only when the message is claimable *now*. A non-zero timeout leaves it
        // invisible, and waking a consumer to find that out would be a wasted trip to
        // the store — unlike an enqueue, where the effective delay is not known here.
        if visibility_timeout.is_zero() {
            self.waiters.notify_one(queue);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use async_trait::async_trait;

    use super::*;
    use crate::store::Result as StoreResult;
    use crate::test_support::{BrokenStore, FakeStore};

    fn engine() -> Engine {
        Engine::new(Arc::new(FakeStore::new()))
    }

    fn name(name: &str) -> QueueName {
        QueueName::new(name).expect("valid queue name")
    }

    fn attributes(visibility_secs: u64) -> QueueAttributes {
        QueueAttributes {
            visibility_timeout: Duration::from_secs(visibility_secs),
            ..QueueAttributes::default()
        }
    }

    #[tokio::test]
    async fn creating_a_queue_returns_it_as_stored() {
        let engine = engine();
        let before = SystemTime::now();

        let created = engine
            .create_queue(name("jobs"), attributes(30))
            .await
            .expect("create");

        assert_eq!(created.name, name("jobs"));
        assert_eq!(created.attributes, attributes(30));
        assert!(
            created.created_at >= before,
            "created_at is stamped by the server"
        );
        assert_eq!(
            engine.get_queue(&name("jobs")).await.expect("get"),
            created,
            "what was returned is what was stored"
        );
    }

    #[tokio::test]
    async fn creating_the_same_queue_again_is_idempotent() {
        let engine = engine();
        let first = engine
            .create_queue(name("jobs"), attributes(30))
            .await
            .expect("create");

        let second = engine
            .create_queue(name("jobs"), attributes(30))
            .await
            .expect("same attributes, so the caller's intent already holds");

        assert_eq!(
            first, second,
            "the existing queue is returned, not a fresh one"
        );
    }

    #[tokio::test]
    async fn recreating_a_queue_with_different_attributes_is_an_error() {
        let engine = engine();
        let original = engine
            .create_queue(name("jobs"), attributes(30))
            .await
            .expect("create");

        let error = engine
            .create_queue(name("jobs"), attributes(600))
            .await
            .expect_err("different attributes");

        assert!(
            matches!(&error, EngineError::QueueAlreadyExists(n) if n == &name("jobs")),
            "{error:?}"
        );
        assert_eq!(
            engine.get_queue(&name("jobs")).await.expect("get"),
            original,
            "the live queue must not have been reconfigured"
        );
    }

    #[tokio::test]
    async fn getting_a_missing_queue_reports_which_one() {
        let error = engine()
            .get_queue(&name("nope"))
            .await
            .expect_err("missing");

        assert!(
            matches!(&error, EngineError::QueueNotFound(n) if n == &name("nope")),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn deleting_a_queue_removes_it() {
        let engine = engine();
        engine
            .create_queue(name("jobs"), attributes(30))
            .await
            .expect("create");

        engine.delete_queue(&name("jobs")).await.expect("delete");

        engine.get_queue(&name("jobs")).await.expect_err("deleted");
        assert!(
            listed_names(&engine, &QueueQuery::default())
                .await
                .is_empty()
        );
    }

    #[tokio::test]
    async fn deleting_a_missing_queue_is_an_error_not_a_no_op() {
        let error = engine()
            .delete_queue(&name("nope"))
            .await
            .expect_err("missing");

        assert!(
            matches!(&error, EngineError::QueueNotFound(n) if n == &name("nope")),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn listing_returns_every_queue_in_name_order() {
        let engine = engine();
        // Created out of order, so the ordering is the engine's doing.
        for queue_name in ["two", "one", "three"] {
            engine
                .create_queue(name(queue_name), QueueAttributes::default())
                .await
                .expect("create");
        }

        assert_eq!(
            listed_names(&engine, &QueueQuery::default()).await,
            ["one", "three", "two"],
            "name order, since that is what makes cursor paging stable"
        );
    }

    #[tokio::test]
    async fn listing_can_be_limited_to_a_prefix() {
        let engine = engine();
        for queue_name in ["jobs", "jobs_dlq", "emails"] {
            engine
                .create_queue(name(queue_name), QueueAttributes::default())
                .await
                .expect("create");
        }

        let matching = QueueQuery {
            prefix: Some("jobs".to_owned()),
            ..QueueQuery::default()
        };
        assert_eq!(listed_names(&engine, &matching).await, ["jobs", "jobs_dlq"]);

        let nothing = QueueQuery {
            prefix: Some("nothing-matches".to_owned()),
            ..QueueQuery::default()
        };
        assert!(
            listed_names(&engine, &nothing).await.is_empty(),
            "a prefix matching nothing is an empty list, not an error"
        );
    }

    #[tokio::test]
    async fn listing_an_empty_deployment_yields_nothing() {
        let page = engine()
            .list_queues(&QueueQuery::default())
            .await
            .expect("list");

        assert!(page.queues.is_empty());
        assert!(page.next.is_none(), "nothing to continue from");
    }

    #[tokio::test]
    async fn a_limit_pages_the_results() {
        let engine = engine();
        for queue_name in ["a", "b", "c"] {
            engine
                .create_queue(name(queue_name), QueueAttributes::default())
                .await
                .expect("create");
        }

        let first = engine
            .list_queues(&QueueQuery {
                limit: Some(2),
                ..QueueQuery::default()
            })
            .await
            .expect("first page");

        assert_eq!(names_of(&first), ["a", "b"]);
        assert_eq!(
            first.next.as_ref(),
            Some(&name("b")),
            "the cursor is the last name returned"
        );

        let second = engine
            .list_queues(&QueueQuery {
                limit: Some(2),
                after: first.next,
                ..QueueQuery::default()
            })
            .await
            .expect("second page");

        assert_eq!(names_of(&second), ["c"]);
        assert!(second.next.is_none(), "the last page has no cursor");
    }

    #[tokio::test]
    async fn walking_the_pages_visits_every_queue_once() {
        let engine = engine();
        let expected: Vec<String> = (0..10).map(|index| format!("q{index}")).collect();
        for queue_name in &expected {
            engine
                .create_queue(name(queue_name), QueueAttributes::default())
                .await
                .expect("create");
        }

        let mut seen = Vec::new();
        let mut cursor = None;
        loop {
            let page = engine
                .list_queues(&QueueQuery {
                    limit: Some(3),
                    after: cursor,
                    ..QueueQuery::default()
                })
                .await
                .expect("page");

            seen.extend(names_of(&page));
            match page.next {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }

        assert_eq!(seen, expected, "every queue exactly once, in order");
    }

    #[tokio::test]
    async fn a_cursor_survives_the_queue_it_names_being_deleted() {
        // The reason for cursor paging rather than an offset: churn between pages must
        // not make a caller skip or repeat a queue that was there all along.
        let engine = engine();
        for queue_name in ["a", "b", "c", "d"] {
            engine
                .create_queue(name(queue_name), QueueAttributes::default())
                .await
                .expect("create");
        }

        let first = engine
            .list_queues(&QueueQuery {
                limit: Some(2),
                ..QueueQuery::default()
            })
            .await
            .expect("first page");
        assert_eq!(names_of(&first), ["a", "b"]);

        // Everything already returned disappears, cursor included.
        engine.delete_queue(&name("a")).await.expect("delete");
        engine.delete_queue(&name("b")).await.expect("delete");

        let second = engine
            .list_queues(&QueueQuery {
                limit: Some(2),
                after: first.next,
                ..QueueQuery::default()
            })
            .await
            .expect("second page");

        assert_eq!(
            names_of(&second),
            ["c", "d"],
            "the rest still follow the cursor's name"
        );
    }

    #[tokio::test]
    async fn a_limit_beyond_the_cap_is_clamped_rather_than_refused() {
        let engine = engine();
        engine
            .create_queue(name("jobs"), QueueAttributes::default())
            .await
            .expect("create");

        for limit in [0, MAX_QUEUES_PER_PAGE + 1, usize::MAX] {
            let page = engine
                .list_queues(&QueueQuery {
                    limit: Some(limit),
                    ..QueueQuery::default()
                })
                .await
                .unwrap_or_else(|error| panic!("limit {limit}: {error}"));

            assert_eq!(names_of(&page), ["jobs"], "limit {limit}");
        }
    }

    #[tokio::test]
    async fn a_prefix_and_a_limit_apply_together() {
        let engine = engine();
        for queue_name in ["jobs_a", "jobs_b", "jobs_c", "emails"] {
            engine
                .create_queue(name(queue_name), QueueAttributes::default())
                .await
                .expect("create");
        }

        let page = engine
            .list_queues(&QueueQuery {
                prefix: Some("jobs".to_owned()),
                limit: Some(2),
                after: None,
            })
            .await
            .expect("page");

        assert_eq!(names_of(&page), ["jobs_a", "jobs_b"]);

        let rest = engine
            .list_queues(&QueueQuery {
                prefix: Some("jobs".to_owned()),
                limit: Some(2),
                after: page.next,
            })
            .await
            .expect("page");

        assert_eq!(
            names_of(&rest),
            ["jobs_c"],
            "the prefix still applies on later pages"
        );
    }

    fn names_of(page: &QueuePage) -> Vec<String> {
        page.queues
            .iter()
            .map(|queue| queue.name.to_string())
            .collect()
    }

    async fn listed_names(engine: &Engine, query: &QueueQuery) -> Vec<String> {
        names_of(&engine.list_queues(query).await.expect("list"))
    }

    #[tokio::test]
    async fn a_backend_failure_stays_a_backend_failure() {
        let engine = Engine::new(Arc::new(BrokenStore));

        let error = engine
            .create_queue(name("jobs"), QueueAttributes::default())
            .await
            .expect_err("backend is down");

        assert!(matches!(error, EngineError::Backend(_)), "{error:?}");
        assert!(
            error.to_string().contains(BrokenStore::FAILURE),
            "the cause must survive: {error}"
        );
    }

    /// Create a queue and put one message in it, returning the queue's name.
    async fn queue_with_message(engine: &Engine, body: &str) -> QueueName {
        let queue = name("jobs");
        engine
            .create_queue(queue.clone(), QueueAttributes::default())
            .await
            .expect("create queue");
        engine
            .enqueue(
                &queue,
                body.to_owned(),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");

        queue
    }

    #[tokio::test]
    async fn an_enqueued_message_gets_server_owned_fields() {
        let engine = engine();
        let queue = name("jobs");
        engine
            .create_queue(queue.clone(), QueueAttributes::default())
            .await
            .expect("create queue");
        let before = SystemTime::now();

        let message = engine
            .enqueue(
                &queue,
                "hello".to_owned(),
                Priority::new(5),
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");

        assert_eq!(message.body, "hello");
        assert_eq!(message.priority, Priority::new(5));
        assert_eq!(message.receive_count, 0);
        assert_eq!(message.first_received_at, None);
        assert!(message.enqueued_at >= before, "stamped by the server");
        assert!(!message.id.as_str().is_empty(), "given an id");
    }

    #[tokio::test]
    async fn enqueueing_to_a_missing_queue_is_an_error() {
        let error = engine()
            .enqueue(
                &name("nope"),
                "hello".to_owned(),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect_err("no such queue");

        assert!(
            matches!(&error, EngineError::QueueNotFound(n) if n == &name("nope")),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn an_oversized_body_is_refused_before_the_store_is_touched() {
        let engine = Engine::new(Arc::new(BrokenStore));

        // A backend that fails everything: reaching it would surface as a backend
        // error instead of the caller's own mistake.
        let error = engine
            .enqueue(
                &name("jobs"),
                "x".repeat(MAX_BODY_BYTES + 1),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect_err("too large");

        assert!(
            matches!(error, EngineError::MessageTooLarge { bytes } if bytes == MAX_BODY_BYTES + 1),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_body_at_the_limit_is_accepted() {
        let engine = engine();
        let queue = name("jobs");
        engine
            .create_queue(queue.clone(), QueueAttributes::default())
            .await
            .expect("create queue");

        engine
            .enqueue(
                &queue,
                "x".repeat(MAX_BODY_BYTES),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("exactly at the limit is allowed");
    }

    #[tokio::test]
    async fn claiming_hands_out_a_message_and_counts_the_delivery() {
        let engine = engine();
        let queue = queue_with_message(&engine, "hello").await;

        let claimed = engine
            .claim_next(&queue, None)
            .await
            .expect("claim")
            .expect("a message is waiting");

        assert_eq!(claimed.message.body, "hello");
        assert_eq!(claimed.message.receive_count, 1, "this delivery counts");
        assert!(claimed.message.first_received_at.is_some());
        assert!(claimed.claim_expires_at > SystemTime::now());
    }

    #[tokio::test]
    async fn claiming_an_empty_queue_returns_nothing_rather_than_failing() {
        let engine = engine();
        let queue = name("jobs");
        engine
            .create_queue(queue.clone(), QueueAttributes::default())
            .await
            .expect("create queue");

        assert!(
            engine
                .claim_next(&queue, None)
                .await
                .expect("claim")
                .is_none(),
            "an empty queue is a normal answer, not an error"
        );
    }

    #[tokio::test]
    async fn claiming_from_a_missing_queue_is_an_error() {
        let error = engine()
            .claim_next(&name("nope"), None)
            .await
            .expect_err("no such queue");

        assert!(matches!(error, EngineError::QueueNotFound(_)), "{error:?}");
    }

    #[tokio::test]
    async fn a_claimed_message_is_not_handed_to_a_second_consumer() {
        let engine = engine();
        let queue = queue_with_message(&engine, "hello").await;
        engine
            .claim_next(&queue, None)
            .await
            .expect("claim")
            .expect("a message");

        assert!(
            engine
                .claim_next(&queue, None)
                .await
                .expect("claim")
                .is_none(),
            "the only message is already claimed"
        );
    }

    #[tokio::test]
    async fn acking_a_claim_removes_the_message() {
        let engine = engine();
        let queue = queue_with_message(&engine, "hello").await;
        let claimed = engine
            .claim_next(&queue, None)
            .await
            .expect("claim")
            .expect("a message");

        engine.ack(&queue, &claimed.receipt).await.expect("ack");

        // Nothing left, and the spent handle no longer names anything.
        assert!(
            engine
                .claim_next(&queue, None)
                .await
                .expect("claim")
                .is_none()
        );
        let error = engine
            .ack(&queue, &claimed.receipt)
            .await
            .expect_err("already acked");
        assert!(matches!(error, EngineError::InvalidReceipt), "{error:?}");
    }

    // ---------------------------------------------------------------------------
    // Long polling
    // ---------------------------------------------------------------------------

    /// Long enough that a missed wake shows up as a failure rather than a slow pass.
    const A_SHORT_WAIT: Duration = Duration::from_millis(200);

    /// Far longer than any of these tests should actually take, so a test that waits
    /// this long has found a real bug rather than a slow machine.
    const LONGER_THAN_NEEDED: Duration = Duration::from_secs(10);

    /// The bound for "this returned rather than waiting".
    ///
    /// Deliberately not [`A_SHORT_WAIT`], even though the operations it covers take
    /// microseconds. These assertions only need to separate *returned* from *waited out a
    /// deadline*, and every one of them sits against a deadline of
    /// [`LONGER_THAN_NEEDED`] — so a second of headroom loses nothing and leaves far less
    /// to go wrong on a machine that is busy doing something else.
    const PROMPTLY: Duration = Duration::from_secs(1);

    fn waiting(wait: Duration) -> ReceiveRequest {
        ReceiveRequest {
            max_messages: 1,
            visibility_timeout: None,
            wait: Some(wait),
        }
    }

    /// An engine with an empty queue named `jobs`.
    async fn engine_with_queue() -> (Arc<Engine>, QueueName) {
        let engine = Arc::new(engine());
        let queue = name("jobs");
        engine
            .create_queue(queue.clone(), QueueAttributes::default())
            .await
            .expect("create queue");

        (engine, queue)
    }

    #[tokio::test]
    async fn a_receive_that_is_not_asked_to_wait_returns_at_once() {
        let (engine, queue) = engine_with_queue().await;
        let started = Instant::now();

        let claimed = engine
            .receive(&queue, &waiting(Duration::ZERO))
            .await
            .expect("receive");

        assert!(claimed.is_empty());
        assert!(
            started.elapsed() < PROMPTLY,
            "a zero wait must not wait: took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_receive_returns_a_message_that_is_already_there_without_waiting() {
        let (engine, queue) = engine_with_queue().await;
        engine
            .enqueue(
                &queue,
                "hello".to_owned(),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");
        let started = Instant::now();

        let claimed = engine
            .receive(&queue, &waiting(LONGER_THAN_NEEDED))
            .await
            .expect("receive");

        assert_eq!(claimed.len(), 1);
        assert!(
            started.elapsed() < PROMPTLY,
            "a message already waiting must not be waited for: took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_waiting_receive_wakes_when_a_message_arrives() {
        // The point of the whole feature: no polling, and no waiting out the timeout.
        let (engine, queue) = engine_with_queue().await;

        let consumer = tokio::spawn({
            let engine = Arc::clone(&engine);
            let queue = queue.clone();
            async move { engine.receive(&queue, &waiting(LONGER_THAN_NEEDED)).await }
        });

        // Long enough that the consumer is genuinely blocked in the wait rather than
        // still on its first look at the queue.
        tokio::time::sleep(A_SHORT_WAIT).await;
        let started = Instant::now();
        engine
            .enqueue(
                &queue,
                "hello".to_owned(),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");

        let claimed = consumer.await.expect("consumer task").expect("receive");

        assert_eq!(
            claimed.len(),
            1,
            "the enqueue should have woken the consumer"
        );
        assert_eq!(claimed[0].message.body, "hello");
        assert!(
            started.elapsed() < PROMPTLY,
            "the wake should be prompt, not a poll: took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_waiting_receive_gives_up_at_its_deadline() {
        let (engine, queue) = engine_with_queue().await;
        let started = Instant::now();

        let claimed = engine
            .receive(&queue, &waiting(A_SHORT_WAIT))
            .await
            .expect("receive");

        assert!(claimed.is_empty(), "empty is the answer, not an error");
        assert!(
            started.elapsed() >= A_SHORT_WAIT,
            "it must actually have waited: took {:?}",
            started.elapsed()
        );
        assert!(
            started.elapsed() < LONGER_THAN_NEEDED,
            "and stopped waiting: took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_wake_does_not_hand_over_a_message_that_is_not_claimable_yet() {
        // A waiter is woken by the enqueue, but the message is delayed, so there is
        // nothing to take. It must re-check and keep waiting rather than deliver the
        // message that caused its wake — which is what "re-evaluated at wake time"
        // means, and what an implementation handing the notification a payload would
        // get wrong.
        let (engine, queue) = engine_with_queue().await;

        let consumer = tokio::spawn({
            let engine = Arc::clone(&engine);
            let queue = queue.clone();
            async move { engine.receive(&queue, &waiting(A_SHORT_WAIT)).await }
        });

        engine
            .enqueue(
                &queue,
                "later".to_owned(),
                Priority::DEFAULT,
                MessageAttributes::new(),
                Some(LONGER_THAN_NEEDED),
            )
            .await
            .expect("enqueue");

        let claimed = consumer.await.expect("consumer task").expect("receive");

        assert!(
            claimed.is_empty(),
            "a delayed message is not claimable, however it woke someone"
        );
    }

    #[tokio::test]
    async fn one_message_goes_to_exactly_one_of_several_waiters() {
        let (engine, queue) = engine_with_queue().await;

        let consumers: Vec<_> = (0..3)
            .map(|_| {
                let engine = Arc::clone(&engine);
                let queue = queue.clone();
                tokio::spawn(async move { engine.receive(&queue, &waiting(A_SHORT_WAIT)).await })
            })
            .collect();

        tokio::time::sleep(Duration::from_millis(50)).await;
        engine
            .enqueue(
                &queue,
                "hello".to_owned(),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");

        let mut served = 0;
        for consumer in consumers {
            let claimed = consumer.await.expect("consumer task").expect("receive");
            served += claimed.len();
        }

        assert_eq!(
            served, 1,
            "the others must wait out their deadline rather than be handed a duplicate"
        );
    }

    #[tokio::test]
    async fn several_waiters_share_out_several_messages() {
        let (engine, queue) = engine_with_queue().await;

        let consumers: Vec<_> = (0..3)
            .map(|_| {
                let engine = Arc::clone(&engine);
                let queue = queue.clone();
                tokio::spawn(
                    async move { engine.receive(&queue, &waiting(LONGER_THAN_NEEDED)).await },
                )
            })
            .collect();

        tokio::time::sleep(Duration::from_millis(50)).await;
        for body in ["one", "two", "three"] {
            engine
                .enqueue(
                    &queue,
                    body.to_owned(),
                    Priority::DEFAULT,
                    MessageAttributes::new(),
                    None,
                )
                .await
                .expect("enqueue");
        }

        let mut bodies = Vec::new();
        for consumer in consumers {
            let claimed = consumer.await.expect("consumer task").expect("receive");
            bodies.extend(claimed.into_iter().map(|claimed| claimed.message.body));
        }
        bodies.sort();

        assert_eq!(
            bodies,
            ["one", "three", "two"],
            "every message delivered exactly once, no consumer left waiting"
        );
    }

    #[tokio::test]
    async fn a_receive_waits_for_the_first_message_but_not_for_a_full_batch() {
        // SQS's behaviour, and the useful one: three messages are there, ten were asked
        // for, and the answer is three rather than a wait for seven more.
        let (engine, queue) = engine_with_queue().await;
        for body in ["one", "two", "three"] {
            engine
                .enqueue(
                    &queue,
                    body.to_owned(),
                    Priority::DEFAULT,
                    MessageAttributes::new(),
                    None,
                )
                .await
                .expect("enqueue");
        }
        let started = Instant::now();

        let claimed = engine
            .receive(
                &queue,
                &ReceiveRequest {
                    max_messages: 10,
                    wait: Some(LONGER_THAN_NEEDED),
                    ..ReceiveRequest::default()
                },
            )
            .await
            .expect("receive");

        assert_eq!(claimed.len(), 3);
        assert!(
            started.elapsed() < PROMPTLY,
            "a short batch is an answer, not a reason to wait: took {:?}",
            started.elapsed()
        );
    }

    /// A batch never contains the same message twice, **including with a zero visibility
    /// timeout** — the case that makes it hard.
    ///
    /// Zero expires each claim the instant it is made, so the message just handed out is
    /// claimable again straight away, and it is the one that ranks first. Before
    /// `claim_next_skipping` existed this returned three copies of one message, two of
    /// them under receipt handles the next claim had already invalidated. Found through
    /// the SQS facade, where `--visibility-timeout 0` with `--max-number-of-messages 3`
    /// is how someone looks at a queue without keeping it.
    #[tokio::test]
    async fn a_batch_never_holds_the_same_message_twice() {
        let (engine, queue) = engine_with_queue().await;
        for body in ["one", "two", "three"] {
            engine
                .enqueue(
                    &queue,
                    body.to_owned(),
                    Priority::DEFAULT,
                    MessageAttributes::new(),
                    None,
                )
                .await
                .expect("enqueue");
        }

        let claimed = engine
            .receive(
                &queue,
                &ReceiveRequest {
                    max_messages: 3,
                    visibility_timeout: Some(Duration::ZERO),
                    ..ReceiveRequest::default()
                },
            )
            .await
            .expect("receive");

        assert_eq!(claimed.len(), 3, "three messages exist, so three come back");

        let mut ids: Vec<&str> = claimed
            .iter()
            .map(|claimed| claimed.message.id.as_str())
            .collect();
        ids.sort_unstable();
        let distinct = ids.len();
        ids.dedup();
        assert_eq!(
            ids.len(),
            distinct,
            "three messages, not one message thrice"
        );

        // The handle handed out with each is the one that is current, which is the second
        // half of the same bug: a re-delivery replaces the claim, so a duplicate in the
        // batch would have left the earlier copies holding dead handles.
        for message in &claimed {
            assert_eq!(
                message.message.receive_count, 1,
                "each message was delivered once, not once per iteration"
            );
        }
    }

    #[tokio::test]
    async fn the_queues_own_wait_applies_when_a_request_does_not_ask() {
        // `ReceiveMessageWaitTimeSeconds` as a queue attribute, which is how a queue
        // makes long polling the default for its consumers.
        let engine = Arc::new(engine());
        let queue = name("jobs");
        engine
            .create_queue(
                queue.clone(),
                QueueAttributes {
                    receive_wait_time: A_SHORT_WAIT,
                    ..QueueAttributes::default()
                },
            )
            .await
            .expect("create queue");
        let started = Instant::now();

        let claimed = engine
            .receive(
                &queue,
                &ReceiveRequest {
                    max_messages: 1,
                    visibility_timeout: None,
                    wait: None,
                },
            )
            .await
            .expect("receive");

        assert!(claimed.is_empty());
        assert!(
            started.elapsed() >= A_SHORT_WAIT,
            "the queue's own wait should have applied: took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_queue_with_no_configured_wait_does_not_wait() {
        // The default, so an unconfigured queue behaves like a plain poll.
        let (engine, queue) = engine_with_queue().await;
        let started = Instant::now();

        let claimed = engine
            .receive(&queue, &ReceiveRequest::default())
            .await
            .expect("receive");

        assert!(claimed.is_empty());
        assert!(started.elapsed() < PROMPTLY, "took {:?}", started.elapsed());
    }

    #[tokio::test(start_paused = true)]
    async fn a_wait_beyond_the_cap_is_clamped() {
        // On a paused clock, so the cap can be observed without the test taking as long
        // as the cap itself. Asking for an hour must not hold a request open for one.
        let (engine, queue) = engine_with_queue().await;
        let started = Instant::now();

        let claimed = engine
            .receive(&queue, &waiting(Duration::from_secs(3600)))
            .await
            .expect("receive");

        assert!(claimed.is_empty());
        assert!(
            started.elapsed() < MAX_RECEIVE_WAIT + Duration::from_secs(1),
            "waited {:?}, which is past the {MAX_RECEIVE_WAIT:?} cap",
            started.elapsed()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_queues_configured_wait_is_capped_too() {
        // The cap belongs to the engine, so a queue configured past it is clamped as
        // well — otherwise config would be a way around the protocol's limit.
        let engine = Arc::new(engine());
        let queue = name("jobs");
        engine
            .create_queue(
                queue.clone(),
                QueueAttributes {
                    receive_wait_time: Duration::from_secs(3600),
                    ..QueueAttributes::default()
                },
            )
            .await
            .expect("create queue");
        let started = Instant::now();

        engine
            .receive(&queue, &ReceiveRequest::default())
            .await
            .expect("receive");

        assert!(
            started.elapsed() < MAX_RECEIVE_WAIT + Duration::from_secs(1),
            "waited {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn deleting_a_queue_releases_the_consumers_waiting_on_it() {
        // Otherwise a consumer sits for its full timeout on a queue that is gone.
        let (engine, queue) = engine_with_queue().await;

        let consumer = tokio::spawn({
            let engine = Arc::clone(&engine);
            let queue = queue.clone();
            async move { engine.receive(&queue, &waiting(LONGER_THAN_NEEDED)).await }
        });

        tokio::time::sleep(A_SHORT_WAIT).await;
        let started = Instant::now();
        engine.delete_queue(&queue).await.expect("delete");

        let error = consumer
            .await
            .expect("consumer task")
            .expect_err("the queue is gone");

        assert!(matches!(error, EngineError::QueueNotFound(_)), "{error:?}");
        assert!(
            started.elapsed() < LONGER_THAN_NEEDED,
            "it should have been released, not timed out: took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_batch_size_outside_the_range_is_clamped_rather_than_refused() {
        let (engine, queue) = engine_with_queue().await;
        for index in 0..12 {
            engine
                .enqueue(
                    &queue,
                    format!("m{index}"),
                    Priority::DEFAULT,
                    MessageAttributes::new(),
                    None,
                )
                .await
                .expect("enqueue");
        }

        // Zero is not a request anyone can satisfy, so it means one.
        let one = engine
            .receive(
                &queue,
                &ReceiveRequest {
                    max_messages: 0,
                    ..ReceiveRequest::default()
                },
            )
            .await
            .expect("receive");
        assert_eq!(one.len(), 1);

        let capped = engine
            .receive(
                &queue,
                &ReceiveRequest {
                    max_messages: usize::MAX,
                    ..ReceiveRequest::default()
                },
            )
            .await
            .expect("receive");
        assert_eq!(capped.len(), MAX_MESSAGES_PER_RECEIVE);
    }

    #[tokio::test]
    async fn waiting_on_a_queue_that_does_not_exist_is_an_error_not_a_wait() {
        let engine = engine();
        let started = Instant::now();

        let error = engine
            .receive(&name("nope"), &waiting(LONGER_THAN_NEEDED))
            .await
            .expect_err("no such queue");

        assert!(matches!(error, EngineError::QueueNotFound(_)), "{error:?}");
        assert!(
            started.elapsed() < PROMPTLY,
            "a missing queue is known immediately: took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn handing_a_message_back_wakes_a_waiting_consumer() {
        // A consumer that cannot do the work sets its visibility to zero. That makes a
        // message claimable by a client action, exactly as an enqueue does, so a
        // consumer waiting next to it should not sit through a long poll.
        let (engine, queue) = engine_with_queue().await;
        engine
            .enqueue(
                &queue,
                "hello".to_owned(),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");
        let holder = engine
            .claim_next(&queue, Some(Duration::from_secs(3600)))
            .await
            .expect("claim")
            .expect("a message");

        let consumer = tokio::spawn({
            let engine = Arc::clone(&engine);
            let queue = queue.clone();
            async move { engine.receive(&queue, &waiting(LONGER_THAN_NEEDED)).await }
        });

        tokio::time::sleep(A_SHORT_WAIT).await;
        let started = Instant::now();
        engine
            .change_visibility(&queue, &holder.receipt, Duration::ZERO)
            .await
            .expect("hand it back");

        let claimed = consumer.await.expect("consumer task").expect("receive");

        assert_eq!(claimed.len(), 1, "the hand-back should have woken it");
        assert_eq!(claimed[0].message.body, "hello");
        assert!(
            started.elapsed() < PROMPTLY,
            "and promptly, not after the hour the holder had asked for: took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn extending_a_claim_does_not_wake_anyone() {
        // The counterpart: a non-zero timeout leaves the message invisible, so waking a
        // consumer would only send it to the store to find nothing.
        let (engine, queue) = engine_with_queue().await;
        engine
            .enqueue(
                &queue,
                "hello".to_owned(),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");
        let holder = engine
            .claim_next(&queue, Some(Duration::from_secs(3600)))
            .await
            .expect("claim")
            .expect("a message");

        let consumer = tokio::spawn({
            let engine = Arc::clone(&engine);
            let queue = queue.clone();
            async move { engine.receive(&queue, &waiting(A_SHORT_WAIT)).await }
        });

        engine
            .change_visibility(&queue, &holder.receipt, Duration::from_secs(7200))
            .await
            .expect("extend");

        let claimed = consumer.await.expect("consumer task").expect("receive");

        assert!(
            claimed.is_empty(),
            "the message is still held, so there was nothing to wake up for"
        );
    }

    #[tokio::test]
    async fn changing_visibility_needs_a_live_claim() {
        let (engine, queue) = engine_with_queue().await;
        engine
            .enqueue(
                &queue,
                "hello".to_owned(),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");
        let claimed = engine
            .claim_next(&queue, None)
            .await
            .expect("claim")
            .expect("a message");
        engine.ack(&queue, &claimed.receipt).await.expect("ack");

        let error = engine
            .change_visibility(&queue, &claimed.receipt, Duration::ZERO)
            .await
            .expect_err("the claim is over");

        assert!(matches!(error, EngineError::InvalidReceipt), "{error:?}");
    }

    #[tokio::test]
    async fn draining_releases_the_consumers_that_are_waiting() {
        // What makes shutdown prompt: a consumer parked for a long wait is an in-flight
        // request, so without this every shutdown would take as long as the longest one.
        let (engine, queue) = engine_with_queue().await;

        let consumers: Vec<_> = (0..3)
            .map(|_| {
                let engine = Arc::clone(&engine);
                let queue = queue.clone();
                tokio::spawn(
                    async move { engine.receive(&queue, &waiting(LONGER_THAN_NEEDED)).await },
                )
            })
            .collect();

        tokio::time::sleep(A_SHORT_WAIT).await;
        let started = Instant::now();
        engine.begin_draining();

        for consumer in consumers {
            let claimed = consumer.await.expect("consumer task").expect("receive");

            assert!(
                claimed.is_empty(),
                "an empty answer, which consumers already handle"
            );
        }
        assert!(
            started.elapsed() < LONGER_THAN_NEEDED,
            "every waiter should have been released at once: took {:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn a_draining_engine_does_not_start_a_new_wait() {
        // A request arriving mid-drain — on a connection that was already open — must
        // not park for its full wait either.
        let (engine, queue) = engine_with_queue().await;
        engine.begin_draining();
        let started = Instant::now();

        let claimed = engine
            .receive(&queue, &waiting(LONGER_THAN_NEEDED))
            .await
            .expect("receive");

        assert!(claimed.is_empty());
        assert!(started.elapsed() < PROMPTLY, "took {:?}", started.elapsed());
    }

    #[tokio::test]
    async fn draining_stops_waiting_but_still_serves() {
        // A drain is not an outage: whatever is already claimable still comes back, so
        // a consumer polling during shutdown gets its message rather than an empty
        // answer or an error.
        let (engine, queue) = engine_with_queue().await;
        engine
            .enqueue(
                &queue,
                "hello".to_owned(),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");

        engine.begin_draining();

        let claimed = engine
            .receive(&queue, &waiting(LONGER_THAN_NEEDED))
            .await
            .expect("receive");

        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].message.body, "hello");
    }

    #[tokio::test]
    async fn draining_is_one_way_and_repeatable() {
        let engine = engine();
        assert!(!engine.is_draining());

        engine.begin_draining();
        engine.begin_draining();

        assert!(engine.is_draining(), "and calling it twice is harmless");
    }

    #[tokio::test]
    async fn no_waiter_entry_is_kept_for_a_queue_that_is_deleted() {
        let (engine, queue) = engine_with_queue().await;

        engine
            .receive(&queue, &ReceiveRequest::default())
            .await
            .expect("receive");
        assert_eq!(engine.waiters.tracked_queues(), 1);

        engine.delete_queue(&queue).await.expect("delete");

        assert_eq!(
            engine.waiters.tracked_queues(),
            0,
            "the registry must not grow with every queue that ever existed"
        );
    }

    #[tokio::test]
    async fn acking_with_an_unissued_handle_is_refused() {
        let engine = engine();
        let queue = queue_with_message(&engine, "hello").await;

        let error = engine
            .ack(&queue, &ReceiptHandle::new())
            .await
            .expect_err("not a handle we issued");

        assert!(matches!(error, EngineError::InvalidReceipt), "{error:?}");
        assert!(
            engine
                .claim_next(&queue, None)
                .await
                .expect("claim")
                .is_some(),
            "the message must still be there"
        );
    }

    /// Reports the queue as existing, then as gone, for a set number of creates —
    /// the interleaving where another caller deletes the queue between our failed
    /// create and our read of it.
    #[derive(Debug)]
    struct VanishingStore {
        /// How many creates should lose the race before one is allowed to succeed.
        races: usize,
        creates: AtomicUsize,
        inner: FakeStore,
    }

    impl VanishingStore {
        fn new(races: usize) -> Self {
            Self {
                races,
                creates: AtomicUsize::new(0),
                inner: FakeStore::new(),
            }
        }
    }

    #[async_trait]
    impl Store for VanishingStore {
        fn backend_name(&self) -> &'static str {
            "vanishing"
        }

        async fn create_queue(&self, queue: Queue) -> StoreResult<()> {
            if self.creates.fetch_add(1, Ordering::SeqCst) < self.races {
                return Err(StoreError::QueueAlreadyExists(queue.name));
            }
            self.inner.create_queue(queue).await
        }

        async fn get_queue(&self, name: &QueueName) -> StoreResult<Queue> {
            self.inner.get_queue(name).await
        }

        async fn set_queue_attributes(
            &self,
            name: &QueueName,
            attributes: QueueAttributes,
            modified_at: SystemTime,
        ) -> StoreResult<()> {
            self.inner
                .set_queue_attributes(name, attributes, modified_at)
                .await
        }

        async fn message_counts(&self, name: &QueueName) -> StoreResult<MessageCounts> {
            self.inner.message_counts(name).await
        }

        async fn delete_queue(&self, name: &QueueName) -> StoreResult<()> {
            self.inner.delete_queue(name).await
        }

        async fn purge_queue(&self, name: &QueueName) -> StoreResult<u64> {
            self.inner.purge_queue(name).await
        }

        async fn list_queues(&self) -> StoreResult<Vec<Queue>> {
            self.inner.list_queues().await
        }

        async fn enqueue(
            &self,
            queue: &QueueName,
            message: Message,
            delay: Option<Duration>,
        ) -> StoreResult<()> {
            self.inner.enqueue(queue, message, delay).await
        }

        async fn claim_next_skipping(
            &self,
            queue: &QueueName,
            visibility_timeout: Option<Duration>,
            skip: &[MessageId],
        ) -> StoreResult<Option<ClaimedMessage>> {
            self.inner
                .claim_next_skipping(queue, visibility_timeout, skip)
                .await
        }

        async fn ack(&self, queue: &QueueName, receipt: &ReceiptHandle) -> StoreResult<()> {
            self.inner.ack(queue, receipt).await
        }

        async fn change_visibility(
            &self,
            queue: &QueueName,
            receipt: &ReceiptHandle,
            visibility_timeout: Duration,
        ) -> StoreResult<()> {
            self.inner
                .change_visibility(queue, receipt, visibility_timeout)
                .await
        }
    }

    #[tokio::test]
    async fn a_create_that_loses_one_race_retries_and_succeeds() {
        let engine = Engine::new(Arc::new(VanishingStore::new(1)));

        let created = engine
            .create_queue(name("jobs"), attributes(30))
            .await
            .expect("the retry should get there");

        assert_eq!(created.name, name("jobs"));
    }

    #[tokio::test]
    async fn a_create_that_keeps_losing_reports_a_conflict() {
        let engine = Engine::new(Arc::new(VanishingStore::new(CREATE_ATTEMPTS)));

        let error = engine
            .create_queue(name("jobs"), attributes(30))
            .await
            .expect_err("churn on this name");

        assert!(
            matches!(&error, EngineError::Conflict(n) if n == &name("jobs")),
            "a caller must be able to tell this from a genuine conflict: {error:?}"
        );
    }
}
