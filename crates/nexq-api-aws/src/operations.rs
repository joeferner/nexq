//! Where each SQS operation is handled.
//!
//! Routing ends here: the request has been authenticated, recognised as a specific
//! [`Operation`], and its input decoded. Each handler translates SQS's wire shape into
//! an engine call and back — no queueing logic lives in this crate, because a facade
//! that decided things for itself would answer differently from REST.

use std::sync::Arc;
use std::time::Duration;

use nexq_core::engine::Engine;
use nexq_core::model::{Priority, QueueName, ReceiptHandle};
use serde_json::{Map, Value, json};

use crate::error::ApiError;
use crate::protocol::Operation;
use crate::queue_url::QueueUrls;
use crate::{attributes, checksum};

/// The engine, plus what this facade needs to talk about queues in URLs.
#[derive(Debug)]
pub struct Operations {
    engine: Arc<Engine>,
    queue_urls: QueueUrls,
}

impl Operations {
    pub fn new(engine: Arc<Engine>, queue_urls: QueueUrls) -> Self {
        Self { engine, queue_urls }
    }

    /// Invoke an operation.
    pub async fn dispatch(
        &self,
        operation: Operation,
        input: Map<String, Value>,
    ) -> Result<Value, ApiError> {
        match operation {
            Operation::CreateQueue => self.create_queue(&input).await,
            Operation::DeleteQueue => self.delete_queue(&input).await,
            Operation::GetQueueUrl => self.get_queue_url(&input).await,
            Operation::ListQueues => self.list_queues(&input).await,
            Operation::SendMessage => self.send_message(&input).await,
            Operation::ReceiveMessage => self.receive_message(&input).await,
            Operation::DeleteMessage => self.delete_message(&input).await,
            not_built_yet => Err(ApiError::not_implemented(not_built_yet)),
        }
    }

    /// `CreateQueue` — idempotent when the attributes match, per the engine.
    async fn create_queue(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let name = queue_name(input, "QueueName")?;
        let attributes = attributes::from_input(input.get("Attributes"))?;

        let queue = self.engine.create_queue(name, attributes).await?;

        Ok(json!({ "QueueUrl": self.queue_urls.for_queue(&queue.name) }))
    }

    /// `DeleteQueue`.
    async fn delete_queue(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let name = self.queue_from_url(input)?;

        self.engine.delete_queue(&name).await?;

        // SQS answers with an empty body, not a confirmation of what was deleted.
        Ok(json!({}))
    }

    /// `GetQueueUrl` — a name-to-URL lookup that also proves the queue exists.
    async fn get_queue_url(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let name = queue_name(input, "QueueName")?;

        // Deliberately a lookup rather than string formatting: a URL for a queue that
        // does not exist would fail confusingly on the client's next request instead
        // of here, where the client asked.
        let queue = self.engine.get_queue(&name).await?;

        Ok(json!({ "QueueUrl": self.queue_urls.for_queue(&queue.name) }))
    }

    /// `ListQueues`.
    ///
    /// `MaxResults` and `NextToken` are accepted and ignored while paging is unbuilt —
    /// every queue comes back in one response. That is a difference from real SQS worth
    /// knowing about, and it is why paging is on the list rather than forgotten.
    async fn list_queues(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let prefix = optional_string(input, "QueueNamePrefix")?;

        let queues = self.engine.list_queues(prefix.as_deref()).await?;

        // SQS omits the field entirely when there are no queues, and `aws sqs
        // list-queues` prints nothing at all in that case.
        if queues.is_empty() {
            return Ok(json!({}));
        }

        let urls: Vec<String> = queues
            .iter()
            .map(|queue| self.queue_urls.for_queue(&queue.name))
            .collect();

        Ok(json!({ "QueueUrls": urls }))
    }

    /// `SendMessage`.
    ///
    /// The MD5 in the response is not decoration: SDKs verify it, and a wrong one makes
    /// a client reject a message that was in fact stored.
    async fn send_message(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let queue = self.queue_from_url(input)?;
        let body = required_string(input, "MessageBody")?.to_owned();
        let delay = optional_duration(input, "DelaySeconds", attributes::DELAY_SECONDS_MAX)?;

        reject_unsupported(
            input,
            &[
                "MessageAttributes",
                "MessageSystemAttributes",
                "MessageDeduplicationId",
                "MessageGroupId",
            ],
        )?;

        // Priority is NexQ's own idea and SQS has no way to express it, so anything
        // arriving through this facade takes the default. The REST API is where a
        // client chooses.
        let message = self
            .engine
            .enqueue(&queue, body, Priority::DEFAULT, delay)
            .await?;

        Ok(json!({
            "MessageId": message.id.as_str(),
            "MD5OfMessageBody": checksum::md5_of_body(&message.body),
        }))
    }

