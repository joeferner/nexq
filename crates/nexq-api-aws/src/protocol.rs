//! Wire-protocol decoding: which operation a request is asking for, and its input.
//!
//! `aws-cli` v2 and current SDKs talk to SQS with AWS JSON 1.0: every operation is a
//! `POST /` naming the operation in an `X-Amz-Target` header, with a JSON body. Older
//! SDKs use the Query protocol (`Action=` in a form-encoded body, XML back), which is
//! not decoded here yet — a request without a target header is reported as such rather
//! than as a generic parse failure, so the distinction stays visible in logs.

use std::fmt;
use std::str::FromStr;

use serde_json::{Map, Value};

use crate::error::ApiError;

/// Header naming the operation to invoke: `X-Amz-Target: AmazonSQS.ListQueues`.
pub const TARGET_HEADER: &str = "x-amz-target";

/// Content type of an AWS JSON 1.0 request and response body.
pub const JSON_CONTENT_TYPE: &str = "application/x-amz-json-1.0";

/// The service prefix on every SQS target. SNS has no JSON protocol and is still
/// Query-only, so it will not arrive this way.
pub const SQS_TARGET_PREFIX: &str = "AmazonSQS";

/// An operation the SQS facade knows by name.
///
/// Being an enum rather than a string is what separates "operation we have not built
/// yet" from "operation that does not exist", which are different errors to a client.
///
/// **Every operation SQS has**, not only the ones implemented here — that is what makes
/// the distinction possible. A client calling `TagQueue` is calling something real, and
/// telling it the operation does not exist would send it looking for a typo it has not
/// made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
    ChangeMessageVisibility,
    ChangeMessageVisibilityBatch,
    CreateQueue,
    DeleteMessage,
    DeleteMessageBatch,
    DeleteQueue,
    GetQueueAttributes,
    GetQueueUrl,
    ListQueues,
    PurgeQueue,
    ReceiveMessage,
    SendMessage,
    SendMessageBatch,
    SetQueueAttributes,

    // Recognised, not built. Access policies, which NexQ answers with its own
    // credential registry instead.
    AddPermission,
    RemovePermission,

    // Recognised, not built. Tagging, which needs somewhere on a queue to put tags.
    TagQueue,
    UntagQueue,
    ListQueueTags,

    // Recognised, not built. All dead-letter queue territory, which arrives with DLQ
    // and redrive.
    ListDeadLetterSourceQueues,
    StartMessageMoveTask,
    CancelMessageMoveTask,
    ListMessageMoveTasks,
}

impl Operation {
    /// The operation's name as it appears after the service prefix in a target.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ChangeMessageVisibility => "ChangeMessageVisibility",
            Self::ChangeMessageVisibilityBatch => "ChangeMessageVisibilityBatch",
            Self::CreateQueue => "CreateQueue",
            Self::DeleteMessage => "DeleteMessage",
            Self::DeleteMessageBatch => "DeleteMessageBatch",
            Self::DeleteQueue => "DeleteQueue",
            Self::GetQueueAttributes => "GetQueueAttributes",
            Self::GetQueueUrl => "GetQueueUrl",
            Self::ListQueues => "ListQueues",
            Self::PurgeQueue => "PurgeQueue",
            Self::ReceiveMessage => "ReceiveMessage",
            Self::SendMessage => "SendMessage",
            Self::SendMessageBatch => "SendMessageBatch",
            Self::SetQueueAttributes => "SetQueueAttributes",
            Self::AddPermission => "AddPermission",
            Self::RemovePermission => "RemovePermission",
            Self::TagQueue => "TagQueue",
            Self::UntagQueue => "UntagQueue",
            Self::ListQueueTags => "ListQueueTags",
            Self::ListDeadLetterSourceQueues => "ListDeadLetterSourceQueues",
            Self::StartMessageMoveTask => "StartMessageMoveTask",
            Self::CancelMessageMoveTask => "CancelMessageMoveTask",
            Self::ListMessageMoveTasks => "ListMessageMoveTasks",
        }
    }

    /// Parse a full `X-Amz-Target` value, prefix included.
    pub fn from_target(target: &str) -> Result<Self, ApiError> {
        let (service, operation) = target
            .split_once('.')
            .ok_or_else(|| ApiError::unknown_operation(target))?;

        if service != SQS_TARGET_PREFIX {
            return Err(ApiError::unknown_operation(target));
        }

        operation
            .parse()
            .map_err(|_| ApiError::unknown_operation(target))
    }
}

