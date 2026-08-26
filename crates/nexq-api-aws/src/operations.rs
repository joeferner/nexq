//! Where each SQS operation is handled.
//!
//! Routing ends here: the request has been authenticated, recognised as a specific
//! [`Operation`], and its input decoded. Each handler translates SQS's wire shape into
//! an engine call and back — no queueing logic lives in this crate, because a facade
//! that decided things for itself would answer differently from REST.

use std::sync::Arc;

use nexq_core::engine::Engine;
use nexq_core::model::QueueName;
use serde_json::{Map, Value, json};

use crate::attributes;
use crate::error::ApiError;
use crate::protocol::Operation;
use crate::queue_url::QueueUrls;

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

    /// The queue a request is about, from the `QueueUrl` it carries.
    fn queue_from_url(&self, input: &Map<String, Value>) -> Result<QueueName, ApiError> {
        let url = required_string(input, "QueueUrl")?;

        self.queue_urls.queue_name(url)
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

    #[tokio::test]
    async fn operations_without_handlers_are_still_not_implemented() {
        let error = call(&operations(), Operation::SendMessage, json!({}))
            .await
            .expect_err("no handler yet");

        assert_eq!(error.code(), "NotImplemented");
    }
}
