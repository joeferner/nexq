//! Client-supplied message attributes, translated between SQS's wire form and the model.
//!
//! Distinct from the other two attribute modules: [`crate::attributes`] is a queue's
//! configuration, [`crate::system_attributes`] is metadata the *server* knows, and these
//! are the producer's own key-value data, carried through untouched.
//!
//! On the wire each attribute is an object naming a `DataType` and exactly one value:
//!
//! ```json
//! { "City": { "DataType": "String", "StringValue": "Any City" },
//!   "Thumb": { "DataType": "Binary", "BinaryValue": "<base64>" } }
//! ```
//!
//! The rules enforced here are SQS's, taken from its own service model, so an attribute
//! this facade accepts is one real SQS would have accepted. They matter more than most
//! validation does: these attributes are covered by a checksum an SDK recomputes, so
//! quietly altering a name or a value would make a client reject its own message.
//!
//! One name means something extra to NexQ: [`PRIORITY`], which is how a client that can
//! only speak SQS chooses a message's priority. It is read **and kept** — see that
//! constant for why keeping it is not optional. Every other name under the same
//! namespace is refused, so a misspelling of it cannot pass as ordinary metadata.

use std::collections::BTreeSet;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use nexq_core::model::{AttributeValue, MessageAttribute, MessageAttributes, Priority};
use serde_json::{Map, Value};

use crate::error::ApiError;

/// Most attributes SQS accepts on one message.
pub const MAX_ATTRIBUTES: usize = 10;

/// Longest attribute name, and longest data type.
pub const MAX_NAME_LEN: usize = 256;

/// Asks for every attribute on the message.
const ALL: &str = "All";

/// What `All` is spelled as by a client speaking the Query protocol, where the value was
/// matched against attribute names as a pattern.
const ALL_QUERY: &str = ".*";

/// The suffix that turns a name into a prefix match, as in `bar.*`.
const PREFIX_SUFFIX: &str = ".*";

/// Prefixes AWS reserves for itself, in any casing.
const RESERVED_PREFIXES: [&str; 2] = ["aws.", "amazon."];

/// The attribute a producer sets to choose a message's priority — NexQ's own extension,
/// and the only way to express priority through a protocol that has no field for it.
///
/// **Read and then kept**, not consumed. An SDK checksums the attributes it sent and
/// compares the digest against `MD5OfMessageAttributes` in the response, so an attribute
/// the server quietly removed would make a client reject a message that was in fact
/// stored correctly. Keeping it also means a consumer sees exactly what the producer
/// wrote, and that it counts against SQS's ten-attribute and message-size limits like any
/// other attribute — it is the producer's data that NexQ additionally reads, rather than a
/// control channel pretending to be data.
///
/// Named as a NexQ-namespaced attribute the way AWS namespaces its own with `aws.`, which
/// is what makes the whole `nexq.` namespace reservable: a client that types
/// `nexq.priorty` is told the right spelling instead of having its message silently stored
/// at the default.
pub const PRIORITY: &str = "NexQ.Priority";

/// The namespace NexQ reserves for itself, in any casing, mirroring AWS's own reservation.
///
/// Reserved rather than merely used: the alternative is that every misspelling of a
/// well-known name is an ordinary attribute the server ignores, which is the one failure
/// mode a producer cannot see from the response.
const NEXQ_PREFIX: &str = "nexq.";

/// Every name under [`NEXQ_PREFIX`] that means something, and so is allowed.
const WELL_KNOWN: [&str; 1] = [PRIORITY];

/// The three logical data types, which a data type must be or extend.
const DATA_TYPES: [&str; 3] = ["String", "Number", "Binary"];

/// Which of a message's attributes one `ReceiveMessage` asked for.
///
/// Unlike system attributes, a name here is the *producer's* choice, so asking for one
/// the message does not carry is an ordinary miss rather than a mistake — it returns
/// nothing, and no error. There is nothing for the server to be authoritative about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    /// Nothing was asked for.
    None,

    /// Everything the message carries.
    All,

    /// Exact names, and prefixes from a `bar.*` request.
    Some {
        names: BTreeSet<String>,
        prefixes: Vec<String>,
    },
}

