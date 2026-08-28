//! Messages: the collection at `/queues/{queue}/messages`, one claim at
//! `/queues/{queue}/messages/{receiptHandle}`, and one message's place in the line at
//! `/queues/{queue}/messages/{messageId}/position`.
//!
//! Those last two are addressed differently on purpose. A receipt handle names a *claim*
//! and only its holder has one; a message id names the message for as long as it exists,
//! and whoever sent it has one. Asking where a message is in the line is a producer's
//! question, and a producer never holds a claim.
//!
//! Three places where this deliberately does not follow SQS:
//!
//! - **Sending is always a list.** SQS has `SendMessage` and `SendMessageBatch` because its
//!   protocol needs two operations; here one endpoint takes `{"messages": [...]}`, and
//!   sending one message is a list of one. That removes a whole duplicate operation rather
//!   than reproducing it.
//! - **Batch entries are identified by position**, not by ids the client invents. SQS needs
//!   `Id` on every entry and has a `BatchEntryIdsNotDistinct` failure to go with it; an
//!   array already has indices, so that failure mode does not exist here.
//! - **A claim is a resource.** Deleting a message is `DELETE` on its receipt handle and
//!   changing its visibility is `PATCH` on the same address, rather than two more verbs in
//!   a body.
//!
//! One thing still left out: **absolute timestamps** on a received message. `enqueued_at`
//! and friends would need a date format decision the receive shape has not needed —
//! what a consumer wants from a claim is how long it has, which is a duration. Queue
//! timestamps are RFC 3339, so the format is settled when a message needs one.

use std::time::{Duration, SystemTime};

use aide::transform::TransformOperation;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use nexq_core::engine::{Engine, MAX_MESSAGES_PER_RECEIVE, MAX_RECEIVE_WAIT, ReceiveRequest};
use nexq_core::model::{
    AttributeValue, ClaimedMessage, MessageAttribute, MessageAttributes, MessageId, MessageState,
    Priority, QueuePosition, ReceiptHandle,
};
use nexq_core::{Message, QueueName};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ErrorBody, ErrorDetail};
use crate::extract::OptionalJson;
use crate::queues::{DELAY_MAX_SECONDS, QueuePath, VISIBILITY_TIMEOUT_MAX_SECONDS};
use crate::server::FacadeState;

/// Most messages one request may carry.
///
/// The same limit the SQS facade has, so a batch that works through one facade works
/// through the other and client code moving between them does not have to change.
pub const MAX_MESSAGES_PER_SEND: usize = 10;

/// The attribute namespace NexQ reserves, in any casing.
///
/// Reserved here even though this facade has a real `priority` field, for two reasons.
/// The SQS facade *reads* `NexQ.Priority` out of this map, since its protocol has nowhere
/// else to carry a priority — so a message sent here with both a `priority` of 1 and an
/// attribute saying 10 would tell two different stories to two consumers. And an
/// attribute set here is meant to survive a round trip through that facade, which refuses
/// these names; accepting one would produce metadata that cannot make the trip.
///
/// `the_two_facades_reserve_the_same_namespace` in `tests/cross_facade.rs` keeps this
/// equal to the AWS facade's own reservation.
const RESERVED_ATTRIBUTE_PREFIX: &str = "nexq.";

/// What kind of value an attribute carries.
///
/// Three kinds, matching what the SQS facade accepts, so an attribute set here survives a
/// round trip through that facade unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum AttributeType {
    /// Text, carried as-is.
    String,

    /// A number, still written as text so no precision is lost on the way through JSON.
    Number,

    /// Bytes, carried base64-encoded in `value`.
    Binary,
}

/// One piece of metadata the producer attached to a message.
///
/// The value is always a JSON string. For `binary` it is base64 of the bytes, which is
/// what keeps the wire format one shape instead of two — and the bytes are what is stored,
/// so text that happens to look like base64 and the bytes it decodes to stay different
/// things.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct MessageAttributeBody {
    /// How to read `value`.
    #[serde(rename = "type")]
    pub kind: AttributeType,

    /// An optional label of the producer's own, describing the value more precisely — a
    /// `string` labelled `uuid`, say. Carried through untouched and given back on receive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,

    /// The value: text for `string` and `number`, base64 for `binary`.
    pub value: String,
}

/// One message to send.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SendMessageBody {
    /// The message body.
    pub body: String,

    /// Higher is served first; defaults to the middle of the road.
    ///
    /// NexQ's own concept, and one of the reasons this API exists: SQS has no field for
    /// it, so a client on that facade has to smuggle it through a `NexQ.Priority` message
    /// attribute. Here it is what it is — a field.
    #[serde(default)]
    pub priority: Option<i32>,

    /// Hold this message back for this long before any consumer can receive it.
    ///
    /// Overrides the queue's own delay for this message alone.
    #[serde(default)]
    #[schemars(range(min = 0, max = 900))]
    pub delay_seconds: Option<u64>,

    /// Metadata to carry alongside the body, keyed by name.
    ///
    /// Names beginning `nexq.` are reserved and refused: the SQS-compatible facade reads
    /// `NexQ.Priority` out of this map because its protocol has no field for a priority,
    /// and here there is one — so an attribute in that namespace could only contradict it.
    #[serde(default)]
    pub attributes: Option<std::collections::BTreeMap<String, MessageAttributeBody>>,
}

/// Messages to send.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SendBody {
    /// One or more messages. Sending a single message is a list of one — there is no
    /// separate batch operation to choose between.
    pub messages: Vec<SendMessageBody>,
}

/// Whether one entry of a multi-entry request was accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum EntryStatus {
    Accepted,
    Refused,
}

/// What happened to one entry.
///
/// **A request carrying several entries is not a transaction**: each succeeds or fails on
/// its own and the response reports both, so nine good messages are not lost to one bad
/// one. The request itself is still a `200` — a client has to read the results rather than
/// rely on an error being raised.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryResult {
    /// Which entry this is, by position in the request's list. Positions rather than
    /// client-supplied ids: an array already has them, and SQS's duplicate-id failure
    /// cannot happen.
    pub index: usize,

    /// Whether it was accepted. Exactly one of `messageId` and `error` accompanies it.
    pub status: EntryStatus,

    /// The id of the accepted message, for a send.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,

    /// Why the entry was refused.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorDetail>,
}

