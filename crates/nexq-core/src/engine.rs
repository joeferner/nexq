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
use tracing::{info, warn};

use crate::dead_letter::Sweep;
use crate::model::{
    ClaimedMessage, MAX_BODY_BYTES, Message, MessageAttributes, MessageCounts, MessageId, Priority,
    Queue, QueueAttributes, QueueName, QueuePosition, ReceiptHandle,
};
use crate::move_task::{MOVE_BATCH, MOVE_HOLD, MoveTask, MoveTaskId, MoveTasks, TaskState};
use crate::store::{Movable, Store, StoreError};
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

    /// A redrive policy names a dead-letter queue that does not exist.
    ///
    /// Refused when the policy is set rather than discovered when the first message is
    /// dead-lettered: the alternative is a queue that reports a redrive policy and quietly
    /// cannot honour it, which is the failure mode dead-lettering exists to prevent.
    #[error("the dead-letter queue {0} does not exist")]
    DeadLetterQueueNotFound(QueueName),

    /// A redrive policy names the queue it is on.
    ///
    /// Dead-lettering a message into the queue it came from is a loop, and with the
    /// delivery counters reset by the move it is an *endless* one.
    #[error("a queue cannot be its own dead-letter queue: {0}")]
    DeadLetterQueueIsItself(QueueName),

    /// A redrive was asked for with no destination, and there is no single obvious one.
    ///
    /// The count is how many queues name this one as their dead-letter queue: zero means
    /// there is nothing to infer from, and more than one means the answer is ambiguous.
    /// Either way the caller has to say where the messages should go.
    // The field is `queue` rather than `source` because `thiserror` reads a field of that
    // name as the error's *cause* and requires it to be an error type.
    #[error(
        "{queue} has {candidates} source queues, so a redrive destination cannot be \
         inferred; name one"
    )]
    MoveDestinationUnknown { queue: QueueName, candidates: usize },

    /// A redrive was asked for from a queue to itself.
    #[error("a redrive must move messages somewhere else: {0}")]
    MoveToSameQueue(QueueName),

    /// A redrive of this queue is already running.
    ///
    /// Two would race for every message and each report half the progress, so the second
    /// is refused. Cancel the first, or wait for it.
    #[error("a redrive of {0} is already running")]
    MoveAlreadyRunning(QueueName),

    /// No redrive task by that handle — it was never issued, or it has been pruned.
    #[error("no redrive task with handle {0}")]
    MoveTaskNotFound(MoveTaskId),

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

    /// Redrives in flight and recently finished, so one can be reported on and called
    /// off while it runs. In process for the same reason the waiters are.
    move_tasks: MoveTasks,

    /// Set once the process is going away, after which nothing waits. See
    /// [`Engine::begin_draining`].
    draining: AtomicBool,
}

