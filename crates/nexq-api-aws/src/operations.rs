//! Where each SQS operation is handled.
//!
//! Routing ends here: the request has been authenticated, recognised as a specific
//! [`Operation`], and its input decoded. Each handler translates SQS's wire shape into
//! an engine call and back — no queueing logic lives in this crate, because a facade
//! that decided things for itself would answer differently from REST.

use std::sync::Arc;
use std::time::Duration;

use nexq_core::engine::{Engine, MAX_QUEUES_PER_PAGE, QueueQuery, ReceiveRequest};
use nexq_core::model::{
    MAX_BODY_BYTES, MessageCounts, Queue, QueueName, ReceiptHandle, epoch_millis,
};
use nexq_core::move_task::{MoveTask, MoveTaskId};
use serde_json::{Map, Value, json};
use tracing::info;

use crate::batch;
use crate::error::ApiError;
use crate::message_attributes::Selection;
use crate::protocol::Operation;
use crate::queue_attributes::Requested as QueueAttributesRequested;
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
            Operation::ChangeMessageVisibility => self.change_message_visibility(&input).await,
            Operation::PurgeQueue => self.purge_queue(&input).await,
            Operation::SendMessageBatch => self.send_message_batch(&input).await,
            Operation::DeleteMessageBatch => self.delete_message_batch(&input).await,
            Operation::ChangeMessageVisibilityBatch => {
                self.change_message_visibility_batch(&input).await
            }
            Operation::GetQueueAttributes => self.get_queue_attributes(&input).await,
            Operation::SetQueueAttributes => self.set_queue_attributes(&input).await,
            Operation::ListDeadLetterSourceQueues => {
                self.list_dead_letter_source_queues(&input).await
            }
            Operation::StartMessageMoveTask => self.start_message_move_task(&input).await,
            Operation::CancelMessageMoveTask => self.cancel_message_move_task(&input),
            Operation::ListMessageMoveTasks => self.list_message_move_tasks(&input),
            not_built_yet => Err(ApiError::not_implemented(not_built_yet)),
        }
    }

    /// `CreateQueue` — idempotent when the attributes match, per the engine.
    async fn create_queue(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let name = queue_name(input, "QueueName")?;
        let attributes = attributes::from_input(input.get("Attributes"), &self.queue_urls)?;

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

    /// `PurgeQueue` — throw away every message, keep the queue.
    ///
    /// Logged at `info`, unlike every other operation here, because it is the one that
    /// destroys data a client cannot get back. How much it destroyed is the thing
    /// somebody will want to know afterwards.
    ///
    /// SQS refuses a second purge within sixty seconds with `PurgeQueueInProgress`; NexQ
    /// does not, since that error exists to cover SQS's purge being asynchronous and
    /// this one is finished when it answers.
    async fn purge_queue(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let name = self.queue_from_url(input)?;

        let purged = self.engine.purge_queue(&name).await?;

        info!(queue = %name, purged, "purged queue");

        Ok(json!({}))
    }

    /// `GetQueueAttributes` — a queue's configuration and the facts about it.
    ///
    /// Only what `AttributeNames` asks for, since SQS returns an empty result when the
    /// parameter is absent rather than defaulting to everything.
    async fn get_queue_attributes(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let name = self.queue_from_url(input)?;
        let requested = QueueAttributesRequested::from_input(input)?;

        // Still a lookup when nothing was asked for: a client naming a queue that does
        // not exist should hear about that rather than get a cheerful empty answer.
        let queue = self.engine.get_queue(&name).await?;

        if requested.is_empty() {
            return Ok(json!({}));
        }

        // Counting means aggregating over the queue's messages, so it is only done when
        // one of the approximate-count attributes was actually asked for.
        let counts = if requested.needs_counts() {
            self.engine.message_counts(&name).await?
        } else {
            MessageCounts::default()
        };

        Ok(json!({
            "Attributes": requested.render(&queue, &counts, &self.queue_urls),
        }))
    }

    /// `SetQueueAttributes` — change some of a queue's attributes, leaving the rest.
    ///
    /// A partial update, which is why the engine takes a function rather than a finished
    /// set: naming `VisibilityTimeout` alone must not reset `DelaySeconds` to its
    /// default. Read-only attributes are refused rather than ignored, so a client that
    /// tries to set `QueueArn` hears that it cannot instead of believing it did.
    async fn set_queue_attributes(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let name = self.queue_from_url(input)?;

        // Required here, unlike on `CreateQueue`: a request to change attributes that
        // names none has not said anything.
        let Some(requested) = input.get("Attributes").filter(|value| !value.is_null()) else {
            return Err(ApiError::missing_parameter("Attributes"));
        };

        // The closure's error is this facade's own, so a rejected attribute comes back
        // as an `ApiError` and nothing is written.
        self.engine
            .set_queue_attributes(&name, |existing| {
                attributes::apply(existing, Some(requested), &self.queue_urls)
            })
            .await?;

        Ok(json!({}))
    }

    /// `SendMessage`.
    ///
    /// The MD5s in the response are not decoration: SDKs verify them, and a wrong one
    /// makes a client reject a message that was in fact stored.
    async fn send_message(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let queue = self.queue_from_url(input)?;

        self.send_one(&queue, input).await
    }

    /// Send one message to a queue already identified.
    ///
    /// Split out from [`Operations::send_message`] so a batch entry runs *this*, rather
    /// than a second implementation that could drift from it. A batch entry differs from
    /// a lone request only in where the queue came from and in carrying an `Id`.
    async fn send_one(
        &self,
        queue: &QueueName,
        input: &Map<String, Value>,
    ) -> Result<Value, ApiError> {
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

        // Priority is NexQ's own idea and SQS has no field for it, so it travels as a
        // well-known message attribute — read here and left on the message, since an SDK
        // checksums the attributes it sent. Absent, it is the default, which is what
        // keeps a client that knows nothing about NexQ behaving as it always has.
        let priority = message_attributes::priority(&message_attributes)?;

        let message = self
            .engine
            .enqueue(queue, body, priority, message_attributes, delay)
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

        self.delete_one(&queue, input).await
    }

    /// Delete one claimed message from a queue already identified.
    async fn delete_one(
        &self,
        queue: &QueueName,
        input: &Map<String, Value>,
    ) -> Result<Value, ApiError> {
        let receipt = ReceiptHandle::from_backend(required_string(input, "ReceiptHandle")?);

        self.engine.ack(queue, &receipt).await?;

        Ok(json!({}))
    }

    /// `ChangeMessageVisibility` — how long a consumer's claim has left, reset.
    ///
    /// Counted from now rather than from the receive, so it extends a claim that needs
    /// longer and shortens one that does not. `VisibilityTimeout: 0` is the useful edge:
    /// it puts the message back at once, which is how a consumer that cannot do the work
    /// hands it back instead of holding it until the claim lapses.
    async fn change_message_visibility(
        &self,
        input: &Map<String, Value>,
    ) -> Result<Value, ApiError> {
        let queue = self.queue_from_url(input)?;
        let visibility_timeout = required_duration(
            input,
            "VisibilityTimeout",
            attributes::VISIBILITY_TIMEOUT_MAX,
        )?;

        self.change_one(&queue, input, visibility_timeout).await
    }

    /// Reset one claim's visibility on a queue already identified.
    ///
    /// The timeout is passed in rather than read from `input` because the batch form
    /// makes it optional per entry, falling back to the queue's default, while the single
    /// form requires it. Resolving that is the caller's business; this does the work.
    async fn change_one(
        &self,
        queue: &QueueName,
        input: &Map<String, Value>,
        visibility_timeout: Duration,
    ) -> Result<Value, ApiError> {
        let receipt = ReceiptHandle::from_backend(required_string(input, "ReceiptHandle")?);

        self.engine
            .change_visibility(queue, &receipt, visibility_timeout)
            .await?;

        Ok(json!({}))
    }

    /// `SendMessageBatch` — up to ten messages, each succeeding or failing on its own.
    ///
    /// The batch total is checked as well as each message: SQS caps both an individual
    /// message and the sum of a batch at the same 256 KiB, so ten messages of 200 KiB is
    /// a `BatchRequestTooLong` rather than ten accepted sends.
    async fn send_message_batch(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let queue = self.batch_queue(input).await?.name;
        let entries = batch::entries(input)?;

        // Checked before anything is sent, so an oversized batch stores none of itself
        // rather than however much fitted before the limit was reached.
        let total: usize = entries.iter().map(|entry| entry_size(&entry.input)).sum();
        if total > MAX_BODY_BYTES {
            return Err(ApiError::batch_request_too_long(total, MAX_BODY_BYTES));
        }

        let mut outcomes = Vec::with_capacity(entries.len());
        for entry in entries {
            let outcome = self.send_one(&queue, &entry.input).await;
            outcomes.push((entry.id, outcome));
        }

        Ok(batch::results(outcomes))
    }

    /// `DeleteMessageBatch` — up to ten acknowledgements, each on its own.
    ///
    /// The one clients lean on hardest, since a consumer that received ten messages has
    /// ten handles to spend, and a stale handle among them must not sink the other nine.
    async fn delete_message_batch(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let queue = self.batch_queue(input).await?.name;
        let entries = batch::entries(input)?;

        let mut outcomes = Vec::with_capacity(entries.len());
        for entry in entries {
            let outcome = self.delete_one(&queue, &entry.input).await;
            outcomes.push((entry.id, outcome));
        }

        Ok(batch::results(outcomes))
    }

    /// `ChangeMessageVisibilityBatch` — up to ten claims retimed, each on its own.
    ///
    /// `VisibilityTimeout` is optional per entry here, unlike on the single operation,
    /// because SQS's own model marks it so. An entry that omits it gets the queue's
    /// configured visibility timeout, which is the only reading of "unspecified" that
    /// means anything — and the queue is read once for the batch rather than per entry.
    async fn change_message_visibility_batch(
        &self,
        input: &Map<String, Value>,
    ) -> Result<Value, ApiError> {
        let queue = self.batch_queue(input).await?;
        let entries = batch::entries(input)?;

        let mut outcomes = Vec::with_capacity(entries.len());
        for entry in entries {
            let outcome = match optional_duration(
                &entry.input,
                "VisibilityTimeout",
                attributes::VISIBILITY_TIMEOUT_MAX,
            ) {
                Ok(timeout) => {
                    let timeout = timeout.unwrap_or(queue.attributes.visibility_timeout);
                    self.change_one(&queue.name, &entry.input, timeout).await
                }
                // A bad timeout is this entry's problem, not the batch's.
                Err(error) => Err(error),
            };
            outcomes.push((entry.id, outcome));
        }

        Ok(batch::results(outcomes))
    }

    /// The queue a batch is about, looked up rather than merely parsed.
    ///
    /// The `QueueUrl` belongs to the request, not to any entry, so a queue that does not
    /// exist is a request-level failure — one clear error rather than ten copies of the
    /// same one buried in a `Failed` list. It is also the difference between an SDK
    /// raising and an SDK handing back a result the caller has to remember to inspect,
    /// which for a misspelled queue name is the difference between noticing and not.
    ///
    /// Costs one read per batch, amortised over up to ten operations, and it gives
    /// `ChangeMessageVisibilityBatch` the queue's default timeout for nothing extra.
    /// Advisory rather than a guarantee: a queue deleted while the batch runs still
    /// surfaces per entry, which is the same race a single request has.
    async fn batch_queue(&self, input: &Map<String, Value>) -> Result<Queue, ApiError> {
        let name = self.queue_from_url(input)?;

        Ok(self.engine.get_queue(&name).await?)
    }

    /// `ListDeadLetterSourceQueues` — which queues dead-letter into this one.
    ///
    /// Paged like `ListQueues` and for the same reason, with the same cursor token. SQS
    /// caps a page at 1000 here as well.
    async fn list_dead_letter_source_queues(
        &self,
        input: &Map<String, Value>,
    ) -> Result<Value, ApiError> {
        let name = self.queue_from_url(input)?;
        let limit = optional_count(input, "MaxResults", MAX_QUEUES_PER_PAGE as u64)?
            .map_or(MAX_QUEUES_PER_PAGE, |limit| limit as usize);
        let after = match optional_string(input, "NextToken")? {
            Some(token) => Some(decode_next_token(&token)?),
            None => None,
        };

        // The engine answers in name order, which is what makes resuming after a name a
        // stable cursor rather than an offset that churn can shift.
        let mut sources = self.engine.dead_letter_sources(&name).await?;
        if let Some(after) = after {
            sources.retain(|source| source > &after);
        }

        let has_more = sources.len() > limit;
        sources.truncate(limit);

        // Unlike `ListQueues`, the field is present even when empty: SQS documents
        // `queueUrls` as required on this response, and an SDK will read it.
        let mut output = json!({
            "queueUrls": sources
                .iter()
                .map(|source| self.queue_urls.for_queue(source))
                .collect::<Vec<_>>(),
        });

        if let Some(last) = has_more.then(|| sources.last()).flatten() {
            output["NextToken"] = Value::String(encode_next_token(last));
        }

        Ok(output)
    }

    /// `StartMessageMoveTask` — redrive a dead-letter queue back to a live one.
    ///
    /// `DestinationArn` is optional in SQS, meaning "back to the queues they came from".
    /// NexQ does not record which queue each individual message arrived from — that would
    /// be a field on every message to serve one operation — so it infers the destination
    /// from the redrive policies pointing at the source, which gives the same answer in the
    /// case that matters and refuses rather than guesses when it cannot.
    async fn start_message_move_task(
        &self,
        input: &Map<String, Value>,
    ) -> Result<Value, ApiError> {
        let source = self
            .queue_urls
            .queue_name_from_arn(required_string(input, "SourceArn")?)?;

        let destination = match optional_string(input, "DestinationArn")? {
            Some(arn) => Some(self.queue_urls.queue_name_from_arn(&arn)?),
            None => None,
        };

        // SQS bounds this at 1..=500. Bounded here rather than in the engine because it is
        // a wire-protocol limit: the engine will honour any rate it is given.
        let max_messages_per_second = optional_count(input, "MaxNumberOfMessagesPerSecond", 500)?
            .map(|rate| rate as u32);

        let task = self
            .engine
            .start_redrive(source, destination, max_messages_per_second)
            .await?;

        Ok(json!({ "TaskHandle": task.id.as_str() }))
    }

    /// `CancelMessageMoveTask`.
    ///
    /// Answers with how many messages had moved by the time the cancel was accepted, which
    /// is what SQS reports. The task itself stops at its next message boundary, so the real
    /// figure may be a message or two higher — `ApproximateNumberOfMessagesMoved` is
    /// approximate here for the same reason every other count is.
    fn cancel_message_move_task(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let handle = MoveTaskId::from_client(required_string(input, "TaskHandle")?);

        let task = self.engine.cancel_redrive(&handle)?;

        Ok(json!({ "ApproximateNumberOfMessagesMoved": task.messages_moved }))
    }

    /// `ListMessageMoveTasks` — the redrives of one source queue, newest first.
    fn list_message_move_tasks(&self, input: &Map<String, Value>) -> Result<Value, ApiError> {
        let source = self
            .queue_urls
            .queue_name_from_arn(required_string(input, "SourceArn")?)?;

        // SQS's cap, and its default when a client does not ask.
        let limit = optional_count(input, "MaxResults", 10)?.unwrap_or(1) as usize;

        let results: Vec<Value> = self
            .engine
            .redrive_tasks(Some(&source))
            .into_iter()
            .take(limit)
            .map(|task| self.render_move_task(&task))
            .collect();

        Ok(json!({ "Results": results }))
    }

    /// One redrive task in SQS's shape.
    ///
    /// The optional members are omitted rather than sent as null, which is how SQS renders
    /// them and what stops an SDK reporting a failure reason of `"null"` on a task that
    /// succeeded.
    fn render_move_task(&self, task: &MoveTask) -> Value {
        let mut rendered = json!({
            "TaskHandle": task.id.as_str(),
            "Status": task.status.as_str(),
            "SourceArn": self.queue_urls.arn_for_queue(&task.source),
            "DestinationArn": self.queue_urls.arn_for_queue(&task.destination),
            "ApproximateNumberOfMessagesMoved": task.messages_moved,
            "ApproximateNumberOfMessagesToMove": task.messages_to_move,
            // Seconds since the epoch, as SQS reports a queue's timestamps — not the
            // milliseconds it uses for a message's.
            "StartedTimestamp": epoch_millis(task.started_at) / 1000,
        });

        if let Some(rate) = task.max_messages_per_second {
            rendered["MaxNumberOfMessagesPerSecond"] = json!(rate);
        }
        if let Some(failure) = &task.failure {
            rendered["FailureReason"] = Value::String(failure.clone());
        }

        rendered
    }

    /// The queue a request is about, from the `QueueUrl` it carries.
    fn queue_from_url(&self, input: &Map<String, Value>) -> Result<QueueName, ApiError> {
        let url = required_string(input, "QueueUrl")?;

        self.queue_urls.queue_name(url)
    }
}

