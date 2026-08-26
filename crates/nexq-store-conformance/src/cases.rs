//! The conformance cases themselves.
//!
//! Every case takes a store and asserts one piece of the [`Store`] contract. They use
//! nothing but the trait: a case that needed a backend's own API would be testing that
//! backend rather than the contract.
//!
//! Cases panic on failure, so they read as ordinary assertions and the failure lands
//! with the generated test's name.

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use nexq_core::model::{
    AttributeValue, Message, MessageAttribute, MessageAttributes, Priority, Queue, QueueAttributes,
    QueueName,
};
use nexq_core::store::{Store, StoreError};

/// Visibility timeout used where a case needs a claim to lapse during the test.
///
/// Short enough to keep the suite quick, long enough that a backend doing real I/O
/// does not lose the race by accident.
const SHORT_CLAIM: Duration = Duration::from_millis(200);

/// How long to wait for a [`SHORT_CLAIM`] to have lapsed. Generously more than the
/// claim itself, since a slow backend must not make the suite flaky.
const AFTER_SHORT_CLAIM: Duration = Duration::from_millis(500);

fn name(name: &str) -> QueueName {
    QueueName::new(name).expect("valid queue name")
}

fn queue(queue_name: &str) -> Queue {
    Queue::new(name(queue_name))
}

fn message(body: &str, priority: i32) -> Message {
    Message::new(body, Priority::new(priority))
}

/// Create a queue named `jobs` and return its name.
async fn jobs(store: &Arc<dyn Store>) -> QueueName {
    store.create_queue(queue("jobs")).await.expect("create");
    name("jobs")
}

/// Claim one message and return its body, or `None` if nothing was claimable.
async fn claim_body(store: &Arc<dyn Store>, queue: &QueueName) -> Option<String> {
    store
        .claim_next(queue, None)
        .await
        .expect("claim")
        .map(|claimed| claimed.message.body)
}

// ---------------------------------------------------------------------------
// Identity
// ---------------------------------------------------------------------------

/// A backend names itself, so diagnostics and metrics can say which one answered.
pub async fn backend_names_itself(store: Arc<dyn Store>) {
    assert!(
        !store.backend_name().is_empty(),
        "a backend must report a name"
    );
}

// ---------------------------------------------------------------------------
// Queue lifecycle
// ---------------------------------------------------------------------------

pub async fn a_created_queue_can_be_read_back(store: Arc<dyn Store>) {
    let queue = jobs(&store).await;

    let found = store.get_queue(&queue).await.expect("get");

    assert_eq!(found.name, queue);
}

/// Attributes are stored, not defaulted away — a queue read back must be usable to
/// decide how the queue behaves.
pub async fn queue_attributes_survive_a_round_trip(store: Arc<dyn Store>) {
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

    assert_eq!(found.attributes, expected.attributes);
}

/// Setting attributes replaces them and stamps the modification, leaving the rest of
/// the queue — its creation time above all — alone.
pub async fn setting_attributes_records_when_it_happened(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    let before = store.get_queue(&queue_name).await.expect("get");

    let modified_at = before.created_at + Duration::from_secs(60);
    let changed = QueueAttributes {
        visibility_timeout: Duration::from_secs(600),
        delay: Duration::from_secs(9),
        ..QueueAttributes::default()
    };
    store
        .set_queue_attributes(&queue_name, changed.clone(), modified_at)
        .await
        .expect("set attributes");

    let after = store.get_queue(&queue_name).await.expect("get");

    assert_eq!(after.attributes, changed);
    assert_eq!(
        after.last_modified_at, modified_at,
        "the timestamp the caller gave, not one the backend invented"
    );
    assert_eq!(
        after.created_at, before.created_at,
        "changing attributes must not look like recreating the queue"
    );
}