    /// `ReceiveMessage`.
    ///
    /// Returns whatever is claimable right now, up to `MaxNumberOfMessages`.
    /// `WaitTimeSeconds` is validated and then ignored: holding the request open until
    /// a message arrives is long polling, which is not built yet, so a client asking to
    /// wait gets an immediate empty answer and will poll again.
    async fn receive_message(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let queue = self.queue_from_url(input)?;
        let wanted =
            optional_count(input, "MaxNumberOfMessages", MAX_MESSAGES_PER_RECEIVE)?.unwrap_or(1);
        let visibility_timeout = optional_duration(
            input,
            "VisibilityTimeout",
            attributes::VISIBILITY_TIMEOUT_MAX,
        )?;
        let _ = optional_duration(input, "WaitTimeSeconds", attributes::RECEIVE_WAIT_TIME_MAX)?;

        let mut claimed = Vec::new();
        for _ in 0..wanted {
            match self.engine.claim_next(&queue, visibility_timeout).await? {
                Some(message) => claimed.push(message),
                // Nothing more available; a short answer is normal for SQS.
                None => break,
            }
        }

        // SQS omits `Messages` entirely rather than sending an empty list, and
        // `aws sqs receive-message` prints nothing at all in that case.
        if claimed.is_empty() {
            return Ok(json!({}));
        }

        let messages: Vec<Value> = claimed
            .iter()
            .map(|claimed| {
                json!({
                    "MessageId": claimed.message.id.as_str(),
                    "ReceiptHandle": claimed.receipt.as_str(),
                    // Note the name: SQS calls this `MD5OfBody` on receive and
                    // `MD5OfMessageBody` on send.
                    "MD5OfBody": checksum::md5_of_body(&claimed.message.body),
                    "Body": claimed.message.body,
                })
            })
            .collect();

        Ok(json!({ "Messages": messages }))
    }

    /// `DeleteMessage` — the acknowledgement that a message was handled.
    async fn delete_message(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let queue = self.queue_from_url(input)?;
        let receipt = ReceiptHandle::from_backend(required_string(input, "ReceiptHandle")?);

        self.engine.ack(&queue, &receipt).await?;

        Ok(json!({}))
    }

    /// The queue a request is about, from the `QueueUrl` it carries.
    fn queue_from_url(&self, input: &Map<String, Value>) -> Result<QueueName, ApiError> {
        let url = required_string(input, "QueueUrl")?;

        self.queue_urls.queue_name(url)
    }
}

/// Most messages SQS will hand back from one `ReceiveMessage`.
const MAX_MESSAGES_PER_RECEIVE: u64 = 10;

/// Refuse inputs that would otherwise be silently dropped.
///
/// Ignoring `MessageAttributes` would lose a client's data without saying so, and
/// ignoring `MessageGroupId` would imply FIFO ordering that does not exist. Both are
/// worse than an error naming what is missing.
fn reject_unsupported(input: &Map<String, Value>, unsupported: &[&str]) -> Result<(), ApiError> {
    for field in unsupported {
        if input.get(*field).is_some_and(|value| !value.is_null()) {
            return Err(ApiError::invalid_parameter_value(format!(
                "{field} is not supported yet."
            )));
        }
    }

    Ok(())
}

/// An optional whole number of seconds, bounded as SQS bounds it.
///
/// Accepts a number or a string: these are numbers in the JSON protocol, unlike queue
/// attributes, but a hand-written client may send either.
fn optional_duration(
    input: &Map<String, Value>,
    field: &str,
    max_seconds: u64,
) -> Result<Option<Duration>, ApiError> {
    let Some(seconds) = optional_u64(input, field)? else {
        return Ok(None);
    };

    if seconds > max_seconds {
        return Err(ApiError::invalid_parameter_value(format!(
            "{field} must be between 0 and {max_seconds}, got {seconds}."
        )));
    }

    Ok(Some(Duration::from_secs(seconds)))
}