/// What happened to each entry of a multi-entry request.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryResults {
    /// One per entry, in request order.
    pub results: Vec<EntryResult>,
}

/// Receipt handles to delete.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DeleteBatchBody {
    /// The claims to finish, by receipt handle.
    pub receipt_handles: Vec<String>,
}

/// A new visibility timeout for one claim.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct VisibilityBody {
    /// How much longer the claim should last, counted from now.
    ///
    /// `0` hands the message straight back, which wakes a consumer waiting for one.
    #[schemars(range(min = 0, max = 43200))]
    pub visibility_timeout_seconds: u64,
}

/// One claim to re-time.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct VisibilityChange {
    /// Which claim.
    pub receipt_handle: String,

    /// How much longer it should last, counted from now.
    #[schemars(range(min = 0, max = 43200))]
    pub visibility_timeout_seconds: u64,
}

/// Claims to re-time.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct VisibilityBatchBody {
    /// One or more claims.
    pub changes: Vec<VisibilityChange>,
}

/// How much a purge threw away.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PurgeResponse {
    /// Messages removed, counting ones a consumer was holding.
    pub purged: u64,
}

/// Which message a request is about, by the id it was sent under.
// A message id rather than a receipt handle, which is the distinction between this and
// `ClaimPath`: an id names the message for as long as it exists and whoever sent it knows
// one, while a handle names a claim and only its holder has one. Asking where a message is
// in the line is a question its *producer* asks, and a producer never holds a claim. Kept
// out of the doc comment because `aide` publishes these — see `published_descriptions_are_
// not_written_for_rustdoc`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MessagePath {
    /// Name of the queue.
    pub queue: String,

    /// The message's identifier, as returned when it was sent.
    pub message_id: String,
}

/// What a message is doing right now.
///
/// The same three groups a queue's counts are split into, for one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MessageStateResponse {
    /// Claimable now: a consumer receiving from this queue could be handed it.
    Visible,

    /// In flight — a consumer holds it under a claim that has not lapsed.
    NotVisible,

    /// Waiting out a delay, and never yet delivered.
    Delayed,
}

/// Where a message sits in the line.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PositionResponse {
    /// The message this is about, echoed back from the path.
    pub message_id: String,

    /// Its place in line, counting from one: `1` is the message a receive would be handed
    /// next.
    ///
    /// Counts **only messages that are claimable right now**, so a queue holding a hundred
    /// delayed or in-flight messages can still answer `1`. Approximate, and named that way
    /// for the same reason SQS names its counts approximate — with one difference worth
    /// knowing: this is not merely a number that lags. A higher-priority message arriving
    /// moves an existing one *backwards*, so a caller polling this will see it go up as
    /// well as down. It is a place in an order, not a countdown.
    pub approximate_position: u64,

    /// What the message itself is doing, which is what says how to read the position.
    ///
    /// For a `visible` message the position is its place among the claimable ones. For a
    /// `delayed` or `notVisible` one it is the count of everything claimable, plus one —
    /// since none of that can be served after a message that cannot be served at all.
    pub state: MessageStateResponse,
}

/// Which claim a request is about.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClaimPath {
    /// Name of the queue.
    pub queue: String,

    /// The receipt handle a receive handed out. It identifies one claim, not the message,
    /// so it stops working when that claim ends.
    pub receipt_handle: String,
}

/// What a consumer may ask for. Every field is optional; an empty body is a plain poll of
/// one message under the queue's own defaults.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default, rename_all = "camelCase")]
pub struct ReceiveBody {
    /// How many messages to return at most. Fewer may come back, including none.
    ///
    /// Defaults to one when omitted. A value outside the range is refused rather than
    /// clamped, so asking for more than the maximum is an error and not a short answer.
    // The literal bounds are what a client can validate against, so they are in the
    // schema rather than only in prose; `the_documented_limits_match_the_engine` keeps
    // them equal to the engine's own constants.
    #[schemars(range(min = 1, max = 10))]
    pub max_messages: Option<usize>,

    /// How long the returned messages stay invisible to other consumers.
    ///
    /// Omitted means the queue's own configured timeout.
    pub visibility_timeout_seconds: Option<u64>,

    /// How long to wait for a message when the queue has none — long polling.
    ///
    /// Omitted means the queue's own configured wait, which is a different thing from
    /// `0`: zero asks for a plain poll that returns immediately.
    #[schemars(range(min = 0, max = 20))]
    pub wait_time_seconds: Option<u64>,
}

/// The answer to a receive.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReceiveResponse {
    /// Claimed messages, **empty rather than absent** when there are none.
    ///
    /// SQS omits the field entirely in that case, which is a quirk clients have to be
    /// written around; a native API has no reason to repeat it. An empty list is a normal
    /// answer, not an error, including when a long poll times out.
    pub messages: Vec<ReceivedMessage>,
}

/// One message, and the claim it came with.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReceivedMessage {
    /// The message's identifier, stable across redeliveries. The same value the
    /// SQS-compatible facade reports as `MessageId`.
    pub id: String,

    /// The token to present to delete this message or change its visibility. Refers to
    /// *this claim*, not to the message: a redelivery comes with a new one.
    pub receipt_handle: String,

    /// The message body, exactly as it was sent.
    pub body: String,

    /// Higher is served first. Reported for every message, including one sent through the
    /// SQS facade, which has no field for priority and carries it as an attribute instead.
    pub priority: i32,

    /// How many times this message has been delivered, counting this delivery.
    pub receive_count: u32,

    /// How long this claim has left.
    ///
    /// A duration rather than an absolute expiry time on purpose: a consumer needs to
    /// know how long it has, and answering that with a timestamp makes the answer depend
    /// on the client's clock agreeing with the server's. Saturates at zero rather than
    /// going negative if the claim lapsed while the response was being written.
    pub claim_expires_in_seconds: u64,

    /// Whatever metadata the producer attached, keyed by name.
    ///
    /// Always present, and empty for a message carrying none — always returned in full
    /// rather than only when asked for, since a consumer that did not want them can ignore
    /// a map it already has, while one that did would otherwise need a second request.
    pub attributes: std::collections::BTreeMap<String, MessageAttributeBody>,
}

