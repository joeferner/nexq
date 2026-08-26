//! In-memory storage backend.
//!
//! Everything lives in process and nothing survives a restart, so this backend cannot
//! support multi-node HA — a second node could never observe the same state. It is
//! still a first-class choice for single-node deployments, and the default one.
//!
//! Its other job is to be the reference implementation: it has no external
//! dependencies, so it is the backend the `Store` trait is designed against and the
//! one the conformance suite is written for first.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use async_trait::async_trait;
use nexq_core::model::{Queue, QueueName};
use nexq_core::store::{Result, Store, StoreError};

/// This backend's name in config.
pub const BACKEND_NAME: &str = "memory";

/// Queues held in process.
///
/// Guarded by a [`std::sync::RwLock`] rather than an async one on purpose: every
/// critical section here is a hash-map operation with no `await` inside it, so an async
/// lock would add machinery to protect against a suspension that cannot happen.
#[derive(Debug, Default)]
pub struct MemoryStore {
    queues: RwLock<HashMap<QueueName, Queue>>,
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

    fn read(&self) -> Result<RwLockReadGuard<'_, HashMap<QueueName, Queue>>> {
        self.queues.read().map_err(|_| Self::poisoned())
    }

    fn write(&self) -> Result<RwLockWriteGuard<'_, HashMap<QueueName, Queue>>> {
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
                slot.insert(queue);
                Ok(())
            }
        }
    }

    async fn get_queue(&self, name: &QueueName) -> Result<Queue> {
        self.read()?
            .get(name)
            .cloned()
            .ok_or_else(|| StoreError::QueueNotFound(name.clone()))
    }

    async fn delete_queue(&self, name: &QueueName) -> Result<()> {
        self.write()?
            .remove(name)
            .map(|_| ())
            .ok_or_else(|| StoreError::QueueNotFound(name.clone()))
    }

    async fn list_queues(&self) -> Result<Vec<Queue>> {
        Ok(self.read()?.values().cloned().collect())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use nexq_core::model::QueueAttributes;

    use super::*;

    fn name(name: &str) -> QueueName {
        QueueName::new(name).expect("valid queue name")
    }

    fn queue(queue_name: &str) -> Queue {
        Queue::new(name(queue_name))
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
    async fn acting_on_a_missing_queue_says_which_one() {
        let store = MemoryStore::new();
        let missing = name("nope");

        for error in [
            store.get_queue(&missing).await.expect_err("get"),
            store.delete_queue(&missing).await.expect_err("delete"),
        ] {
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
}