/// Attributes are replaced wholesale, so a value left out of the new set goes back to
/// whatever the caller put there — merging is the engine's job, not a backend's.
pub async fn setting_attributes_replaces_rather_than_merges(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .set_queue_attributes(
            &queue_name,
            QueueAttributes {
                visibility_timeout: Duration::from_secs(600),
                delay: Duration::from_secs(9),
                ..QueueAttributes::default()
            },
            SystemTime::now(),
        )
        .await
        .expect("set attributes");

    store
        .set_queue_attributes(&queue_name, QueueAttributes::default(), SystemTime::now())
        .await
        .expect("set attributes again");

    assert_eq!(
        store.get_queue(&queue_name).await.expect("get").attributes,
        QueueAttributes::default()
    );
}

/// Messages sort into exactly one of visible, in flight, and delayed.
///
/// The three feed `GetQueueAttributes`, and being wrong about which bucket a message is
/// in makes a queue look either idle or backed up when it is neither.
pub async fn message_counts_split_by_visibility(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;

    assert_eq!(
        store.message_counts(&queue_name).await.expect("counts"),
        nexq_core::model::MessageCounts::default(),
        "an empty queue counts zero of everything"
    );

    // Two claimable, one delayed well past the end of the test, one claimed and held.
    for body in ["visible-one", "visible-two", "to-be-claimed"] {
        store
            .enqueue(&queue_name, message(body, 0), None)
            .await
            .expect("enqueue");
    }
    store
        .enqueue(
            &queue_name,
            message("delayed", 0),
            Some(Duration::from_secs(3600)),
        )
        .await
        .expect("enqueue delayed");
    store
        .claim_next(&queue_name, Some(Duration::from_secs(3600)))
        .await
        .expect("claim")
        .expect("a message");

    let counts = store.message_counts(&queue_name).await.expect("counts");

    assert_eq!(counts.visible, 2, "{counts:?}");
    assert_eq!(counts.not_visible, 1, "the claim is still live: {counts:?}");
    assert_eq!(counts.delayed, 1, "{counts:?}");
    assert_eq!(counts.total(), 4, "and the three cover everything stored");
}

/// A claim lapsing moves a message from in flight back to visible, with no other call.
pub async fn a_lapsed_claim_counts_as_visible_again(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");
    store
        .claim_next(&queue_name, Some(SHORT_CLAIM))
        .await
        .expect("claim")
        .expect("a message");

    let held = store.message_counts(&queue_name).await.expect("counts");
    assert_eq!(held.not_visible, 1, "{held:?}");
    assert_eq!(held.visible, 0, "{held:?}");

    tokio::time::sleep(AFTER_SHORT_CLAIM).await;
    let lapsed = store.message_counts(&queue_name).await.expect("counts");

    assert_eq!(lapsed.visible, 1, "the claim lapsed: {lapsed:?}");
    assert_eq!(lapsed.not_visible, 0, "{lapsed:?}");
}

/// Deleting a message takes it out of the counts entirely.
pub async fn an_acked_message_is_counted_nowhere(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");
    let claimed = store
        .claim_next(&queue_name, None)
        .await
        .expect("claim")
        .expect("a message");

    store.ack(&queue_name, &claimed.receipt).await.expect("ack");

    assert_eq!(
        store
            .message_counts(&queue_name)
            .await
            .expect("counts")
            .total(),
        0
    );
}

pub async fn an_empty_store_lists_no_queues(store: Arc<dyn Store>) {
    assert!(store.list_queues().await.expect("list").is_empty());
}

pub async fn every_queue_is_listed(store: Arc<dyn Store>) {
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
}

/// A second create must fail rather than replace, and must leave the first untouched:
/// overwriting would silently discard whatever the queue held.
pub async fn creating_a_queue_twice_is_refused_without_disturbing_the_first(store: Arc<dyn Store>) {
    store.create_queue(queue("jobs")).await.expect("create");
    let original = store.get_queue(&name("jobs")).await.expect("get");

    let mut different = queue("jobs");
    different.attributes.visibility_timeout = Duration::from_secs(600);
    let error = store
        .create_queue(different)
        .await
        .expect_err("already exists");

    assert!(
        matches!(&error, StoreError::QueueAlreadyExists(reported) if reported == &name("jobs")),
        "expected QueueAlreadyExists, got {error:?}"
    );
    assert_eq!(
        store
            .get_queue(&name("jobs"))
            .await
            .expect("get")
            .attributes,
        original.attributes,
        "a refused create must not have changed the existing queue"
    );
}