impl From<ClaimedMessage> for ReceivedMessage {
    fn from(claimed: ClaimedMessage) -> Self {
        Self {
            id: claimed.message.id.to_string(),
            receipt_handle: claimed.receipt.as_str().to_owned(),
            body: claimed.message.body,
            priority: claimed.message.priority.get(),
            receive_count: claimed.message.receive_count,
            claim_expires_in_seconds: claimed
                .claim_expires_at
                .duration_since(SystemTime::now())
                .unwrap_or(Duration::ZERO)
                .as_secs(),
            attributes: claimed
                .message
                .attributes
                .into_iter()
                .map(|(name, attribute)| (name, MessageAttributeBody::from(attribute)))
                .collect(),
        }
    }
}

impl From<MessageState> for MessageStateResponse {
    fn from(state: MessageState) -> Self {
        match state {
            MessageState::Visible => Self::Visible,
            MessageState::NotVisible => Self::NotVisible,
            MessageState::Delayed => Self::Delayed,
        }
    }
}

impl AttributeType {
    /// How this kind is spelled in the stored `data_type`, which is SQS's spelling because
    /// that is what the other facade reports.
    fn base(self) -> &'static str {
        match self {
            Self::String => "String",
            Self::Number => "Number",
            Self::Binary => "Binary",
        }
    }

    fn parse(base: &str) -> Option<Self> {
        match base {
            "String" => Some(Self::String),
            "Number" => Some(Self::Number),
            "Binary" => Some(Self::Binary),
            _ => None,
        }
    }
}

impl From<MessageAttribute> for MessageAttributeBody {
    fn from(attribute: MessageAttribute) -> Self {
        // `data_type` is `Base` or `Base.label`. Both facades validate the base on the way
        // in, so an unrecognised one cannot arrive through either of them — a message
        // written directly to a backend by something else could still carry one, and
        // reporting it as text with the whole thing as its label loses nothing and invents
        // nothing.
        let (base, label) = match attribute.data_type.split_once('.') {
            Some((base, label)) => (base, Some(label.to_owned())),
            None => (attribute.data_type.as_str(), None),
        };

        let (kind, label) = match AttributeType::parse(base) {
            Some(kind) => (kind, label),
            None => (AttributeType::String, Some(attribute.data_type.clone())),
        };

        Self {
            kind,
            label,
            value: match attribute.value {
                AttributeValue::Text(text) => text,
                AttributeValue::Binary(bytes) => BASE64.encode(bytes),
            },
        }
    }
}

impl MessageAttributeBody {
    /// Validate and convert to what the engine stores.
    fn to_attribute(&self, name: &str) -> Result<MessageAttribute, ApiError> {
        if let Some(label) = &self.label
            && (label.is_empty() || label.contains('.'))
        {
            return Err(ApiError::bad_request(
                "invalid_message_attribute",
                format!(
                    "attribute {name:?} has a label of {label:?}; a label must not be empty \
                     or contain a period"
                ),
            ));
        }

        let data_type = match &self.label {
            Some(label) => format!("{}.{label}", self.kind.base()),
            None => self.kind.base().to_owned(),
        };

        let value = match self.kind {
            AttributeType::Binary => {
                AttributeValue::Binary(BASE64.decode(&self.value).map_err(|error| {
                    ApiError::bad_request(
                        "invalid_message_attribute",
                        format!(
                            "attribute {name:?} is binary, so its value must be base64: {error}"
                        ),
                    )
                })?)
            }
            // A number is carried as text so JSON cannot round it, but it still has to be
            // one — storing "banana" as a Number would hand the next reader a lie.
            AttributeType::Number => {
                if self.value.parse::<f64>().is_err() {
                    return Err(ApiError::bad_request(
                        "invalid_message_attribute",
                        format!(
                            "attribute {name:?} is a number, but {:?} is not",
                            self.value
                        ),
                    ));
                }
                AttributeValue::Text(self.value.clone())
            }
            AttributeType::String => AttributeValue::Text(self.value.clone()),
        };

        Ok(MessageAttribute { data_type, value })
    }
}

impl SendMessageBody {
    /// Validate the parts the engine does not.
    fn parts(&self) -> Result<(Priority, MessageAttributes, Option<Duration>), ApiError> {
        if let Some(delay) = self.delay_seconds
            && delay > DELAY_MAX_SECONDS
        {
            return Err(ApiError::bad_request(
                "invalid_delay",
                format!("delaySeconds must be between 0 and {DELAY_MAX_SECONDS}, got {delay}"),
            ));
        }

        let mut attributes = MessageAttributes::new();
        for (name, attribute) in self.attributes.iter().flatten() {
            if name.is_empty() {
                return Err(ApiError::bad_request(
                    "invalid_message_attribute",
                    "an attribute name must not be empty",
                ));
            }

            if name
                .to_ascii_lowercase()
                .starts_with(RESERVED_ATTRIBUTE_PREFIX)
            {
                return Err(ApiError::bad_request(
                    "invalid_message_attribute",
                    format!(
                        "attribute {name:?} is in the {RESERVED_ATTRIBUTE_PREFIX:?} namespace, \
                         which NexQ reserves; set the message's `priority` instead"
                    ),
                ));
            }

            attributes.insert(name.clone(), attribute.to_attribute(name)?);
        }

        Ok((
            self.priority.map_or(Priority::DEFAULT, Priority::new),
            attributes,
            self.delay_seconds.map(Duration::from_secs),
        ))
    }
}

impl EntryResult {
    fn accepted(index: usize, message: &Message) -> Self {
        Self {
            index,
            status: EntryStatus::Accepted,
            message_id: Some(message.id.to_string()),
            error: None,
        }
    }

