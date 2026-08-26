//! Where each SQS operation is handled.
//!
//! Routing ends here: the request has been recognised as a specific [`Operation`] and
//! its input decoded. Each operation gets its own handler as the engine behind it
//! lands; until then they are all reported as not implemented, which is a different
//! answer to the client than "no such operation".
//!
//! Nothing here authenticates. SigV4 signatures are currently accepted without being
//! verified — see [`crate::server`] — so every handler runs for any caller.

use serde_json::{Map, Value, json};

use crate::error::ApiError;
use crate::protocol::Operation;

/// Invoke an operation.
pub async fn dispatch(operation: Operation, input: Map<String, Value>) -> Result<Value, ApiError> {
    match operation {
        Operation::ListQueues => list_queues(&input),
        not_built_yet => Err(ApiError::not_implemented(not_built_yet)),
    }
}

/// `ListQueues`.
///
/// There is no store behind the facade yet, so there are never any queues. The
/// `QueueNamePrefix`, `MaxResults`, and `NextToken` inputs are accepted and ignored,
/// since nothing they could filter or page over exists.
///
/// The response is an empty object rather than `{"QueueUrls": []}`: real SQS omits the
/// field when there are no queues, and `aws sqs list-queues` prints nothing at all in
/// that case, where an explicit empty array would make it print one.
fn list_queues(_input: &Map<String, Value>) -> Result<Value, ApiError> {
    Ok(json!({}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn list_queues_reports_no_queues() {
        let output = dispatch(Operation::ListQueues, Map::new())
            .await
            .expect("list queues");

        assert_eq!(output, json!({}));
    }

    #[tokio::test]
    async fn list_queues_ignores_inputs_it_has_nothing_to_apply_them_to() {
        let mut input = Map::new();
        input.insert("QueueNamePrefix".to_owned(), json!("jobs"));
        input.insert("MaxResults".to_owned(), json!(10));

        let output = dispatch(Operation::ListQueues, input).await.expect("list");

        assert_eq!(output, json!({}));
    }

    #[tokio::test]
    async fn operations_without_handlers_are_still_not_implemented() {
        let error = dispatch(Operation::SendMessage, Map::new())
            .await
            .expect_err("no handler yet");

        assert_eq!(error.code(), "NotImplemented");
    }
}