/// Read a `SendMessage`-style attribute map.
///
/// A missing map means no attributes, which is what `aws sqs send-message` with no
/// `--message-attributes` sends.
pub fn from_input(input: Option<&Value>) -> Result<MessageAttributes, ApiError> {
    let Some(value) = input else {
        return Ok(MessageAttributes::new());
    };

    if value.is_null() {
        return Ok(MessageAttributes::new());
    }

    let Value::Object(map) = value else {
        return Err(ApiError::invalid_parameter_value(
            "MessageAttributes must be a map of attribute names to values.",
        ));
    };

    if map.len() > MAX_ATTRIBUTES {
        return Err(ApiError::invalid_parameter_value(format!(
            "Number of message attributes {} exceeds the allowed maximum {MAX_ATTRIBUTES}.",
            map.len()
        )));
    }

    let mut attributes = MessageAttributes::new();
    for (name, value) in map {
        check_name(name)?;
        attributes.insert(name.clone(), attribute(name, value)?);
    }

    Ok(attributes)
}

/// Render attributes back out in SQS's wire form.
pub fn to_output(attributes: &MessageAttributes) -> Map<String, Value> {
    attributes
        .iter()
        .map(|(name, attribute)| {
            let mut rendered = Map::new();
            rendered.insert(
                "DataType".to_owned(),
                Value::String(attribute.data_type.clone()),
            );

            match &attribute.value {
                AttributeValue::Text(text) => {
                    rendered.insert("StringValue".to_owned(), Value::String(text.clone()));
                }
                // Binary travels base64-encoded, which is how the JSON protocol carries
                // any blob.
                AttributeValue::Binary(bytes) => {
                    rendered.insert(
                        "BinaryValue".to_owned(),
                        Value::String(BASE64.encode(bytes)),
                    );
                }
            }

            (name.clone(), Value::Object(rendered))
        })
        .collect()
}

impl Selection {
    /// Read the `MessageAttributeNames` of a `ReceiveMessage` input.
    pub fn from_input(input: &Map<String, Value>) -> Result<Self, ApiError> {
        let requested = match input.get("MessageAttributeNames") {
            None | Some(Value::Null) => return Ok(Self::None),
            Some(Value::Array(values)) => values,
            Some(_) => {
                return Err(ApiError::invalid_parameter_value(
                    "MessageAttributeNames must be a list of attribute names.",
                ));
            }
        };

        let mut names = BTreeSet::new();
        let mut prefixes = Vec::new();
        let mut everything = false;

        // The whole list is checked even once `All` has been seen: a non-string in it is
        // a client bug, and reporting it is more use than quietly answering the request
        // it probably did not mean.
        for value in requested {
            let Value::String(asked) = value else {
                return Err(ApiError::invalid_parameter_value(
                    "MessageAttributeNames must be a list of attribute names.",
                ));
            };

            match asked.as_str() {
                ALL | ALL_QUERY => everything = true,
                asked => match asked.strip_suffix(PREFIX_SUFFIX) {
                    Some(prefix) => prefixes.push(prefix.to_owned()),
                    None => {
                        names.insert(asked.to_owned());
                    }
                },
            }
        }

        if everything {
            return Ok(Self::All);
        }

        if names.is_empty() && prefixes.is_empty() {
            return Ok(Self::None);
        }

        Ok(Self::Some { names, prefixes })
    }

    /// Whether nothing at all was asked for, so neither the attributes nor their
    /// checksum belong in the response.
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    /// The attributes of one message that this selection asks for.
    ///
    /// Cloned rather than borrowed because the result is checksummed and rendered as a
    /// unit, and a subset is a different unit from the whole.
    pub fn select(&self, attributes: &MessageAttributes) -> MessageAttributes {
        match self {
            Self::None => MessageAttributes::new(),
            Self::All => attributes.clone(),
            Self::Some { names, prefixes } => attributes
                .iter()
                .filter(|(name, _)| {
                    names.contains(name.as_str())
                        || prefixes
                            .iter()
                            .any(|prefix| name.starts_with(prefix.as_str()))
                })
                .map(|(name, attribute)| (name.clone(), attribute.clone()))
                .collect(),
        }
    }
}