    fn done(index: usize) -> Self {
        Self {
            index,
            status: EntryStatus::Accepted,
            message_id: None,
            error: None,
        }
    }

    fn refused(index: usize, error: &ApiError) -> Self {
        Self {
            index,
            status: EntryStatus::Refused,
            message_id: None,
            error: Some(error.to_body().error),
        }
    }
}

/// Refuse a list that cannot be served whatever is in it.
///
/// The two whole-request failures that remain once entries are identified by position:
/// SQS's `EmptyBatchRequest` and `TooManyEntriesInBatchRequest`. Its
/// `BatchEntryIdsNotDistinct` and `InvalidBatchEntryId` have no counterpart here, because
/// nobody supplies an id.
fn check_entries<T>(entries: &[T], what: &str) -> Result<(), ApiError> {
    if entries.is_empty() {
        return Err(ApiError::bad_request(
            "empty_request",
            format!("name at least one {what}"),
        ));
    }

    if entries.len() > MAX_MESSAGES_PER_SEND {
        return Err(ApiError::bad_request(
            "too_many_entries",
            format!(
                "at most {MAX_MESSAGES_PER_SEND} {what} entries per request, got {}",
                entries.len()
            ),
        ));
    }

    Ok(())
}

/// Send one message. Shared by the single case and every entry of a list, so a batched
/// send cannot behave differently from a lone one.
async fn send_one(
    engine: &Engine,
    queue: &QueueName,
    message: &SendMessageBody,
) -> Result<Message, ApiError> {
    let (priority, attributes, delay) = message.parts()?;

    Ok(engine
        .enqueue(queue, message.body.clone(), priority, attributes, delay)
        .await?)
}

/// Finish one claim.
async fn delete_one(
    engine: &Engine,
    queue: &QueueName,
    receipt_handle: &str,
) -> Result<(), ApiError> {
    Ok(engine
        .ack(queue, &ReceiptHandle::from_backend(receipt_handle))
        .await?)
}

/// Re-time one claim.
async fn change_one(
    engine: &Engine,
    queue: &QueueName,
    receipt_handle: &str,
    seconds: u64,
) -> Result<(), ApiError> {
    if seconds > VISIBILITY_TIMEOUT_MAX_SECONDS {
        return Err(ApiError::bad_request(
            "invalid_visibility_timeout",
            format!(
                "visibilityTimeoutSeconds must be between 0 and \
                 {VISIBILITY_TIMEOUT_MAX_SECONDS}, got {seconds}"
            ),
        ));
    }

    Ok(engine
        .change_visibility(
            queue,
            &ReceiptHandle::from_backend(receipt_handle),
            Duration::from_secs(seconds),
        )
        .await?)
}

impl ReceiveBody {
    /// Validate and convert to what the engine takes.
    ///
    /// Out-of-range values are **refused rather than clamped**. The engine clamps, since
    /// it must cope with any caller, but silently serving something other than what was
    /// asked for is worse at a protocol boundary: a consumer asking for fifty messages
    /// has misunderstood something, and quietly handing back ten leaves it misunderstood.
    fn to_request(&self) -> Result<ReceiveRequest, ApiError> {
        if let Some(max) = self.max_messages
            && !(1..=MAX_MESSAGES_PER_RECEIVE).contains(&max)
        {
            return Err(ApiError::bad_request(
                "invalid_max_messages",
                format!("maxMessages must be between 1 and {MAX_MESSAGES_PER_RECEIVE}, got {max}"),
            ));
        }

        if let Some(wait) = self.wait_time_seconds
            && wait > MAX_RECEIVE_WAIT.as_secs()
        {
            return Err(ApiError::bad_request(
                "invalid_wait_time",
                format!(
                    "waitTimeSeconds must be at most {}, got {wait}",
                    MAX_RECEIVE_WAIT.as_secs()
                ),
            ));
        }

        Ok(ReceiveRequest {
            max_messages: self.max_messages.unwrap_or(1),
            visibility_timeout: self.visibility_timeout_seconds.map(Duration::from_secs),
            wait: self.wait_time_seconds.map(Duration::from_secs),
        })
    }
}

/// `POST /api/v1/queues/{queue}/messages/receive`
///
/// A `POST` because receiving claims the message, so it is neither safe nor idempotent —
/// two identical calls return different messages. That is also why it is a sub-resource
/// action rather than a `GET` on the message collection.
pub async fn receive(
    State(facade): State<FacadeState>,
    Path(QueuePath { queue }): Path<QueuePath>,
    body: OptionalJson<ReceiveBody>,
) -> Result<Json<ReceiveResponse>, ApiError> {
    let queue = QueueName::new(queue)?;
    let request = body.unwrap_or_default().to_request()?;

    let claimed = facade.engine.receive(&queue, &request).await?;

    Ok(Json(ReceiveResponse {
        messages: claimed.into_iter().map(ReceivedMessage::from).collect(),
    }))
}

/// `POST /api/v1/queues/{queue}/messages`
///
/// Creating messages in the collection, one or many. Partial success: the request is a
/// `200` and each entry reports its own outcome.
pub async fn send(
    State(facade): State<FacadeState>,
    Path(QueuePath { queue }): Path<QueuePath>,
    Json(body): Json<SendBody>,
) -> Result<Json<EntryResults>, ApiError> {
    let name = QueueName::new(queue)?;
    check_entries(&body.messages, "message")?;

    // The queue is looked up once, before any entry runs: `QueueUrl` belongs to the
    // request rather than to an entry, so a missing queue is one raised error and not the
    // same failure repeated ten times in a list a client might not read.
    facade.engine.get_queue(&name).await?;

    let mut results = Vec::with_capacity(body.messages.len());
    for (index, message) in body.messages.iter().enumerate() {
        results.push(match send_one(&facade.engine, &name, message).await {
            Ok(message) => EntryResult::accepted(index, &message),
            Err(error) => EntryResult::refused(index, &error),
        });
    }

    Ok(Json(EntryResults { results }))
}

