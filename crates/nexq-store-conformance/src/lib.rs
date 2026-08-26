//! The behavior every NexQ storage backend must have.
//!
//! A backend that compiles against [`nexq_core::store::Store`] satisfies the *types*.
//! This crate is what says whether it satisfies the *contract* — that a claimed message
//! is hidden from other consumers, that an expired claim comes back under a new handle,
//! that priority beats arrival order. Those are the promises the engine relies on, and
//! they cannot be expressed in a trait signature.
//!
//! # Using it
//!
//! Add this crate as a dev-dependency and invoke the macro from a test target,
//! supplying an async function that produces a store:
//!
//! ```ignore
//! use std::sync::Arc;
//!
//! use nexq_core::store::Store;
//! use nexq_store_memory::MemoryStore;
//!
//! async fn new_store() -> Arc<dyn Store> {
//!     Arc::new(MemoryStore::new())
//! }
//!
//! nexq_store_conformance::conformance_tests!(new_store);
//! ```
//!
//! That expands to one `#[tokio::test]` per case, so a failure names the behavior that
//! broke rather than reporting "the conformance suite failed".
//!
//! # What the factory must return
//!
//! **An empty store, isolated from every other call.** Cases run concurrently and each
//! assumes it starts with no queues. For an in-process backend that is a fresh value;
//! for a shared one — a database — it means a fresh schema, database, or key prefix per
//! call. A factory that hands out the same underlying state twice will produce failures
//! that look like contract violations but are test interference.
//!
//! Some cases wait for a claim to expire, so the suite takes on the order of a second
//! to run.

pub mod cases;

/// Generate one test per conformance case.
///
/// `$new_store` is a path to an `async fn() -> Arc<dyn Store>`. See the crate docs for
/// what it has to guarantee.
///
/// Every case in [`cases`] must be listed here — one that is not listed simply never
/// runs, which is worse than a failure because it looks like coverage.
#[macro_export]
macro_rules! conformance_tests {
    ($new_store:path) => {
        $crate::conformance_case!($new_store, backend_names_itself);

        // Queue lifecycle.
        $crate::conformance_case!($new_store, a_created_queue_can_be_read_back);
        $crate::conformance_case!($new_store, queue_attributes_survive_a_round_trip);
        $crate::conformance_case!($new_store, an_empty_store_lists_no_queues);
        $crate::conformance_case!($new_store, every_queue_is_listed);
        $crate::conformance_case!(
            $new_store,
            creating_a_queue_twice_is_refused_without_disturbing_the_first
        );
        $crate::conformance_case!($new_store, only_one_concurrent_create_wins);
        $crate::conformance_case!($new_store, a_deleted_queue_is_gone_and_its_name_is_reusable);
        $crate::conformance_case!($new_store, deleting_a_queue_discards_its_messages);
        $crate::conformance_case!($new_store, operations_on_a_missing_queue_report_it);

        // Sending and claiming.
        $crate::conformance_case!($new_store, an_enqueued_message_can_be_claimed);
        $crate::conformance_case!($new_store, message_attributes_survive_a_round_trip);
        $crate::conformance_case!($new_store, a_message_without_attributes_has_none);
        $crate::conformance_case!($new_store, claiming_an_empty_queue_yields_nothing);
        $crate::conformance_case!($new_store, a_claimed_message_is_hidden_from_other_consumers);
        $crate::conformance_case!($new_store, claiming_counts_the_delivery);
        $crate::conformance_case!($new_store, higher_priority_is_served_first);
        $crate::conformance_case!($new_store, equal_priority_is_served_in_order_of_arrival);
        $crate::conformance_case!($new_store, priority_outranks_arrival_order);
        $crate::conformance_case!($new_store, each_message_is_claimed_by_exactly_one_consumer);

        // Delay.
        $crate::conformance_case!($new_store, a_delayed_message_waits_before_it_can_be_claimed);
        $crate::conformance_case!($new_store, the_queues_own_delay_applies_when_none_is_given);

        // Claim expiry and acknowledgement.
        $crate::conformance_case!(
            $new_store,
            an_expired_claim_is_redelivered_under_a_new_handle
        );
        $crate::conformance_case!($new_store, extending_a_claim_holds_off_redelivery);
        $crate::conformance_case!($new_store, a_zero_visibility_returns_the_message_at_once);
        $crate::conformance_case!($new_store, shortening_a_claim_brings_redelivery_forward);
        $crate::conformance_case!(
            $new_store,
            changing_visibility_with_a_spent_handle_is_refused
        );
        $crate::conformance_case!(
            $new_store,
            a_handle_from_a_lapsed_claim_cannot_change_visibility
        );
        $crate::conformance_case!($new_store, a_handle_from_a_lapsed_claim_cannot_ack);
        $crate::conformance_case!($new_store, acking_removes_the_message_for_good);
        $crate::conformance_case!($new_store, acking_the_same_claim_twice_is_refused);
        $crate::conformance_case!($new_store, acking_an_unissued_handle_is_refused);
    };
}

/// Generate a single case's test. Used by [`conformance_tests`].
#[macro_export]
macro_rules! conformance_case {
    ($new_store:path, $case:ident) => {
        #[tokio::test]
        async fn $case() {
            $crate::cases::$case($new_store().await).await;
        }
    };
}