/// Exactly one of many simultaneous creates may win. A backend that lets two through
/// has a check-then-insert race, which is how two callers end up believing they each
/// own a fresh queue.
pub async fn only_one_concurrent_create_wins(store: Arc<dyn Store>) {
    let attempts: Vec<_> = (0..8)
        .map(|_| {
            let store = Arc::clone(&store);
            tokio::spawn(async move { store.create_queue(queue("jobs")).await })
        })
        .collect();

    let mut created = 0;
    for attempt in attempts {
        match attempt.await.expect("task") {
            Ok(()) => created += 1,
            Err(StoreError::QueueAlreadyExists(_)) => {}
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }

    assert_eq!(created, 1, "exactly one create should have succeeded");
    assert_eq!(store.list_queues().await.expect("list").len(), 1);
}

pub async fn a_deleted_queue_is_gone_and_its_name_is_reusable(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;

    store.delete_queue(&queue_name).await.expect("delete");

    store.get_queue(&queue_name).await.expect_err("deleted");
    assert!(store.list_queues().await.expect("list").is_empty());
    store
        .create_queue(queue("jobs"))
        .await
        .expect("the name is free again");
}

/// Deleting a queue takes its messages with it, so a recreated queue starts empty
/// rather than inheriting a previous incarnation's backlog.
pub async fn deleting_a_queue_discards_its_messages(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");

    store.delete_queue(&queue_name).await.expect("delete");
    store.create_queue(queue("jobs")).await.expect("recreate");

    assert!(
        claim_body(&store, &queue_name).await.is_none(),
        "a recreated queue must not inherit messages"
    );
}

/// Purging empties a queue without removing it, and takes in-flight messages too.
///
/// The in-flight part is the one worth pinning: a backend that only deleted what was
/// visible would leave claimed messages behind, and they would reappear when their claims
/// lapsed — a purge that quietly did not purge.
pub async fn purging_removes_every_message_including_claimed_ones(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    for body in ["visible", "to-be-claimed"] {
        store
            .enqueue(&queue_name, message(body, 0), None)
            .await
            .expect("enqueue");
    }
    store
        .enqueue(
            &queue_name,
            message("delayed", 0),
            Some(Duration::from_secs(3600)),
        )
        .await
        .expect("enqueue delayed");
    store
        .claim_next(&queue_name, Some(Duration::from_secs(3600)))
        .await
        .expect("claim")
        .expect("a message");

    let purged = store.purge_queue(&queue_name).await.expect("purge");

    assert_eq!(purged, 3, "visible, claimed, and delayed alike");
    assert_eq!(
        store
            .message_counts(&queue_name)
            .await
            .expect("counts")
            .total(),
        0
    );
    assert!(
        claim_body(&store, &queue_name).await.is_none(),
        "and nothing comes back later"
    );
}

/// The queue survives a purge, attributes and all — it is emptied, not recreated.
pub async fn purging_keeps_the_queue_and_its_attributes(store: Arc<dyn Store>) {
    let mut configured = queue("jobs");
    configured.attributes.visibility_timeout = Duration::from_secs(600);
    store.create_queue(configured).await.expect("create");
    let queue_name = name("jobs");
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");

    store.purge_queue(&queue_name).await.expect("purge");

    let after = store
        .get_queue(&queue_name)
        .await
        .expect("the queue is still there");
    assert_eq!(
        after.attributes.visibility_timeout,
        Duration::from_secs(600)
    );
}

/// A receipt handle outlives its message by exactly nothing: purging invalidates it.
pub async fn a_handle_from_before_a_purge_is_spent(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");
    let claimed = store
        .claim_next(&queue_name, Some(Duration::from_secs(3600)))
        .await
        .expect("claim")
        .expect("a message");

    store.purge_queue(&queue_name).await.expect("purge");

    let error = store
        .ack(&queue_name, &claimed.receipt)
        .await
        .expect_err("the message it named is gone");
    assert!(
        matches!(error, StoreError::InvalidReceipt),
        "expected InvalidReceipt, got {error:?}"
    );
}

/// Purging an empty queue is a no-op rather than an error, so a caller need not check
/// first — and it says it removed nothing.
pub async fn purging_an_empty_queue_removes_nothing(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;

    assert_eq!(store.purge_queue(&queue_name).await.expect("purge"), 0);

    // And twice running, since nothing here is rate limited.
    assert_eq!(store.purge_queue(&queue_name).await.expect("purge"), 0);
}

/// A purge is confined to the queue it names.
pub async fn purging_one_queue_leaves_the_others_alone(store: Arc<dyn Store>) {
    let jobs_name = jobs(&store).await;
    store.create_queue(queue("emails")).await.expect("create");
    let emails = name("emails");

    for queue_name in [&jobs_name, &emails] {
        store
            .enqueue(queue_name, message("hello", 0), None)
            .await
            .expect("enqueue");
    }

    assert_eq!(store.purge_queue(&jobs_name).await.expect("purge"), 1);

    assert!(claim_body(&store, &jobs_name).await.is_none());
    assert_eq!(
        claim_body(&store, &emails).await.as_deref(),
        Some("hello"),
        "the other queue must be untouched"
    );
}

/// Every operation naming a queue that does not exist reports the same way, so a
/// caller does not have to guess which failure means "no such queue".
pub async fn operations_on_a_missing_queue_report_it(store: Arc<dyn Store>) {
    let missing = name("nope");

    let errors = vec![
        ("get_queue", store.get_queue(&missing).await.err()),
        ("delete_queue", store.delete_queue(&missing).await.err()),
        (
            "enqueue",
            store
                .enqueue(&missing, message("hello", 0), None)
                .await
                .err(),
        ),
        ("claim_next", store.claim_next(&missing, None).await.err()),
        (
            "ack",
            store
                .ack(&missing, &nexq_core::model::ReceiptHandle::new())
                .await
                .err(),
        ),
        (
            "change_visibility",
            store
                .change_visibility(
                    &missing,
                    &nexq_core::model::ReceiptHandle::new(),
                    Duration::from_secs(30),
                )
                .await
                .err(),
        ),
        (
            "set_queue_attributes",
            store
                .set_queue_attributes(&missing, QueueAttributes::default(), SystemTime::now())
                .await
                .err(),
        ),
        ("message_counts", store.message_counts(&missing).await.err()),
        ("purge_queue", store.purge_queue(&missing).await.err()),
    ];

    for (operation, error) in errors {
        let error = error.unwrap_or_else(|| panic!("{operation} should have failed"));
        assert!(
            matches!(&error, StoreError::QueueNotFound(reported) if reported == &missing),
            "{operation} should report QueueNotFound, got {error:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// Sending and claiming
// ---------------------------------------------------------------------------

pub async fn an_enqueued_message_can_be_claimed(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");

    let claimed = store
        .claim_next(&queue_name, None)
        .await
        .expect("claim")
        .expect("a message is waiting");

    assert_eq!(claimed.message.body, "hello");
    assert!(
        !claimed.receipt.as_str().is_empty(),
        "a claim must come with a handle"
    );
}

/// A backend must hand back exactly the attributes it was given.
///
/// Worth asserting across backends rather than trusting: these are covered by a checksum
/// the client recomputes, so a backend that reorders them, loses one, or turns bytes into
/// text makes a consumer reject a message whose data arrived intact. A store column too
/// narrow for a value, or a JSON round trip that mangles binary, would show up here.
pub async fn message_attributes_survive_a_round_trip(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;

    let attributes: MessageAttributes = [
        (
            "City".to_owned(),
            MessageAttribute {
                data_type: "String".to_owned(),
                value: AttributeValue::Text("Any City".to_owned()),
            },
        ),
        (
            "Population".to_owned(),
            MessageAttribute {
                data_type: "Number.count".to_owned(),
                value: AttributeValue::Text("1250800".to_owned()),
            },
        ),
        (
            "Thumb".to_owned(),
            MessageAttribute {
                data_type: "Binary".to_owned(),
                // Deliberately not valid UTF-8, and with an interior zero byte: a
                // backend that stores binary values as text would corrupt this.
                value: AttributeValue::Binary(vec![0xff, 0x00, 0x01, 0xfe]),
            },
        ),
    ]
    .into();

    store
        .enqueue(
            &queue_name,
            message("hello", 0).with_attributes(attributes.clone()),
            None,
        )
        .await
        .expect("enqueue");

    let claimed = store
        .claim_next(&queue_name, None)
        .await
        .expect("claim")
        .expect("a message is waiting");

    assert_eq!(
        claimed.message.attributes, attributes,
        "attributes must come back exactly as they went in"
    );
}

/// A message with no attributes must come back with none, not with an absent-yet-present
/// empty entry that would change its checksum.
pub async fn a_message_without_attributes_has_none(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");

    let claimed = store
        .claim_next(&queue_name, None)
        .await
        .expect("claim")
        .expect("a message is waiting");

    assert!(claimed.message.attributes.is_empty());
}

/// An empty queue is a normal answer, not a failure — every consumer polls one.
pub async fn claiming_an_empty_queue_yields_nothing(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;

    assert!(
        store
            .claim_next(&queue_name, None)
            .await
            .expect("claim")
            .is_none()
    );
}

/// The property a queue exists to provide: one message, one consumer.
pub async fn a_claimed_message_is_hidden_from_other_consumers(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");

    store
        .claim_next(&queue_name, None)
        .await
        .expect("claim")
        .expect("a message");

    assert!(
        claim_body(&store, &queue_name).await.is_none(),
        "two consumers must not hold the same message"
    );
}

/// Claiming counts the delivery and records the first one, which is what lets a
/// consumer tell a retry from a first attempt.
pub async fn claiming_counts_the_delivery(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");

    let claimed = store
        .claim_next(&queue_name, None)
        .await
        .expect("claim")
        .expect("a message");

    assert_eq!(
        claimed.message.receive_count, 1,
        "the delivery in progress counts"
    );
    assert!(
        claimed.message.first_received_at.is_some(),
        "first delivery time must be recorded"
    );
}

pub async fn higher_priority_is_served_first(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    for (body, priority) in [("low", 0), ("urgent", 10), ("medium", 5)] {
        store
            .enqueue(&queue_name, message(body, priority), None)
            .await
            .expect("enqueue");
    }

    assert_eq!(
        claim_body(&store, &queue_name).await.as_deref(),
        Some("urgent")
    );
    assert_eq!(
        claim_body(&store, &queue_name).await.as_deref(),
        Some("medium")
    );
    assert_eq!(
        claim_body(&store, &queue_name).await.as_deref(),
        Some("low")
    );
}

/// Within one priority, order of arrival decides — and arrival means the message's
/// own timestamp, not the order a backend happened to write rows in.
pub async fn equal_priority_is_served_in_order_of_arrival(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    let mut first = message("first", 0);
    let mut second = message("second", 0);
    first.enqueued_at = SystemTime::UNIX_EPOCH + Duration::from_secs(1);
    second.enqueued_at = SystemTime::UNIX_EPOCH + Duration::from_secs(2);

    // Stored in the opposite order, so arrival time is what has to decide.
    store
        .enqueue(&queue_name, second, None)
        .await
        .expect("enqueue");
    store
        .enqueue(&queue_name, first, None)
        .await
        .expect("enqueue");

    assert_eq!(
        claim_body(&store, &queue_name).await.as_deref(),
        Some("first")
    );
    assert_eq!(
        claim_body(&store, &queue_name).await.as_deref(),
        Some("second")
    );
}

pub async fn priority_outranks_arrival_order(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("early_but_low", 0), None)
        .await
        .expect("enqueue");
    store
        .enqueue(&queue_name, message("late_but_high", 1), None)
        .await
        .expect("enqueue");

    assert_eq!(
        claim_body(&store, &queue_name).await.as_deref(),
        Some("late_but_high")
    );
}

/// Under concurrency, every message goes to exactly one consumer: none claimed twice,
/// none skipped.
pub async fn each_message_is_claimed_by_exactly_one_consumer(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    let expected: Vec<String> = (0..8).map(|index| format!("m{index}")).collect();
    for body in &expected {
        store
            .enqueue(&queue_name, message(body, 0), None)
            .await
            .expect("enqueue");
    }

    let consumers: Vec<_> = (0..8)
        .map(|_| {
            let store = Arc::clone(&store);
            let queue_name = queue_name.clone();
            tokio::spawn(async move { claim_body(&store, &queue_name).await })
        })
        .collect();

    let mut claimed = Vec::new();
    for consumer in consumers {
        claimed.push(
            consumer
                .await
                .expect("task")
                .expect("every consumer should have got a message"),
        );
    }
    claimed.sort();

    assert_eq!(claimed, expected, "no message claimed twice, none skipped");
}

// ---------------------------------------------------------------------------
// Delay
// ---------------------------------------------------------------------------

/// A delayed message is not claimable until its delay has passed.
pub async fn a_delayed_message_waits_before_it_can_be_claimed(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("later", 0), Some(SHORT_CLAIM))
        .await
        .expect("enqueue");

    assert!(
        claim_body(&store, &queue_name).await.is_none(),
        "still within its delay"
    );

    tokio::time::sleep(AFTER_SHORT_CLAIM).await;

    assert_eq!(
        claim_body(&store, &queue_name).await.as_deref(),
        Some("later")
    );
}

/// `None` means the queue's configured delay, so a queue-wide delay applies without
/// every caller having to know about it.
pub async fn the_queues_own_delay_applies_when_none_is_given(store: Arc<dyn Store>) {
    let mut delayed = queue("jobs");
    delayed.attributes.delay = SHORT_CLAIM;
    store.create_queue(delayed).await.expect("create");
    let queue_name = name("jobs");

    store
        .enqueue(&queue_name, message("later", 0), None)
        .await
        .expect("enqueue");

    assert!(
        claim_body(&store, &queue_name).await.is_none(),
        "the queue's own delay should apply"
    );

    tokio::time::sleep(AFTER_SHORT_CLAIM).await;

    assert_eq!(
        claim_body(&store, &queue_name).await.as_deref(),
        Some("later")
    );
}

// ---------------------------------------------------------------------------
// Claim expiry and acknowledgement
// ---------------------------------------------------------------------------

/// An expired claim makes the message claimable again, under a new handle. This is
/// what makes delivery at-least-once rather than at-most-once.
pub async fn an_expired_claim_is_redelivered_under_a_new_handle(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");

    let first = store
        .claim_next(&queue_name, Some(SHORT_CLAIM))
        .await
        .expect("claim")
        .expect("a message");

    tokio::time::sleep(AFTER_SHORT_CLAIM).await;

    let second = store
        .claim_next(&queue_name, None)
        .await
        .expect("claim")
        .expect("the lapsed claim should make it claimable again");

    assert_eq!(second.message.id, first.message.id, "the same message");
    assert_eq!(second.message.receive_count, 2, "a second delivery");
    assert_ne!(
        second.receipt, first.receipt,
        "a redelivery must come with a new handle"
    );
    assert_eq!(
        second.message.first_received_at, first.message.first_received_at,
        "the first delivery time must not be overwritten"
    );
}

// ---------------------------------------------------------------------------
// Changing a claim's visibility
// ---------------------------------------------------------------------------

/// Extending a claim keeps the message out of other consumers' hands.
///
/// The reason the operation exists: a consumer whose work outlasts its claim would
/// otherwise have the message handed to someone else while it is still working on it.
pub async fn extending_a_claim_holds_off_redelivery(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");
    let claimed = store
        .claim_next(&queue_name, Some(SHORT_CLAIM))
        .await
        .expect("claim")
        .expect("a message");

    // Extended well past the point the original claim would have lapsed.
    store
        .change_visibility(&queue_name, &claimed.receipt, Duration::from_secs(60))
        .await
        .expect("extend");

    tokio::time::sleep(AFTER_SHORT_CLAIM).await;

    assert!(
        store
            .claim_next(&queue_name, None)
            .await
            .expect("claim")
            .is_none(),
        "the original claim would have lapsed by now, but it was extended"
    );

    // And the handle still works, since extending changes when the claim ends rather
    // than whose it is.
    store
        .ack(&queue_name, &claimed.receipt)
        .await
        .expect("the handle should still be the current one");
}

/// A zero timeout hands the message straight back.
///
/// How a consumer that cannot do the work returns it, instead of holding it until the
/// claim lapses while the queue waits on work nobody is doing.
pub async fn a_zero_visibility_returns_the_message_at_once(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");
    let claimed = store
        .claim_next(&queue_name, Some(Duration::from_secs(60)))
        .await
        .expect("claim")
        .expect("a message");

    store
        .change_visibility(&queue_name, &claimed.receipt, Duration::ZERO)
        .await
        .expect("hand it back");

    // No sleep: it must be claimable now, not after the original minute.
    let again = store
        .claim_next(&queue_name, None)
        .await
        .expect("claim")
        .expect("handed back, so claimable at once");

    assert_eq!(again.message.body, "hello");
    assert_eq!(
        again.message.id, claimed.message.id,
        "the same message, not a copy"
    );
    assert_ne!(
        again.receipt, claimed.receipt,
        "a redelivery comes with a new handle, as any other does"
    );
}

/// Shortening a claim brings redelivery forward.
///
/// The mirror of extending, and the same call: the timeout is counted from now, so a
/// smaller value than what is left means the claim ends sooner.
pub async fn shortening_a_claim_brings_redelivery_forward(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");
    let claimed = store
        .claim_next(&queue_name, Some(Duration::from_secs(60)))
        .await
        .expect("claim")
        .expect("a message");

    store
        .change_visibility(&queue_name, &claimed.receipt, SHORT_CLAIM)
        .await
        .expect("shorten");
    tokio::time::sleep(AFTER_SHORT_CLAIM).await;

    assert!(
        store
            .claim_next(&queue_name, None)
            .await
            .expect("claim")
            .is_some(),
        "the shortened claim should have lapsed, despite the original minute"
    );
}

/// A handle whose claim has ended names nothing to change.
pub async fn changing_visibility_with_a_spent_handle_is_refused(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");
    let claimed = store
        .claim_next(&queue_name, None)
        .await
        .expect("claim")
        .expect("a message");
    store.ack(&queue_name, &claimed.receipt).await.expect("ack");

    let error = store
        .change_visibility(&queue_name, &claimed.receipt, Duration::from_secs(30))
        .await
        .expect_err("the message is gone");
    assert!(
        matches!(error, StoreError::InvalidReceipt),
        "expected InvalidReceipt, got {error:?}"
    );

    let error = store
        .change_visibility(
            &queue_name,
            &nexq_core::model::ReceiptHandle::new(),
            Duration::from_secs(30),
        )
        .await
        .expect_err("that handle was never issued");
    assert!(
        matches!(error, StoreError::InvalidReceipt),
        "expected InvalidReceipt, got {error:?}"
    );
}

/// A handle from a lapsed claim cannot change visibility either.
///
/// Otherwise a consumer whose claim expired could reach in and hide a message another
/// consumer is now working on.
pub async fn a_handle_from_a_lapsed_claim_cannot_change_visibility(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");
    let lapsed = store
        .claim_next(&queue_name, Some(SHORT_CLAIM))
        .await
        .expect("claim")
        .expect("a message");

    tokio::time::sleep(AFTER_SHORT_CLAIM).await;
    let current = store
        .claim_next(&queue_name, None)
        .await
        .expect("claim")
        .expect("redelivered");

    let error = store
        .change_visibility(&queue_name, &lapsed.receipt, Duration::ZERO)
        .await
        .expect_err("the lapsed handle is spent");
    assert!(
        matches!(error, StoreError::InvalidReceipt),
        "expected InvalidReceipt, got {error:?}"
    );

    // The current holder is unaffected, which is what was being protected.
    store
        .ack(&queue_name, &current.receipt)
        .await
        .expect("the current claim should be untouched");
}

/// A handle from a claim that lapsed must not delete the message someone else now
/// holds — otherwise a slow consumer can destroy another's work.
pub async fn a_handle_from_a_lapsed_claim_cannot_ack(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");
    let lapsed = store
        .claim_next(&queue_name, Some(SHORT_CLAIM))
        .await
        .expect("claim")
        .expect("a message");

    tokio::time::sleep(AFTER_SHORT_CLAIM).await;
    let current = store
        .claim_next(&queue_name, None)
        .await
        .expect("claim")
        .expect("redelivered");

    let error = store
        .ack(&queue_name, &lapsed.receipt)
        .await
        .expect_err("the lapsed handle is spent");
    assert!(
        matches!(error, StoreError::InvalidReceipt),
        "expected InvalidReceipt, got {error:?}"
    );

    // The current holder can still ack, which proves nothing was removed.
    store
        .ack(&queue_name, &current.receipt)
        .await
        .expect("the current handle should still work");
}

/// Acking removes the message for good: it must not reappear when the claim it was
/// holding would have expired.
pub async fn acking_removes_the_message_for_good(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");
    let claimed = store
        .claim_next(&queue_name, Some(SHORT_CLAIM))
        .await
        .expect("claim")
        .expect("a message");

    store.ack(&queue_name, &claimed.receipt).await.expect("ack");

    tokio::time::sleep(AFTER_SHORT_CLAIM).await;

    assert!(
        claim_body(&store, &queue_name).await.is_none(),
        "an acked message must not come back when its claim would have expired"
    );
}

pub async fn acking_the_same_claim_twice_is_refused(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");
    let claimed = store
        .claim_next(&queue_name, None)
        .await
        .expect("claim")
        .expect("a message");
    store.ack(&queue_name, &claimed.receipt).await.expect("ack");

    let error = store
        .ack(&queue_name, &claimed.receipt)
        .await
        .expect_err("the handle is spent");

    assert!(
        matches!(error, StoreError::InvalidReceipt),
        "expected InvalidReceipt, got {error:?}"
    );
}

/// A handle the backend never issued must not delete anything — presenting one is how
/// a client would guess its way into another consumer's message.
pub async fn acking_an_unissued_handle_is_refused(store: Arc<dyn Store>) {
    let queue_name = jobs(&store).await;
    store
        .enqueue(&queue_name, message("hello", 0), None)
        .await
        .expect("enqueue");

    let error = store
        .ack(&queue_name, &nexq_core::model::ReceiptHandle::new())
        .await
        .expect_err("not a handle this store issued");

    assert!(
        matches!(error, StoreError::InvalidReceipt),
        "expected InvalidReceipt, got {error:?}"
    );
    assert!(
        claim_body(&store, &queue_name).await.is_some(),
        "the message must still be there"
    );
}