/// `DELETE /api/v1/queues/{queue}/messages/{receiptHandle}`
///
/// Finishing one claim, which is what deleting a message means: the handle identifies the
/// claim, so a stale one is refused rather than silently doing nothing.
pub async fn delete_message(
    State(facade): State<FacadeState>,
    Path(ClaimPath {
        queue,
        receipt_handle,
    }): Path<ClaimPath>,
) -> Result<StatusCode, ApiError> {
    let name = QueueName::new(queue)?;
    delete_one(&facade.engine, &name, &receipt_handle).await?;

    Ok(StatusCode::NO_CONTENT)
}

/// `PATCH /api/v1/queues/{queue}/messages/{receiptHandle}`
pub async fn change_visibility(
    State(facade): State<FacadeState>,
    Path(ClaimPath {
        queue,
        receipt_handle,
    }): Path<ClaimPath>,
    Json(body): Json<VisibilityBody>,
) -> Result<StatusCode, ApiError> {
    let name = QueueName::new(queue)?;
    change_one(
        &facade.engine,
        &name,
        &receipt_handle,
        body.visibility_timeout_seconds,
    )
    .await?;

    Ok(StatusCode::NO_CONTENT)
}

/// `POST /api/v1/queues/{queue}/messages/delete`
///
/// Finishing several claims at once. A `POST` rather than a `DELETE` carrying a body,
/// which some proxies and clients drop.
pub async fn delete_batch(
    State(facade): State<FacadeState>,
    Path(QueuePath { queue }): Path<QueuePath>,
    Json(body): Json<DeleteBatchBody>,
) -> Result<Json<EntryResults>, ApiError> {
    let name = QueueName::new(queue)?;
    check_entries(&body.receipt_handles, "receipt handle")?;
    facade.engine.get_queue(&name).await?;

    let mut results = Vec::with_capacity(body.receipt_handles.len());
    for (index, handle) in body.receipt_handles.iter().enumerate() {
        results.push(match delete_one(&facade.engine, &name, handle).await {
            Ok(()) => EntryResult::done(index),
            Err(error) => EntryResult::refused(index, &error),
        });
    }

    Ok(Json(EntryResults { results }))
}

/// `POST /api/v1/queues/{queue}/messages/visibility`
pub async fn visibility_batch(
    State(facade): State<FacadeState>,
    Path(QueuePath { queue }): Path<QueuePath>,
    Json(body): Json<VisibilityBatchBody>,
) -> Result<Json<EntryResults>, ApiError> {
    let name = QueueName::new(queue)?;
    check_entries(&body.changes, "change")?;
    facade.engine.get_queue(&name).await?;

    let mut results = Vec::with_capacity(body.changes.len());
    for (index, change) in body.changes.iter().enumerate() {
        let outcome = change_one(
            &facade.engine,
            &name,
            &change.receipt_handle,
            change.visibility_timeout_seconds,
        )
        .await;

        results.push(match outcome {
            Ok(()) => EntryResult::done(index),
            Err(error) => EntryResult::refused(index, &error),
        });
    }

    Ok(Json(EntryResults { results }))
}

/// `GET /api/v1/queues/{queue}/messages/{messageId}/position`
///
/// Where a message is in the line. A `GET`, and the only safe operation on a message: it
/// claims nothing and changes nothing, so a producer can ask it as often as it likes.
///
/// Its own sub-resource rather than a field on the message, because a message is not
/// readable by id through this API — receiving is how a message is read, and receiving
/// claims it. A producer wanting to know where its job got to should not have to take it
/// out of the queue to find out.
pub async fn position(
    State(facade): State<FacadeState>,
    Path(MessagePath { queue, message_id }): Path<MessagePath>,
) -> Result<Json<PositionResponse>, ApiError> {
    let name = QueueName::new(queue)?;

    // A message id is opaque — whatever minted it decides its shape — so there is nothing
    // to validate here. The only question worth asking is whether this queue holds one,
    // and the store answers that.
    let position = facade
        .engine
        .position_of(&name, &MessageId::from_backend(message_id.clone()))
        .await?
        .ok_or_else(|| {
            ApiError::not_found(
                "message_not_found",
                format!("queue {name} holds no message with id {message_id}"),
            )
        })?;

    Ok(Json(PositionResponse::of(message_id, position)))
}

impl PositionResponse {
    /// The wire form of a position, which is one-based where the engine's is a count of
    /// what is ahead.
    fn of(message_id: String, position: QueuePosition) -> Self {
        Self {
            message_id,
            approximate_position: position.place(),
            state: position.state.into(),
        }
    }
}

/// `DELETE /api/v1/queues/{queue}/messages`
///
/// Purging: deleting the message *collection* while keeping the queue, which is what
/// `DELETE` on a collection means. Distinct from deleting the queue itself, which is
/// `DELETE` one level up.
pub async fn purge(
    State(facade): State<FacadeState>,
    Path(QueuePath { queue }): Path<QueuePath>,
) -> Result<Json<PurgeResponse>, ApiError> {
    let name = QueueName::new(queue)?;

    Ok(Json(PurgeResponse {
        purged: facade.engine.purge_queue(&name).await?,
    }))
}

