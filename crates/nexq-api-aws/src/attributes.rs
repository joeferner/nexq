//! Queue attributes, translated between SQS's wire form and the domain model.
//!
//! On the wire every attribute is a string-to-string map, numbers included, so
//! `VisibilityTimeout` arrives as `"30"` rather than `30`. The ranges enforced here are
//! SQS's, so a value this facade accepts is one real SQS would have accepted too.
//!
//! Only the attributes the domain model has a home for are understood. Anything else
//! is rejected rather than ignored: silently dropping `FifoQueue` would leave a client
//! believing it had ordering guarantees that do not exist.

use std::time::Duration;

use nexq_core::model::QueueAttributes;
use serde_json::{Map, Value};

use crate::error::ApiError;

/// How long a claimed message stays invisible.
const VISIBILITY_TIMEOUT: &str = "VisibilityTimeout";

/// How long a sent message waits before becoming visible.
const DELAY_SECONDS: &str = "DelaySeconds";

/// Default long-poll duration for a receive.
const RECEIVE_WAIT_TIME: &str = "ReceiveMessageWaitTimeSeconds";

/// Longest visibility timeout SQS accepts: 12 hours.
///
/// Shared with the per-request overrides on `ReceiveMessage`, which are bounded the
/// same way — one limit, so a value accepted as a queue default is also accepted as an
/// override.
pub const VISIBILITY_TIMEOUT_MAX: u64 = 12 * 60 * 60;

/// Longest delay SQS accepts: 15 minutes.
pub const DELAY_SECONDS_MAX: u64 = 15 * 60;

/// Longest long-poll wait SQS accepts: 20 seconds.
pub const RECEIVE_WAIT_TIME_MAX: u64 = 20;

/// Read a `CreateQueue`-style attribute map, starting from the defaults.
///
/// A missing map means "all defaults", which is what `aws sqs create-queue` with no
/// `--attributes` sends.
pub fn from_input(input: Option<&Value>) -> Result<QueueAttributes, ApiError> {
    apply(QueueAttributes::default(), input)
}

/// Apply an attribute map onto attributes a queue already has.
///
/// What `SetQueueAttributes` needs: it names only the attributes it wants changed, and
/// the rest have to keep their current values rather than being reset to defaults. That
/// makes `from_input` the special case where "already has" means "the defaults".
pub fn apply(
    mut attributes: QueueAttributes,
    input: Option<&Value>,
) -> Result<QueueAttributes, ApiError> {
    let Some(value) = input else {
        return Ok(attributes);
    };

    let Value::Object(map) = value else {
        return Err(ApiError::invalid_parameter_value(
            "Attributes must be a map of attribute names to values.",
        ));
    };

    for (name, value) in map {
        match name.as_str() {
            VISIBILITY_TIMEOUT => {
                attributes.visibility_timeout = seconds(name, value, VISIBILITY_TIMEOUT_MAX)?;
            }
            DELAY_SECONDS => {
                attributes.delay = seconds(name, value, DELAY_SECONDS_MAX)?;
            }
            RECEIVE_WAIT_TIME => {
                attributes.receive_wait_time = seconds(name, value, RECEIVE_WAIT_TIME_MAX)?;
            }
            unknown => return Err(ApiError::invalid_attribute_name(unknown)),
        }
    }

    Ok(attributes)
}

/// Whether this is an attribute a client may set, and which this module handles.
///
/// The read side needs to ask, so it can tell a settable attribute from one it derives
/// itself — see [`crate::queue_attributes`].
pub fn is_settable(name: &str) -> bool {
    matches!(name, VISIBILITY_TIMEOUT | DELAY_SECONDS | RECEIVE_WAIT_TIME)
}

/// Render the settable attributes in SQS's string-to-string form.
pub fn to_output(attributes: &QueueAttributes) -> Map<String, Value> {
    let mut map = Map::new();

    for (name, duration) in [
        (VISIBILITY_TIMEOUT, attributes.visibility_timeout),
        (DELAY_SECONDS, attributes.delay),
        (RECEIVE_WAIT_TIME, attributes.receive_wait_time),
    ] {
        map.insert(
            name.to_owned(),
            Value::String(duration.as_secs().to_string()),
        );
    }

    map
}