/// An optional count, between 1 and `max`.
fn optional_count(
    input: &Map<String, Value>,
    field: &str,
    max: u64,
) -> Result<Option<u64>, ApiError> {
    let Some(count) = optional_u64(input, field)? else {
        return Ok(None);
    };

    if count == 0 || count > max {
        return Err(ApiError::invalid_parameter_value(format!(
            "{field} must be between 1 and {max}, got {count}."
        )));
    }

    Ok(Some(count))
}

fn optional_u64(input: &Map<String, Value>, field: &str) -> Result<Option<u64>, ApiError> {
    let invalid =
        || ApiError::invalid_parameter_value(format!("{field} must be a whole, positive number."));

    match input.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => number.as_u64().map(Some).ok_or_else(invalid),
        Some(Value::String(text)) => text.trim().parse().map(Some).map_err(|_| invalid()),
        Some(_) => Err(invalid()),
    }
}

/// A required string input.
fn required_string<'input>(
    input: &'input Map<String, Value>,
    field: &str,
) -> Result<&'input str, ApiError> {
    match input.get(field) {
        Some(Value::String(value)) if !value.is_empty() => Ok(value),
        Some(Value::String(_)) | None => Err(ApiError::missing_parameter(field)),
        Some(other) => Err(ApiError::invalid_parameter_value(format!(
            "{field} must be a string, got {other}."
        ))),
    }
}

/// An optional string input. Present-but-empty is treated as absent, the way an unset
/// command-line flag reaches a server.
fn optional_string(input: &Map<String, Value>, field: &str) -> Result<Option<String>, ApiError> {
    match input.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if value.is_empty() => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(other) => Err(ApiError::invalid_parameter_value(format!(
            "{field} must be a string, got {other}."
        ))),
    }
}