/// What the spec says about [`receive`].
///
/// Only what cannot be derived from the types: aide already knows the path parameter, the
/// request body, and both response shapes from the handler's signature, and `schemars`
/// takes every field description from the doc comments above. What is added here is the
/// prose a reader needs and the specific failures worth naming — [`ApiError`] documents
/// itself as one default response, since any operation can fail several ways with the same
/// body, so an operation that wants statuses spelled out says so.
pub fn receive_docs(mut operation: TransformOperation) -> TransformOperation {
    // aide infers `required: true` for the body of a handler taking `Option<Json<T>>` —
    // its own `Option` input impl relaxes path and query parameters and carries a TODO
    // for the body. Corrected here rather than left wrong: an empty `POST` is a valid
    // request, and a generated client that believed otherwise would demand a body its
    // caller does not have to supply.
    if let Some(body) = operation
        .inner_mut()
        .request_body
        .as_mut()
        .and_then(|body| body.as_item_mut())
    {
        body.required = false;
    }

    operation
        .id("receiveMessages")
        .summary("Receive messages from a queue")
        .description(
            "Claims up to `maxMessages` messages, making them invisible to other \
             consumers until the claim lapses or they are deleted. A `POST` because it \
             changes something: two identical calls return different messages.\n\n\
             With `waitTimeSeconds` the request is held open until a message arrives or \
             the wait runs out — long polling — and returns the moment one is sent, \
             including one sent through the SQS-compatible facade, since both run over one \
             queue. Omitting it uses the queue's own configured wait, which is a different \
             thing from sending `0`: zero asks for a plain poll. The wait applies to the \
             *first* message only, so asking for ten when three exist returns three rather \
             than holding on for seven more.\n\n\
             An empty list is a normal answer, not an error — including when a long poll \
             runs out, and when the server is shutting down and releases waiting \
             consumers.\n\n\
             Every message comes with a receipt handle identifying **that claim**, not the \
             message: a redelivery arrives with a new one. There is no way to delete a \
             message through this facade yet, so a claim taken here lapses and the message \
             returns; use the SQS facade to delete it.",
        )
        .response_with::<200, Json<ReceiveResponse>, _>(|response| {
            response.description(
                "Zero or more claimed messages. `messages` is an empty list rather than \
                 absent when there are none.",
            )
        })
        .response_with::<400, Json<ErrorBody>, _>(|response| {
            response.description(
                "The queue name is not legal, or `maxMessages` or `waitTimeSeconds` is \
                 outside its range — refused rather than clamped.",
            )
        })
        .response_with::<404, Json<ErrorBody>, _>(|response| {
            response.description("No queue by that name.")
        })
        .with(crate::error::needs_a_token)
        .with(crate::error::reads_a_json_body)
}

pub fn send_docs(operation: TransformOperation) -> TransformOperation {
    operation
        .id("sendMessages")
        .summary("Send messages to a queue")
        .description(
            "Takes a **list**, always. Sending one message is a list of one — there is no \
             separate batch operation to choose between, which is one fewer thing to get \
             wrong than the SQS pair it replaces.\n\n\
             Not a transaction. Each message is accepted or refused on its own and the \
             response reports both, so nine good messages are not lost to one bad one. The \
             request itself is a `200` in that case, so **read the results** rather than \
             relying on an error being raised. Entries are identified by their position in \
             the request, so unlike SQS there are no ids to supply and no duplicate-id \
             failure.\n\n\
             `priority` is NexQ's own and one of the reasons this API exists. Higher is \
             served first, and omitting it means the middle of the road. SQS has no field \
             for it, so a client on that facade has to carry it as a `NexQ.Priority` \
             message attribute; here it is an ordinary field, and it is reported back on \
             receive.\n\n\
             A queue that does not exist is one raised `404` rather than the same failure \
             repeated in every entry, because the queue belongs to the request and not to \
             any message in it.",
        )
        .response_with::<200, Json<EntryResults>, _>(|response| {
            response.description(
                "One result per message, in request order. Some may be refused — check \
                 `status` on each.",
            )
        })
        .response_with::<400, Json<ErrorBody>, _>(|response| {
            response.description(
                "The list is empty or too long, the queue name is not legal, or the body \
                 does not fit the schema. Nothing was sent.",
            )
        })
        .response_with::<404, Json<ErrorBody>, _>(|response| {
            response.description("No queue by that name.")
        })
        .with(crate::error::needs_a_token)
        .with(crate::error::reads_a_json_body)
}

pub fn delete_message_docs(operation: TransformOperation) -> TransformOperation {
    operation
        .id("deleteMessage")
        .summary("Finish with a message")
        .description(
            "Deletes the message a receipt handle refers to, which is how a consumer says \
             it is done. Until this happens the message comes back when its claim lapses — \
             that is what makes delivery at-least-once.\n\n\
             The handle identifies **one claim**, not the message. A handle whose claim has \
             already ended — deleted, or lapsed so the message went to another consumer — \
             is refused rather than silently doing nothing, since a consumer that has lost \
             its claim needs to know it may have duplicated work.",
        )
        .response_with::<204, (), _>(|response| {
            response.description("The message is gone. No body.")
        })
        .response_with::<400, Json<ErrorBody>, _>(|response| {
            response.description(
                "The queue name is not legal, or the receipt handle does not identify a \
                 current claim.",
            )
        })
        .response_with::<404, Json<ErrorBody>, _>(|response| {
            response.description("No queue by that name.")
        })
        .with(crate::error::needs_a_token)
}

pub fn change_visibility_docs(operation: TransformOperation) -> TransformOperation {
    operation
        .id("changeMessageVisibility")
        .summary("Re-time a claim")
        .description(
            "Sets how much longer a claim lasts, **counted from now** rather than from when \
             the message was received. One operation for two jobs, which is what makes that \
             choice the useful one: a consumer that needs longer extends its claim instead \
             of having the message handed to someone else mid-work, and a consumer that \
             cannot do the work at all sets `0`.\n\n\
             `0` hands the message straight back, and because that is a client action making \
             a message claimable it **wakes a consumer that is long-polling** for one — work \
             returned by one consumer reaches the next without waiting out a timeout.\n\n\
             The receipt handle stays valid on a non-zero change: this alters when a claim \
             ends, not whose it is.",
        )
        .response_with::<204, (), _>(|response| {
            response.description("The claim has been re-timed. No body.")
        })
        .response_with::<400, Json<ErrorBody>, _>(|response| {
            response.description(
                "The queue name is not legal, the timeout is out of range, or the receipt \
                 handle does not identify a current claim.",
            )
        })
        .response_with::<404, Json<ErrorBody>, _>(|response| {
            response.description("No queue by that name.")
        })
        .with(crate::error::needs_a_token)
        .with(crate::error::reads_a_json_body)
}

