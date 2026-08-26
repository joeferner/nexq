//! Receiving messages.
//!
//! **Deliberately the only operation here.** It is the one the supervisor's claim needs
//! — that a message sent through the SQS facade comes back through this one, because
//! both run over a single [`Engine`](nexq_core::engine::Engine) — and the rest of the
//! surface lands with the parity item rather than being guessed at now.
//!
//! Two things are left out for the same reason, and would be wrong to invent ahead of the
//! wire types being settled:
//!
//! - **Message attributes.** Carrying them means deciding how a binary value is
//!   represented, which belongs with the OpenAPI schemas rather than here.
//! - **Absolute timestamps.** `enqueued_at` and friends need a date format, and picking
//!   one pulls in a date crate. What a consumer actually needs from a claim is how long
//!   it has left, which is a *duration* — see [`ReceivedMessage::claim_expires_in_seconds`].

use std::time::{Duration, SystemTime};

use axum::Json;
use axum::extract::{Path, State};
use nexq_core::QueueName;
use nexq_core::engine::{MAX_MESSAGES_PER_RECEIVE, MAX_RECEIVE_WAIT, ReceiveRequest};
use nexq_core::model::ClaimedMessage;
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::server::FacadeState;

/// What a consumer may ask for. Every field is optional; an empty body is a plain poll of
/// one message under the queue's own defaults.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ReceiveBody {
    /// How many messages to return at most, up to [`MAX_MESSAGES_PER_RECEIVE`].
    pub max_messages: Option<usize>,

    /// How long the returned messages stay invisible to other consumers. Omitted means
    /// the queue's configured default.
    pub visibility_timeout_seconds: Option<u64>,

    /// How long to wait for a message when the queue has none, up to
    /// [`MAX_RECEIVE_WAIT`]. Omitted means the queue's configured default, and `0` makes
    /// this a plain poll.
    pub wait_time_seconds: Option<u64>,
}

/// The answer to a receive.
#[derive(Debug, Serialize)]
pub struct ReceiveResponse {
    /// Claimed messages, **empty rather than absent** when there are none.
    ///
    /// SQS omits the field entirely in that case, which is a quirk clients have to be
    /// written around; a native API has no reason to repeat it. An empty list is a normal
    /// answer, not an error, including when a long poll times out.
    pub messages: Vec<ReceivedMessage>,
}

/// One message, and the claim it came with.
#[derive(Debug, Serialize)]
pub struct ReceivedMessage {
    pub id: String,

    /// The token to present to delete this message or change its visibility. Refers to
    /// *this claim*, not to the message: a redelivery comes with a new one.
    pub receipt_handle: String,

    pub body: String,

    /// Higher is served first. Messages sent through the SQS facade all carry the
    /// default, since that protocol cannot express a priority.
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
        }
    }
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
                format!("max_messages must be between 1 and {MAX_MESSAGES_PER_RECEIVE}, got {max}"),
            ));
        }

        if let Some(wait) = self.wait_time_seconds
            && wait > MAX_RECEIVE_WAIT.as_secs()
        {
            return Err(ApiError::bad_request(
                "invalid_wait_time",
                format!(
                    "wait_time_seconds must be at most {}, got {wait}",
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
    Path(queue): Path<String>,
    body: Option<Json<ReceiveBody>>,
) -> Result<Json<ReceiveResponse>, ApiError> {
    let queue = QueueName::new(queue)?;
    let request = body.unwrap_or_default().to_request()?;

    let claimed = facade.engine.receive(&queue, &request).await?;

    Ok(Json(ReceiveResponse {
        messages: claimed.into_iter().map(ReceivedMessage::from).collect(),
    }))
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
        let request = body(r#"{"visibility_timeout_seconds": 45, "wait_time_seconds": 20}"#)
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
            body(r#"{"wait_time_seconds": 0}"#)
                .to_request()
                .expect("valid")
                .wait,
            Some(Duration::ZERO)
        );
        assert_eq!(body("{}").to_request().expect("valid").wait, None);
    }

    #[test]
    fn too_many_messages_is_refused_rather_than_clamped() {
        let error = body(r#"{"max_messages": 50}"#)
            .to_request()
            .expect_err("over the cap");

        assert_eq!(error.code(), "invalid_max_messages");
        assert!(error.message().contains("50"), "{}", error.message());
    }

    #[test]
    fn zero_messages_is_refused() {
        // Distinct from omitting the field: asking for none cannot be what was meant.
        assert_eq!(
            body(r#"{"max_messages": 0}"#)
                .to_request()
                .expect_err("zero messages")
                .code(),
            "invalid_max_messages"
        );
    }

    #[test]
    fn a_wait_over_the_protocol_cap_is_refused() {
        let error = body(r#"{"wait_time_seconds": 300}"#)
            .to_request()
            .expect_err("over the cap");

        assert_eq!(error.code(), "invalid_wait_time");
        assert!(error.message().contains("20"), "{}", error.message());
    }

    /// A typo must not be accepted as a default. `visibility_timeout` without the unit
    /// suffix is the mistake this actually catches.
    #[test]
    fn an_unknown_field_is_refused() {
        serde_json::from_str::<ReceiveBody>(r#"{"visibility_timeout": 30}"#)
            .expect_err("an unknown field must not be silently ignored");
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
