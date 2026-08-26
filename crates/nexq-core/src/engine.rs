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
//!
//! Only queue lifecycle so far. One store is held for now; per-queue backend selection
//! arrives with the config that describes it, and belongs here since routing a queue to
//! its backend is exactly this layer's job.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use thiserror::Error;

use crate::model::{
    ClaimedMessage, MAX_BODY_BYTES, Message, MessageAttributes, Priority, Queue, QueueAttributes,
    QueueName, ReceiptHandle,
};
use crate::store::{Store, StoreError};

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
}

impl Engine {
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self { store }
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
            let queue = Queue {
                name: name.clone(),
                created_at: SystemTime::now(),
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

    /// Delete a queue and everything in it.
    ///
    /// Deleting a queue that is not there is an error rather than a no-op: a client
    /// that deletes an unknown name has misunderstood something, and SQS reports it.
    pub async fn delete_queue(&self, name: &QueueName) -> Result<()> {
        Ok(self.store.delete_queue(name).await?)
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

        Ok(message)
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

        async fn delete_queue(&self, name: &QueueName) -> StoreResult<()> {
            self.inner.delete_queue(name).await
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

        async fn claim_next(
            &self,
            queue: &QueueName,
            visibility_timeout: Option<Duration>,
        ) -> StoreResult<Option<ClaimedMessage>> {
            self.inner.claim_next(queue, visibility_timeout).await
        }

        async fn ack(&self, queue: &QueueName, receipt: &ReceiptHandle) -> StoreResult<()> {
            self.inner.ack(queue, receipt).await
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
