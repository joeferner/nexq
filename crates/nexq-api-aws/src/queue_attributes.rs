//! Which queue attributes a `GetQueueAttributes` asked for, and what to answer.
//!
//! Distinct from [`crate::attributes`], which reads the *settable* attributes off a
//! `CreateQueue` or `SetQueueAttributes` request. This is the read side, and it is a
//! wider set: alongside the three NexQ stores, a queue has facts derived from it — how
//! many messages it holds, when it was created, what its ARN is — that a client can ask
//! for but not set.
//!
//! `All` returns everything there is to report. A name asked for explicitly that NexQ
//! cannot answer is refused, on the same reasoning as message system attributes: `All`
//! means "whatever you have", while naming an attribute means a client needs it, and
//! answering without it reads as "this queue has no such value".

use std::collections::BTreeSet;

use nexq_core::model::{MAX_BODY_BYTES, MessageCounts, Queue, epoch_millis};
use serde_json::{Map, Value};

use crate::attributes;
use crate::error::ApiError;
use crate::queue_url::QueueUrls;

/// Asks for every attribute there is.
const ALL: &str = "All";

/// What `All` is spelled as by a client speaking the Query protocol.
const ALL_QUERY: &str = ".*";

/// An attribute a client can read but not set.
///
/// The settable ones are [`crate::attributes`]'s business; these are facts *about* the
/// queue rather than knobs on it, which is why asking to set one is an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Derived {
    ApproximateNumberOfMessages,
    ApproximateNumberOfMessagesNotVisible,
    ApproximateNumberOfMessagesDelayed,
    CreatedTimestamp,
    LastModifiedTimestamp,
    MaximumMessageSize,
    QueueArn,
}

/// Every derived attribute, which is what `All` adds to the settable ones.
const DERIVED: [Derived; 7] = [
    Derived::ApproximateNumberOfMessages,
    Derived::ApproximateNumberOfMessagesNotVisible,
    Derived::ApproximateNumberOfMessagesDelayed,
    Derived::CreatedTimestamp,
    Derived::LastModifiedTimestamp,
    Derived::MaximumMessageSize,
    Derived::QueueArn,
];

/// Attributes NexQ knows the name of but has no honest answer for.
///
/// Kept as a list so the error can say *why* rather than "unknown attribute", since a
/// client asking for one of these has not made a spelling mistake — the attribute is
/// real, and this deployment does not implement what it describes.
const UNIMPLEMENTED: [(&str, &str); 10] = [
    (
        "MessageRetentionPeriod",
        "NexQ does not expire messages, so there is no retention period to report",
    ),
    ("Policy", "access policies are not implemented"),
    (
        "RedriveAllowPolicy",
        "NexQ does not restrict which queues may use a queue as their dead-letter queue, \
         so there is no allow policy to report",
    ),
    ("FifoQueue", "FIFO queues are not implemented"),
    (
        "ContentBasedDeduplication",
        "FIFO queues are not implemented",
    ),
    ("DeduplicationScope", "FIFO queues are not implemented"),
    ("FifoThroughputLimit", "FIFO queues are not implemented"),
    (
        "KmsMasterKeyId",
        "server-side encryption is not implemented",
    ),
    (
        "KmsDataKeyReusePeriodSeconds",
        "server-side encryption is not implemented",
    ),
    (
        "SqsManagedSseEnabled",
        "server-side encryption is not implemented",
    ),
];

/// Which attributes one `GetQueueAttributes` asked for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Requested {
    /// Settable attributes asked for by name, or all of them.
    settable: Selection,

    /// Derived attributes asked for.
    derived: BTreeSet<Derived>,
}

/// Whether a group of attributes was asked for wholesale or by name.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum Selection {
    #[default]
    None,
    All,
    Named(BTreeSet<String>),
}

impl Requested {
    /// Read the `AttributeNames` of a `GetQueueAttributes` input.
    ///
    /// No names means no attributes, which is what SQS does — "if you don't specify
    /// values for this parameter, the request returns empty results".
    pub fn from_input(input: &Map<String, Value>) -> Result<Self, ApiError> {
        let names = match input.get("AttributeNames") {
            None | Some(Value::Null) => return Ok(Self::default()),
            Some(Value::Array(values)) => values,
            Some(_) => return Err(wrong_shape()),
        };

        let mut settable = BTreeSet::new();
        let mut derived = BTreeSet::new();
        let mut everything = false;

        for value in names {
            let Value::String(asked) = value else {
                return Err(wrong_shape());
            };

            match asked.as_str() {
                ALL | ALL_QUERY => everything = true,
                asked if attributes::is_settable(asked) => {
                    settable.insert(asked.to_owned());
                }
                asked => match Derived::parse(asked) {
                    Some(attribute) => {
                        derived.insert(attribute);
                    }
                    None => return Err(unsupported(asked)),
                },
            }
        }

        if everything {
            return Ok(Self {
                settable: Selection::All,
                derived: DERIVED.into_iter().collect(),
            });
        }

        Ok(Self {
            settable: if settable.is_empty() {
                Selection::None
            } else {
                Selection::Named(settable)
            },
            derived,
        })
    }