/// Check an attribute name against SQS's rules.
///
/// Uniqueness is not checked here: a JSON object cannot hold the same key twice, so the
/// wire form already guarantees it.
fn check_name(name: &str) -> Result<(), ApiError> {
    let invalid = |detail: String| Err(ApiError::invalid_parameter_value(detail));

    if name.is_empty() {
        return invalid("A message attribute name must not be empty.".to_owned());
    }

    if name.chars().count() > MAX_NAME_LEN {
        return invalid(format!(
            "Message attribute name {name:?} is longer than {MAX_NAME_LEN} characters."
        ));
    }

    if let Some(character) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
    {
        return invalid(format!(
            "Message attribute name {name:?} contains {character:?}, but may only contain \
             letters, digits, underscores, hyphens, and periods."
        ));
    }

    // A leading or trailing period, or two in a row, are all called out separately by
    // SQS, and all of them would survive the character check above.
    if name.starts_with('.') || name.ends_with('.') || name.contains("..") {
        return invalid(format!(
            "Message attribute name {name:?} must not start or end with a period, or \
             contain periods in succession."
        ));
    }

    let lowercased = name.to_ascii_lowercase();
    if let Some(reserved) = RESERVED_PREFIXES
        .iter()
        .find(|prefix| lowercased.starts_with(*prefix))
    {
        return invalid(format!(
            "Message attribute name {name:?} starts with {reserved:?}, which AWS reserves."
        ));
    }

    // NexQ's own namespace, checked case-insensitively so `nexq.priority` is refused
    // rather than accepted as a different attribute from `NexQ.Priority`. Names are
    // case-sensitive on the wire and the value is checksummed, so the facade honours one
    // spelling and tells a client which.
    if lowercased.starts_with(NEXQ_PREFIX) && !WELL_KNOWN.contains(&name) {
        return invalid(format!(
            "Message attribute name {name:?} is in the {NEXQ_PREFIX:?} namespace, which \
             NexQ reserves. The names it defines are: {}.",
            WELL_KNOWN.join(", ")
        ));
    }

    Ok(())
}

/// The priority a message's attributes ask for.
///
/// [`Priority::DEFAULT`] when [`PRIORITY`] is absent, so a message from a client that
/// knows nothing about NexQ behaves exactly as it does today.
///
/// A value that is not a whole number is **refused rather than defaulted**: a producer
/// that set a priority and got the default would have its urgency silently dropped, which
/// is worse than a rejected send it can see and fix. Any textual attribute is accepted —
/// `Number` is the type to use, but a client that sends the digits as a `String` has said
/// the same thing unambiguously, so refusing it would be pedantry. `Binary` is not
/// textual and is refused.
pub fn priority(attributes: &MessageAttributes) -> Result<Priority, ApiError> {
    let Some(attribute) = attributes.get(PRIORITY) else {
        return Ok(Priority::DEFAULT);
    };

    let invalid = |detail: String| {
        Err(ApiError::invalid_parameter_value(format!(
            "Message attribute {PRIORITY:?} sets a message's priority, so {detail}"
        )))
    };

    let AttributeValue::Text(text) = &attribute.value else {
        return invalid("it must carry a StringValue and not a BinaryValue.".to_owned());
    };

    match text.parse::<i32>() {
        Ok(priority) => Ok(Priority::new(priority)),
        // The bounds are named because they are the ones a client will hit: SQS's
        // `Number` type permits any finite decimal, so `1.5` and `3e9` both arrive here
        // having passed every other check.
        Err(_) => invalid(format!(
            "{text:?} must be a whole number between {} and {}.",
            Priority::MIN,
            Priority::MAX
        )),
    }
}

