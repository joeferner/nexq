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

use nexq_core::model::{QueueAttributes, RedrivePolicy};
use serde_json::{Map, Value, json};

use crate::error::ApiError;
use crate::queue_url::QueueUrls;

/// How long a claimed message stays invisible.
const VISIBILITY_TIMEOUT: &str = "VisibilityTimeout";

/// How long a sent message waits before becoming visible.
const DELAY_SECONDS: &str = "DelaySeconds";

/// Default long-poll duration for a receive.
const RECEIVE_WAIT_TIME: &str = "ReceiveMessageWaitTimeSeconds";

/// When to give up on a message, and where to send it.
const REDRIVE_POLICY: &str = "RedrivePolicy";

/// The redrive policy's two members, as SQS spells them inside the JSON document.
///
/// Note that these are camel-case where every attribute *name* is pascal-case — SQS's own
/// inconsistency, faithfully reproduced, because a client that sends what SQS accepts has
/// to be understood here.
const DEAD_LETTER_TARGET_ARN: &str = "deadLetterTargetArn";
const MAX_RECEIVE_COUNT: &str = "maxReceiveCount";

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
pub fn from_input(
    input: Option<&Value>,
    queue_urls: &QueueUrls,
) -> Result<QueueAttributes, ApiError> {
    apply(QueueAttributes::default(), input, queue_urls)
}