/// A required, validated queue name.
fn queue_name(input: &Map<String, Value>, field: &str) -> Result<QueueName, ApiError> {
    let name = required_string(input, field)?;

    QueueName::new(name).map_err(|error| {
        // The client sees why, since it is their input that was wrong.
        ApiError::invalid_parameter_value(format!("{field} is invalid: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use nexq_core::model::QueueAttributes;
    use nexq_core::store::Store;
    use nexq_store_memory::MemoryStore;

    use super::*;
    use crate::test_support::test_queue_urls;

    fn operations() -> Operations {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
        Operations::new(Arc::new(Engine::new(store)), test_queue_urls())
    }

    async fn call(
        operations: &Operations,
        operation: Operation,
        input: Value,
    ) -> Result<Value, ApiError> {
        let Value::Object(input) = input else {
            panic!("test input must be an object");
        };

        operations.dispatch(operation, input).await
    }

    #[tokio::test]
    async fn create_queue_reports_the_url_clients_should_use() {
        let operations = operations();

        let output = call(
            &operations,
            Operation::CreateQueue,
            json!({ "QueueName": "jobs" }),
        )
        .await
        .expect("create");

        assert_eq!(
            output["QueueUrl"],
            "http://localhost:8080/000000000000/jobs"
        );
    }

    #[tokio::test]
    async fn create_queue_applies_attributes() {
        let operations = operations();

        call(
            &operations,
            Operation::CreateQueue,
            json!({
                "QueueName": "jobs",
                "Attributes": { "VisibilityTimeout": "120", "DelaySeconds": "5" },
            }),
        )
        .await
        .expect("create");

        // Same name and attributes again: idempotent, so this must succeed.
        call(
            &operations,
            Operation::CreateQueue,
            json!({
                "QueueName": "jobs",
                "Attributes": { "VisibilityTimeout": "120", "DelaySeconds": "5" },
            }),
        )
        .await
        .expect("same attributes");

        // Different attributes: a conflict, reported the way SQS reports it.
        let error = call(
            &operations,
            Operation::CreateQueue,
            json!({ "QueueName": "jobs", "Attributes": { "VisibilityTimeout": "600" } }),
        )
        .await
        .expect_err("different attributes");
        assert_eq!(error.code(), "QueueNameExists");
    }

    #[tokio::test]
    async fn create_queue_needs_a_valid_name() {
        let operations = operations();

        for (input, expected) in [
            (json!({}), "MissingParameter"),
            (json!({ "QueueName": "" }), "MissingParameter"),
            (json!({ "QueueName": "not valid" }), "InvalidParameterValue"),
            (json!({ "QueueName": "jobs.fifo" }), "InvalidParameterValue"),
            (json!({ "QueueName": 42 }), "InvalidParameterValue"),
        ] {
            let error = call(&operations, Operation::CreateQueue, input.clone())
                .await
                .expect_err(&input.to_string());

            assert_eq!(error.code(), expected, "{input}");
        }
    }

    #[tokio::test]
    async fn get_queue_url_finds_a_queue_that_exists() {
        let operations = operations();
        call(
            &operations,
            Operation::CreateQueue,
            json!({ "QueueName": "jobs" }),
        )
        .await
        .expect("create");

        let output = call(
            &operations,
            Operation::GetQueueUrl,
            json!({ "QueueName": "jobs" }),
        )
        .await
        .expect("get url");

        assert_eq!(
            output["QueueUrl"],
            "http://localhost:8080/000000000000/jobs"
        );
    }

    #[tokio::test]
    async fn get_queue_url_refuses_to_invent_a_url() {
        // A URL for a nonexistent queue would fail on the client's *next* request,
        // somewhere it did not ask a question.
        let error = call(
            &operations(),
            Operation::GetQueueUrl,
            json!({ "QueueName": "nope" }),
        )
        .await
        .expect_err("no such queue");

        assert_eq!(error.code(), "QueueDoesNotExist");
    }

    #[tokio::test]
    async fn delete_queue_takes_the_url_it_handed_out() {
        let operations = operations();
        let created = call(
            &operations,
            Operation::CreateQueue,
            json!({ "QueueName": "jobs" }),
        )
        .await
        .expect("create");
        let url = created["QueueUrl"].clone();

        let output = call(
            &operations,
            Operation::DeleteQueue,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("delete");

        assert_eq!(output, json!({}), "SQS answers with an empty body");
        let error = call(
            &operations,
            Operation::GetQueueUrl,
            json!({ "QueueName": "jobs" }),
        )
        .await
        .expect_err("deleted");
        assert_eq!(error.code(), "QueueDoesNotExist");
    }

    #[tokio::test]
    async fn delete_queue_rejects_a_url_it_cannot_place() {
        let operations = operations();

        for (input, expected) in [
            (json!({}), "MissingParameter"),
            (
                json!({ "QueueUrl": "http://localhost:8080/123456789012/jobs" }),
                "InvalidAddress",
            ),
            (json!({ "QueueUrl": "jobs" }), "InvalidAddress"),
        ] {
            let error = call(&operations, Operation::DeleteQueue, input.clone())
                .await
                .expect_err(&input.to_string());

            assert_eq!(error.code(), expected, "{input}");
        }
    }

    #[tokio::test]
    async fn delete_queue_reports_a_queue_that_is_not_there() {
        let error = call(
            &operations(),
            Operation::DeleteQueue,
            json!({ "QueueUrl": "http://localhost:8080/000000000000/nope" }),
        )
        .await
        .expect_err("no such queue");

        assert_eq!(error.code(), "QueueDoesNotExist");
    }

    #[tokio::test]
    async fn list_queues_omits_the_field_when_there_are_none() {
        let output = call(&operations(), Operation::ListQueues, json!({}))
            .await
            .expect("list");

        assert_eq!(
            output,
            json!({}),
            "an explicit empty array would make the CLI print one"
        );
    }

    #[tokio::test]
    async fn list_queues_returns_urls() {
        let operations = operations();
        for name in ["jobs", "emails"] {
            call(
                &operations,
                Operation::CreateQueue,
                json!({ "QueueName": name }),
            )
            .await
            .expect("create");
        }

        let output = call(&operations, Operation::ListQueues, json!({}))
            .await
            .expect("list");

        let mut urls: Vec<&str> = output["QueueUrls"]
            .as_array()
            .expect("array")
            .iter()
            .map(|url| url.as_str().expect("string"))
            .collect();
        urls.sort_unstable();

        assert_eq!(
            urls,
            [
                "http://localhost:8080/000000000000/emails",
                "http://localhost:8080/000000000000/jobs",
            ]
        );
    }

    #[tokio::test]
    async fn list_queues_honours_a_prefix() {
        let operations = operations();
        for name in ["jobs", "jobs_dlq", "emails"] {
            call(
                &operations,
                Operation::CreateQueue,
                json!({ "QueueName": name }),
            )
            .await
            .expect("create");
        }

        let output = call(
            &operations,
            Operation::ListQueues,
            json!({ "QueueNamePrefix": "jobs" }),
        )
        .await
        .expect("list");

        assert_eq!(output["QueueUrls"].as_array().expect("array").len(), 2);

        // An empty prefix is what an unset `--queue-name-prefix` looks like on the
        // wire, and must not filter everything out.
        let output = call(
            &operations,
            Operation::ListQueues,
            json!({ "QueueNamePrefix": "" }),
        )
        .await
        .expect("list");
        assert_eq!(output["QueueUrls"].as_array().expect("array").len(), 3);
    }

    #[tokio::test]
    async fn a_queue_created_here_keeps_its_attributes() {
        let operations = operations();
        call(
            &operations,
            Operation::CreateQueue,
            json!({ "QueueName": "jobs", "Attributes": { "VisibilityTimeout": "120" } }),
        )
        .await
        .expect("create");

        let queue = operations
            .engine
            .get_queue(&QueueName::new("jobs").expect("valid"))
            .await
            .expect("get");

        assert_eq!(
            queue.attributes,
            QueueAttributes {
                visibility_timeout: std::time::Duration::from_secs(120),
                ..QueueAttributes::default()
            }
        );
    }

    /// A queue named `jobs`, and its URL.
    async fn queue(operations: &Operations) -> Value {
        call(
            operations,
            Operation::CreateQueue,
            json!({ "QueueName": "jobs" }),
        )
        .await
        .expect("create")["QueueUrl"]
            .clone()
    }

    #[tokio::test]
    async fn a_message_can_be_sent_received_and_deleted() {
        let operations = operations();
        let url = queue(&operations).await;

        let sent = call(
            &operations,
            Operation::SendMessage,
            json!({ "QueueUrl": url, "MessageBody": "hello" }),
        )
        .await
        .expect("send");

        assert!(!sent["MessageId"].as_str().expect("id").is_empty());
        assert_eq!(
            sent["MD5OfMessageBody"], "5d41402abc4b2a76b9719d911017c592",
            "clients verify this, so it must be the real MD5 of the body"
        );

        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("receive");

        let messages = received["Messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(message["Body"], "hello");
        assert_eq!(message["MessageId"], sent["MessageId"]);
        // The field is named differently on receive than on send.
        assert_eq!(message["MD5OfBody"], sent["MD5OfMessageBody"]);

        let deleted = call(
            &operations,
            Operation::DeleteMessage,
            json!({
                "QueueUrl": url,
                "ReceiptHandle": message["ReceiptHandle"].clone(),
            }),
        )
        .await
        .expect("delete");
        assert_eq!(deleted, json!({}), "SQS answers with an empty body");

        // Gone for good.
        let empty = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("receive");
        assert_eq!(empty, json!({}));
    }

    #[tokio::test]
    async fn receiving_from_an_empty_queue_omits_the_messages_field() {
        let operations = operations();
        let url = queue(&operations).await;

        let output = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("receive");

        assert_eq!(
            output,
            json!({}),
            "an empty list would make the CLI print one"
        );
    }

    #[tokio::test]
    async fn one_message_comes_back_unless_more_are_asked_for() {
        let operations = operations();
        let url = queue(&operations).await;
        for body in ["a", "b", "c"] {
            call(
                &operations,
                Operation::SendMessage,
                json!({ "QueueUrl": url, "MessageBody": body }),
            )
            .await
            .expect("send");
        }

        let one = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("receive");
        assert_eq!(one["Messages"].as_array().expect("messages").len(), 1);

        let rest = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "MaxNumberOfMessages": 10 }),
        )
        .await
        .expect("receive");
        assert_eq!(
            rest["Messages"].as_array().expect("messages").len(),
            2,
            "a short answer is normal when fewer are available"
        );
    }

    #[tokio::test]
    async fn a_received_message_is_not_handed_out_again_until_its_claim_lapses() {
        let operations = operations();
        let url = queue(&operations).await;
        call(
            &operations,
            Operation::SendMessage,
            json!({ "QueueUrl": url, "MessageBody": "hello" }),
        )
        .await
        .expect("send");

        call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("receive");

        let again = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("receive");
        assert_eq!(again, json!({}), "someone else already holds it");
    }

    #[tokio::test]
    async fn deleting_with_a_spent_handle_is_refused() {
        let operations = operations();
        let url = queue(&operations).await;
        call(
            &operations,
            Operation::SendMessage,
            json!({ "QueueUrl": url, "MessageBody": "hello" }),
        )
        .await
        .expect("send");
        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("receive");
        let handle = received["Messages"][0]["ReceiptHandle"].clone();

        call(
            &operations,
            Operation::DeleteMessage,
            json!({ "QueueUrl": url, "ReceiptHandle": handle.clone() }),
        )
        .await
        .expect("delete");

        let error = call(
            &operations,
            Operation::DeleteMessage,
            json!({ "QueueUrl": url, "ReceiptHandle": handle }),
        )
        .await
        .expect_err("already deleted");

        assert_eq!(error.code(), "ReceiptHandleIsInvalid");
    }

    #[tokio::test]
    async fn sending_needs_a_queue_and_a_body() {
        let operations = operations();
        let url = queue(&operations).await;

        for (input, expected) in [
            (json!({ "MessageBody": "hello" }), "MissingParameter"),
            (json!({ "QueueUrl": url.clone() }), "MissingParameter"),
            (
                json!({ "QueueUrl": url.clone(), "MessageBody": "" }),
                "MissingParameter",
            ),
            (
                json!({ "QueueUrl": "http://localhost:8080/000000000000/nope",
                        "MessageBody": "hello" }),
                "QueueDoesNotExist",
            ),
        ] {
            let error = call(&operations, Operation::SendMessage, input.clone())
                .await
                .expect_err(&input.to_string());

            assert_eq!(error.code(), expected, "{input}");
        }
    }

    #[tokio::test]
    async fn inputs_that_would_be_silently_dropped_are_refused() {
        let operations = operations();
        let url = queue(&operations).await;

        // Accepting these while ignoring them would lose a client's data, or imply
        // FIFO ordering that does not exist.
        for field in [
            "MessageAttributes",
            "MessageSystemAttributes",
            "MessageDeduplicationId",
            "MessageGroupId",
        ] {
            let error = call(
                &operations,
                Operation::SendMessage,
                json!({ "QueueUrl": url, "MessageBody": "hello", field: { "any": "thing" } }),
            )
            .await
            .expect_err(field);

            assert_eq!(error.code(), "InvalidParameterValue", "{field}");
            assert!(error.message().contains(field), "{}", error.message());
        }
    }

    #[tokio::test]
    async fn numeric_inputs_are_bounded_the_way_sqs_bounds_them() {
        let operations = operations();
        let url = queue(&operations).await;

        let too_big = [
            (Operation::SendMessage, "DelaySeconds", 901),
            (Operation::ReceiveMessage, "VisibilityTimeout", 43_201),
            (Operation::ReceiveMessage, "WaitTimeSeconds", 21),
            (Operation::ReceiveMessage, "MaxNumberOfMessages", 11),
        ];

        for (operation, field, value) in too_big {
            let error = call(
                &operations,
                operation,
                json!({ "QueueUrl": url, "MessageBody": "hello", field: value }),
            )
            .await
            .expect_err(field);

            assert_eq!(error.code(), "InvalidParameterValue", "{field}");
        }

        // Zero messages is not a request anyone can satisfy.
        let error = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "MaxNumberOfMessages": 0 }),
        )
        .await
        .expect_err("zero");
        assert_eq!(error.code(), "InvalidParameterValue");
    }

    #[tokio::test]
    async fn a_per_message_delay_holds_the_message_back() {
        let operations = operations();
        let url = queue(&operations).await;

        call(
            &operations,
            Operation::SendMessage,
            json!({ "QueueUrl": url, "MessageBody": "later", "DelaySeconds": 900 }),
        )
        .await
        .expect("send");

        let output = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("receive");

        assert_eq!(output, json!({}), "still delayed");
    }

    #[tokio::test]
    async fn an_oversized_body_is_refused() {
        let operations = operations();
        let url = queue(&operations).await;

        let error = call(
            &operations,
            Operation::SendMessage,
            json!({ "QueueUrl": url, "MessageBody": "x".repeat(256 * 1024 + 1) }),
        )
        .await
        .expect_err("too large");

        assert_eq!(error.code(), "InvalidParameterValue");
        assert!(error.message().contains("262144"), "{}", error.message());
    }

    #[tokio::test]
    async fn operations_without_handlers_are_still_not_implemented() {
        let error = call(&operations(), Operation::PurgeQueue, json!({}))
            .await
            .expect_err("no handler yet");

        assert_eq!(error.code(), "NotImplemented");
    }
}