impl FromStr for Operation {
    type Err = UnknownOperation;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Ok(match name {
            "ChangeMessageVisibility" => Self::ChangeMessageVisibility,
            "ChangeMessageVisibilityBatch" => Self::ChangeMessageVisibilityBatch,
            "CreateQueue" => Self::CreateQueue,
            "DeleteMessage" => Self::DeleteMessage,
            "DeleteMessageBatch" => Self::DeleteMessageBatch,
            "DeleteQueue" => Self::DeleteQueue,
            "GetQueueAttributes" => Self::GetQueueAttributes,
            "GetQueueUrl" => Self::GetQueueUrl,
            "ListQueues" => Self::ListQueues,
            "PurgeQueue" => Self::PurgeQueue,
            "ReceiveMessage" => Self::ReceiveMessage,
            "SendMessage" => Self::SendMessage,
            "SendMessageBatch" => Self::SendMessageBatch,
            "SetQueueAttributes" => Self::SetQueueAttributes,
            "AddPermission" => Self::AddPermission,
            "RemovePermission" => Self::RemovePermission,
            "TagQueue" => Self::TagQueue,
            "UntagQueue" => Self::UntagQueue,
            "ListQueueTags" => Self::ListQueueTags,
            "ListDeadLetterSourceQueues" => Self::ListDeadLetterSourceQueues,
            "StartMessageMoveTask" => Self::StartMessageMoveTask,
            "CancelMessageMoveTask" => Self::CancelMessageMoveTask,
            "ListMessageMoveTasks" => Self::ListMessageMoveTasks,
            _ => return Err(UnknownOperation),
        })
    }
}

impl fmt::Display for Operation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// No operation goes by that name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownOperation;

/// Decode a request body into the operation's input.
///
/// An empty body means "no input", which clients send for parameterless operations
/// instead of an explicit `{}`. Anything else must be a JSON object: a bare array or
/// scalar is well-formed JSON but cannot carry named parameters.
pub fn decode_input(body: &[u8]) -> Result<Map<String, Value>, ApiError> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(Map::new());
    }

    match serde_json::from_slice(body) {
        Ok(Value::Object(input)) => Ok(input),
        Ok(other) => Err(ApiError::malformed_body(format!(
            "expected a JSON object, got {}",
            json_kind(&other)
        ))),
        Err(error) => Err(ApiError::malformed_body(error.to_string())),
    }
}

fn json_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_target_names_the_service_and_the_operation() {
        assert_eq!(
            Operation::from_target("AmazonSQS.ListQueues").expect("known operation"),
            Operation::ListQueues
        );
    }

    #[test]
    fn every_operation_round_trips_through_its_target() {
        let operations = [
            Operation::ChangeMessageVisibility,
            Operation::ChangeMessageVisibilityBatch,
            Operation::CreateQueue,
            Operation::DeleteMessage,
            Operation::DeleteMessageBatch,
            Operation::DeleteQueue,
            Operation::GetQueueAttributes,
            Operation::GetQueueUrl,
            Operation::ListQueues,
            Operation::PurgeQueue,
            Operation::ReceiveMessage,
            Operation::SendMessage,
            Operation::SendMessageBatch,
            Operation::SetQueueAttributes,
            Operation::AddPermission,
            Operation::RemovePermission,
            Operation::TagQueue,
            Operation::UntagQueue,
            Operation::ListQueueTags,
            Operation::ListDeadLetterSourceQueues,
            Operation::StartMessageMoveTask,
            Operation::CancelMessageMoveTask,
            Operation::ListMessageMoveTasks,
        ];

        assert_eq!(
            operations.len(),
            23,
            "SQS has 23 operations, and all of them should be recognised — an \
             unimplemented one is a different answer from an unknown one"
        );

        for operation in operations {
            let target = format!("{SQS_TARGET_PREFIX}.{operation}");
            assert_eq!(
                Operation::from_target(&target).expect("round trip"),
                operation,
                "{target}"
            );
        }
    }

    #[test]
    fn a_target_for_another_service_is_not_ours() {
        // SNS is Query-only, so this can only be a misdirected client.
        Operation::from_target("AmazonSNS.Publish").expect_err("wrong service");
        Operation::from_target("AmazonS3.GetObject").expect_err("wrong service");
    }

    #[test]
    fn a_target_without_an_operation_is_rejected() {
        Operation::from_target("AmazonSQS").expect_err("no separator");
        Operation::from_target("ListQueues").expect_err("no service");
        Operation::from_target("AmazonSQS.Nope").expect_err("no such operation");
        Operation::from_target("amazonsqs.listqueues").expect_err("case matters");
    }

    #[test]
    fn an_empty_body_decodes_to_no_input() {
        assert!(decode_input(b"").expect("empty").is_empty());
        assert!(decode_input(b"   \n").expect("whitespace").is_empty());
        assert!(decode_input(b"{}").expect("empty object").is_empty());
    }

    #[test]
    fn an_object_body_decodes_to_its_fields() {
        let input = decode_input(br#"{"QueueName":"jobs","Attributes":{"DelaySeconds":"5"}}"#)
            .expect("object");

        assert_eq!(input["QueueName"], Value::String("jobs".to_owned()));
        assert!(input["Attributes"].is_object());
    }

    #[test]
    fn a_body_that_is_not_an_object_is_rejected() {
        let error = decode_input(b"[1,2,3]").expect_err("array");
        assert!(error.message().contains("an array"), "{error:?}");

        decode_input(b"\"just a string\"").expect_err("string");
        decode_input(b"{not json}").expect_err("malformed");
    }
}