pub fn delete_batch_docs(operation: TransformOperation) -> TransformOperation {
    operation
        .id("deleteMessages")
        .summary("Finish with several messages")
        .description(
            "The same as deleting one message, several at a time. A `POST` rather than a \
             `DELETE` carrying a body, since a body on `DELETE` is legal but widely \
             mishandled by proxies and clients.\n\n\
             Not a transaction: each handle succeeds or fails on its own and the response \
             reports both, identified by position. A handle whose claim already ended is \
             refused as itself rather than sinking the rest.",
        )
        .response_with::<200, Json<EntryResults>, _>(|response| {
            response.description("One result per receipt handle, in request order.")
        })
        .response_with::<400, Json<ErrorBody>, _>(|response| {
            response.description("The list is empty or too long, or the queue name is not legal.")
        })
        .response_with::<404, Json<ErrorBody>, _>(|response| {
            response.description("No queue by that name.")
        })
        .with(crate::error::needs_a_token)
        .with(crate::error::reads_a_json_body)
}

pub fn visibility_batch_docs(operation: TransformOperation) -> TransformOperation {
    operation
        .id("changeMessageVisibilityBatch")
        .summary("Re-time several claims")
        .description(
            "The same as re-timing one claim, several at a time, each with its own timeout. \
             Not a transaction: outcomes are per entry and identified by position.\n\n\
             Unlike the SQS operation this replaces, a timeout is required on every entry \
             rather than optional — there is no queue default to fall back to that would not \
             be a guess about what the caller meant.",
        )
        .response_with::<200, Json<EntryResults>, _>(|response| {
            response.description("One result per change, in request order.")
        })
        .response_with::<400, Json<ErrorBody>, _>(|response| {
            response.description("The list is empty or too long, or the queue name is not legal.")
        })
        .response_with::<404, Json<ErrorBody>, _>(|response| {
            response.description("No queue by that name.")
        })
        .with(crate::error::needs_a_token)
        .with(crate::error::reads_a_json_body)
}

pub fn position_docs(operation: TransformOperation) -> TransformOperation {
    operation
        .id("getMessagePosition")
        .summary("Ask where a message is in the line")
        .description(
            "Answers \"where am I in line\" for one message, by the id returned when it was \
             sent. One of the operations the SQS-compatible facade cannot express, which is \
             why this API exists.\n\n\
             `approximatePosition` counts from one, and counts **only messages that are \
             claimable right now**: `1` means a receive would be handed this message next. \
             Delayed and in-flight messages are not counted, so a queue holding a hundred of \
             them can still answer `1` — counting them would be the other kind of wrong, \
             since a consumer polling now would not be given them either.\n\n\
             Approximate, and not only in the way SQS's counts are approximate. It is true \
             at the instant it was computed, and **a higher-priority message arriving moves \
             an existing one backwards** — this is a place in an order, not a countdown, so \
             a caller polling it will see it rise as well as fall. Read `state` alongside \
             it: a message that is delayed or in flight is behind everything claimable, and \
             its position says so rather than pretending it is about to be served.\n\n\
             A message that is not in the queue — never sent, or received and deleted — is \
             a `404`. That is a normal outcome for a producer polling a job to completion, \
             and it does not distinguish \"finished\" from \"never existed\", because \
             nothing at this layer can.",
        )
        .response_with::<200, Json<PositionResponse>, _>(|response| {
            response.description("Where the message is, and what it is doing.")
        })
        .response_with::<400, Json<ErrorBody>, _>(|response| {
            response.description("The queue name is not legal.")
        })
        .response_with::<404, Json<ErrorBody>, _>(|response| {
            response.description("No queue by that name, or the queue holds no such message.")
        })
        .with(crate::error::needs_a_token)
}

