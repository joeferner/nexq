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

use async_trait::async_trait;
use thiserror::Error;

use crate::model::{Queue, QueueName};

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

    /// Delete a queue and everything in it.
    async fn delete_queue(&self, name: &QueueName) -> Result<()>;

    /// Every queue, in no particular order.
    ///
    /// Unpaged and unfiltered on purpose while the surface stays small; prefix
    /// matching and paging come with the operations that need them, and belong here
    /// rather than in the caller so a backend can push them down to storage.
    async fn list_queues(&self) -> Result<Vec<Queue>>;
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A stand-in store, just enough to prove the trait is usable behind `dyn` and to
    /// pin the error cases.
    ///
    /// Not a backend: the real in-memory one lives in `nexq-store-memory`, which
    /// depends on this crate and so cannot be used from these tests. This exists so
    /// the engine has something to be tested against without that cycle, and stays
    /// deliberately dumb — `nexq-store-conformance` is what holds real backends to
    /// their behavior.
    #[derive(Debug, Default)]
    struct FakeStore {
        queues: Mutex<HashMap<QueueName, Queue>>,
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
