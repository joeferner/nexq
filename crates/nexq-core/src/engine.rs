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
use std::time::SystemTime;

use thiserror::Error;

use crate::model::{Queue, QueueAttributes, QueueName};
use crate::store::{Store, StoreError};

/// The result of an engine operation.
pub type Result<T> = std::result::Result<T, EngineError>;

/// How many times [`Engine::create_queue`] will retry a create that lost a race.
const CREATE_ATTEMPTS: usize = 2;

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

    /// The storage backend failed.
    #[error("backend failure: {0}")]
    Backend(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl From<StoreError> for EngineError {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::QueueNotFound(name) => Self::QueueNotFound(name),
            StoreError::QueueAlreadyExists(name) => Self::QueueAlreadyExists(name),
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

    /// Every queue, optionally limited to those whose name starts with `prefix`.
    ///
    /// Filtering lives here rather than in a facade so every protocol gets the same
    /// answer for the same question. It is applied after loading for now; when the
    /// store learns to filter, this pushes down into it so a backend with many queues
    /// need not send them all back. Paging arrives the same way.
    pub async fn list_queues(&self, prefix: Option<&str>) -> Result<Vec<Queue>> {
        let mut queues = self.store.list_queues().await?;

        if let Some(prefix) = prefix {
            queues.retain(|queue| queue.name.as_str().starts_with(prefix));
        }

        Ok(queues)
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
        assert!(engine.list_queues(None).await.expect("list").is_empty());
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
    async fn listing_returns_every_queue() {
        let engine = engine();
        for queue_name in ["one", "two"] {
            engine
                .create_queue(name(queue_name), QueueAttributes::default())
                .await
                .expect("create");
        }

        let mut listed = listed_names(&engine, None).await;
        listed.sort();

        assert_eq!(listed, ["one", "two"]);
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

        let mut listed = listed_names(&engine, Some("jobs")).await;
        listed.sort();

        assert_eq!(listed, ["jobs", "jobs_dlq"]);
        assert!(
            listed_names(&engine, Some("nothing-matches"))
                .await
                .is_empty(),
            "a prefix matching nothing is an empty list, not an error"
        );
    }

    #[tokio::test]
    async fn listing_an_empty_deployment_yields_nothing() {
        assert!(engine().list_queues(None).await.expect("list").is_empty());
    }

    async fn listed_names(engine: &Engine, prefix: Option<&str>) -> Vec<String> {
        engine
            .list_queues(prefix)
            .await
            .expect("list")
            .into_iter()
            .map(|queue| queue.name.to_string())
            .collect()
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
