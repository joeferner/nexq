//! The in-memory backend against the shared conformance suite.
//!
//! Every backend runs this same suite, so behavior that differs between backends shows
//! up here rather than in production.

use std::sync::Arc;

use nexq_core::store::Store;
use nexq_store_memory::MemoryStore;

/// A fresh, empty store per case — this backend holds nothing outside the value
/// itself, so isolation is free.
async fn new_store() -> Arc<dyn Store> {
    Arc::new(MemoryStore::new())
}

nexq_store_conformance::conformance_tests!(new_store);