/// Most messages SQS will hand back from one `ReceiveMessage`.
const MAX_MESSAGES_PER_RECEIVE: u64 = 10;

/// What one `SendMessageBatch` entry counts towards the batch's size limit.
///
/// A rough measure on purpose: it is the body plus whatever the attributes weigh, read
/// straight off the wire form rather than by parsing them, because an entry that will be
/// refused for a bad attribute should still be counted before that is discovered. The
/// authoritative per-message check is the engine's, on the parsed message.
fn entry_size(input: &Map<String, Value>) -> usize {
    let body = input
        .get("MessageBody")
        .and_then(Value::as_str)
        .map_or(0, str::len);

    let attributes = input
        .get("MessageAttributes")
        .and_then(Value::as_object)
        .map_or(0, |attributes| {
            attributes
                .iter()
                .map(|(name, value)| {
                    name.len()
                        + value.as_object().map_or(0, |fields| {
                            fields
                                .values()
                                .filter_map(Value::as_str)
                                .map(str::len)
                                .sum()
                        })
                })
                .sum()
        });

    body + attributes
}

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

/// A required whole number of seconds, bounded as SQS bounds it.
///
/// Distinct from [`optional_duration`] because absent is a client error here rather than
/// a default: an operation whose whole purpose is to set a timeout has nothing to do
/// without one, and silently picking a value would be worse than saying so.
fn required_duration(
    input: &Map<String, Value>,
    field: &str,
    max_seconds: u64,
) -> Result<Duration, ApiError> {
    optional_duration(input, field, max_seconds)?.ok_or_else(|| ApiError::missing_parameter(field))
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
    async fn send_message_batch_sends_every_entry() {
        let operations = operations();
        let url = queue(&operations).await;

        let output = call(
            &operations,
            Operation::SendMessageBatch,
            json!({ "QueueUrl": url, "Entries": [
                { "Id": "a", "MessageBody": "one" },
                { "Id": "b", "MessageBody": "two" },
                { "Id": "c", "MessageBody": "three", "DelaySeconds": 0 },
            ] }),
        )
        .await
        .expect("send batch");

        let successful = output["Successful"].as_array().expect("successful");
        assert_eq!(successful.len(), 3);
        assert!(output.get("Failed").is_none(), "{output}");

        // Each entry gets back what a lone `SendMessage` would have.
        for entry in successful {
            assert!(!entry["MessageId"].as_str().expect("id").is_empty());
            assert!(!entry["MD5OfMessageBody"].as_str().expect("md5").is_empty());
        }
        assert_eq!(
            successful[0]["MD5OfMessageBody"], "f97c5d29941bfb1b2fdab0874906ab82",
            "the MD5 of \"one\", the same as a single send would report"
        );

        // And all three actually arrived.
        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "MaxNumberOfMessages": 10 }),
        )
        .await
        .expect("receive");
        let mut bodies: Vec<&str> = received["Messages"]
            .as_array()
            .expect("messages")
            .iter()
            .map(|message| message["Body"].as_str().expect("body"))
            .collect();
        bodies.sort_unstable();
        assert_eq!(bodies, ["one", "three", "two"]);
    }

    #[tokio::test]
    async fn a_batch_entry_carries_its_own_message_attributes() {
        // A batch entry must go through the same code a single send does, MD5 included.
        let operations = operations();
        let url = queue(&operations).await;

        let batched = call(
            &operations,
            Operation::SendMessageBatch,
            json!({ "QueueUrl": url, "Entries": [
                { "Id": "a", "MessageBody": "hello", "MessageAttributes": {
                    "City": { "DataType": "String", "StringValue": "Any City" },
                } },
            ] }),
        )
        .await
        .expect("send batch");

        let single = call(
            &operations,
            Operation::SendMessage,
            json!({ "QueueUrl": url, "MessageBody": "hello", "MessageAttributes": {
                "City": { "DataType": "String", "StringValue": "Any City" },
            } }),
        )
        .await
        .expect("send");

        assert_eq!(
            batched["Successful"][0]["MD5OfMessageAttributes"], single["MD5OfMessageAttributes"],
            "a batched send and a lone one must agree"
        );
    }

    #[tokio::test]
    async fn one_bad_entry_does_not_sink_the_others() {
        // The whole point of a batch, and the thing a client would be hurt most by
        // getting wrong: nine good messages must not be lost to one bad one.
        let operations = operations();
        let url = queue(&operations).await;

        let output = call(
            &operations,
            Operation::SendMessageBatch,
            json!({ "QueueUrl": url, "Entries": [
                { "Id": "good", "MessageBody": "fine" },
                { "Id": "no-body" },
                { "Id": "bad-delay", "MessageBody": "x", "DelaySeconds": 901 },
                { "Id": "also-good", "MessageBody": "also fine" },
            ] }),
        )
        .await
        .expect("the batch itself is valid");

        let successful = output["Successful"].as_array().expect("successful");
        let failed = output["Failed"].as_array().expect("failed");
        assert_eq!(successful.len(), 2, "{output}");
        assert_eq!(failed.len(), 2, "{output}");

        assert_eq!(failed[0]["Id"], "no-body");
        assert_eq!(failed[0]["Code"], "MissingParameter");
        assert_eq!(failed[0]["SenderFault"], true);
        assert_eq!(failed[1]["Id"], "bad-delay");
        assert_eq!(failed[1]["Code"], "InvalidParameterValue");

        // The good two are really in the queue, not merely reported.
        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "MaxNumberOfMessages": 10 }),
        )
        .await
        .expect("receive");
        assert_eq!(received["Messages"].as_array().expect("messages").len(), 2);
    }

    #[tokio::test]
    async fn a_batch_bigger_than_one_message_may_be_is_refused_whole() {
        // SQS caps a batch's total at the same 256 KiB as one message. Checked before
        // anything is sent, so an oversized batch stores none of itself rather than
        // however much fitted before the limit was reached.
        let operations = operations();
        let url = queue(&operations).await;

        let entries: Vec<Value> = (0..4)
            .map(
                |index| json!({ "Id": format!("e{index}"), "MessageBody": "x".repeat(100 * 1024) }),
            )
            .collect();

        let error = call(
            &operations,
            Operation::SendMessageBatch,
            json!({ "QueueUrl": url, "Entries": entries }),
        )
        .await
        .expect_err("400 KiB in total");

        assert_eq!(error.code(), "BatchRequestTooLong");

        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("receive");
        assert_eq!(received, json!({}), "nothing should have been stored");
    }

    #[tokio::test]
    async fn delete_message_batch_spends_every_handle() {
        let operations = operations();
        let url = queue(&operations).await;
        call(
            &operations,
            Operation::SendMessageBatch,
            json!({ "QueueUrl": url, "Entries": [
                { "Id": "a", "MessageBody": "one" },
                { "Id": "b", "MessageBody": "two" },
            ] }),
        )
        .await
        .expect("send batch");

        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "MaxNumberOfMessages": 10 }),
        )
        .await
        .expect("receive");
        let entries: Vec<Value> = received["Messages"]
            .as_array()
            .expect("messages")
            .iter()
            .enumerate()
            .map(|(index, message)| {
                json!({ "Id": format!("d{index}"), "ReceiptHandle": message["ReceiptHandle"] })
            })
            .collect();

        let output = call(
            &operations,
            Operation::DeleteMessageBatch,
            json!({ "QueueUrl": url, "Entries": entries }),
        )
        .await
        .expect("delete batch");

        let successful = output["Successful"].as_array().expect("successful");
        assert_eq!(successful.len(), 2);
        assert_eq!(
            successful[0].as_object().expect("object").len(),
            1,
            "a successful delete reports only its id: {}",
            successful[0]
        );
        assert!(output.get("Failed").is_none(), "{output}");
    }

    #[tokio::test]
    async fn a_stale_handle_in_a_delete_batch_fails_only_itself() {
        let operations = operations();
        let url = queue(&operations).await;
        call(
            &operations,
            Operation::SendMessage,
            json!({ "QueueUrl": url, "MessageBody": "hello" }),
        )
        .await
        .expect("send");
        let handle = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("receive")["Messages"][0]["ReceiptHandle"]
            .clone();

        let output = call(
            &operations,
            Operation::DeleteMessageBatch,
            json!({ "QueueUrl": url, "Entries": [
                { "Id": "real", "ReceiptHandle": handle },
                { "Id": "stale", "ReceiptHandle": "00000000-0000-0000-0000-000000000000" },
            ] }),
        )
        .await
        .expect("the batch itself is valid");

        assert_eq!(output["Successful"][0]["Id"], "real");
        assert_eq!(output["Failed"][0]["Id"], "stale");
        assert_eq!(output["Failed"][0]["Code"], "ReceiptHandleIsInvalid");
    }

    #[tokio::test]
    async fn change_message_visibility_batch_retimes_every_claim() {
        let operations = operations();
        let url = queue(&operations).await;
        call(
            &operations,
            Operation::SendMessageBatch,
            json!({ "QueueUrl": url, "Entries": [
                { "Id": "a", "MessageBody": "one" },
                { "Id": "b", "MessageBody": "two" },
            ] }),
        )
        .await
        .expect("send batch");
        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "MaxNumberOfMessages": 10,
                    "VisibilityTimeout": 43_200 }),
        )
        .await
        .expect("receive");
        let entries: Vec<Value> = received["Messages"]
            .as_array()
            .expect("messages")
            .iter()
            .enumerate()
            .map(|(index, message)| {
                json!({
                    "Id": format!("c{index}"),
                    "ReceiptHandle": message["ReceiptHandle"],
                    "VisibilityTimeout": 0,
                })
            })
            .collect();

        let output = call(
            &operations,
            Operation::ChangeMessageVisibilityBatch,
            json!({ "QueueUrl": url, "Entries": entries }),
        )
        .await
        .expect("change batch");

        assert_eq!(output["Successful"].as_array().expect("ok").len(), 2);

        // Both handed back, so both claimable again despite the twelve-hour claims.
        let again = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "MaxNumberOfMessages": 10 }),
        )
        .await
        .expect("receive");
        assert_eq!(again["Messages"].as_array().expect("messages").len(), 2);
    }

    #[tokio::test]
    async fn a_visibility_batch_entry_may_omit_its_timeout() {
        // SQS's own model marks `VisibilityTimeout` optional on a batch entry while
        // requiring it on the single operation. An entry that omits it gets the queue's
        // configured visibility timeout.
        let operations = operations();
        let url = call(
            &operations,
            Operation::CreateQueue,
            json!({ "QueueName": "jobs", "Attributes": { "VisibilityTimeout": "43200" } }),
        )
        .await
        .expect("create")["QueueUrl"]
            .clone();
        call(
            &operations,
            Operation::SendMessage,
            json!({ "QueueUrl": url, "MessageBody": "hello" }),
        )
        .await
        .expect("send");
        let handle = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "VisibilityTimeout": 0 }),
        )
        .await
        .expect("receive")["Messages"][0]["ReceiptHandle"]
            .clone();

        call(
            &operations,
            Operation::ChangeMessageVisibilityBatch,
            json!({ "QueueUrl": url, "Entries": [{ "Id": "a", "ReceiptHandle": handle }] }),
        )
        .await
        .expect("change batch")["Successful"][0]["Id"]
            .as_str()
            .expect("succeeded");

        // The queue's twelve hours now apply, so it is no longer claimable.
        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("receive");
        assert_eq!(
            received,
            json!({}),
            "the queue's own visibility timeout should have been applied"
        );
    }

    #[tokio::test]
    async fn a_bad_timeout_in_a_visibility_batch_fails_only_that_entry() {
        let operations = operations();
        let url = queue(&operations).await;
        call(
            &operations,
            Operation::SendMessage,
            json!({ "QueueUrl": url, "MessageBody": "hello" }),
        )
        .await
        .expect("send");
        let handle = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "VisibilityTimeout": 43_200 }),
        )
        .await
        .expect("receive")["Messages"][0]["ReceiptHandle"]
            .clone();

        let output = call(
            &operations,
            Operation::ChangeMessageVisibilityBatch,
            json!({ "QueueUrl": url, "Entries": [
                { "Id": "ok", "ReceiptHandle": handle, "VisibilityTimeout": 0 },
                { "Id": "too-long", "ReceiptHandle": handle, "VisibilityTimeout": 43_201 },
            ] }),
        )
        .await
        .expect("the batch itself is valid");

        assert_eq!(output["Successful"][0]["Id"], "ok");
        assert_eq!(output["Failed"][0]["Id"], "too-long");
        assert_eq!(output["Failed"][0]["Code"], "InvalidParameterValue");
    }

    #[tokio::test]
    async fn the_batch_operations_share_their_whole_batch_failures() {
        // All three reject a malformed batch the same way, since the rules are about the
        // list rather than what the entries mean.
        let operations = operations();
        let url = queue(&operations).await;

        let too_many: Vec<Value> = (0..11)
            .map(|index| {
                json!({ "Id": format!("e{index}"), "MessageBody": "x",
                                 "ReceiptHandle": "h", "VisibilityTimeout": 0 })
            })
            .collect();

        for operation in [
            Operation::SendMessageBatch,
            Operation::DeleteMessageBatch,
            Operation::ChangeMessageVisibilityBatch,
        ] {
            for (input, expected) in [
                (json!({ "QueueUrl": url }), "EmptyBatchRequest"),
                (
                    json!({ "QueueUrl": url, "Entries": [] }),
                    "EmptyBatchRequest",
                ),
                (
                    json!({ "QueueUrl": url, "Entries": too_many }),
                    "TooManyEntriesInBatchRequest",
                ),
                (
                    json!({ "QueueUrl": url, "Entries": [
                        { "Id": "a", "MessageBody": "x", "ReceiptHandle": "h" },
                        { "Id": "a", "MessageBody": "y", "ReceiptHandle": "h" },
                    ] }),
                    "BatchEntryIdsNotDistinct",
                ),
                (
                    json!({ "QueueUrl": url, "Entries": [
                        { "Id": "not valid", "MessageBody": "x", "ReceiptHandle": "h" },
                    ] }),
                    "InvalidBatchEntryId",
                ),
            ] {
                let error = call(&operations, operation, input.clone())
                    .await
                    .expect_err(&format!("{operation}: {input}"));

                assert_eq!(error.code(), expected, "{operation}: {input}");
            }
        }
    }

    #[tokio::test]
    async fn a_batch_for_a_queue_that_does_not_exist_fails_whole() {
        // The queue is read from the batch, not the entries, so this is not a per-entry
        // failure — there is no queue for any of them.
        let operations = operations();

        let error = call(
            &operations,
            Operation::SendMessageBatch,
            json!({
                "QueueUrl": "http://localhost:8080/000000000000/nope",
                "Entries": [{ "Id": "a", "MessageBody": "hello" }],
            }),
        )
        .await
        .expect_err("no such queue");

        assert_eq!(error.code(), "QueueDoesNotExist");
    }

    #[tokio::test]
    async fn purge_queue_empties_a_queue_without_removing_it() {
        let operations = operations();
        let url = queue(&operations).await;
        for body in ["one", "two", "three"] {
            call(
                &operations,
                Operation::SendMessage,
                json!({ "QueueUrl": url, "MessageBody": body }),
            )
            .await
            .expect("send");
        }

        let output = call(
            &operations,
            Operation::PurgeQueue,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("purge");
        assert_eq!(output, json!({}), "SQS answers with an empty body");

        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "MaxNumberOfMessages": 10 }),
        )
        .await
        .expect("receive");
        assert_eq!(received, json!({}), "nothing left");

        // The queue itself is still usable, which is what separates purge from delete.
        call(
            &operations,
            Operation::SendMessage,
            json!({ "QueueUrl": url, "MessageBody": "after" }),
        )
        .await
        .expect("the queue should still accept messages");
    }

    #[tokio::test]
    async fn purging_takes_in_flight_messages_with_it() {
        // The case that would otherwise pass unnoticed: a purge that only removed
        // visible messages would leave claimed ones to reappear when their claims lapse.
        let operations = operations();
        let url = queue(&operations).await;
        call(
            &operations,
            Operation::SendMessage,
            json!({ "QueueUrl": url, "MessageBody": "in flight" }),
        )
        .await
        .expect("send");
        let handle = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "VisibilityTimeout": 43_200 }),
        )
        .await
        .expect("receive")["Messages"][0]["ReceiptHandle"]
            .clone();

        call(
            &operations,
            Operation::PurgeQueue,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("purge");

        // The consumer still working on it now holds a handle to nothing.
        let error = call(
            &operations,
            Operation::DeleteMessage,
            json!({ "QueueUrl": url, "ReceiptHandle": handle }),
        )
        .await
        .expect_err("the message was purged");
        assert_eq!(error.code(), "ReceiptHandleIsInvalid");

        let counts = call(
            &operations,
            Operation::GetQueueAttributes,
            json!({ "QueueUrl": url, "AttributeNames": ["All"] }),
        )
        .await
        .expect("attributes");
        assert_eq!(
            counts["Attributes"]["ApproximateNumberOfMessagesNotVisible"],
            "0"
        );
    }

    #[tokio::test]
    async fn purging_is_allowed_twice_running() {
        // SQS refuses a second purge within a minute, because its own purge is
        // asynchronous. NexQ's has finished when it answers, so there is nothing to
        // protect and refusing would be a limitation invented for its own sake.
        let operations = operations();
        let url = queue(&operations).await;

        for attempt in 0..3 {
            call(
                &operations,
                Operation::PurgeQueue,
                json!({ "QueueUrl": url }),
            )
            .await
            .unwrap_or_else(|error| panic!("purge {attempt}: {error:?}"));
        }
    }

    #[tokio::test]
    async fn purging_a_queue_that_is_not_there_is_an_error() {
        let operations = operations();

        for (input, expected) in [
            (json!({}), "MissingParameter"),
            (
                json!({ "QueueUrl": "http://localhost:8080/000000000000/nope" }),
                "QueueDoesNotExist",
            ),
            (json!({ "QueueUrl": "nope" }), "InvalidAddress"),
        ] {
            let error = call(&operations, Operation::PurgeQueue, input.clone())
                .await
                .expect_err(&input.to_string());

            assert_eq!(error.code(), expected, "{input}");
        }
    }

    /// `GetQueueAttributes` for a queue URL, asking for the given names.
    async fn attributes_of(operations: &Operations, url: &Value, names: Value) -> Value {
        call(
            operations,
            Operation::GetQueueAttributes,
            json!({ "QueueUrl": url, "AttributeNames": names }),
        )
        .await
        .expect("get queue attributes")
    }

    #[tokio::test]
    async fn get_queue_attributes_reports_what_create_queue_set() {
        let operations = operations();
        let url = call(
            &operations,
            Operation::CreateQueue,
            json!({
                "QueueName": "jobs",
                "Attributes": { "VisibilityTimeout": "120", "DelaySeconds": "5" },
            }),
        )
        .await
        .expect("create")["QueueUrl"]
            .clone();

        let output = attributes_of(&operations, &url, json!(["All"])).await;

        let attributes = output["Attributes"].as_object().expect("attributes");
        assert_eq!(attributes["VisibilityTimeout"], "120");
        assert_eq!(attributes["DelaySeconds"], "5");
        assert_eq!(attributes["ReceiveMessageWaitTimeSeconds"], "0");
        assert_eq!(
            attributes["QueueArn"], "arn:aws:sqs:us-east-1:000000000000:jobs",
            "built from the configured region and account id"
        );
        assert_eq!(attributes["MaximumMessageSize"], "262144");
    }

    #[tokio::test]
    async fn get_queue_attributes_returns_nothing_when_nothing_was_asked_for() {
        // SQS: "if you don't specify values for this parameter, the request returns
        // empty results".
        let operations = operations();
        let url = queue(&operations).await;

        let output = call(
            &operations,
            Operation::GetQueueAttributes,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("get queue attributes");

        assert_eq!(output, json!({}));
    }

    #[tokio::test]
    async fn get_queue_attributes_still_checks_the_queue_exists() {
        // Even with nothing asked for: a client naming a queue that is not there should
        // hear about it rather than get a cheerful empty answer.
        let error = call(
            &operations(),
            Operation::GetQueueAttributes,
            json!({ "QueueUrl": "http://localhost:8080/000000000000/nope" }),
        )
        .await
        .expect_err("no such queue");

        assert_eq!(error.code(), "QueueDoesNotExist");
    }

    #[tokio::test]
    async fn the_message_counts_track_what_the_queue_holds() {
        let operations = operations();
        let url = queue(&operations).await;

        for body in ["one", "two"] {
            call(
                &operations,
                Operation::SendMessage,
                json!({ "QueueUrl": url, "MessageBody": body }),
            )
            .await
            .expect("send");
        }
        call(
            &operations,
            Operation::SendMessage,
            json!({ "QueueUrl": url, "MessageBody": "later", "DelaySeconds": 900 }),
        )
        .await
        .expect("send delayed");
        call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "VisibilityTimeout": 43_200 }),
        )
        .await
        .expect("receive");

        let output = attributes_of(
            &operations,
            &url,
            json!([
                "ApproximateNumberOfMessages",
                "ApproximateNumberOfMessagesNotVisible",
                "ApproximateNumberOfMessagesDelayed",
            ]),
        )
        .await;

        let attributes = &output["Attributes"];
        assert_eq!(attributes["ApproximateNumberOfMessages"], "1");
        assert_eq!(attributes["ApproximateNumberOfMessagesNotVisible"], "1");
        assert_eq!(attributes["ApproximateNumberOfMessagesDelayed"], "1");
    }

    #[tokio::test]
    async fn set_queue_attributes_changes_only_what_it_names() {
        // The whole point of a partial update: naming one attribute must not reset the
        // others to their defaults.
        let operations = operations();
        let url = call(
            &operations,
            Operation::CreateQueue,
            json!({
                "QueueName": "jobs",
                "Attributes": { "VisibilityTimeout": "120", "DelaySeconds": "5" },
            }),
        )
        .await
        .expect("create")["QueueUrl"]
            .clone();

        let output = call(
            &operations,
            Operation::SetQueueAttributes,
            json!({ "QueueUrl": url, "Attributes": { "VisibilityTimeout": "600" } }),
        )
        .await
        .expect("set queue attributes");
        assert_eq!(output, json!({}), "SQS answers with an empty body");

        let attributes = attributes_of(&operations, &url, json!(["All"])).await["Attributes"]
            .as_object()
            .expect("attributes")
            .clone();
        assert_eq!(attributes["VisibilityTimeout"], "600", "changed");
        assert_eq!(attributes["DelaySeconds"], "5", "left alone");
    }

    #[tokio::test]
    async fn set_queue_attributes_takes_effect_on_the_next_receive() {
        // Not just recorded: the queue has to behave differently afterwards.
        let operations = operations();
        let url = queue(&operations).await;
        call(
            &operations,
            Operation::SetQueueAttributes,
            json!({ "QueueUrl": url, "Attributes": { "DelaySeconds": "900" } }),
        )
        .await
        .expect("set queue attributes");

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
        assert_eq!(
            received,
            json!({}),
            "the new queue-wide delay should be holding it back"
        );
    }

    #[tokio::test]
    async fn setting_attributes_moves_the_modified_timestamp_but_not_the_created_one() {
        let operations = operations();
        let url = queue(&operations).await;
        let before = attributes_of(
            &operations,
            &url,
            json!(["CreatedTimestamp", "LastModifiedTimestamp"]),
        )
        .await;
        assert_eq!(
            before["Attributes"]["CreatedTimestamp"], before["Attributes"]["LastModifiedTimestamp"],
            "a queue that has never been changed reports the same time for both"
        );

        call(
            &operations,
            Operation::SetQueueAttributes,
            json!({ "QueueUrl": url, "Attributes": { "VisibilityTimeout": "600" } }),
        )
        .await
        .expect("set queue attributes");

        let after = attributes_of(
            &operations,
            &url,
            json!(["CreatedTimestamp", "LastModifiedTimestamp"]),
        )
        .await;
        assert_eq!(
            after["Attributes"]["CreatedTimestamp"], before["Attributes"]["CreatedTimestamp"],
            "changing attributes is not recreating the queue"
        );
        // Both are whole seconds, so a fast test may not advance the clock past one —
        // what must hold is that it never goes backwards.
        let created: u64 = after["Attributes"]["CreatedTimestamp"]
            .as_str()
            .expect("string")
            .parse()
            .expect("epoch seconds");
        let modified: u64 = after["Attributes"]["LastModifiedTimestamp"]
            .as_str()
            .expect("string")
            .parse()
            .expect("epoch seconds");
        assert!(modified >= created, "{modified} < {created}");
    }

    #[tokio::test]
    async fn set_queue_attributes_refuses_what_it_cannot_change() {
        let operations = operations();
        let url = queue(&operations).await;

        for (input, expected) in [
            // Nothing named at all: a change request that says nothing.
            (json!({ "QueueUrl": url }), "MissingParameter"),
            // Read-only, so accepting it would let a client believe it had renamed a
            // queue or moved its ARN.
            (
                json!({ "QueueUrl": url, "Attributes": { "QueueArn": "arn:aws:sqs:x:y:z" } }),
                "InvalidAttributeName",
            ),
            (
                json!({ "QueueUrl": url,
                        "Attributes": { "ApproximateNumberOfMessages": "0" } }),
                "InvalidAttributeName",
            ),
            (
                json!({ "QueueUrl": url, "Attributes": { "CreatedTimestamp": "0" } }),
                "InvalidAttributeName",
            ),
            // Not supported, and silently dropping it would promise FIFO ordering.
            (
                json!({ "QueueUrl": url, "Attributes": { "FifoQueue": "true" } }),
                "InvalidAttributeName",
            ),
            (
                json!({ "QueueUrl": url, "Attributes": { "VisibilityTimeout": "43201" } }),
                "InvalidAttributeValue",
            ),
        ] {
            let error = call(&operations, Operation::SetQueueAttributes, input.clone())
                .await
                .expect_err(&input.to_string());

            assert_eq!(error.code(), expected, "{input}");
        }
    }

    #[tokio::test]
    async fn a_refused_change_leaves_the_queue_alone() {
        let operations = operations();
        let url = call(
            &operations,
            Operation::CreateQueue,
            json!({ "QueueName": "jobs", "Attributes": { "VisibilityTimeout": "120" } }),
        )
        .await
        .expect("create")["QueueUrl"]
            .clone();

        // One good attribute and one bad one in the same request.
        call(
            &operations,
            Operation::SetQueueAttributes,
            json!({
                "QueueUrl": url,
                "Attributes": { "DelaySeconds": "30", "FifoQueue": "true" },
            }),
        )
        .await
        .expect_err("FifoQueue is not supported");

        let attributes = attributes_of(&operations, &url, json!(["All"])).await["Attributes"]
            .as_object()
            .expect("attributes")
            .clone();
        assert_eq!(
            attributes["DelaySeconds"], "0",
            "a refused request must be all-or-nothing, not partly applied"
        );
        assert_eq!(attributes["VisibilityTimeout"], "120");
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

    /// `VisibilityTimeout=0` with a batch is how someone looks at a queue without keeping
    /// what they find, and it must still return *distinct* messages.
    ///
    /// Here as well as in `nexq-core`, because this is the layer it was found at and the
    /// one a client sees: a zero timeout expires each claim as it is made, and before the
    /// engine learned to skip what it already held this returned three copies of one
    /// message under three handles, only the last of which still worked.
    #[tokio::test]
    async fn a_batch_with_a_zero_visibility_timeout_returns_distinct_messages() {
        let operations = operations();
        let url = queue(&operations).await;

        for body in ["one", "two", "three"] {
            call(
                &operations,
                Operation::SendMessage,
                json!({ "QueueUrl": url, "MessageBody": body }),
            )
            .await
            .expect(body);
        }

        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({
                "QueueUrl": url,
                "MaxNumberOfMessages": 3,
                "VisibilityTimeout": 0,
                "MessageSystemAttributeNames": ["ApproximateReceiveCount"],
            }),
        )
        .await
        .expect("receive");
        let messages = received["Messages"].as_array().expect("three messages");

        assert_eq!(messages.len(), 3);

        let mut ids: Vec<&str> = messages
            .iter()
            .map(|message| message["MessageId"].as_str().expect("an id"))
            .collect();
        ids.sort_unstable();
        let distinct = ids.len();
        ids.dedup();
        assert_eq!(
            ids.len(),
            distinct,
            "three messages, not one message thrice"
        );

        for message in messages {
            assert_eq!(
                message["Attributes"]["ApproximateReceiveCount"], "1",
                "each was delivered once: {message}"
            );
        }
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

    /// One send, one message attribute: `NexQ.Priority` at the given value.
    fn with_priority(url: &Value, body: &str, priority: &str) -> Value {
        json!({
            "QueueUrl": url,
            "MessageBody": body,
            "MessageAttributes": {
                message_attributes::PRIORITY: { "DataType": "Number", "StringValue": priority },
            },
        })
    }

    /// The claim this feature exists for: an *unmodified* SQS client can choose a
    /// message's priority, and the engine serves the urgent one first.
    #[tokio::test]
    async fn a_well_known_attribute_lets_an_sqs_client_set_priority() {
        let operations = operations();
        let url = queue(&operations).await;

        // Sent in the order that makes ordering the only explanation for the result: the
        // low-priority message is enqueued first, so first-in-first-out would return it.
        for (body, priority) in [("later", "-5"), ("urgent", "10"), ("normal", "0")] {
            call(
                &operations,
                Operation::SendMessage,
                with_priority(&url, body, priority),
            )
            .await
            .expect(body);
        }

        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "MaxNumberOfMessages": 3 }),
        )
        .await
        .expect("receive");

        let bodies: Vec<&str> = received["Messages"]
            .as_array()
            .expect("three messages")
            .iter()
            .map(|message| message["Body"].as_str().expect("a body"))
            .collect();

        assert_eq!(bodies, ["urgent", "normal", "later"]);
    }

    /// Kept, not consumed — and this is the test that says why: the digest an SDK computes
    /// over what it sent must still match what the server reports.
    #[tokio::test]
    async fn the_priority_attribute_stays_on_the_message() {
        let operations = operations();
        let url = queue(&operations).await;

        let sent = call(
            &operations,
            Operation::SendMessage,
            with_priority(&url, "hello", "10"),
        )
        .await
        .expect("send");

        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "MessageAttributeNames": ["All"] }),
        )
        .await
        .expect("receive");
        let message = &received["Messages"][0];

        assert_eq!(
            message["MessageAttributes"][message_attributes::PRIORITY],
            json!({ "DataType": "Number", "StringValue": "10" }),
            "a consumer sees exactly what the producer wrote"
        );
        assert_eq!(
            message["MD5OfMessageAttributes"], sent["MD5OfMessageAttributes"],
            "and the digest matches the one the SDK computed over what it sent"
        );
    }

    #[tokio::test]
    async fn priority_is_readable_as_a_system_attribute_but_not_under_all() {
        let operations = operations();
        let url = queue(&operations).await;
        call(
            &operations,
            Operation::SendMessage,
            with_priority(&url, "hello", "10"),
        )
        .await
        .expect("send");

        let receive = |names: Value| {
            call(
                &operations,
                Operation::ReceiveMessage,
                json!({
                    "QueueUrl": url,
                    "VisibilityTimeout": 0,
                    "MessageSystemAttributeNames": names,
                }),
            )
        };

        let named = receive(json!([message_attributes::PRIORITY]))
            .await
            .expect("named");
        assert_eq!(
            named["Messages"][0]["Attributes"][message_attributes::PRIORITY],
            "10"
        );

        // `All` means "whatever SQS would give you", which is why NexQ's own name is not
        // in it — a client gets the extension by asking for it.
        let all = receive(json!(["All"])).await.expect("all");
        let attributes = all["Messages"][0]["Attributes"]
            .as_object()
            .expect("attributes");
        assert!(
            !attributes.contains_key(message_attributes::PRIORITY),
            "{attributes:?}"
        );
    }

    /// The counterpart for a message that carries no priority attribute at all, which is
    /// what everything sent through the REST facade looks like from here.
    #[tokio::test]
    async fn the_priority_of_a_message_sent_elsewhere_is_still_readable() {
        let operations = operations();
        let url = queue(&operations).await;
        let name = QueueName::new("jobs").expect("valid");

        operations
            .engine
            .enqueue(
                &name,
                "from rest".to_owned(),
                nexq_core::model::Priority::new(3),
                nexq_core::model::MessageAttributes::new(),
                None,
            )
            .await
            .expect("enqueue");

        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({
                "QueueUrl": url,
                "MessageSystemAttributeNames": [message_attributes::PRIORITY],
                "MessageAttributeNames": ["All"],
            }),
        )
        .await
        .expect("receive");
        let message = &received["Messages"][0];

        assert_eq!(message["Attributes"][message_attributes::PRIORITY], "3");
        assert!(
            message["MessageAttributes"].is_null(),
            "nothing is fabricated in the producer's own map: {message}"
        );
    }

    #[tokio::test]
    async fn a_priority_that_is_not_a_whole_number_refuses_the_send() {
        let operations = operations();
        let url = queue(&operations).await;

        // `1.5` passes every SQS rule — `Number` permits any finite decimal — so this is
        // NexQ's own refusal, and it must leave the queue empty rather than storing the
        // message at a priority nobody asked for.
        let error = call(
            &operations,
            Operation::SendMessage,
            with_priority(&url, "hello", "1.5"),
        )
        .await
        .expect_err("not a whole number");

        assert_eq!(error.code(), "InvalidParameterValue");
        assert!(
            error.message().contains(message_attributes::PRIORITY),
            "{}",
            error.message()
        );

        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("receive");
        assert!(received["Messages"].is_null(), "nothing was stored");
    }

    /// Batches run the same send path, and this is what proves it rather than assuming it.
    #[tokio::test]
    async fn a_batch_entry_may_set_its_own_priority() {
        let operations = operations();
        let url = queue(&operations).await;

        call(
            &operations,
            Operation::SendMessageBatch,
            json!({
                "QueueUrl": url,
                "Entries": [
                    {
                        "Id": "slow",
                        "MessageBody": "later",
                        "MessageAttributes": {
                            message_attributes::PRIORITY: {
                                "DataType": "Number", "StringValue": "-1",
                            },
                        },
                    },
                    {
                        "Id": "fast",
                        "MessageBody": "urgent",
                        "MessageAttributes": {
                            message_attributes::PRIORITY: {
                                "DataType": "Number", "StringValue": "1",
                            },
                        },
                    },
                ],
            }),
        )
        .await
        .expect("send batch");

        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("receive");

        assert_eq!(received["Messages"][0]["Body"], "urgent");
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

    /// Send one message and receive it, returning its receipt handle.
    async fn claimed_handle(operations: &Operations, url: &Value) -> Value {
        call(
            operations,
            Operation::SendMessage,
            json!({ "QueueUrl": url, "MessageBody": "hello" }),
        )
        .await
        .expect("send");

        call(
            operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url, "VisibilityTimeout": 43_200 }),
        )
        .await
        .expect("receive")["Messages"][0]["ReceiptHandle"]
            .clone()
    }

    #[tokio::test]
    async fn changing_visibility_to_zero_hands_a_message_straight_back() {
        // The message was claimed for twelve hours, so getting it again at once is only
        // possible because the hand-back worked.
        let operations = operations();
        let url = queue(&operations).await;
        let handle = claimed_handle(&operations, &url).await;

        let output = call(
            &operations,
            Operation::ChangeMessageVisibility,
            json!({ "QueueUrl": url, "ReceiptHandle": handle, "VisibilityTimeout": 0 }),
        )
        .await
        .expect("change visibility");
        assert_eq!(output, json!({}), "SQS answers with an empty body");

        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("receive");

        assert_eq!(received["Messages"][0]["Body"], "hello");
        assert_ne!(
            received["Messages"][0]["ReceiptHandle"], handle,
            "a redelivery comes with a new handle"
        );
    }

    #[tokio::test]
    async fn extending_a_claim_keeps_the_message_held() {
        let operations = operations();
        let url = queue(&operations).await;
        let handle = claimed_handle(&operations, &url).await;

        call(
            &operations,
            Operation::ChangeMessageVisibility,
            json!({ "QueueUrl": url, "ReceiptHandle": handle, "VisibilityTimeout": 43_200 }),
        )
        .await
        .expect("extend");

        let received = call(
            &operations,
            Operation::ReceiveMessage,
            json!({ "QueueUrl": url }),
        )
        .await
        .expect("receive");
        assert_eq!(received, json!({}), "still held by the first consumer");

        // And the handle still works, since extending changes when the claim ends
        // rather than whose it is.
        call(
            &operations,
            Operation::DeleteMessage,
            json!({ "QueueUrl": url, "ReceiptHandle": handle }),
        )
        .await
        .expect("the extended claim's handle should still delete");
    }

    #[tokio::test]
    async fn changing_visibility_with_a_spent_handle_is_refused() {
        let operations = operations();
        let url = queue(&operations).await;
        let handle = claimed_handle(&operations, &url).await;
        call(
            &operations,
            Operation::DeleteMessage,
            json!({ "QueueUrl": url, "ReceiptHandle": handle.clone() }),
        )
        .await
        .expect("delete");

        let error = call(
            &operations,
            Operation::ChangeMessageVisibility,
            json!({ "QueueUrl": url, "ReceiptHandle": handle, "VisibilityTimeout": 0 }),
        )
        .await
        .expect_err("the message is gone");

        assert_eq!(error.code(), "ReceiptHandleIsInvalid");
    }

    #[tokio::test]
    async fn changing_visibility_needs_a_timeout_to_change_it_to() {
        // Absent is a client error rather than a default: an operation whose only job is
        // to set a timeout has nothing to do without one.
        let operations = operations();
        let url = queue(&operations).await;
        let handle = claimed_handle(&operations, &url).await;

        for (input, expected) in [
            (
                json!({ "QueueUrl": url, "ReceiptHandle": handle }),
                "MissingParameter",
            ),
            (
                json!({ "QueueUrl": url, "ReceiptHandle": handle,
                        "VisibilityTimeout": 43_201 }),
                "InvalidParameterValue",
            ),
            (
                json!({ "QueueUrl": url, "ReceiptHandle": handle,
                        "VisibilityTimeout": "soon" }),
                "InvalidParameterValue",
            ),
            (
                json!({ "QueueUrl": url, "VisibilityTimeout": 30 }),
                "MissingParameter",
            ),
        ] {
            let error = call(
                &operations,
                Operation::ChangeMessageVisibility,
                input.clone(),
            )
            .await
            .expect_err(&input.to_string());

            assert_eq!(error.code(), expected, "{input}");
        }
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
        // Real SQS operations with no handler here, which must be distinguishable from
        // ones that do not exist.
        for operation in [
            Operation::TagQueue,
            Operation::ListQueueTags,
            Operation::AddPermission,
            Operation::StartMessageMoveTask,
        ] {
            let error = call(&operations(), operation, json!({}))
                .await
                .expect_err(&format!("{operation} should not be implemented"));

            assert_eq!(error.code(), "NotImplemented", "{operation}");
        }
    }
}
