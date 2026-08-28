//! Message system attributes: the metadata SQS reports alongside a received message.
//!
//! Distinct from [`crate::attributes`], which is about a *queue's* configuration. These
//! are per-message facts the server knows — when it was sent, how many times it has been
//! delivered — and a client only gets them if it asks, by name or with `All`.
//!
//! Two parameters name them, and `ReceiveMessage` accepts both: `AttributeNames`, which
//! SQS has deprecated, and `MessageSystemAttributeNames`, which replaced it. A request
//! carrying both gets the union, since either one is a request for that attribute.
//!
//! Anything asked for by name that NexQ cannot answer is refused rather than omitted.
//! `All` is the opposite: it means "whatever you have", so it returns what exists and
//! says nothing about what does not — the same answer real SQS gives for a message with
//! no value for an attribute.
//!
//! One name here is NexQ's own rather than SQS's — `NexQ.Priority`, which is the only way
//! an SQS client can read the priority of a message it did not send. It answers when it is
//! **named** and is deliberately left out of `All`, so the response a client gets without
//! asking for anything NexQ-specific stays exactly the shape real SQS returns.

use std::collections::BTreeSet;

use nexq_core::model::{Message, epoch_millis};
use serde_json::{Map, Value};

use crate::error::ApiError;
use crate::message_attributes;

/// The deprecated parameter naming system attributes.
const ATTRIBUTE_NAMES: &str = "AttributeNames";

/// The parameter that replaced it.
const MESSAGE_SYSTEM_ATTRIBUTE_NAMES: &str = "MessageSystemAttributeNames";

/// Asks for every attribute that has a value.
const ALL: &str = "All";

/// What `All` is spelled as by an older client speaking the Query protocol, where the
/// value was a regular expression matched against attribute names.
const ALL_QUERY: &str = ".*";

/// A system attribute NexQ can report.
///
/// Deliberately not every name SQS defines: `SenderId` would need the sending
/// principal recorded on the message, and the FIFO trio (`MessageGroupId`,
/// `MessageDeduplicationId`, `SequenceNumber`) describes ordering NexQ does not
/// implement. Both are refused when named, so nobody builds on an answer of `None`.
///
/// Ordered as declared, which is the order [`Requested::render`] emits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Attribute {
    SentTimestamp,
    ApproximateReceiveCount,
    ApproximateFirstReceiveTimestamp,

    /// NexQ's own, declared last so it renders after the SQS names.
    Priority,
}

/// Every attribute a client may name.
const KNOWN: [Attribute; 4] = [
    Attribute::SentTimestamp,
    Attribute::ApproximateReceiveCount,
    Attribute::ApproximateFirstReceiveTimestamp,
    Attribute::Priority,
];

/// What `All` selects: everything SQS itself would return, and nothing NexQ invented.
///
/// `Priority` is the one [`KNOWN`] attribute missing from this list, and the omission is
/// the point. Every message has a priority, so including it would put a NexQ-only name on
/// every response to `AttributeNames: ["All"]` — turning the shape of the *default* answer
/// into something real SQS never sends, for the benefit of clients that did not ask.
/// Naming it explicitly is a client saying it wants NexQ's extension, and that request is
/// always answered.
const REPORTABLE: [Attribute; 3] = [
    Attribute::SentTimestamp,
    Attribute::ApproximateReceiveCount,
    Attribute::ApproximateFirstReceiveTimestamp,
];

/// Which system attributes one `ReceiveMessage` asked for.
///
/// A set, so naming an attribute twice — or once by name and again under `All` — asks
/// for it once.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Requested(BTreeSet<Attribute>);

impl Attribute {
    /// The name a client uses, and the key in the response map.
    fn name(self) -> &'static str {
        match self {
            Self::SentTimestamp => "SentTimestamp",
            Self::ApproximateReceiveCount => "ApproximateReceiveCount",
            Self::ApproximateFirstReceiveTimestamp => "ApproximateFirstReceiveTimestamp",
            // The same spelling a producer uses to *set* it, so one concept has one name
            // however it is reached.
            Self::Priority => message_attributes::PRIORITY,
        }
    }

    fn parse(name: &str) -> Option<Self> {
        KNOWN.into_iter().find(|attribute| attribute.name() == name)
    }

    /// This attribute's value for a message, or `None` when the message has none.
    ///
    /// Every SQS timestamp is milliseconds since the epoch, rendered as a string —
    /// system attribute values are strings on the wire whatever they mean.
    fn value(self, message: &Message) -> Option<String> {
        match self {
            Self::SentTimestamp => Some(epoch_millis(message.enqueued_at).to_string()),
            Self::ApproximateReceiveCount => Some(message.receive_count.to_string()),
            // Absent only for a message that has never been delivered, which cannot be
            // one that is being handed to a consumer. Omitted rather than faked so the
            // rule holds even if some future caller renders an unclaimed message.
            Self::ApproximateFirstReceiveTimestamp => message
                .first_received_at
                .map(|first| epoch_millis(first).to_string()),
            // Always present: a message always has a priority, even if nothing chose it.
            // Reported whichever facade sent the message, which is the reason to answer it
            // here at all — a message sent through REST carries no priority *attribute*
            // for an SQS consumer to read.
            Self::Priority => Some(message.priority.to_string()),
        }
    }
}