impl Engine {
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self {
            store,
            waiters: Waiters::new(),
            move_tasks: MoveTasks::new(),
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
        self.check_redrive_policy(&name, &attributes).await?;

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

        // Checked here rather than in the caller's `change`, so a queue cannot be given an
        // unhonourable redrive policy through whichever facade forgot to look.
        self.check_redrive_policy(name, &attributes)
            .await
            .map_err(E::from)?;

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

    /// Where a message sits in the line, or `None` if the queue does not hold it.
    ///
    /// One of the operations no AWS facade can express, which is why it is here and
    /// reachable only over NexQ's own API. The answer counts what is claimable right now
    /// and nothing else — the [`Store`] contract has the whole rule and the two ways it
    /// surprises people.
    ///
    /// `None` is a normal answer, not a failure: a message that was received and deleted
    /// while the question was being asked has no position, and neither does one whose id
    /// was never issued. A caller cannot tell those apart, and does not need to.
    pub async fn position_of(
        &self,
        queue: &QueueName,
        message: &MessageId,
    ) -> Result<Option<QueuePosition>> {
        Ok(self.store.position_of(queue, message).await?)
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

    // -----------------------------------------------------------------------------
    // Dead-letter queues
    // -----------------------------------------------------------------------------

    /// Refuse a redrive policy this server could not honour.
    ///
    /// Two things are checked, and both are checked *here* rather than in a facade so that
    /// a queue configured through one door cannot be configured in a way the other door
    /// would have refused.
    ///
    /// Not checked: whether the dead-letter queue has a redrive policy of its own that
    /// leads back here. A chain is a legitimate arrangement — a DLQ that itself gives up
    /// eventually — and a cycle is only reachable by an operator who wrote one deliberately,
    /// so refusing it would cost more in surprise than it saves.
    async fn check_redrive_policy(
        &self,
        queue: &QueueName,
        attributes: &QueueAttributes,
    ) -> Result<()> {
        let Some(policy) = &attributes.redrive else {
            return Ok(());
        };

        if policy.dead_letter_queue == *queue {
            return Err(EngineError::DeadLetterQueueIsItself(queue.clone()));
        }

        match self.store.get_queue(&policy.dead_letter_queue).await {
            Ok(_) => Ok(()),
            Err(StoreError::QueueNotFound(name)) => Err(EngineError::DeadLetterQueueNotFound(name)),
            Err(other) => Err(other.into()),
        }
    }

    /// Move one message from one queue to another.
    ///
    /// The single primitive under both directions of dead-lettering, so the durability
    /// argument is made once. The message is **enqueued into the destination before it is
    /// acknowledged in the source**, and that order is the whole story: a failure between
    /// the two leaves the message in both queues, which is a duplicate — something every
    /// consumer of an at-least-once queue already has to handle — where the other order
    /// would lose it outright.
    ///
    /// The message is enqueued with an explicit zero delay rather than the destination's
    /// configured one. A delay is for messages a producer has just sent; this one was sent
    /// a while ago, and holding a dead-lettered message back from the operator who went
    /// looking for it would be the wrong reading of the same setting.
    async fn move_message(
        &self,
        from: &QueueName,
        to: &QueueName,
        claimed: ClaimedMessage,
    ) -> Result<()> {
        self.store
            .enqueue(to, claimed.message.moved(), Some(Duration::ZERO))
            .await?;

        // Available now, so anyone long-polling the destination should find out. Matters
        // for a redrive, whose destination is a live queue with real consumers on it.
        self.waiters.notify_one(to);

        match self.store.ack(from, &claimed.receipt).await {
            Ok(()) => Ok(()),

            // The hold lapsed and something else took the message before this got here.
            // The move itself succeeded — the message is in the destination — so this is a
            // duplicate rather than a failure, and stopping a sweep or a redrive over it
            // would leave the rest of the queue unmoved for no gain.
            Err(StoreError::InvalidReceipt) => {
                warn!(
                    from = %from,
                    to = %to,
                    message = %claimed.message.id,
                    "moved a message but could not remove it from its source, so it is now \
                     in both queues"
                );

                Ok(())
            }

            Err(other) => Err(other.into()),
        }
    }

    /// Move every message this queue's redrive policy has given up on to its dead-letter
    /// queue, returning how many moved.
    ///
    /// Zero for a queue with no redrive policy, and zero for the ordinary case of a queue
    /// whose consumers are keeping up — which is why the sweep that calls this is quiet
    /// almost all of the time.
    ///
    /// **Not driven by receives.** A message becomes eligible when its last claim lapses,
    /// which is a deadline passing rather than anything a client did, so nothing would
    /// notice it on a queue whose consumers have stopped calling — and a queue whose
    /// consumers have stopped calling is exactly the one whose messages need
    /// dead-lettering. [`crate::dead_letter`] is what makes it happen anyway.
    pub async fn dead_letter_exhausted(&self, queue: &QueueName) -> Result<u64> {
        let Some(policy) = self.store.get_queue(queue).await?.attributes.redrive else {
            return Ok(0);
        };

        let mut moved = 0;

        loop {
            let batch = self
                .store
                .claim_for_move(queue, Movable::Exhausted, MOVE_HOLD, MOVE_BATCH)
                .await?;
            let taken = batch.len();

            for claimed in batch {
                let message = claimed.message.id.clone();
                let receive_count = claimed.message.receive_count;

                self.move_message(queue, &policy.dead_letter_queue, claimed)
                    .await?;
                moved += 1;

                // At `info`, not `debug`: a message leaving the queue it was sent to is
                // the kind of thing someone comes looking for afterwards, and "where did
                // my message go" is not a question a log should be silent about.
                info!(
                    queue = %queue,
                    dead_letter_queue = %policy.dead_letter_queue,
                    message = %message,
                    receive_count,
                    max_receive_count = policy.max_receive_count(),
                    "dead-lettered a message that ran out of deliveries"
                );
            }

            // A short batch means the queue had nothing more, so there is no reason to ask
            // again. Anything that became eligible since will be found by the next sweep.
            if taken < MOVE_BATCH {
                return Ok(moved);
            }
        }
    }

    /// Dead-letter every exhausted message in every queue.
    ///
    /// One pass, which is what [`crate::dead_letter::Sweeper`] runs on a timer. A queue
    /// that fails is recorded and the rest are still swept: one unreachable dead-letter
    /// queue must not stop every other queue's messages from moving.
    pub async fn sweep_dead_letters(&self) -> Result<Sweep> {
        let queues = self.store.list_queues().await?;
        let mut sweep = Sweep {
            queues: queues.len(),
            ..Sweep::default()
        };

        for queue in queues {
            // Filtered here as well as inside `dead_letter_exhausted`, which re-reads the
            // queue: this is the whole reason the sweep is cheap on a deployment where few
            // queues have a policy.
            if queue.attributes.redrive.is_none() {
                continue;
            }

            match self.dead_letter_exhausted(&queue.name).await {
                Ok(moved) => sweep.moved += moved,
                Err(error) => sweep.failures.push((queue.name, error)),
            }
        }

        Ok(sweep)
    }

    /// Which queues name this one as their dead-letter queue.
    ///
    /// In name order, so the answer is stable. Answered by scanning the queues rather than
    /// by an index the other way round: a redrive policy lives on the source, so this is
    /// the derived direction, and keeping a reverse index in every backend would be a
    /// second copy of the same fact to get out of step.
    ///
    /// A queue that is nobody's dead-letter queue gets an empty list, which is a normal
    /// answer. The queue itself must exist, since a caller asking about a name that was
    /// never a queue has made a mistake worth reporting.
    pub async fn dead_letter_sources(&self, dead_letter_queue: &QueueName) -> Result<Vec<QueueName>> {
        self.store.get_queue(dead_letter_queue).await?;

        let mut sources: Vec<QueueName> = self
            .store
            .list_queues()
            .await?
            .into_iter()
            .filter(|queue| {
                queue
                    .attributes
                    .redrive
                    .as_ref()
                    .is_some_and(|policy| &policy.dead_letter_queue == dead_letter_queue)
            })
            .map(|queue| queue.name)
            .collect();

        sources.sort();

        Ok(sources)
    }

    /// Start moving a queue's messages to another queue, in the background.
    ///
    /// The way out of a dead-letter queue: the messages that failed go back to the queue
    /// they came from, once whatever was wrong has been fixed. Nothing here is specific to
    /// dead-letter queues, though — it moves messages between any two queues, because a DLQ
    /// is an ordinary queue and giving this a narrower type would be pretending otherwise.
    ///
    /// `destination` of `None` means "back where they came from", inferred from the redrive
    /// policies pointing at `source`: exactly one source queue and that is the answer,
    /// otherwise [`EngineError::MoveDestinationUnknown`] and the caller has to say. NexQ
    /// does not record which queue each individual message arrived from — that would be a
    /// field on every message to serve one operation — so a dead-letter queue shared by
    /// several sources needs the destination named.
    ///
    /// `max_messages_per_second` throttles it. The reason to is that the destination has
    /// live consumers: dropping a dead-letter queue's worth of messages onto them at once
    /// is how a redrive becomes a second outage.
    ///
    /// Returns as soon as the task is registered, not when it finishes. The task it returns
    /// is a snapshot from before any message has moved; [`Engine::redrive_task`] is how to
    /// see where it got to and [`Engine::cancel_redrive`] is how to stop it.
    pub async fn start_redrive(
        self: &Arc<Self>,
        source: QueueName,
        destination: Option<QueueName>,
        max_messages_per_second: Option<u32>,
    ) -> Result<MoveTask> {
        // Proves the source exists before anything else is decided, so a typo is reported
        // as a missing queue rather than as an undeducible destination.
        self.store.get_queue(&source).await?;

        let destination = match destination {
            Some(named) => {
                self.store.get_queue(&named).await?;
                named
            }
            None => {
                let candidates = self.dead_letter_sources(&source).await?;
                match candidates.as_slice() {
                    [only] => only.clone(),
                    other => {
                        return Err(EngineError::MoveDestinationUnknown {
                            queue: source,
                            candidates: other.len(),
                        });
                    }
                }
            }
        };

        if destination == source {
            return Err(EngineError::MoveToSameQueue(source));
        }

        // Checked before the task is registered, so a refused second redrive leaves no
        // trace. Two would race for every message and each report half the progress.
        if self.move_tasks.is_moving(&source) {
            return Err(EngineError::MoveAlreadyRunning(source));
        }

        // A snapshot of what the task set out to do. Taken before it starts, and wrong in
        // both directions by the time it ends — producers may add more, and messages a
        // consumer holds are not moved at all — which is why it is reported next to the
        // count of what actually moved rather than instead of it.
        let messages_to_move = self.store.message_counts(&source).await?.total();

        let state = self.move_tasks.start(
            source,
            destination,
            max_messages_per_second,
            messages_to_move,
        );

        info!(
            task = %state.id(),
            source = %state.source(),
            destination = %state.destination(),
            messages_to_move,
            max_messages_per_second,
            "starting a redrive"
        );

        tokio::spawn(Arc::clone(self).run_redrive(Arc::clone(&state)));

        Ok(state.snapshot())
    }

    /// The loop that actually moves a redrive's messages.
    ///
    /// Owns an `Arc<Engine>` because it outlives the request that started it: a redrive of
    /// a large queue keeps running long after its `POST` returned, which is the reason it
    /// is a task with a handle rather than a request that blocks.
    async fn run_redrive(self: Arc<Self>, state: Arc<TaskState>) {
        let pace = state.pace();
        let batch_size = state.batch_size();

        loop {
            if state.stop_if_cancelled() {
                info!(task = %state.id(), moved = state.snapshot().messages_moved, "redrive cancelled");
                return;
            }

            let batch = match self
                .store
                .claim_for_move(state.source(), Movable::Everything, MOVE_HOLD, batch_size)
                .await
            {
                Ok(batch) => batch,
                Err(error) => return self.abandon_redrive(&state, EngineError::from(error)),
            };

            if batch.is_empty() {
                state.complete();
                info!(
                    task = %state.id(),
                    moved = state.snapshot().messages_moved,
                    "redrive finished"
                );
                return;
            }

            for claimed in batch {
                // Per message rather than per batch, so a batch of a hundred does not make
                // a cancel a hundred moves slow to take effect. Whatever is left of the
                // batch stays held until its hold lapses and then becomes claimable again,
                // which is the same thing that happens if this node dies.
                if state.stop_if_cancelled() {
                    info!(task = %state.id(), moved = state.snapshot().messages_moved, "redrive cancelled");
                    return;
                }

                if let Err(error) = self
                    .move_message(state.source(), state.destination(), claimed)
                    .await
                {
                    return self.abandon_redrive(&state, error);
                }
                state.record_moved();

                if let Some(pace) = pace {
                    tokio::time::sleep(pace).await;
                }
            }
        }
    }

    /// Record that a redrive stopped because something went wrong.
    ///
    /// Separate so that the reason is logged and recorded in one place: a task that failed
    /// silently would look to an operator exactly like one that finished.
    fn abandon_redrive(&self, state: &TaskState, error: EngineError) {
        warn!(
            task = %state.id(),
            source = %state.source(),
            destination = %state.destination(),
            moved = state.snapshot().messages_moved,
            "redrive failed: {error}"
        );
        state.fail(error);
    }

    /// One redrive task, or `None` if this node never had it or has since forgotten it.
    ///
    /// Tasks are in-process and bounded in number, so a handle from a restarted server —
    /// or one from long enough ago — names nothing. A caller cannot tell those apart and
    /// does not need to.
    pub fn redrive_task(&self, id: &MoveTaskId) -> Option<MoveTask> {
        self.move_tasks.get(id)
    }

    /// Every redrive task, newest first, optionally only those draining one queue.
    pub fn redrive_tasks(&self, source: Option<&QueueName>) -> Vec<MoveTask> {
        self.move_tasks.list(source)
    }

    /// Ask a redrive to stop, returning it as it now is.
    ///
    /// Cooperative, and the returned status says so: the task is left
    /// [`crate::move_task::MoveTaskStatus::Cancelling`] until it reaches its next message
    /// boundary, because a message abandoned between its enqueue and its acknowledgement
    /// would be a duplicate for no reason. Whatever has already moved stays moved — this
    /// stops a redrive, it does not undo one.
    ///
    /// Cancelling a task that has already stopped is reported by the status rather than
    /// refused. An operator who cancels a redrive that finished a moment ago wanted it
    /// stopped, and it is.
    pub fn cancel_redrive(&self, id: &MoveTaskId) -> Result<MoveTask> {
        self.move_tasks
            .cancel(id)
            .ok_or_else(|| EngineError::MoveTaskNotFound(id.clone()))
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

    /// The engine hands the question to the store and hands the answer back, `None`
    /// included — a message that is gone is an answer here rather than an error, and a
    /// facade turning it into a `404` is that facade's decision.
    #[tokio::test]
    async fn a_message_that_is_gone_has_no_position() {
        let engine = engine();
        let queue = queue_with_message(&engine, "hello").await;
        let sent = engine
            .enqueue(
                &queue,
                "second".to_owned(),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");

        let position = engine
            .position_of(&queue, &sent.id)
            .await
            .expect("position")
            .expect("the queue holds it");
        assert_eq!(position.ahead, 1, "one message arrived before it");
        assert_eq!(position.place(), 2, "one-based for a caller");

        assert!(
            engine
                .position_of(&queue, &MessageId::new())
                .await
                .expect("position")
                .is_none(),
            "an id this queue never issued has no position"
        );
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

        async fn position_of(
            &self,
            queue: &QueueName,
            message: &MessageId,
        ) -> StoreResult<Option<crate::model::QueuePosition>> {
            self.inner.position_of(queue, message).await
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

        async fn claim_for_move(
            &self,
            queue: &QueueName,
            movable: Movable,
            hold: Duration,
            limit: usize,
        ) -> StoreResult<Vec<ClaimedMessage>> {
            self.inner.claim_for_move(queue, movable, hold, limit).await
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

    // ---------------------------------------------------------------------------
    // Dead-letter queues
    // ---------------------------------------------------------------------------

    use crate::model::RedrivePolicy;
    use crate::move_task::MoveTaskStatus;

    fn redrive(max_receive_count: u32, dead_letter_queue: &str) -> QueueAttributes {
        QueueAttributes {
            redrive: Some(
                RedrivePolicy::new(max_receive_count, name(dead_letter_queue))
                    .expect("a valid policy"),
            ),
            ..QueueAttributes::default()
        }
    }

    /// An engine with `jobs_dlq`, and `jobs` configured to dead-letter into it.
    async fn engine_with_dlq(max_receive_count: u32) -> (Arc<Engine>, QueueName, QueueName) {
        let engine = Arc::new(engine());
        let dead_letter_queue = name("jobs_dlq");
        engine
            .create_queue(dead_letter_queue.clone(), QueueAttributes::default())
            .await
            .expect("create the dead-letter queue");

        let queue = name("jobs");
        engine
            .create_queue(queue.clone(), redrive(max_receive_count, "jobs_dlq"))
            .await
            .expect("create the queue");

        (engine, queue, dead_letter_queue)
    }

    /// Deliver whatever is claimable and let each claim lapse at once, `times` over.
    ///
    /// A zero visibility timeout is what makes this quick: the claim expires the instant it
    /// is made, so the message is claimable again without the test waiting for anything.
    async fn burn_deliveries(engine: &Engine, queue: &QueueName, times: u32) {
        for _ in 0..times {
            engine
                .claim_next(queue, Some(Duration::ZERO))
                .await
                .expect("claim")
                .expect("the message should still be deliverable");
        }
    }

    /// Every body a queue holds, taking each message with a claim that never lapses.
    async fn drain_bodies(engine: &Engine, queue: &QueueName) -> Vec<String> {
        let mut bodies = Vec::new();
        while let Some(claimed) = engine
            .claim_next(queue, Some(Duration::from_secs(3600)))
            .await
            .expect("claim")
        {
            bodies.push(claimed.message.body);
        }
        bodies.sort();

        bodies
    }

    #[tokio::test]
    async fn a_message_that_runs_out_of_deliveries_is_dead_lettered() {
        let (engine, queue, dead_letter_queue) = engine_with_dlq(3).await;
        engine
            .enqueue(
                &queue,
                "poison".to_owned(),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");

        burn_deliveries(&engine, &queue, 3).await;
        let moved = engine
            .dead_letter_exhausted(&queue)
            .await
            .expect("dead-letter");

        assert_eq!(moved, 1);
        assert_eq!(
            engine
                .message_counts(&queue)
                .await
                .expect("counts")
                .total(),
            0,
            "it has left the queue it was sent to"
        );
        assert_eq!(
            drain_bodies(&engine, &dead_letter_queue).await,
            ["poison"],
            "and is claimable in the dead-letter queue"
        );
    }

    /// The message an operator finds in the dead-letter queue has to be recognisable as
    /// the one that was sent, and has to be startable again from zero.
    #[tokio::test]
    async fn a_dead_lettered_message_is_the_same_message_with_its_deliveries_reset() {
        let (engine, queue, dead_letter_queue) = engine_with_dlq(1).await;
        let sent = engine
            .enqueue(
                &queue,
                "poison".to_owned(),
                Priority::new(4),
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");

        burn_deliveries(&engine, &queue, 1).await;
        engine
            .dead_letter_exhausted(&queue)
            .await
            .expect("dead-letter");

        let dead = engine
            .claim_next(&dead_letter_queue, None)
            .await
            .expect("claim")
            .expect("the dead-letter queue holds it");

        assert_eq!(dead.message.id, sent.id, "the id a producer holds still finds it");
        assert_eq!(dead.message.priority, sent.priority);
        assert_eq!(dead.message.enqueued_at, sent.enqueued_at);
        assert_eq!(
            dead.message.receive_count, 1,
            "this delivery is the first one out of the dead-letter queue, not the fourth \
             out of the last one"
        );
    }

    /// The half that has to hold *between* sweeps: an exhausted message is out of
    /// deliveries the moment it runs out, not when something gets round to moving it.
    #[tokio::test]
    async fn an_exhausted_message_is_not_delivered_again_before_it_is_moved() {
        let (engine, queue, _) = engine_with_dlq(2).await;
        engine
            .enqueue(
                &queue,
                "poison".to_owned(),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");

        burn_deliveries(&engine, &queue, 2).await;

        assert!(
            engine
                .claim_next(&queue, None)
                .await
                .expect("claim")
                .is_none(),
            "its two allowed deliveries are spent, and nothing has swept yet"
        );
        assert_eq!(
            engine
                .message_counts(&queue)
                .await
                .expect("counts")
                .total(),
            1,
            "but it is still there, waiting to be moved"
        );
    }

    #[tokio::test]
    async fn a_message_still_within_its_policy_is_left_alone() {
        let (engine, queue, dead_letter_queue) = engine_with_dlq(3).await;
        engine
            .enqueue(
                &queue,
                "retry me".to_owned(),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");

        burn_deliveries(&engine, &queue, 2).await;

        assert_eq!(
            engine
                .dead_letter_exhausted(&queue)
                .await
                .expect("dead-letter"),
            0
        );
        assert_eq!(drain_bodies(&engine, &dead_letter_queue).await, [] as [String; 0]);
    }

    /// A consumer that is still working on its last allowed delivery may yet succeed, and
    /// moving the message out from under it would dead-letter work that was about to be
    /// acknowledged.
    #[tokio::test]
    async fn a_message_a_consumer_is_still_holding_is_not_dead_lettered() {
        let (engine, queue, _) = engine_with_dlq(1).await;
        engine
            .enqueue(
                &queue,
                "in progress".to_owned(),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");

        let claimed = engine
            .claim_next(&queue, Some(Duration::from_secs(3600)))
            .await
            .expect("claim")
            .expect("a message");

        assert_eq!(
            engine
                .dead_letter_exhausted(&queue)
                .await
                .expect("dead-letter"),
            0,
            "its count is exhausted but its consumer has not finished"
        );

        engine
            .ack(&queue, &claimed.receipt)
            .await
            .expect("the consumer must still be able to finish");
    }

    #[tokio::test]
    async fn a_queue_with_no_redrive_policy_dead_letters_nothing() {
        let engine = engine();
        let queue = queue_with_message(&engine, "forever").await;

        burn_deliveries(&engine, &queue, 5).await;

        assert_eq!(
            engine
                .dead_letter_exhausted(&queue)
                .await
                .expect("dead-letter"),
            0,
            "nothing runs out when there is no limit"
        );
        assert!(
            engine
                .claim_next(&queue, None)
                .await
                .expect("claim")
                .is_some(),
            "and it stays deliverable"
        );
    }

    #[tokio::test]
    async fn dead_lettering_moves_every_exhausted_message_and_leaves_the_rest() {
        let (engine, queue, dead_letter_queue) = engine_with_dlq(1).await;
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

        // Two of the three burn their one allowed delivery; the third is never touched.
        burn_deliveries(&engine, &queue, 2).await;

        assert_eq!(
            engine
                .dead_letter_exhausted(&queue)
                .await
                .expect("dead-letter"),
            2
        );
        assert_eq!(drain_bodies(&engine, &queue).await.len(), 1, "one survivor");
        assert_eq!(drain_bodies(&engine, &dead_letter_queue).await.len(), 2);
    }

    #[tokio::test]
    async fn the_sweep_covers_every_queue_that_has_a_policy() {
        let engine = Arc::new(engine());
        engine
            .create_queue(name("shared_dlq"), QueueAttributes::default())
            .await
            .expect("create the dead-letter queue");
        engine
            .create_queue(name("no_policy"), QueueAttributes::default())
            .await
            .expect("create");

        for queue_name in ["first", "second"] {
            let queue = name(queue_name);
            engine
                .create_queue(queue.clone(), redrive(1, "shared_dlq"))
                .await
                .expect("create");
            engine
                .enqueue(
                    &queue,
                    queue_name.to_owned(),
                    Priority::DEFAULT,
                    MessageAttributes::new(),
                    None,
                )
                .await
                .expect("enqueue");
            burn_deliveries(&engine, &queue, 1).await;
        }

        let sweep = engine.sweep_dead_letters().await.expect("sweep");

        assert_eq!(sweep.moved, 2, "both queues, one message each");
        assert_eq!(sweep.queues, 4, "every queue is looked at");
        assert!(sweep.failures.is_empty(), "{:?}", sweep.failures);
        assert_eq!(
            drain_bodies(&engine, &name("shared_dlq")).await,
            ["first", "second"]
        );
    }

    #[tokio::test]
    async fn a_sweep_is_quiet_when_nothing_has_failed() {
        let (engine, _, _) = engine_with_dlq(3).await;

        let sweep = engine.sweep_dead_letters().await.expect("sweep");

        assert!(sweep.is_quiet());
        assert_eq!(sweep.moved, 0);
    }

    /// One unreachable dead-letter queue must not stop every other queue's messages from
    /// moving, and the messages it could not move have to stay put rather than vanish.
    #[tokio::test]
    async fn one_queue_failing_does_not_stop_the_sweep() {
        let engine = Arc::new(engine());
        for dlq in ["good_dlq", "doomed_dlq"] {
            engine
                .create_queue(name(dlq), QueueAttributes::default())
                .await
                .expect("create");
        }

        for (queue_name, dlq) in [("good", "good_dlq"), ("broken", "doomed_dlq")] {
            let queue = name(queue_name);
            engine
                .create_queue(queue.clone(), redrive(1, dlq))
                .await
                .expect("create");
            engine
                .enqueue(
                    &queue,
                    queue_name.to_owned(),
                    Priority::DEFAULT,
                    MessageAttributes::new(),
                    None,
                )
                .await
                .expect("enqueue");
            burn_deliveries(&engine, &queue, 1).await;
        }

        // The policy was honourable when it was set and is not any more, which is the only
        // way a queue reaches this state.
        engine
            .delete_queue(&name("doomed_dlq"))
            .await
            .expect("delete");

        let sweep = engine.sweep_dead_letters().await.expect("sweep");

        assert_eq!(sweep.moved, 1, "the healthy queue still moved its message");
        assert_eq!(sweep.failures.len(), 1, "{:?}", sweep.failures);
        assert_eq!(sweep.failures[0].0, name("broken"));
        assert_eq!(
            engine
                .message_counts(&name("broken"))
                .await
                .expect("counts")
                .total(),
            1,
            "a message that could not be moved is still there to try again"
        );
    }

    #[tokio::test]
    async fn dead_lettering_wakes_a_consumer_waiting_on_the_dead_letter_queue() {
        // The dead-letter queue is an ordinary queue, so somebody may be long-polling it —
        // an alerting consumer, most likely. A message arriving there should wake them for
        // the same reason any other arrival does.
        let (engine, queue, dead_letter_queue) = engine_with_dlq(1).await;
        engine
            .enqueue(
                &queue,
                "poison".to_owned(),
                Priority::DEFAULT,
                MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");
        burn_deliveries(&engine, &queue, 1).await;

        let watcher = tokio::spawn({
            let engine = Arc::clone(&engine);
            let dead_letter_queue = dead_letter_queue.clone();
            async move {
                engine
                    .receive(&dead_letter_queue, &waiting(LONGER_THAN_NEEDED))
                    .await
            }
        });

        tokio::time::sleep(A_SHORT_WAIT).await;
        let started = Instant::now();
        engine
            .dead_letter_exhausted(&queue)
            .await
            .expect("dead-letter");

        let claimed = watcher.await.expect("watcher task").expect("receive");

        assert_eq!(claimed.len(), 1);
        assert!(
            started.elapsed() < PROMPTLY,
            "the wake should be prompt: took {:?}",
            started.elapsed()
        );
    }

    // ---------------------------------------------------------------------------
    // Redrive policy validation
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn a_redrive_policy_naming_a_queue_that_does_not_exist_is_refused() {
        let engine = engine();

        let error = engine
            .create_queue(name("jobs"), redrive(3, "nowhere"))
            .await
            .expect_err("the dead-letter queue does not exist");

        assert!(
            matches!(&error, EngineError::DeadLetterQueueNotFound(n) if n == &name("nowhere")),
            "{error:?}"
        );
        engine
            .get_queue(&name("jobs"))
            .await
            .expect_err("and the queue was not created");
    }

    #[tokio::test]
    async fn a_queue_cannot_be_its_own_dead_letter_queue() {
        let engine = engine();

        let error = engine
            .create_queue(name("jobs"), redrive(3, "jobs"))
            .await
            .expect_err("that is a loop");

        assert!(
            matches!(&error, EngineError::DeadLetterQueueIsItself(n) if n == &name("jobs")),
            "{error:?}"
        );
    }

    /// The same checks apply to a change, not only to a create — otherwise the way round
    /// the rule is to create the queue first and reconfigure it afterwards.
    #[tokio::test]
    async fn changing_to_an_unhonourable_redrive_policy_is_refused() {
        let engine = engine();
        engine
            .create_queue(name("jobs"), QueueAttributes::default())
            .await
            .expect("create");

        let error = engine
            .set_queue_attributes::<_, EngineError>(&name("jobs"), |_| Ok(redrive(3, "nowhere")))
            .await
            .expect_err("the dead-letter queue does not exist");

        assert!(
            matches!(&error, EngineError::DeadLetterQueueNotFound(n) if n == &name("nowhere")),
            "{error:?}"
        );
        assert_eq!(
            engine
                .get_queue(&name("jobs"))
                .await
                .expect("get")
                .attributes,
            QueueAttributes::default(),
            "and nothing was changed"
        );
    }

    #[tokio::test]
    async fn a_redrive_policy_can_be_taken_off_again() {
        let (engine, queue, _) = engine_with_dlq(3).await;

        let updated = engine
            .set_queue_attributes::<_, EngineError>(&queue, |current| {
                Ok(QueueAttributes {
                    redrive: None,
                    ..current
                })
            })
            .await
            .expect("remove the policy");

        assert_eq!(updated.attributes.redrive, None);
    }

    // ---------------------------------------------------------------------------
    // Which queues point at a dead-letter queue
    // ---------------------------------------------------------------------------

    #[tokio::test]
    async fn dead_letter_sources_reports_the_queues_that_point_here() {
        let engine = Arc::new(engine());
        engine
            .create_queue(name("shared_dlq"), QueueAttributes::default())
            .await
            .expect("create");
        engine
            .create_queue(name("other_dlq"), QueueAttributes::default())
            .await
            .expect("create");

        // Created out of order, so the name ordering is this call's doing.
        for queue_name in ["zebra", "alpha"] {
            engine
                .create_queue(name(queue_name), redrive(3, "shared_dlq"))
                .await
                .expect("create");
        }
        engine
            .create_queue(name("elsewhere"), redrive(3, "other_dlq"))
            .await
            .expect("create");
        engine
            .create_queue(name("plain"), QueueAttributes::default())
            .await
            .expect("create");

        assert_eq!(
            engine
                .dead_letter_sources(&name("shared_dlq"))
                .await
                .expect("sources"),
            [name("alpha"), name("zebra")],
            "in name order"
        );
        assert!(
            engine
                .dead_letter_sources(&name("plain"))
                .await
                .expect("sources")
                .is_empty(),
            "being nobody's dead-letter queue is a normal answer"
        );
    }

    #[tokio::test]
    async fn asking_about_a_queue_that_does_not_exist_reports_it() {
        let error = engine()
            .dead_letter_sources(&name("nope"))
            .await
            .expect_err("no such queue");

        assert!(matches!(error, EngineError::QueueNotFound(_)), "{error:?}");
    }

    // ---------------------------------------------------------------------------
    // Redrive: back out of a dead-letter queue
    // ---------------------------------------------------------------------------

    /// Wait for a spawned redrive to stop, and return it as it ended.
    async fn finished_redrive(engine: &Engine, id: &MoveTaskId) -> MoveTask {
        for _ in 0..500 {
            let task = engine.redrive_task(id).expect("the task is registered");
            if !task.status.is_active() {
                return task;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        panic!("the redrive should have finished long before now");
    }

    /// `jobs_dlq` holding `count` messages, with `jobs` as its only source queue.
    async fn dead_letter_queue_holding(count: usize) -> (Arc<Engine>, QueueName, QueueName) {
        let (engine, queue, dead_letter_queue) = engine_with_dlq(1).await;

        for index in 0..count {
            engine
                .enqueue(
                    &queue,
                    format!("failed-{index}"),
                    Priority::DEFAULT,
                    MessageAttributes::new(),
                    None,
                )
                .await
                .expect("enqueue");
        }
        burn_deliveries(&engine, &queue, count as u32).await;
        assert_eq!(
            engine
                .dead_letter_exhausted(&queue)
                .await
                .expect("dead-letter"),
            count as u64
        );

        (engine, queue, dead_letter_queue)
    }

    #[tokio::test]
    async fn a_redrive_moves_messages_back_to_the_queue_they_came_from() {
        let (engine, queue, dead_letter_queue) = dead_letter_queue_holding(3).await;

        let started = engine
            .start_redrive(dead_letter_queue.clone(), Some(queue.clone()), None)
            .await
            .expect("start");
        assert_eq!(started.status, MoveTaskStatus::Running);
        assert_eq!(started.messages_to_move, 3);
        assert_eq!(started.messages_moved, 0, "a snapshot from before it ran");

        let finished = finished_redrive(&engine, &started.id).await;

        assert_eq!(finished.status, MoveTaskStatus::Completed);
        assert_eq!(finished.messages_moved, 3);
        assert!(finished.finished_at.is_some());
        assert_eq!(
            drain_bodies(&engine, &dead_letter_queue).await,
            [] as [String; 0],
            "the dead-letter queue is empty"
        );
        assert_eq!(
            drain_bodies(&engine, &queue).await,
            ["failed-0", "failed-1", "failed-2"],
            "and they are all deliverable again"
        );
    }

    /// The common case, and the reason a destination is optional: there is exactly one
    /// queue that dead-letters here, so "back where they came from" has one answer.
    #[tokio::test]
    async fn a_redrive_with_no_destination_goes_back_to_the_only_source() {
        let (engine, queue, dead_letter_queue) = dead_letter_queue_holding(1).await;

        let started = engine
            .start_redrive(dead_letter_queue, None, None)
            .await
            .expect("start");

        assert_eq!(started.destination, queue);
        assert_eq!(
            finished_redrive(&engine, &started.id).await.messages_moved,
            1
        );
    }

    #[tokio::test]
    async fn a_redrive_with_no_inferable_destination_is_refused() {
        let engine = Arc::new(engine());
        engine
            .create_queue(name("shared_dlq"), QueueAttributes::default())
            .await
            .expect("create");

        // Nothing points at it yet.
        let error = engine
            .start_redrive(name("shared_dlq"), None, None)
            .await
            .expect_err("nothing to infer from");
        assert!(
            matches!(
                &error,
                EngineError::MoveDestinationUnknown { candidates: 0, .. }
            ),
            "{error:?}"
        );

        for queue_name in ["first", "second"] {
            engine
                .create_queue(name(queue_name), redrive(3, "shared_dlq"))
                .await
                .expect("create");
        }

        let error = engine
            .start_redrive(name("shared_dlq"), None, None)
            .await
            .expect_err("ambiguous");
        assert!(
            matches!(
                &error,
                EngineError::MoveDestinationUnknown { candidates: 2, .. }
            ),
            "a dead-letter queue with two sources cannot guess: {error:?}"
        );
    }

    #[tokio::test]
    async fn a_redrive_needs_both_queues_to_exist_and_to_differ() {
        let (engine, _, dead_letter_queue) = engine_with_dlq(3).await;

        let error = engine
            .start_redrive(name("nope"), Some(dead_letter_queue.clone()), None)
            .await
            .expect_err("no such source");
        assert!(matches!(error, EngineError::QueueNotFound(_)), "{error:?}");

        let error = engine
            .start_redrive(dead_letter_queue.clone(), Some(name("nope")), None)
            .await
            .expect_err("no such destination");
        assert!(matches!(error, EngineError::QueueNotFound(_)), "{error:?}");

        let error = engine
            .start_redrive(
                dead_letter_queue.clone(),
                Some(dead_letter_queue.clone()),
                None,
            )
            .await
            .expect_err("nowhere to go");
        assert!(
            matches!(&error, EngineError::MoveToSameQueue(n) if n == &dead_letter_queue),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_second_redrive_of_one_queue_is_refused_while_the_first_runs() {
        // Throttled hard, so the first task is certainly still running when the second is
        // asked for.
        let (engine, queue, dead_letter_queue) = dead_letter_queue_holding(3).await;
        let first = engine
            .start_redrive(dead_letter_queue.clone(), Some(queue.clone()), Some(1))
            .await
            .expect("start");

        let error = engine
            .start_redrive(dead_letter_queue.clone(), Some(queue), None)
            .await
            .expect_err("one at a time");

        assert!(
            matches!(&error, EngineError::MoveAlreadyRunning(n) if n == &dead_letter_queue),
            "{error:?}"
        );
        assert_eq!(
            engine.redrive_tasks(None).len(),
            1,
            "a refused redrive leaves no trace"
        );

        engine.cancel_redrive(&first.id).expect("cancel");
    }

    #[tokio::test]
    async fn a_redrive_can_be_called_off_and_keeps_what_it_already_moved() {
        // One message per second, so the task is certainly mid-run when it is cancelled and
        // certainly has not moved all ten.
        let (engine, queue, dead_letter_queue) = dead_letter_queue_holding(10).await;
        let started = engine
            .start_redrive(dead_letter_queue.clone(), Some(queue.clone()), Some(1))
            .await
            .expect("start");

        let asked = engine.cancel_redrive(&started.id).expect("cancel");
        assert_eq!(
            asked.status,
            MoveTaskStatus::Cancelling,
            "cooperative: asked to stop, not stopped"
        );

        let finished = finished_redrive(&engine, &started.id).await;

        assert_eq!(finished.status, MoveTaskStatus::Cancelled);
        assert!(
            finished.messages_moved < 10,
            "it should not have finished, moved {}",
            finished.messages_moved
        );
        assert_eq!(
            engine
                .message_counts(&queue)
                .await
                .expect("counts")
                .total(),
            finished.messages_moved,
            "whatever it moved stays moved"
        );
    }

    #[tokio::test]
    async fn a_redrive_of_an_empty_queue_completes_having_moved_nothing() {
        let (engine, queue, dead_letter_queue) = engine_with_dlq(3).await;

        let started = engine
            .start_redrive(dead_letter_queue, Some(queue), None)
            .await
            .expect("start");
        let finished = finished_redrive(&engine, &started.id).await;

        assert_eq!(finished.status, MoveTaskStatus::Completed);
        assert_eq!(finished.messages_moved, 0);
        assert_eq!(finished.messages_to_move, 0);
    }

    /// A redrive leaves in-flight messages alone rather than pulling them out from under
    /// their consumers — so it moves what the queue was holding, which is approximate in
    /// the same way every other count here is.
    #[tokio::test]
    async fn a_redrive_leaves_messages_a_consumer_is_holding() {
        let (engine, queue, dead_letter_queue) = dead_letter_queue_holding(2).await;
        let held = engine
            .claim_next(&dead_letter_queue, Some(Duration::from_secs(3600)))
            .await
            .expect("claim")
            .expect("the dead-letter queue holds two");

        let started = engine
            .start_redrive(dead_letter_queue.clone(), Some(queue.clone()), None)
            .await
            .expect("start");
        let finished = finished_redrive(&engine, &started.id).await;

        assert_eq!(finished.messages_moved, 1, "the claimable one");
        assert_eq!(
            finished.messages_to_move, 2,
            "what it set out to do, which is a snapshot and not a promise"
        );
        engine
            .ack(&dead_letter_queue, &held.receipt)
            .await
            .expect("the consumer's claim survived the redrive");
    }

    #[tokio::test]
    async fn redrive_tasks_can_be_listed_and_filtered_by_source() {
        let (engine, queue, dead_letter_queue) = dead_letter_queue_holding(1).await;
        let started = engine
            .start_redrive(dead_letter_queue.clone(), Some(queue), None)
            .await
            .expect("start");
        finished_redrive(&engine, &started.id).await;

        assert_eq!(engine.redrive_tasks(None).len(), 1);
        assert_eq!(engine.redrive_tasks(Some(&dead_letter_queue)).len(), 1);
        assert!(engine.redrive_tasks(Some(&name("elsewhere"))).is_empty());
    }

    #[tokio::test]
    async fn a_handle_this_server_never_issued_names_no_redrive() {
        let engine = engine();
        let unknown = MoveTaskId::from_client("not-a-handle");

        assert!(engine.redrive_task(&unknown).is_none());
        let error = engine.cancel_redrive(&unknown).expect_err("no such task");
        assert!(
            matches!(&error, EngineError::MoveTaskNotFound(id) if id == &unknown),
            "{error:?}"
        );
    }

    /// A redrive is blind to the destination's redrive policy on the way in — the messages
    /// arrive with their counters reset, so they get the full allowance again. Otherwise
    /// rescuing a message would fail on the grounds that it had already failed.
    #[tokio::test]
    async fn redriven_messages_get_their_deliveries_back() {
        let (engine, queue, dead_letter_queue) = dead_letter_queue_holding(1).await;

        let started = engine
            .start_redrive(dead_letter_queue, Some(queue.clone()), None)
            .await
            .expect("start");
        finished_redrive(&engine, &started.id).await;

        // The source allows one delivery, and the redriven message must have that one
        // delivery available rather than being immediately exhausted again.
        let claimed = engine
            .claim_next(&queue, None)
            .await
            .expect("claim")
            .expect("deliverable again");
        assert_eq!(claimed.message.receive_count, 1);
    }
}