    /// Whether anything was asked for, so the response can omit `Attributes` entirely.
    pub fn is_empty(&self) -> bool {
        self.settable == Selection::None && self.derived.is_empty()
    }

    /// Whether answering needs the message counts, which is the one expensive part.
    ///
    /// Asked separately so a request that only wants, say, `VisibilityTimeout` does not
    /// make a backend aggregate over a queue's messages for nothing.
    pub fn needs_counts(&self) -> bool {
        self.derived.iter().any(|attribute| {
            matches!(
                attribute,
                Derived::ApproximateNumberOfMessages
                    | Derived::ApproximateNumberOfMessagesNotVisible
                    | Derived::ApproximateNumberOfMessagesDelayed
            )
        })
    }

    /// The `Attributes` map for a queue.
    pub fn render(
        &self,
        queue: &Queue,
        counts: &MessageCounts,
        queue_urls: &QueueUrls,
    ) -> Map<String, Value> {
        let mut rendered = Map::new();

        let settable = attributes::to_output(&queue.attributes, queue_urls);
        match &self.settable {
            Selection::None => {}
            Selection::All => rendered.extend(settable),
            Selection::Named(names) => {
                for (name, value) in settable {
                    if names.contains(&name) {
                        rendered.insert(name, value);
                    }
                }
            }
        }

        for attribute in &self.derived {
            rendered.insert(
                attribute.name().to_owned(),
                Value::String(attribute.value(queue, counts, queue_urls)),
            );
        }

        rendered
    }
}

impl Derived {
    fn name(self) -> &'static str {
        match self {
            Self::ApproximateNumberOfMessages => "ApproximateNumberOfMessages",
            Self::ApproximateNumberOfMessagesNotVisible => "ApproximateNumberOfMessagesNotVisible",
            Self::ApproximateNumberOfMessagesDelayed => "ApproximateNumberOfMessagesDelayed",
            Self::CreatedTimestamp => "CreatedTimestamp",
            Self::LastModifiedTimestamp => "LastModifiedTimestamp",
            Self::MaximumMessageSize => "MaximumMessageSize",
            Self::QueueArn => "QueueArn",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        DERIVED
            .into_iter()
            .find(|attribute| attribute.name() == name)
    }

    fn value(self, queue: &Queue, counts: &MessageCounts, queue_urls: &QueueUrls) -> String {
        match self {
            Self::ApproximateNumberOfMessages => counts.visible.to_string(),
            Self::ApproximateNumberOfMessagesNotVisible => counts.not_visible.to_string(),
            Self::ApproximateNumberOfMessagesDelayed => counts.delayed.to_string(),
            // Seconds, not milliseconds. SQS reports message timestamps in millis and
            // these two in seconds, which is easy to get wrong in the same codebase.
            Self::CreatedTimestamp => epoch_seconds(queue.created_at).to_string(),
            Self::LastModifiedTimestamp => epoch_seconds(queue.last_modified_at).to_string(),
            Self::MaximumMessageSize => MAX_BODY_BYTES.to_string(),
            Self::QueueArn => queue_urls.arn_for_queue(&queue.name),
        }
    }
}

/// Seconds since the epoch, which is how SQS reports a queue's timestamps.
fn epoch_seconds(time: std::time::SystemTime) -> u64 {
    epoch_millis(time) / 1000
}

fn wrong_shape() -> ApiError {
    ApiError::invalid_parameter_value("AttributeNames must be a list of attribute names.")
}