/// Read one attribute's data type and value.
fn attribute(name: &str, value: &Value) -> Result<MessageAttribute, ApiError> {
    let Value::Object(fields) = value else {
        return Err(ApiError::invalid_parameter_value(format!(
            "Message attribute {name:?} must be an object with a DataType and a value."
        )));
    };

    // SQS documents these as reserved for future use and does not implement them, so
    // accepting them while dropping them would lose data on a promise AWS never made.
    for unsupported in ["StringListValues", "BinaryListValues"] {
        if fields.get(unsupported).is_some_and(|value| {
            !value.is_null() && value.as_array().is_some_and(|v| !v.is_empty())
        }) {
            return Err(ApiError::invalid_parameter_value(format!(
                "{unsupported} is not supported, and is not implemented by SQS either."
            )));
        }
    }

    let data_type = data_type(name, fields)?;
    let text = string_field(name, fields, "StringValue")?;
    let binary = string_field(name, fields, "BinaryValue")?;

    // Which logical type it is decides which value field must be set. `Number` travels
    // in `StringValue`, as SQS's own model says it must.
    let expects_binary = data_type.starts_with("Binary");

    let value = match (expects_binary, text, binary) {
        (false, Some(text), None) => AttributeValue::Text(text.to_owned()),
        (true, None, Some(encoded)) => {
            AttributeValue::Binary(BASE64.decode(encoded).map_err(|_| {
                ApiError::invalid_parameter_value(format!(
                    "Message attribute {name:?} has a BinaryValue that is not valid base64."
                ))
            })?)
        }

        // Everything else is a mismatch: the wrong field for the type, both fields, or
        // neither. Reported as one error naming what the type requires, since that is
        // the fact the client needs.
        _ => {
            let expected = if expects_binary {
                "BinaryValue"
            } else {
                "StringValue"
            };

            return Err(ApiError::invalid_parameter_value(format!(
                "Message attribute {name:?} of type {data_type:?} must carry exactly one \
                 {expected}."
            )));
        }
    };

    if value.is_empty() {
        return Err(ApiError::invalid_parameter_value(format!(
            "Message attribute {name:?} has an empty value, which SQS does not allow."
        )));
    }

    if data_type.starts_with("Number") {
        check_number(name, &value)?;
    }

    Ok(MessageAttribute {
        data_type: data_type.to_owned(),
        value,
    })
}

/// Read and check an attribute's `DataType`.
///
/// Must be `String`, `Number`, or `Binary`, optionally with a custom label appended
/// after a period — `String.uuid` is a client saying "text, and here is what it means".
fn data_type<'fields>(
    name: &str,
    fields: &'fields Map<String, Value>,
) -> Result<&'fields str, ApiError> {
    let Some(Value::String(data_type)) = fields.get("DataType") else {
        return Err(ApiError::invalid_parameter_value(format!(
            "Message attribute {name:?} must have a DataType of String, Number, or Binary."
        )));
    };

    if data_type.chars().count() > MAX_NAME_LEN {
        return Err(ApiError::invalid_parameter_value(format!(
            "Message attribute {name:?} has a DataType longer than {MAX_NAME_LEN} characters."
        )));
    }

    let recognised = DATA_TYPES.iter().any(|known| {
        data_type == known
            || data_type
                .strip_prefix(known)
                .is_some_and(|label| label.starts_with('.') && label.len() > 1)
    });

    if !recognised {
        return Err(ApiError::invalid_parameter_value(format!(
            "Message attribute {name:?} has DataType {data_type:?}, which is not String, \
             Number, or Binary, or a custom label on one of them."
        )));
    }

    Ok(data_type)
}

/// One optional string field of an attribute.
fn string_field<'fields>(
    name: &str,
    fields: &'fields Map<String, Value>,
    field: &str,
) -> Result<Option<&'fields str>, ApiError> {
    match fields.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(ApiError::invalid_parameter_value(format!(
            "Message attribute {name:?} has a {field} that is not a string."
        ))),
    }
}