/// Apply an attribute map onto attributes a queue already has.
///
/// What `SetQueueAttributes` needs: it names only the attributes it wants changed, and
/// the rest have to keep their current values rather than being reset to defaults. That
/// makes `from_input` the special case where "already has" means "the defaults".
pub fn apply(
    mut attributes: QueueAttributes,
    input: Option<&Value>,
    queue_urls: &QueueUrls,
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
            // An empty string takes the policy off, which is how SQS spells "no redrive
            // policy" on the way in — there is no other way to say it, since the attribute
            // map has no null.
            REDRIVE_POLICY if is_blank(value) => attributes.redrive = None,
            REDRIVE_POLICY => {
                attributes.redrive = Some(redrive_policy(value, queue_urls)?);
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
    matches!(
        name,
        VISIBILITY_TIMEOUT | DELAY_SECONDS | RECEIVE_WAIT_TIME | REDRIVE_POLICY
    )
}

/// Render the settable attributes in SQS's string-to-string form.
///
/// `RedrivePolicy` is present only when the queue has one. SQS reports it that way, and an
/// empty policy would be a third spelling of "none" next to the attribute being absent and
/// its value being an empty string.
pub fn to_output(attributes: &QueueAttributes, queue_urls: &QueueUrls) -> Map<String, Value> {
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

    if let Some(policy) = &attributes.redrive {
        // A JSON *document inside a string*, which is how SQS carries it: the attribute
        // map is string-to-string, so a nested object has to be serialised into one of
        // those strings rather than sent as an object. `maxReceiveCount` is a string in
        // there too, for the same reason every other number here is.
        map.insert(
            REDRIVE_POLICY.to_owned(),
            Value::String(
                json!({
                    DEAD_LETTER_TARGET_ARN: queue_urls.arn_for_queue(&policy.dead_letter_queue),
                    MAX_RECEIVE_COUNT: policy.max_receive_count().to_string(),
                })
                .to_string(),
            ),
        );
    }

    map
}

/// Whether a value is the empty string a client sends to remove an attribute.
fn is_blank(value: &Value) -> bool {
    matches!(value, Value::String(text) if text.trim().is_empty())
}

/// Read a `RedrivePolicy`.
///
/// Accepts the document either as a JSON string — which is the wire form, since the
/// attribute map is string-to-string — or as an object, on the same reasoning that lets a
/// number through where a string is expected: a hand-written client sending what it means
/// gains nothing from being refused.
fn redrive_policy(value: &Value, queue_urls: &QueueUrls) -> Result<RedrivePolicy, ApiError> {
    let parsed;
    let document = match value {
        Value::String(text) => {
            parsed = serde_json::from_str(text).map_err(|error| {
                ApiError::invalid_attribute_value(format!(
                    "Attribute {REDRIVE_POLICY} must be a JSON document: {error}."
                ))
            })?;
            &parsed
        }
        object @ Value::Object(_) => object,
        other => {
            return Err(ApiError::invalid_attribute_value(format!(
                "Attribute {REDRIVE_POLICY} must be a JSON document, got {other}."
            )));
        }
    };

    let Value::Object(document) = document else {
        return Err(ApiError::invalid_attribute_value(format!(
            "Attribute {REDRIVE_POLICY} must be a JSON object with {DEAD_LETTER_TARGET_ARN} \
             and {MAX_RECEIVE_COUNT}."
        )));
    };

    for member in document.keys() {
        if member != DEAD_LETTER_TARGET_ARN && member != MAX_RECEIVE_COUNT {
            return Err(ApiError::invalid_attribute_value(format!(
                "Attribute {REDRIVE_POLICY} has no member {member}."
            )));
        }
    }

    // Both halves are required. Neither means anything alone — a limit with nowhere to
    // send the message would have to drop it or ignore the limit — which is why the domain
    // model holds them as one value and why half a policy is refused here.
    let arn = document
        .get(DEAD_LETTER_TARGET_ARN)
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ApiError::invalid_attribute_value(format!(
                "Attribute {REDRIVE_POLICY} must name a {DEAD_LETTER_TARGET_ARN}."
            ))
        })?;
    let dead_letter_queue = queue_urls.queue_name_from_arn(arn)?;

    let max_receive_count = match document.get(MAX_RECEIVE_COUNT) {
        Some(Value::String(text)) => text.trim().parse::<u32>().ok(),
        Some(Value::Number(number)) => number.as_u64().and_then(|count| u32::try_from(count).ok()),
        _ => None,
    }
    .ok_or_else(|| {
        ApiError::invalid_attribute_value(format!(
            "Attribute {REDRIVE_POLICY} must give a whole-number {MAX_RECEIVE_COUNT}."
        ))
    })?;

    RedrivePolicy::new(max_receive_count, dead_letter_queue).map_err(|error| {
        ApiError::invalid_attribute_value(format!("Attribute {REDRIVE_POLICY} is invalid: {error}."))
    })
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
    use crate::test_support::test_queue_urls;

    /// The two-argument forms against this deployment's queue URLs, which only a
    /// `RedrivePolicy`'s ARN needs — so every other case reads as it did before.
    fn from_input(input: Option<&Value>) -> Result<QueueAttributes, ApiError> {
        super::from_input(input, &test_queue_urls())
    }

    fn to_output(attributes: &QueueAttributes) -> Map<String, Value> {
        super::to_output(attributes, &test_queue_urls())
    }

    /// Read one `RedrivePolicy` value, whichever shape it arrives in.
    fn policy(value: Value) -> Result<RedrivePolicy, ApiError> {
        from_input(Some(&json!({ "RedrivePolicy": value })))?
            .redrive
            .ok_or_else(|| ApiError::invalid_attribute_value("no policy was read"))
    }

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

    /// The wire form: a JSON document inside a string, with a number that is also a
    /// string, because the attribute map is string-to-string all the way down.
    #[test]
    fn a_redrive_policy_arrives_as_a_json_document_in_a_string() {
        let policy = policy(json!(
            r#"{"deadLetterTargetArn":"arn:aws:sqs:us-east-1:000000000000:jobs_dlq",
                "maxReceiveCount":"3"}"#
        ))
        .expect("a valid policy");

        assert_eq!(policy.max_receive_count(), 3);
        assert_eq!(policy.dead_letter_queue.as_str(), "jobs_dlq");
    }

    /// On the same reasoning that accepts a number where a string is expected elsewhere: a
    /// hand-written client sending what it means gains nothing from being refused.
    #[test]
    fn an_object_and_a_numeric_count_are_accepted_too() {
        let policy = policy(json!({
            "deadLetterTargetArn": "arn:aws:sqs:us-east-1:000000000000:jobs_dlq",
            "maxReceiveCount": 3,
        }))
        .expect("a valid policy");

        assert_eq!(policy.max_receive_count(), 3);
    }

    /// Half a policy means nothing: a limit with nowhere to send the message would have to
    /// drop it or ignore the limit, and a target with no limit is never reached.
    #[test]
    fn half_a_redrive_policy_is_refused() {
        for value in [
            json!({ "maxReceiveCount": "3" }),
            json!({ "deadLetterTargetArn": "arn:aws:sqs:us-east-1:000000000000:jobs_dlq" }),
            json!({}),
        ] {
            let error = policy(value.clone()).expect_err(&value.to_string());

            assert_eq!(error.code(), "InvalidAttributeValue", "{value}");
        }
    }

    #[test]
    fn a_malformed_redrive_policy_is_refused() {
        for value in [
            json!("not json at all"),
            json!(3),
            json!([]),
            // A member that is not part of the policy, refused rather than ignored for the
            // same reason an unknown attribute name is.
            json!({
                "deadLetterTargetArn": "arn:aws:sqs:us-east-1:000000000000:jobs_dlq",
                "maxReceiveCount": "3",
                "nope": "1",
            }),
            // Outside the range SQS allows, so a policy accepted here is one SQS would
            // have accepted too.
            json!({
                "deadLetterTargetArn": "arn:aws:sqs:us-east-1:000000000000:jobs_dlq",
                "maxReceiveCount": "0",
            }),
            json!({
                "deadLetterTargetArn": "arn:aws:sqs:us-east-1:000000000000:jobs_dlq",
                "maxReceiveCount": "1001",
            }),
        ] {
            let error = policy(value.clone()).expect_err(&value.to_string());

            assert_eq!(error.code(), "InvalidAttributeValue", "{value}");
        }
    }

    /// An ARN from some other deployment names a queue this one does not have, and acting
    /// on the name alone would silently point the policy at a local queue that happens to
    /// share it.
    #[test]
    fn an_arn_from_another_account_is_refused() {
        let error = policy(json!({
            "deadLetterTargetArn": "arn:aws:sqs:us-east-1:999999999999:jobs_dlq",
            "maxReceiveCount": "3",
        }))
        .expect_err("a different account");

        assert_eq!(error.code(), "InvalidAddress");
    }

    /// There is no null in an attribute map, so an empty string is how SQS says "take
    /// this off". Accepting it is what makes a redrive policy removable at all.
    #[test]
    fn an_empty_redrive_policy_removes_it() {
        let mut attributes = QueueAttributes::default();
        attributes.redrive =
            Some(RedrivePolicy::new(3, nexq_core::model::QueueName::new("jobs_dlq").expect("valid"))
                .expect("valid"));

        let cleared = apply(
            attributes,
            Some(&json!({ "RedrivePolicy": "" })),
            &test_queue_urls(),
        )
        .expect("valid");

        assert_eq!(cleared.redrive, None);
    }

    /// SQS omits `RedrivePolicy` from a queue that has none, and so does this: an empty
    /// policy would be a third spelling of "none".
    #[test]
    fn a_queue_with_no_redrive_policy_reports_none() {
        assert!(!to_output(&QueueAttributes::default()).contains_key("RedrivePolicy"));
    }

    #[test]
    fn a_redrive_policy_round_trips_through_the_wire_form() {
        let attributes = from_input(Some(&json!({
            "RedrivePolicy":
                r#"{"deadLetterTargetArn":"arn:aws:sqs:us-east-1:000000000000:jobs_dlq",
                    "maxReceiveCount":"3"}"#,
        })))
        .expect("valid");

        let rendered = to_output(&attributes);
        let document: Value = serde_json::from_str(
            rendered["RedrivePolicy"]
                .as_str()
                .expect("a JSON document in a string"),
        )
        .expect("valid JSON");

        assert_eq!(
            document["deadLetterTargetArn"],
            "arn:aws:sqs:us-east-1:000000000000:jobs_dlq"
        );
        assert_eq!(
            document["maxReceiveCount"], "3",
            "a string, as every number in an attribute map is"
        );
        assert_eq!(
            from_input(Some(&Value::Object(rendered)))
                .expect("re-read")
                .redrive,
            attributes.redrive
        );
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