impl Requested {
    /// Read the attribute names out of a `ReceiveMessage` input.
    pub fn from_input(input: &Map<String, Value>) -> Result<Self, ApiError> {
        let mut requested = BTreeSet::new();

        for parameter in [ATTRIBUTE_NAMES, MESSAGE_SYSTEM_ATTRIBUTE_NAMES] {
            for name in names(input, parameter)? {
                match name.as_str() {
                    ALL | ALL_QUERY => requested.extend(REPORTABLE),
                    named => match Attribute::parse(named) {
                        Some(attribute) => {
                            requested.insert(attribute);
                        }
                        // Refused rather than skipped: a client that named an attribute
                        // needs it, and silently answering without it reads as "this
                        // message has no such value".
                        None => return Err(ApiError::invalid_attribute_name(named)),
                    },
                }
            }
        }

        Ok(Self(requested))
    }

    /// Whether anything was asked for.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The `Attributes` map for one message.
    ///
    /// Empty when nothing was asked for, or when nothing asked for has a value — the
    /// caller omits the field entirely in that case, the way SQS does.
    pub fn render(&self, message: &Message) -> Map<String, Value> {
        self.0
            .iter()
            .filter_map(|attribute| {
                let value = attribute.value(message)?;
                Some((attribute.name().to_owned(), Value::String(value)))
            })
            .collect()
    }
}