/// Parse a duration in seconds, bounded as SQS bounds it.
///
/// Accepts a JSON number as well as a string. The wire form is a string, but a
/// hand-written client or a config generator may send the number it means, and there is
/// nothing to gain from refusing it.
fn seconds(name: &str, value: &Value, max: u64) -> Result<Duration, ApiError> {
    let seconds = match value {
        Value::String(text) => text.trim().parse::<u64>().map_err(|_| {
            ApiError::invalid_attribute_value(format!(
                "Attribute {name} must be a whole number of seconds, got {text:?}."
            ))
        })?,
        Value::Number(number) => number.as_u64().ok_or_else(|| {
            ApiError::invalid_attribute_value(format!(
                "Attribute {name} must be a whole number of seconds, got {number}."
            ))
        })?,
        other => {
            return Err(ApiError::invalid_attribute_value(format!(
                "Attribute {name} must be a number of seconds, got {other}."
            )));
        }
    };

    if seconds > max {
        return Err(ApiError::invalid_attribute_value(format!(
            "Attribute {name} must be between 0 and {max} seconds, got {seconds}."
        )));
    }

    Ok(Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn no_attributes_means_defaults() {
        assert_eq!(from_input(None).expect("none"), QueueAttributes::default());
        assert_eq!(
            from_input(Some(&json!({}))).expect("empty"),
            QueueAttributes::default()
        );
    }

    #[test]
    fn the_supported_attributes_are_read() {
        let attributes = from_input(Some(&json!({
            "VisibilityTimeout": "120",
            "DelaySeconds": "5",
            "ReceiveMessageWaitTimeSeconds": "20",
        })))
        .expect("valid attributes");

        assert_eq!(attributes.visibility_timeout, Duration::from_secs(120));
        assert_eq!(attributes.delay, Duration::from_secs(5));
        assert_eq!(attributes.receive_wait_time, Duration::from_secs(20));
    }

    #[test]
    fn numbers_are_accepted_as_well_as_strings() {
        // The wire form is a string, but there is nothing to gain from refusing a
        // client that sends the number it means.
        let attributes =
            from_input(Some(&json!({ "VisibilityTimeout": 45 }))).expect("numeric value");

        assert_eq!(attributes.visibility_timeout, Duration::from_secs(45));
    }

    #[test]
    fn an_unset_attribute_keeps_its_default() {
        let attributes =
            from_input(Some(&json!({ "DelaySeconds": "5" }))).expect("partial attributes");

        assert_eq!(attributes.delay, Duration::from_secs(5));
        assert_eq!(
            attributes.visibility_timeout,
            QueueAttributes::default().visibility_timeout
        );
    }

    #[test]
    fn an_unknown_attribute_is_rejected_rather_than_ignored() {
        // FifoQueue is the case that matters: accepting it silently would promise
        // ordering guarantees that do not exist.
        for name in ["FifoQueue", "KmsMasterKeyId", "VisibilityTimeOut"] {
            let error = from_input(Some(&json!({ name: "true" }))).expect_err(name);

            assert_eq!(error.code(), "InvalidAttributeName", "{name}");
            assert!(error.message().contains(name), "{}", error.message());
        }
    }

    #[test]
    fn a_value_that_is_not_a_number_of_seconds_is_rejected() {
        for value in [
            json!("soon"),
            json!(""),
            json!("-5"),
            json!(true),
            json!(null),
        ] {
            let error = from_input(Some(&json!({ "VisibilityTimeout": value })))
                .expect_err(&value.to_string());

            assert_eq!(error.code(), "InvalidAttributeValue", "{value}");
        }
    }

    #[test]
    fn values_outside_the_ranges_sqs_allows_are_rejected() {
        for (name, over_the_limit) in [
            (VISIBILITY_TIMEOUT, VISIBILITY_TIMEOUT_MAX + 1),
            (DELAY_SECONDS, DELAY_SECONDS_MAX + 1),
            (RECEIVE_WAIT_TIME, RECEIVE_WAIT_TIME_MAX + 1),
        ] {
            let error =
                from_input(Some(&json!({ name: over_the_limit.to_string() }))).expect_err(name);

            assert_eq!(error.code(), "InvalidAttributeValue", "{name}");
        }
    }

    #[test]
    fn the_boundaries_themselves_are_allowed() {
        for (name, at_the_limit) in [
            (VISIBILITY_TIMEOUT, VISIBILITY_TIMEOUT_MAX),
            (DELAY_SECONDS, DELAY_SECONDS_MAX),
            (RECEIVE_WAIT_TIME, RECEIVE_WAIT_TIME_MAX),
        ] {
            from_input(Some(&json!({ name: at_the_limit.to_string() }))).expect(name);
            from_input(Some(&json!({ name: "0" }))).expect(name);
        }
    }

    #[test]
    fn attributes_are_not_a_map_at_all() {
        let error = from_input(Some(&json!("VisibilityTimeout=30"))).expect_err("not a map");

        assert_eq!(error.code(), "InvalidParameterValue");
    }

    #[test]
    fn attributes_round_trip_through_the_wire_form() {
        let attributes = from_input(Some(&json!({
            "VisibilityTimeout": "120",
            "DelaySeconds": "5",
            "ReceiveMessageWaitTimeSeconds": "20",
        })))
        .expect("valid");

        let rendered = to_output(&attributes);
        assert_eq!(rendered["VisibilityTimeout"], "120");
        assert_eq!(rendered["DelaySeconds"], "5");
        assert_eq!(rendered["ReceiveMessageWaitTimeSeconds"], "20");

        assert_eq!(
            from_input(Some(&Value::Object(rendered))).expect("re-read"),
            attributes
        );
    }
}