/// Check that a `Number` attribute's value is actually a number.
///
/// Refused rather than passed through, because a consumer that sees `DataType: Number`
/// will parse the value, and finding it unparseable there is worse than finding out here.
fn check_number(name: &str, value: &AttributeValue) -> Result<(), ApiError> {
    let AttributeValue::Text(text) = value else {
        return Ok(());
    };

    if text.parse::<f64>().is_ok_and(|parsed| parsed.is_finite()) {
        return Ok(());
    }

    Err(ApiError::invalid_parameter_value(format!(
        "Message attribute {name:?} is a Number, but {text:?} is not one."
    )))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parsed(input: Value) -> Result<MessageAttributes, ApiError> {
        from_input(Some(&input))
    }

    fn text(value: &str) -> AttributeValue {
        AttributeValue::Text(value.to_owned())
    }

    #[test]
    fn no_attributes_is_an_empty_map() {
        assert!(from_input(None).expect("none").is_empty());
        assert!(parsed(json!(null)).expect("null").is_empty());
        assert!(parsed(json!({})).expect("empty").is_empty());
    }

    #[test]
    fn the_three_data_types_are_read() {
        let attributes = parsed(json!({
            "City": { "DataType": "String", "StringValue": "Any City" },
            "Population": { "DataType": "Number", "StringValue": "1250800" },
            "Thumb": { "DataType": "Binary", "BinaryValue": "SGVsbG8=" },
        }))
        .expect("valid");

        assert_eq!(attributes["City"].value, text("Any City"));
        assert_eq!(attributes["City"].data_type, "String");
        assert_eq!(attributes["Population"].value, text("1250800"));
        assert_eq!(
            attributes["Thumb"].value,
            AttributeValue::Binary(b"Hello".to_vec()),
            "base64 is decoded, since the bytes are what the checksum covers"
        );
    }

    #[test]
    fn a_custom_label_is_kept_as_the_client_wrote_it() {
        // The label is part of the checksum, so changing or normalising it would make a
        // client reject its own message.
        for data_type in ["String.uuid", "Number.float", "Binary.png", "String.a.b"] {
            let value = if data_type.starts_with("Binary") {
                json!({ "DataType": data_type, "BinaryValue": "SGVsbG8=" })
            } else if data_type.starts_with("Number") {
                json!({ "DataType": data_type, "StringValue": "1.5" })
            } else {
                json!({ "DataType": data_type, "StringValue": "x" })
            };

            let attributes = parsed(json!({ "x": value })).expect(data_type);
            assert_eq!(attributes["x"].data_type, data_type);
        }
    }

    #[test]
    fn a_data_type_that_is_not_one_of_the_three_is_refused() {
        for data_type in [
            json!("Str"),
            json!("string"),
            json!("Text"),
            json!("String."),
            json!(""),
            json!(7),
            json!(null),
        ] {
            let error = parsed(json!({ "x": { "DataType": data_type, "StringValue": "v" } }))
                .expect_err(&data_type.to_string());

            assert_eq!(error.code(), "InvalidParameterValue", "{data_type}");
        }

        let error = parsed(json!({ "x": { "StringValue": "v" } })).expect_err("no DataType");
        assert_eq!(error.code(), "InvalidParameterValue");
    }

    #[test]
    fn the_value_field_has_to_match_the_data_type() {
        for (label, attribute) in [
            (
                "binary type, string value",
                json!({ "DataType": "Binary", "StringValue": "hello" }),
            ),
            (
                "string type, binary value",
                json!({ "DataType": "String", "BinaryValue": "SGVsbG8=" }),
            ),
            (
                "number type, binary value",
                json!({ "DataType": "Number", "BinaryValue": "MQ==" }),
            ),
            ("no value at all", json!({ "DataType": "String" })),
            (
                "both values",
                json!({ "DataType": "String", "StringValue": "a", "BinaryValue": "YQ==" }),
            ),
        ] {
            let error = parsed(json!({ "x": attribute })).expect_err(label);

            assert_eq!(error.code(), "InvalidParameterValue", "{label}");
        }
    }

    #[test]
    fn an_empty_value_is_refused() {
        // SQS's own model: "Name, type, value and the message body must not be empty."
        for attribute in [
            json!({ "DataType": "String", "StringValue": "" }),
            json!({ "DataType": "Binary", "BinaryValue": "" }),
        ] {
            let error = parsed(json!({ "x": attribute })).expect_err(&attribute.to_string());

            assert_eq!(error.code(), "InvalidParameterValue");
        }
    }

    #[test]
    fn a_number_attribute_must_hold_a_number() {
        for value in ["1250800", "-5", "1.99", "0", "1e10"] {
            parsed(json!({ "n": { "DataType": "Number", "StringValue": value } })).expect(value);
        }

        for value in ["not a number", "1.2.3", "", "NaN", "inf", "0x10"] {
            let error = parsed(json!({ "n": { "DataType": "Number", "StringValue": value } }))
                .expect_err(value);

            assert_eq!(error.code(), "InvalidParameterValue", "{value:?}");
        }
    }

    #[test]
    fn binary_that_is_not_base64_is_refused() {
        let error = parsed(json!({ "x": { "DataType": "Binary", "BinaryValue": "not base64!" } }))
            .expect_err("bad base64");

        assert_eq!(error.code(), "InvalidParameterValue");
        assert!(error.message().contains("base64"), "{}", error.message());
    }

    #[test]
    fn attribute_names_follow_the_rules_sqs_publishes() {
        for name in ["City", "a", "a_b-c.d", "A1", &"n".repeat(MAX_NAME_LEN)] {
            parsed(json!({ name: { "DataType": "String", "StringValue": "v" } }))
                .unwrap_or_else(|error| panic!("{name:?} should be allowed: {error:?}"));
        }

        for (name, why) in [
            ("", "empty"),
            ("with space", "space"),
            ("with/slash", "slash"),
            ("emoji🎉", "not ascii"),
            (".leading", "leading period"),
            ("trailing.", "trailing period"),
            ("two..periods", "periods in succession"),
            ("AWS.Reserved", "reserved prefix"),
            ("aws.reserved", "reserved prefix, lowercase"),
            ("AwS.Reserved", "reserved prefix, mixed case"),
            ("Amazon.Thing", "reserved prefix"),
        ] {
            let error = parsed(json!({ name: { "DataType": "String", "StringValue": "v" } }))
                .expect_err(why);

            assert_eq!(error.code(), "InvalidParameterValue", "{name:?}: {why}");
        }

        let too_long = "n".repeat(MAX_NAME_LEN + 1);
        parsed(json!({ too_long: { "DataType": "String", "StringValue": "v" } }))
            .expect_err("too long");
    }

    #[test]
    fn nexqs_own_namespace_is_reserved_the_way_awss_is() {
        // The point of reserving it: a misspelled well-known name is the one mistake a
        // producer cannot see in the response, since the message is stored and sent —
        // just at the wrong priority.
        for (name, why) in [
            ("nexq.priority", "the wrong casing"),
            ("NexQ.Priorty", "a typo"),
            ("NEXQ.PRIORITY", "shouting"),
            ("nexq.whatever", "a name NexQ does not define"),
        ] {
            let error = parsed(json!({ name: { "DataType": "Number", "StringValue": "1" } }))
                .expect_err(why);

            assert_eq!(error.code(), "InvalidParameterValue", "{name:?}: {why}");
            assert!(
                error.message().contains(PRIORITY),
                "the error should name the spelling that works: {}",
                error.message()
            );
        }

        // And the names it does define are allowed, or the reservation would be a wall.
        for name in WELL_KNOWN {
            parsed(json!({ name: { "DataType": "Number", "StringValue": "1" } }))
                .unwrap_or_else(|error| panic!("{name:?} must be allowed: {error:?}"));
        }

        // Only as a prefix, matching how `aws.` behaves: an attribute merely mentioning
        // NexQ is the producer's own business.
        for name in ["nexq", "nexqpriority", "NexQPriority", "my.nexq.thing"] {
            parsed(json!({ name: { "DataType": "String", "StringValue": "v" } }))
                .unwrap_or_else(|error| panic!("{name:?} should be allowed: {error:?}"));
        }
    }

    #[test]
    fn priority_comes_from_the_well_known_attribute() {
        let attributes = parsed(json!({
            PRIORITY: { "DataType": "Number", "StringValue": "10" },
        }))
        .expect("valid");

        assert_eq!(
            priority(&attributes).expect("a priority"),
            Priority::new(10)
        );
        assert!(
            attributes.contains_key(PRIORITY),
            "and it stays on the message: an SDK checksums what it sent"
        );
    }

    #[test]
    fn a_message_with_no_priority_attribute_takes_the_default() {
        // The property the whole feature rests on: a client that knows nothing about
        // NexQ behaves exactly as it did before.
        assert_eq!(
            priority(&MessageAttributes::new()).expect("default"),
            Priority::DEFAULT
        );

        let attributes =
            parsed(json!({ "City": { "DataType": "String", "StringValue": "x" } })).expect("valid");
        assert_eq!(priority(&attributes).expect("default"), Priority::DEFAULT);
    }

    #[test]
    fn a_priority_may_be_negative_or_at_the_extremes() {
        for value in ["-1", "0", "-2147483648", "2147483647"] {
            let attributes =
                parsed(json!({ PRIORITY: { "DataType": "Number", "StringValue": value } }))
                    .expect(value);

            assert_eq!(
                priority(&attributes).expect(value).get(),
                value.parse::<i32>().expect("test value"),
            );
        }
    }

    #[test]
    fn digits_sent_as_a_string_say_the_same_thing() {
        // `Number` is the type to use, but a `String` of digits is unambiguous, and a
        // client is not always in charge of how its framework types an attribute.
        let attributes =
            parsed(json!({ PRIORITY: { "DataType": "String", "StringValue": "7" } })).expect("v");

        assert_eq!(priority(&attributes).expect("a priority"), Priority::new(7));
    }

    #[test]
    fn a_priority_that_is_not_a_whole_number_is_refused_rather_than_defaulted() {
        // Every one of these passes SQS's own attribute validation — `Number` permits any
        // finite decimal — so this check is the only thing between a producer that asked
        // for urgency and a message quietly stored at the default.
        for value in ["1.5", "3e9", "9999999999", "-9999999999", " 1", "high"] {
            let attributes =
                parsed(json!({ PRIORITY: { "DataType": "String", "StringValue": value } }))
                    .expect("a valid attribute");

            let error = priority(&attributes).expect_err(value);
            assert_eq!(error.code(), "InvalidParameterValue", "{value:?}");
            assert!(error.message().contains(PRIORITY), "{}", error.message());
        }
    }

    #[test]
    fn a_binary_priority_is_refused() {
        let attributes =
            parsed(json!({ PRIORITY: { "DataType": "Binary", "BinaryValue": "MQ==" } }))
                .expect("a valid attribute");

        assert_eq!(
            priority(&attributes).expect_err("binary").code(),
            "InvalidParameterValue"
        );
    }

    #[test]
    fn a_reserved_prefix_only_bites_when_it_is_a_prefix() {
        // `AWS` and `Amazon` are only reserved followed by a period, so an attribute
        // merely mentioning them is fine.
        for name in ["AWSomeThing", "Amazonian", "my.aws.thing", "AWS"] {
            parsed(json!({ name: { "DataType": "String", "StringValue": "v" } }))
                .unwrap_or_else(|error| panic!("{name:?} should be allowed: {error:?}"));
        }
    }

    #[test]
    fn more_than_ten_attributes_is_refused() {
        let mut input = Map::new();
        for index in 0..=MAX_ATTRIBUTES {
            input.insert(
                format!("a{index}"),
                json!({ "DataType": "String", "StringValue": "v" }),
            );
        }

        let error = from_input(Some(&Value::Object(input))).expect_err("eleven");

        assert_eq!(error.code(), "InvalidParameterValue");
        assert!(error.message().contains("10"), "{}", error.message());
    }

    #[test]
    fn the_list_value_fields_sqs_never_implemented_are_refused() {
        for field in ["StringListValues", "BinaryListValues"] {
            let error = parsed(json!({
                "x": { "DataType": "String", "StringValue": "v", field: ["a"] }
            }))
            .expect_err(field);

            assert_eq!(error.code(), "InvalidParameterValue", "{field}");
        }

        // An empty list is what some SDKs send for an unset field, and must not trip it.
        for field in ["StringListValues", "BinaryListValues"] {
            parsed(json!({
                "x": { "DataType": "String", "StringValue": "v", field: [] }
            }))
            .unwrap_or_else(|error| panic!("empty {field}: {error:?}"));
        }
    }

    #[test]
    fn an_attribute_that_is_not_an_object_is_refused() {
        let error = parsed(json!({ "x": "just a string" })).expect_err("not an object");
        assert_eq!(error.code(), "InvalidParameterValue");

        let error = from_input(Some(&json!("City=Any City"))).expect_err("not a map");
        assert_eq!(error.code(), "InvalidParameterValue");
    }

    #[test]
    fn attributes_round_trip_through_the_wire_form() {
        let original = parsed(json!({
            "City": { "DataType": "String", "StringValue": "Any City" },
            "Population": { "DataType": "Number.count", "StringValue": "1250800" },
            "Thumb": { "DataType": "Binary", "BinaryValue": "SGVsbG8sIFdvcmxkIQ==" },
        }))
        .expect("valid");

        let rendered = to_output(&original);
        assert_eq!(rendered["City"]["StringValue"], "Any City");
        assert_eq!(rendered["Thumb"]["BinaryValue"], "SGVsbG8sIFdvcmxkIQ==");
        assert_eq!(rendered["Population"]["DataType"], "Number.count");

        assert_eq!(
            from_input(Some(&Value::Object(rendered))).expect("re-read"),
            original,
            "what a consumer receives must parse back to what the producer sent"
        );
    }

    /// Three attributes to select from.
    fn selectable() -> MessageAttributes {
        parsed(json!({
            "City": { "DataType": "String", "StringValue": "Any City" },
            "bar.one": { "DataType": "String", "StringValue": "1" },
            "bar.two": { "DataType": "String", "StringValue": "2" },
        }))
        .expect("valid")
    }

    fn selection(input: Value) -> Result<Selection, ApiError> {
        let Value::Object(input) = input else {
            panic!("test input must be an object");
        };

        Selection::from_input(&input)
    }

    fn selected(input: Value) -> Vec<String> {
        selection(input)
            .expect("valid selection")
            .select(&selectable())
            .into_keys()
            .collect()
    }

    #[test]
    fn nothing_is_selected_unless_it_is_asked_for() {
        assert!(selection(json!({})).expect("absent").is_none());
        assert!(
            selection(json!({ "MessageAttributeNames": [] }))
                .expect("empty")
                .is_none()
        );
        assert!(selected(json!({})).is_empty());
    }

    #[test]
    fn all_selects_everything() {
        assert_eq!(
            selected(json!({ "MessageAttributeNames": ["All"] })),
            ["City", "bar.one", "bar.two"]
        );
        assert_eq!(
            selection(json!({ "MessageAttributeNames": [".*"] })).expect(".*"),
            Selection::All,
            "the Query protocol's spelling of All"
        );
    }

    #[test]
    fn names_select_exactly_themselves() {
        assert_eq!(
            selected(json!({ "MessageAttributeNames": ["City"] })),
            ["City"]
        );
        assert_eq!(
            selected(json!({ "MessageAttributeNames": ["City", "bar.two"] })),
            ["City", "bar.two"]
        );
    }

    #[test]
    fn a_prefix_selects_the_family_under_it() {
        // Documented by SQS as `bar.*`, and worth having: attribute families are how
        // clients namespace their own metadata.
        assert_eq!(
            selected(json!({ "MessageAttributeNames": ["bar.*"] })),
            ["bar.one", "bar.two"]
        );
        assert_eq!(
            selected(json!({ "MessageAttributeNames": ["bar.one", "Cit.*"] })),
            ["City", "bar.one"],
            "names and prefixes combine"
        );
    }

    #[test]
    fn asking_for_an_attribute_the_message_does_not_have_is_not_an_error() {
        // The opposite of system attributes: these names are the producer's, so the
        // server has nothing to be authoritative about and a miss is just a miss.
        assert!(selected(json!({ "MessageAttributeNames": ["Nope"] })).is_empty());
        assert_eq!(
            selected(json!({ "MessageAttributeNames": ["Nope", "City"] })),
            ["City"],
            "a miss does not take the hits with it"
        );
    }

    #[test]
    fn a_selection_that_is_not_a_list_of_strings_is_refused() {
        for input in [
            json!({ "MessageAttributeNames": "All" }),
            json!({ "MessageAttributeNames": ["All", 7] }),
            json!({ "MessageAttributeNames": {} }),
        ] {
            let error = selection(input.clone()).expect_err(&input.to_string());

            assert_eq!(error.code(), "InvalidParameterValue", "{input}");
        }
    }
}