pub fn purge_docs(operation: TransformOperation) -> TransformOperation {
    operation
        .id("purgeQueue")
        .summary("Throw away every message in a queue")
        .description(
            "Empties the queue and keeps the queue — `DELETE` on the *message collection*, \
             where deleting the queue itself is `DELETE` one level up.\n\n\
             **Irreversible, and it takes in-flight messages with it**: a consumer working \
             on a message right now will find its receipt handle refused, because the \
             message is gone. A purge that spared claimed messages would be one that \
             quietly did not purge, since those messages would reappear when their claims \
             lapsed.\n\n\
             Unlike SQS there is no sixty-second cooldown. SQS needs one because its purge \
             runs asynchronously and it refuses a second while the first is still going; \
             this one has finished by the time it answers, so refusing would be a limitation \
             invented for its own sake. The count of what went is returned, and logged.",
        )
        .response_with::<200, Json<PurgeResponse>, _>(|response| {
            response.description("How many messages were removed.")
        })
        .response_with::<400, Json<ErrorBody>, _>(|response| {
            response.description("The queue name is not legal.")
        })
        .response_with::<404, Json<ErrorBody>, _>(|response| {
            response.description("No queue by that name.")
        })
        .with(crate::error::needs_a_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(json: &str) -> ReceiveBody {
        serde_json::from_str(json).expect("parses")
    }

    #[test]
    fn an_empty_body_is_a_poll_for_one_message() {
        let request = body("{}").to_request().expect("valid");

        assert_eq!(request.max_messages, 1);
        assert!(request.visibility_timeout.is_none(), "queue's default");
        assert!(request.wait.is_none(), "queue's default");
    }

    #[test]
    fn seconds_become_durations() {
        let request = body(r#"{"visibilityTimeoutSeconds": 45, "waitTimeSeconds": 20}"#)
            .to_request()
            .expect("valid");

        assert_eq!(request.visibility_timeout, Some(Duration::from_secs(45)));
        assert_eq!(request.wait, Some(Duration::from_secs(20)));
    }

    /// Zero is meaningful — it asks for a plain poll — so it must survive as `Some(0)`
    /// rather than being folded into "unset", which would mean the queue's own wait.
    #[test]
    fn a_zero_wait_is_not_the_same_as_an_absent_one() {
        assert_eq!(
            body(r#"{"waitTimeSeconds": 0}"#)
                .to_request()
                .expect("valid")
                .wait,
            Some(Duration::ZERO)
        );
        assert_eq!(body("{}").to_request().expect("valid").wait, None);
    }

    #[test]
    fn too_many_messages_is_refused_rather_than_clamped() {
        let error = body(r#"{"maxMessages": 50}"#)
            .to_request()
            .expect_err("over the cap");

        assert_eq!(error.code(), "invalid_max_messages");
        assert!(error.message().contains("50"), "{}", error.message());
    }

    #[test]
    fn zero_messages_is_refused() {
        // Distinct from omitting the field: asking for none cannot be what was meant.
        assert_eq!(
            body(r#"{"maxMessages": 0}"#)
                .to_request()
                .expect_err("zero messages")
                .code(),
            "invalid_max_messages"
        );
    }

    #[test]
    fn a_wait_over_the_protocol_cap_is_refused() {
        let error = body(r#"{"waitTimeSeconds": 300}"#)
            .to_request()
            .expect_err("over the cap");

        assert_eq!(error.code(), "invalid_wait_time");
        assert!(error.message().contains("20"), "{}", error.message());
    }

    /// A typo must not be accepted as a default. The two this actually catches are
    /// `visibilityTimeout` without the unit suffix, and the snake_case spelling — every
    /// field on the wire is camelCase, and a client carrying the other habit should be
    /// told so rather than silently served the queue's default.
    #[test]
    fn an_unknown_field_is_refused() {
        for json in [
            r#"{"visibilityTimeout": 30}"#,
            r#"{"visibility_timeout_seconds": 30}"#,
        ] {
            serde_json::from_str::<ReceiveBody>(json)
                .expect_err("an unknown field must not be silently ignored");
        }
    }

    /// A binary attribute must be stored as the **bytes**, not as the base64 text that
    /// carried them.
    ///
    /// Asserted against the stored value rather than through a round trip, because a round
    /// trip cannot see the difference: text that happens to hold base64 comes back out as
    /// the same string either way. Where it does show is the SQS facade, whose checksum and
    /// `BinaryValue` are defined over the decoded bytes — so a wire-level test here passes
    /// while real clients get the wrong digest. Found by mutation: making this store the
    /// text left every other test green.
    #[test]
    fn a_binary_attribute_is_stored_as_bytes_not_as_its_base64_text() {
        let attribute = MessageAttributeBody {
            kind: AttributeType::Binary,
            label: None,
            value: "aGVsbG8=".to_owned(),
        }
        .to_attribute("Thumb")
        .expect("valid base64");

        assert_eq!(attribute.data_type, "Binary");
        assert_eq!(
            attribute.value,
            AttributeValue::Binary(b"hello".to_vec()),
            "the five bytes, not the eight characters that spelled them"
        );
    }

    /// And back the other way, so the pair round-trips through storage rather than only
    /// through this module.
    #[test]
    fn stored_bytes_come_back_as_base64() {
        let body = MessageAttributeBody::from(MessageAttribute {
            data_type: "Binary".to_owned(),
            value: AttributeValue::Binary(b"hello".to_vec()),
        });

        assert_eq!(body.kind, AttributeType::Binary);
        assert_eq!(body.value, "aGVsbG8=");
    }

    /// A producer's label survives the trip through `data_type` and back.
    #[test]
    fn a_label_round_trips_through_the_stored_data_type() {
        let stored = MessageAttributeBody {
            kind: AttributeType::String,
            label: Some("uuid".to_owned()),
            value: "3f2b1c".to_owned(),
        }
        .to_attribute("Label")
        .expect("valid");

        assert_eq!(
            stored.data_type, "String.uuid",
            "stored the way the SQS facade spells it, so that facade reports it correctly"
        );

        let back = MessageAttributeBody::from(stored);
        assert_eq!(back.kind, AttributeType::String);
        assert_eq!(back.label.as_deref(), Some("uuid"));
    }

    /// A label containing a period would produce a `data_type` that parses back wrong, so
    /// it is refused rather than silently changing meaning on the way home.
    #[test]
    fn a_label_that_would_not_survive_the_trip_is_refused() {
        for label in ["", "with.period"] {
            MessageAttributeBody {
                kind: AttributeType::String,
                label: Some(label.to_owned()),
                value: "x".to_owned(),
            }
            .to_attribute("Label")
            .expect_err(&format!("{label:?} must be refused"));
        }
    }

    /// Sending, with an attribute map to validate.
    fn send_body(json: &str) -> SendMessageBody {
        serde_json::from_str(json).expect("parses")
    }

    #[test]
    fn an_attribute_in_nexqs_own_namespace_is_refused() {
        // The SQS facade reads `NexQ.Priority` out of this map because its protocol has
        // nowhere else to put a priority. Accepting one here — where `priority` is a real
        // field — would let a message carry two different answers.
        for name in ["NexQ.Priority", "nexq.priority", "nexq.anything"] {
            let error = send_body(&format!(
                r#"{{"body":"x","attributes":{{"{name}":{{"type":"number","value":"10"}}}}}}"#
            ))
            .parts()
            .expect_err(name);

            assert_eq!(error.code(), "invalid_message_attribute", "{name}");
            assert!(error.message().contains("priority"), "{}", error.message());
        }
    }

    #[test]
    fn the_reservation_is_a_prefix_and_not_a_word() {
        // Only followed by a period, the way AWS's own `aws.` reservation works.
        for name in ["nexq", "NexQPriority", "my.nexq.thing"] {
            send_body(&format!(
                r#"{{"body":"x","attributes":{{"{name}":{{"type":"string","value":"v"}}}}}}"#
            ))
            .parts()
            .unwrap_or_else(|error| panic!("{name:?} should be allowed: {error:?}"));
        }
    }

    #[test]
    fn a_claim_that_already_lapsed_reports_zero_rather_than_underflowing() {
        use nexq_core::model::{Message, Priority, ReceiptHandle};

        let claimed = ClaimedMessage {
            message: Message::new("body", Priority::DEFAULT),
            receipt: ReceiptHandle::new(),
            claim_expires_at: SystemTime::now() - Duration::from_secs(60),
        };

        assert_eq!(ReceivedMessage::from(claimed).claim_expires_in_seconds, 0);
    }
}