/// The strings in one attribute-names parameter.
///
/// A missing parameter is an empty list, not an error: not asking for attributes is the
/// ordinary case.
fn names(input: &Map<String, Value>, parameter: &str) -> Result<Vec<String>, ApiError> {
    let wrong_shape = || {
        ApiError::invalid_parameter_value(format!("{parameter} must be a list of attribute names."))
    };

    match input.get(parameter) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| match value {
                Value::String(name) => Ok(name.clone()),
                _ => Err(wrong_shape()),
            })
            .collect(),
        Some(_) => Err(wrong_shape()),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    use nexq_core::model::Priority;
    use serde_json::json;

    use super::*;

    /// A message sent at a known time and delivered twice.
    fn message() -> Message {
        let sent = UNIX_EPOCH + Duration::from_millis(1_760_000_000_123);

        Message {
            enqueued_at: sent,
            receive_count: 2,
            first_received_at: Some(sent + Duration::from_secs(5)),
            ..Message::new("hello", Priority::DEFAULT)
        }
    }

    fn requested(input: Value) -> Result<Requested, ApiError> {
        let Value::Object(input) = input else {
            panic!("test input must be an object");
        };

        Requested::from_input(&input)
    }

    #[test]
    fn asking_for_nothing_reports_nothing() {
        let requested = requested(json!({})).expect("no attributes");

        assert!(requested.is_empty());
        assert!(requested.render(&message()).is_empty());
    }

    #[test]
    fn an_empty_list_is_the_same_as_no_list() {
        // What `--attribute-names` with no values looks like on the wire.
        let requested = requested(json!({ "AttributeNames": [] })).expect("empty list");

        assert!(requested.is_empty());
    }

    #[test]
    fn all_reports_everything_there_is() {
        let requested = requested(json!({ "AttributeNames": ["All"] })).expect("all");

        let rendered = requested.render(&message());
        assert_eq!(rendered["SentTimestamp"], "1760000000123");
        assert_eq!(rendered["ApproximateReceiveCount"], "2");
        assert_eq!(
            rendered["ApproximateFirstReceiveTimestamp"], "1760000005123",
            "five seconds after it was sent, in epoch millis"
        );
        assert_eq!(rendered.len(), 3, "and nothing else: {rendered:?}");
    }

    #[test]
    fn all_leaves_out_the_attribute_nexq_invented() {
        // Deliberate: every message has a priority, so reporting it under `All` would put
        // a name real SQS never sends on every response to a request that asked for
        // "whatever you have".
        let rendered = requested(json!({ "AttributeNames": ["All"] }))
            .expect("all")
            .render(&message());

        assert!(
            !rendered.contains_key(message_attributes::PRIORITY),
            "{rendered:?}"
        );
    }

    #[test]
    fn priority_is_reported_when_it_is_named() {
        // The point of answering it here rather than only as a message attribute: this
        // message carries no priority attribute — it is what a message sent through REST
        // looks like — and its priority is still readable.
        let named = json!({ "AttributeNames": [message_attributes::PRIORITY] });

        let urgent = Message {
            priority: Priority::new(7),
            ..message()
        };

        let rendered = requested(named.clone()).expect("named").render(&urgent);
        assert_eq!(rendered[message_attributes::PRIORITY], "7");
        assert_eq!(rendered.len(), 1, "and nothing else: {rendered:?}");

        // Including the default, which is a value like any other rather than an absence.
        let rendered = requested(named).expect("named").render(&message());
        assert_eq!(rendered[message_attributes::PRIORITY], "0");
    }

    #[test]
    fn a_query_protocol_client_spells_all_as_a_pattern() {
        assert_eq!(
            requested(json!({ "AttributeNames": [".*"] })).expect(".*"),
            requested(json!({ "AttributeNames": ["All"] })).expect("All")
        );
    }

    #[test]
    fn only_what_was_named_comes_back() {
        let requested =
            requested(json!({ "AttributeNames": ["ApproximateReceiveCount"] })).expect("one");

        let rendered = requested.render(&message());
        assert_eq!(rendered.len(), 1, "{rendered:?}");
        assert_eq!(rendered["ApproximateReceiveCount"], "2");
    }

    #[test]
    fn both_parameters_are_accepted_and_combined() {
        // `AttributeNames` is deprecated but still what plenty of clients send, so
        // supporting only its replacement would break them.
        let requested = requested(json!({
            "AttributeNames": ["SentTimestamp"],
            "MessageSystemAttributeNames": ["ApproximateReceiveCount"],
        }))
        .expect("both");

        let rendered = requested.render(&message());
        assert_eq!(rendered.len(), 2, "{rendered:?}");
        assert!(rendered.contains_key("SentTimestamp"));
        assert!(rendered.contains_key("ApproximateReceiveCount"));
    }

    #[test]
    fn asking_twice_asks_once() {
        let requested = requested(json!({
            "AttributeNames": ["SentTimestamp", "SentTimestamp", "All"],
            "MessageSystemAttributeNames": ["SentTimestamp"],
        }))
        .expect("duplicates");

        assert_eq!(requested.render(&message()).len(), 3);
    }

    #[test]
    fn an_attribute_nexq_cannot_answer_is_refused_when_it_is_named() {
        // Real SQS names for values NexQ does not have. Answering without them would
        // read as "this message has no sender", which is not what is true.
        for name in [
            "SenderId",
            "AWSTraceHeader",
            "MessageGroupId",
            "MessageDeduplicationId",
            "SequenceNumber",
            "DeadLetterQueueSourceArn",
            "NotAnAttributeAtAll",
            "senttimestamp",
            // NexQ's own name is answered, but only spelled the one way it is defined.
            "nexq.priority",
            "NexQ.Priorty",
        ] {
            let error = requested(json!({ "AttributeNames": [name] })).expect_err(name);

            assert_eq!(error.code(), "InvalidAttributeName", "{name}");
            assert!(error.message().contains(name), "{}", error.message());
        }
    }

    #[test]
    fn all_stays_silent_about_what_it_cannot_answer() {
        // The counterpart to the test above: `All` asks for whatever exists, so the
        // attributes NexQ has no value for are simply absent, exactly as they would be
        // for a standard queue in real SQS.
        let rendered = requested(json!({ "AttributeNames": ["All"] }))
            .expect("all")
            .render(&message());

        for absent in ["SenderId", "MessageGroupId", "SequenceNumber"] {
            assert!(!rendered.contains_key(absent), "{absent}");
        }
    }

    #[test]
    fn a_never_delivered_message_has_no_first_receive_time() {
        let fresh = Message::new("hello", Priority::DEFAULT);

        let rendered = requested(json!({ "AttributeNames": ["All"] }))
            .expect("all")
            .render(&fresh);

        assert_eq!(rendered["ApproximateReceiveCount"], "0");
        assert!(
            !rendered.contains_key("ApproximateFirstReceiveTimestamp"),
            "omitted rather than zero, which would claim a delivery at the epoch"
        );
    }

    #[test]
    fn a_parameter_that_is_not_a_list_of_strings_is_refused() {
        for input in [
            json!({ "AttributeNames": "All" }),
            json!({ "AttributeNames": ["All", 7] }),
            json!({ "AttributeNames": {} }),
            json!({ "MessageSystemAttributeNames": 3 }),
        ] {
            let error = requested(input.clone()).expect_err(&input.to_string());

            assert_eq!(error.code(), "InvalidParameterValue", "{input}");
        }
    }

    #[test]
    fn values_are_strings_even_when_they_are_numbers() {
        // SQS renders every system attribute value as a string, and SDKs parse them as
        // such; a JSON number here would break a client that expects to parse one.
        let rendered = requested(json!({ "AttributeNames": ["All"] }))
            .expect("all")
            .render(&message());

        for (name, value) in &rendered {
            assert!(value.is_string(), "{name} is {value}");
        }
    }
}
