//! Where each SQS operation is handled.
//!
//! Routing ends here: the request has been authenticated, recognised as a specific
//! [`Operation`], and its input decoded. Each handler translates SQS's wire shape into
//! an engine call and back — no queueing logic lives in this crate, because a facade
//! that decided things for itself would answer differently from REST.

use std::sync::Arc;
use std::time::Duration;

use nexq_core::engine::{Engine, MAX_QUEUES_PER_PAGE, QueueQuery, ReceiveRequest};
use nexq_core::model::{Priority, QueueName, ReceiptHandle};
use serde_json::{Map, Value, json};

use crate::error::ApiError;
use crate::message_attributes::Selection;
use crate::protocol::Operation;
use crate::queue_url::QueueUrls;
use crate::system_attributes::Requested;
use crate::{attributes, checksum, message_attributes};

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

    /// `ListQueues`, paged.
    ///
    /// A `NextToken` comes back whenever more queues remain — including when the client
    /// did not ask for a `MaxResults`, because the alternative is truncating at the cap
    /// and saying nothing about it. SQS only returns a token when `MaxResults` was
    /// given; losing queues silently seemed the worse difference to have.
    async fn list_queues(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let query = QueueQuery {
            prefix: optional_string(input, "QueueNamePrefix")?,
            limit: optional_count(input, "MaxResults", MAX_QUEUES_PER_PAGE as u64)?
                .map(|limit| limit as usize),
            after: match optional_string(input, "NextToken")? {
                Some(token) => Some(decode_next_token(&token)?),
                None => None,
            },
        };

        let page = self.engine.list_queues(&query).await?;

        // SQS omits the field entirely when there are no queues, and `aws sqs
        // list-queues` prints nothing at all in that case.
        if page.queues.is_empty() {
            return Ok(json!({}));
        }

        let urls: Vec<String> = page
            .queues
            .iter()
            .map(|queue| self.queue_urls.for_queue(&queue.name))
            .collect();

        let mut output = json!({ "QueueUrls": urls });
        if let Some(next) = page.next {
            output["NextToken"] = json!(encode_next_token(&next));
        }

        Ok(output)
    }

    /// `SendMessage`.
    ///
    /// The MD5s in the response are not decoration: SDKs verify them, and a wrong one
    /// makes a client reject a message that was in fact stored.
    async fn send_message(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let queue = self.queue_from_url(input)?;
        let body = required_string(input, "MessageBody")?.to_owned();
        let delay = optional_duration(input, "DelaySeconds", attributes::DELAY_SECONDS_MAX)?;
        let message_attributes = message_attributes::from_input(input.get("MessageAttributes"))?;

        reject_unsupported(
            input,
            &[
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
            .enqueue(&queue, body, Priority::DEFAULT, message_attributes, delay)
            .await?;

        let mut output = json!({
            "MessageId": message.id.as_str(),
            "MD5OfMessageBody": checksum::md5_of_body(&message.body),
        });

        // Omitted rather than sent as the digest of nothing, which is what SQS does and
        // what stops a client verifying a checksum over attributes it never sent.
        if !message.attributes.is_empty() {
            output["MD5OfMessageAttributes"] =
                json!(checksum::md5_of_attributes(&message.attributes));
        }

        Ok(output)
    }

    /// `ReceiveMessage`.
    ///
    /// Returns up to `MaxNumberOfMessages`, waiting up to `WaitTimeSeconds` for the
    /// first one — long polling, which the engine owns. Omitting `WaitTimeSeconds`
    /// falls back to the queue's `ReceiveMessageWaitTimeSeconds`, so a queue can make
    /// long polling the default for its consumers.
    ///
    /// System attributes come back only when asked for — see
    /// [`crate::system_attributes`] for which ones exist and how they are selected —
    /// and message attributes likewise, via [`crate::message_attributes`].
    async fn receive_message(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let queue = self.queue_from_url(input)?;
        let wanted =
            optional_count(input, "MaxNumberOfMessages", MAX_MESSAGES_PER_RECEIVE)?.unwrap_or(1);
        let visibility_timeout = optional_duration(
            input,
            "VisibilityTimeout",
            attributes::VISIBILITY_TIMEOUT_MAX,
        )?;
        let wait = optional_duration(input, "WaitTimeSeconds", attributes::RECEIVE_WAIT_TIME_MAX)?;

        // Read before anything is claimed, so a request naming an attribute NexQ cannot
        // report fails without having made a message invisible for nothing — and, with
        // a wait in play, without having held the request open first.
        let requested = Requested::from_input(input)?;
        let selection = Selection::from_input(input)?;

        let claimed = self
            .engine
            .receive(
                &queue,
                &ReceiveRequest {
                    max_messages: wanted as usize,
                    visibility_timeout,
                    wait,
                },
            )
            .await?;

        // SQS omits `Messages` entirely rather than sending an empty list, and
        // `aws sqs receive-message` prints nothing at all in that case.
        if claimed.is_empty() {
            return Ok(json!({}));
        }

        let messages: Vec<Value> = claimed
            .iter()
            .map(|claimed| {
                let mut message = json!({
                    "MessageId": claimed.message.id.as_str(),
                    "ReceiptHandle": claimed.receipt.as_str(),
                    // Note the name: SQS calls this `MD5OfBody` on receive and
                    // `MD5OfMessageBody` on send.
                    "MD5OfBody": checksum::md5_of_body(&claimed.message.body),
                    "Body": claimed.message.body,
                });

                // Omitted rather than sent empty when nothing was asked for, which is
                // what SQS does and what keeps the common response as small as it was.
                let system_attributes = requested.render(&claimed.message);
                if !system_attributes.is_empty() {
                    message["Attributes"] = Value::Object(system_attributes);
                }

                // The checksum covers what is *returned*, not everything the message
                // holds — a client asking for one of three attributes verifies the
                // digest of that one. AWS's own published digests show this, and the
                // checksum tests pin it.
                let selected = selection.select(&claimed.message.attributes);
                if !selected.is_empty() {
                    message["MD5OfMessageAttributes"] =
                        json!(checksum::md5_of_attributes(&selected));
                    message["MessageAttributes"] =
                        Value::Object(message_attributes::to_output(&selected));
                }

                message
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

/// Render a paging cursor as a token to hand to a client.
///
/// Hex-encoded so it reads as opaque. A client that decoded it would find a queue name,
/// which is nothing it could not already list — the encoding is there to stop anyone
/// building on the shape, since cursors will change when paging pushes down into the
/// storage backends.
fn encode_next_token(cursor: &QueueName) -> String {
    hex::encode(cursor.as_str())
}

/// Read a cursor back from a token a client returned.
fn decode_next_token(token: &str) -> Result<QueueName, ApiError> {
    let invalid = || ApiError::invalid_parameter_value("NextToken is not valid.");

    let bytes = hex::decode(token).map_err(|_| invalid())?;
    let name = String::from_utf8(bytes).map_err(|_| invalid())?;

    QueueName::new(name).map_err(|_| invalid())
}

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
    async fn list_queues_pages_with_a_token() {
        let operations = operations();
        for queue_name in ["a", "b", "c"] {
            call(
                &operations,
                Operation::CreateQueue,
                json!({ "QueueName": queue_name }),
            )
            .await
            .expect("create");
        }

        let first = call(
            &operations,
            Operation::ListQueues,
            json!({ "MaxResults": 2 }),
        )
        .await
        .expect("first page");

        assert_eq!(first["QueueUrls"].as_array().expect("urls").len(), 2);
        let token = first["NextToken"].as_str().expect("a token").to_owned();

        let second = call(
            &operations,
            Operation::ListQueues,
            json!({ "MaxResults": 2, "NextToken": token }),
        )
        .await
        .expect("second page");

        assert_eq!(second["QueueUrls"].as_array().expect("urls").len(), 1);
        assert!(
            second.get("NextToken").is_none(),
            "the last page must not offer to continue: {second}"
        );
    }

    #[tokio::test]
    async fn walking_the_pages_yields_every_queue_once() {
        let operations = operations();
        let expected: Vec<String> = (0..7).map(|index| format!("q{index}")).collect();
        for queue_name in &expected {
            call(
                &operations,
                Operation::CreateQueue,
                json!({ "QueueName": queue_name }),
            )
            .await
            .expect("create");
        }

        let mut seen: Vec<String> = Vec::new();
        let mut token: Option<String> = None;
        loop {
            let input = match &token {
                Some(token) => json!({ "MaxResults": 3, "NextToken": token }),
                None => json!({ "MaxResults": 3 }),
            };
            let page = call(&operations, Operation::ListQueues, input)
                .await
                .expect("page");

            seen.extend(
                page["QueueUrls"]
                    .as_array()
                    .expect("urls")
                    .iter()
                    .map(|url| {
                        url.as_str()
                            .expect("string")
                            .rsplit('/')
                            .next()
                            .expect("name")
                            .to_owned()
                    }),
            );

            match page.get("NextToken").and_then(Value::as_str) {
                Some(next) => token = Some(next.to_owned()),
                None => break,
            }
        }

        assert_eq!(seen, expected, "every queue exactly once, in order");
    }

    #[tokio::test]
    async fn a_token_is_opaque_but_round_trips() {
        let operations = operations();
        for queue_name in ["a", "b"] {
            call(
                &operations,
                Operation::CreateQueue,
                json!({ "QueueName": queue_name }),
            )
            .await
            .expect("create");
        }

        let page = call(
            &operations,
            Operation::ListQueues,
            json!({ "MaxResults": 1 }),
        )
        .await
        .expect("page");
        let token = page["NextToken"].as_str().expect("token");

        assert_ne!(token, "a", "the cursor should not be handed over verbatim");
        assert_eq!(
            decode_next_token(token).expect("decodes"),
            QueueName::new("a").expect("valid")
        );
    }

    #[tokio::test]
    async fn a_token_that_did_not_come_from_here_is_refused() {
        let operations = operations();

        for token in ["not-hex", "", "zz", "6e6f7420612071756575652e"] {
            let error = call(
                &operations,
                Operation::ListQueues,
                json!({ "MaxResults": 1, "NextToken": token }),
            )
            .await;

            match token {
                // An empty token reads as absent, the way an unset flag arrives.
                "" => {
                    error.expect("an empty token is the same as none");
                }
                _ => {
                    let error = error.expect_err(token);
                    assert_eq!(error.code(), "InvalidParameterValue", "{token}");
                }
            }
        }
    }

    #[tokio::test]
    async fn max_results_is_bounded_the_way_sqs_bounds_it() {
        let operations = operations();

        for value in [0, (MAX_QUEUES_PER_PAGE + 1) as u64] {
            let error = call(
                &operations,
                Operation::ListQueues,
                json!({ "MaxResults": value }),
            )
            .await
            .expect_err(&value.to_string());

            assert_eq!(error.code(), "InvalidParameterValue", "{value}");
        }
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
    async fn wait_time_seconds_holds_the_request_open_until_a_message_arrives() {
        // `WaitTimeSeconds` used to be validated and then dropped on the floor. This
        // asserts the wiring rather than the semantics — which the engine's own tests
        // cover — by requiring a message that only exists *after* the receive began: an
        // ignored wait would return empty immediately and fail here.
        let operations = operations();
        let url = queue(&operations).await;

        let (received, sent) = tokio::join!(
            call(
                &operations,
                Operation::ReceiveMessage,
                json!({ "QueueUrl": url, "WaitTimeSeconds": 20 }),
            ),
            async {
                // Long enough that the receive is genuinely waiting rather than still
                // on its first look at the queue.
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                call(
                    &operations,
                    Operation::SendMessage,
                    json!({ "QueueUrl": url, "MessageBody": "arrived late" }),
                )
                .await
            }
        );
        sent.expect("send");

        let received = received.expect("receive");
        assert_eq!(
            received["Messages"][0]["Body"], "arrived late",
            "the wait should have been woken by the send: {received}"
        );
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

    /// The attribute map used by the send/receive round-trip tests.
    fn sent_attributes() -> Value {
        json!({
            "City": { "DataType": "String", "StringValue": "Any City" },
            "Population": { "DataType": "Number", "StringValue": "1250800" },
            "Thumb": { "DataType": "Binary", "BinaryValue": "SGVsbG8sIFdvcmxkIQ==" },
        })
    }

    #[tokio::test]
    async fn message_attributes_survive_a_send_and_receive() {
        let operations = operations();
        let url = queue(&operations).await;

        let sent = call(
            &operations,
            Operation::SendMessage,
            json!({
                "QueueUrl": url,
                "MessageBody": "hello",
                "MessageAttributes": sent_attributes(),
            }),
        )
        .await
        .expect("send");

        let digest = sent["MD5OfMessageAttributes"]
            .as_str()
            .expect("a digest of the attributes");

        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "MessageAttributeNames": ["All"] }),
        )
        .await
        .expect("receive");
        let message = &received["Messages"][0];

        assert_eq!(
            message["MessageAttributes"],
            sent_attributes(),
            "what comes back must be what went in, byte for byte"
        );
        assert_eq!(
            message["MD5OfMessageAttributes"], digest,
            "and it must checksum the same on the way out as on the way in"
        );
    }

    #[tokio::test]
    async fn attributes_are_omitted_entirely_unless_asked_for() {
        let operations = operations();
        let url = queue(&operations).await;
        call(
            &operations,
            Operation::SendMessage,
            json!({
                "QueueUrl": url,
                "MessageBody": "hello",
                "MessageAttributes": sent_attributes(),
            }),
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
        let message = &received["Messages"][0];

        assert!(message.get("MessageAttributes").is_none(), "{message}");
        assert!(
            message.get("MD5OfMessageAttributes").is_none(),
            "a digest with nothing to verify is worse than no digest: {message}"
        );
    }

    #[tokio::test]
    async fn a_subset_is_checksummed_as_a_subset() {
        // The behaviour AWS's own published digests show: asking for some attributes
        // gives the digest of those, not of everything the message holds. Getting this
        // wrong makes an SDK reject a message whose data arrived intact.
        let operations = operations();
        let url = queue(&operations).await;
        let sent = call(
            &operations,
            Operation::SendMessage,
            json!({
                "QueueUrl": url,
                "MessageBody": "hello",
                "MessageAttributes": sent_attributes(),
            }),
        )
        .await
        .expect("send");

        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({
                "QueueUrl": url,
                "VisibilityTimeout": 0,
                "MessageAttributeNames": ["City"],
            }),
        )
        .await
        .expect("receive");
        let message = &received["Messages"][0];

        assert_eq!(
            message["MessageAttributes"],
            json!({ "City": { "DataType": "String", "StringValue": "Any City" } })
        );
        assert_ne!(
            message["MD5OfMessageAttributes"], sent["MD5OfMessageAttributes"],
            "a subset must not carry the digest of the whole"
        );
        assert_eq!(
            message["MD5OfMessageAttributes"],
            json!(checksum::md5_of_attributes(
                &message_attributes::from_input(Some(&message["MessageAttributes"]))
                    .expect("parse")
            )),
            "the digest must cover exactly the attributes alongside it"
        );
    }

    #[tokio::test]
    async fn a_prefix_selects_a_family_of_attributes() {
        let operations = operations();
        let url = queue(&operations).await;
        call(
            &operations,
            Operation::SendMessage,
            json!({
                "QueueUrl": url,
                "MessageBody": "hello",
                "MessageAttributes": {
                    "bar.one": { "DataType": "String", "StringValue": "1" },
                    "bar.two": { "DataType": "String", "StringValue": "2" },
                    "other": { "DataType": "String", "StringValue": "3" },
                },
            }),
        )
        .await
        .expect("send");

        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "MessageAttributeNames": ["bar.*"] }),
        )
        .await
        .expect("receive");

        let attributes = received["Messages"][0]["MessageAttributes"]
            .as_object()
            .expect("attributes");
        assert_eq!(
            attributes.keys().collect::<Vec<_>>(),
            ["bar.one", "bar.two"]
        );
    }

    #[tokio::test]
    async fn an_attribute_a_message_does_not_carry_is_simply_absent() {
        // Unlike a system attribute, the name here is the producer's, so a miss is not
        // the server's to complain about.
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
            json!({ "QueueUrl": url, "MessageAttributeNames": ["Nope"] }),
        )
        .await
        .expect("no error for a name that is not there");

        let message = &received["Messages"][0];
        assert_eq!(message["Body"], "hello");
        assert!(message.get("MessageAttributes").is_none(), "{message}");
    }

    #[tokio::test]
    async fn attributes_that_break_sqs_rules_are_refused_on_send() {
        let operations = operations();
        let url = queue(&operations).await;

        for (label, attributes) in [
            ("no DataType", json!({ "x": { "StringValue": "v" } })),
            (
                "value field does not match the type",
                json!({ "x": { "DataType": "Binary", "StringValue": "v" } }),
            ),
            (
                "a Number that is not a number",
                json!({ "x": { "DataType": "Number", "StringValue": "soon" } }),
            ),
            (
                "an AWS-reserved name",
                json!({ "AWS.Thing": { "DataType": "String", "StringValue": "v" } }),
            ),
            (
                "an empty value",
                json!({ "x": { "DataType": "String", "StringValue": "" } }),
            ),
        ] {
            let error = call(
                &operations,
                Operation::SendMessage,
                json!({ "QueueUrl": url, "MessageBody": "hello",
                        "MessageAttributes": attributes }),
            )
            .await
            .expect_err(label);

            assert_eq!(error.code(), "InvalidParameterValue", "{label}");
        }

        // And nothing was stored on the way to failing.
        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("receive");
        assert_eq!(received, json!({}), "no message should have been accepted");
    }

    #[tokio::test]
    async fn attributes_count_towards_the_size_limit() {
        // A body just under the limit plus attributes that push it over. Accounting for
        // the body alone would let a producer smuggle unbounded metadata past the cap.
        let operations = operations();
        let url = queue(&operations).await;

        let error = call(
            &operations,
            Operation::SendMessage,
            json!({
                "QueueUrl": url,
                "MessageBody": "x".repeat(256 * 1024 - 4),
                "MessageAttributes": {
                    "City": { "DataType": "String", "StringValue": "Any City" },
                },
            }),
        )
        .await
        .expect_err("over the limit once the attributes count");

        assert_eq!(error.code(), "InvalidParameterValue");
        assert!(error.message().contains("262144"), "{}", error.message());
    }

    #[tokio::test]
    async fn no_attributes_come_back_unless_they_are_asked_for() {
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

        assert!(
            received["Messages"][0].get("Attributes").is_none(),
            "an empty map would make the CLI print one: {received}"
        );
    }

    #[tokio::test]
    async fn system_attributes_come_back_when_asked_for() {
        let operations = operations();
        let url = queue(&operations).await;
        let before = nexq_core::model::epoch_millis(std::time::SystemTime::now());
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
            json!({ "QueueUrl": url, "MessageSystemAttributeNames": ["All"] }),
        )
        .await
        .expect("receive");

        let attributes = received["Messages"][0]["Attributes"]
            .as_object()
            .expect("attributes");
        assert_eq!(
            attributes["ApproximateReceiveCount"], "1",
            "the delivery in progress counts, so a first receive is 1"
        );

        // A real timestamp rather than a placeholder: sent between the two readings.
        let sent: u64 = attributes["SentTimestamp"]
            .as_str()
            .expect("string")
            .parse()
            .expect("epoch millis");
        let after = nexq_core::model::epoch_millis(std::time::SystemTime::now());
        assert!(
            (before..=after).contains(&sent),
            "{sent} not in {before}..{after}"
        );

        // First receive, so this is the same delivery that is happening now.
        assert!(attributes.contains_key("ApproximateFirstReceiveTimestamp"));
    }

    #[tokio::test]
    async fn the_receive_count_climbs_with_each_redelivery() {
        // The attribute exists to let a consumer notice a message that keeps coming
        // back, so counting redeliveries is the whole point of it.
        let operations = operations();
        let url = queue(&operations).await;
        call(
            &operations,
            Operation::SendMessage,
            json!({ "QueueUrl": url, "MessageBody": "hello" }),
        )
        .await
        .expect("send");

        let mut first_receive: Option<String> = None;
        for expected in ["1", "2", "3"] {
            let received = call(
                &operations,
                Operation::ReceiveMessage,
                // A zero visibility timeout, so the claim lapses at once and the next
                // receive is a redelivery without waiting for a real timeout.
                json!({
                    "QueueUrl": url,
                    "VisibilityTimeout": 0,
                    "AttributeNames": ["All"],
                }),
            )
            .await
            .expect("receive");

            let attributes = received["Messages"][0]["Attributes"]
                .as_object()
                .expect("attributes");
            assert_eq!(attributes["ApproximateReceiveCount"], expected);

            // First delivery, not most recent: it must not move.
            let first = attributes["ApproximateFirstReceiveTimestamp"]
                .as_str()
                .expect("string")
                .to_owned();
            match &first_receive {
                Some(original) => assert_eq!(&first, original, "first delivery, not latest"),
                None => first_receive = Some(first),
            }
        }
    }

    #[tokio::test]
    async fn an_attribute_that_cannot_be_answered_does_not_consume_a_message() {
        // The request is refused before anything is claimed, so a client that asks for
        // an unsupported attribute has not quietly made its message invisible.
        let operations = operations();
        let url = queue(&operations).await;
        call(
            &operations,
            Operation::SendMessage,
            json!({ "QueueUrl": url, "MessageBody": "hello" }),
        )
        .await
        .expect("send");

        let error = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "AttributeNames": ["SenderId"] }),
        )
        .await
        .expect_err("not supported");
        assert_eq!(error.code(), "InvalidAttributeName");

        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("receive");
        assert_eq!(
            received["Messages"][0]["Body"], "hello",
            "still there, still visible"
        );
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