/// A real SQS attribute name this deployment cannot answer, or one that is not an
/// attribute at all.
fn unsupported(name: &str) -> ApiError {
    match UNIMPLEMENTED
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
    {
        Some((known, why)) => {
            ApiError::invalid_attribute_value(format!("Attribute {known} is not supported: {why}."))
        }
        None => ApiError::invalid_attribute_name(name),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use nexq_core::model::{QueueAttributes, QueueName};
    use serde_json::json;

    use super::*;
    use crate::test_support::test_queue_urls;

    /// A queue created at a known time and modified five minutes later.
    fn queue() -> Queue {
        let created = UNIX_EPOCH + Duration::from_millis(1_760_000_000_123);

        Queue {
            name: QueueName::new("jobs").expect("valid"),
            created_at: created,
            last_modified_at: created + Duration::from_secs(300),
            attributes: QueueAttributes {
                visibility_timeout: Duration::from_secs(120),
                delay: Duration::from_secs(5),
                receive_wait_time: Duration::from_secs(20),
                ..QueueAttributes::default()
            },
        }
    }

    fn counts() -> MessageCounts {
        MessageCounts {
            visible: 7,
            not_visible: 3,
            delayed: 2,
        }
    }

    fn requested(input: Value) -> Result<Requested, ApiError> {
        let Value::Object(input) = input else {
            panic!("test input must be an object");
        };

        Requested::from_input(&input)
    }

    fn rendered(input: Value) -> Map<String, Value> {
        requested(input)
            .expect("valid selection")
            .render(&queue(), &counts(), &test_queue_urls())
    }

    #[test]
    fn asking_for_nothing_reports_nothing() {
        // SQS: "if you don't specify values for this parameter, the request returns
        // empty results".
        assert!(requested(json!({})).expect("absent").is_empty());
        assert!(
            requested(json!({ "AttributeNames": [] }))
                .expect("empty")
                .is_empty()
        );
        assert!(rendered(json!({})).is_empty());
    }

    #[test]
    fn all_reports_everything_there_is() {
        let rendered = rendered(json!({ "AttributeNames": ["All"] }));

        // The settable three.
        assert_eq!(rendered["VisibilityTimeout"], "120");
        assert_eq!(rendered["DelaySeconds"], "5");
        assert_eq!(rendered["ReceiveMessageWaitTimeSeconds"], "20");
        // The derived seven.
        assert_eq!(rendered["ApproximateNumberOfMessages"], "7");
        assert_eq!(rendered["ApproximateNumberOfMessagesNotVisible"], "3");
        assert_eq!(rendered["ApproximateNumberOfMessagesDelayed"], "2");
        assert_eq!(rendered["CreatedTimestamp"], "1760000000");
        assert_eq!(rendered["LastModifiedTimestamp"], "1760000300");
        assert_eq!(rendered["MaximumMessageSize"], "262144");
        assert_eq!(
            rendered["QueueArn"],
            "arn:aws:sqs:us-east-1:000000000000:jobs"
        );

        assert_eq!(rendered.len(), 10, "and nothing else: {rendered:?}");
    }

    #[test]
    fn a_query_protocol_client_spells_all_as_a_pattern() {
        assert_eq!(
            requested(json!({ "AttributeNames": [".*"] })).expect(".*"),
            requested(json!({ "AttributeNames": ["All"] })).expect("All")
        );
    }

    #[test]
    fn timestamps_are_in_seconds_not_milliseconds() {
        // The trap: SQS reports message timestamps in milliseconds and a queue's in
        // seconds, so the same codebase has to do both.
        let rendered = rendered(json!({
            "AttributeNames": ["CreatedTimestamp", "LastModifiedTimestamp"]
        }));

        assert_eq!(rendered["CreatedTimestamp"], "1760000000");
        assert_eq!(
            rendered["LastModifiedTimestamp"], "1760000300",
            "five minutes after creation"
        );
    }

    #[test]
    fn only_what_was_named_comes_back() {
        let rendered = rendered(json!({
            "AttributeNames": ["VisibilityTimeout", "QueueArn"]
        }));

        assert_eq!(rendered.len(), 2, "{rendered:?}");
        assert_eq!(rendered["VisibilityTimeout"], "120");
        assert!(rendered.contains_key("QueueArn"));
    }

    #[test]
    fn settable_and_derived_attributes_mix_freely() {
        let rendered = rendered(json!({
            "AttributeNames": ["DelaySeconds", "ApproximateNumberOfMessages"]
        }));

        assert_eq!(rendered["DelaySeconds"], "5");
        assert_eq!(rendered["ApproximateNumberOfMessages"], "7");
        assert_eq!(rendered.len(), 2, "{rendered:?}");
    }

    #[test]
    fn asking_twice_asks_once() {
        let rendered = rendered(json!({
            "AttributeNames": ["QueueArn", "QueueArn", "VisibilityTimeout"]
        }));

        assert_eq!(rendered.len(), 2, "{rendered:?}");
    }

    #[test]
    fn the_counts_are_only_fetched_when_they_are_wanted() {
        // Counting means aggregating over a queue's messages, so a request that does not
        // want the numbers should not make a backend produce them.
        for (input, expected) in [
            (json!({ "AttributeNames": ["VisibilityTimeout"] }), false),
            (json!({ "AttributeNames": ["QueueArn"] }), false),
            (json!({ "AttributeNames": ["CreatedTimestamp"] }), false),
            (json!({}), false),
            (
                json!({ "AttributeNames": ["ApproximateNumberOfMessages"] }),
                true,
            ),
            (
                json!({ "AttributeNames": ["ApproximateNumberOfMessagesDelayed"] }),
                true,
            ),
            (
                json!({ "AttributeNames": ["ApproximateNumberOfMessagesNotVisible"] }),
                true,
            ),
            (json!({ "AttributeNames": ["All"] }), true),
        ] {
            assert_eq!(
                requested(input.clone()).expect("valid").needs_counts(),
                expected,
                "{input}"
            );
        }
    }

    #[test]
    fn a_real_attribute_nexq_does_not_implement_says_why() {
        // These are not typos, so "Unknown Attribute" would be a misleading answer: the
        // attribute exists and this deployment has nothing behind it.
        for (name, expected_in_message) in [
            ("MessageRetentionPeriod", "does not expire messages"),
            ("RedrivePolicy", "dead-letter"),
            ("FifoQueue", "FIFO"),
            ("KmsMasterKeyId", "encryption"),
            ("SqsManagedSseEnabled", "encryption"),
            ("Policy", "access policies"),
        ] {
            let error = requested(json!({ "AttributeNames": [name] })).expect_err(name);

            assert_eq!(error.code(), "InvalidAttributeValue", "{name}");
            assert!(
                error.message().contains(expected_in_message),
                "{name}: {}",
                error.message()
            );
        }
    }

    #[test]
    fn a_name_that_is_not_an_attribute_at_all_is_unknown() {
        for name in ["Nope", "visibilitytimeout", "QueueARN", ""] {
            let error = requested(json!({ "AttributeNames": [name] })).expect_err(name);

            assert_eq!(error.code(), "InvalidAttributeName", "{name:?}");
        }
    }

    #[test]
    fn all_answers_without_the_attributes_it_cannot_report() {
        // The counterpart: `All` asks for whatever exists, so an unimplemented attribute
        // is simply absent rather than an error.
        let rendered = rendered(json!({ "AttributeNames": ["All"] }));

        for absent in [
            "MessageRetentionPeriod",
            "Policy",
            "RedrivePolicy",
            "FifoQueue",
            "SqsManagedSseEnabled",
        ] {
            assert!(!rendered.contains_key(absent), "{absent}");
        }
    }

    #[test]
    fn a_parameter_that_is_not_a_list_of_strings_is_refused() {
        for input in [
            json!({ "AttributeNames": "All" }),
            json!({ "AttributeNames": ["All", 7] }),
            json!({ "AttributeNames": {} }),
        ] {
            let error = requested(input.clone()).expect_err(&input.to_string());

            assert_eq!(error.code(), "InvalidParameterValue", "{input}");
        }
    }

    #[test]
    fn every_value_is_a_string() {
        // SQS renders the whole map as strings, counts and timestamps included, and SDKs
        // parse them as such.
        for (name, value) in &rendered(json!({ "AttributeNames": ["All"] })) {
            assert!(value.is_string(), "{name} is {value}");
        }
    }

    #[test]
    fn the_counts_cover_everything_stored() {
        let counts = counts();

        assert_eq!(counts.total(), 12, "7 visible + 3 in flight + 2 delayed");
    }

    #[test]
    fn a_created_queue_reports_the_same_two_timestamps() {
        // Never modified, so the two agree — which is what makes a later difference
        // meaningful.
        let queue = Queue::new(QueueName::new("jobs").expect("valid"));
        let rendered = Requested::from_input(
            json!({ "AttributeNames": ["CreatedTimestamp", "LastModifiedTimestamp"] })
                .as_object()
                .expect("object"),
        )
        .expect("valid")
        .render(&queue, &MessageCounts::default(), &test_queue_urls());

        assert_eq!(
            rendered["CreatedTimestamp"],
            rendered["LastModifiedTimestamp"]
        );
    }

    #[test]
    fn a_queue_arn_is_built_from_config() {
        let arn = test_queue_urls().arn_for_queue(&QueueName::new("jobs").expect("valid"));

        assert_eq!(arn, "arn:aws:sqs:us-east-1:000000000000:jobs");
    }

    #[test]
    fn a_time_before_the_epoch_does_not_panic() {
        let queue = Queue {
            created_at: SystemTime::UNIX_EPOCH - Duration::from_secs(1),
            ..queue()
        };

        let rendered = Requested::from_input(
            json!({ "AttributeNames": ["CreatedTimestamp"] })
                .as_object()
                .expect("object"),
        )
        .expect("valid")
        .render(&queue, &MessageCounts::default(), &test_queue_urls());

        assert_eq!(rendered["CreatedTimestamp"], "0");
    }
}
