//! Queues as a resource: a collection at `/queues` and a member at `/queues/{queue}`.
//!
//! Shaped as a resource rather than transliterated from SQS, which is the point of this
//! module. Three differences are deliberate:
//!
//! - **A queue is addressed by name in the path**, not by a URL the server hands out and
//!   the client must send back. SQS's queue-URL-as-identifier exists because it has
//!   accounts and regions to encode; repeating it here would be copying a workaround.
//! - **Creating is `PUT` on the member**, not `POST` to the collection. The engine's
//!   creation is idempotent when the attributes match (decided once, in M2, so every
//!   facade inherits it) — and "idempotent, addressed by name, supplied by the client" is
//!   exactly what `PUT` means.
//! - **Paging is by cursor**, carrying the same keyset token the store guarantees, so
//!   queues created or deleted between pages cannot make a caller skip or repeat one.
//!   Offsets would look simpler on the wire and be wrong under churn.

use std::time::Duration;

use aide::transform::TransformOperation;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use nexq_core::engine::{MAX_QUEUES_PER_PAGE, QueueQuery};
use nexq_core::model::{Queue, QueueAttributes, QueueName};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::{ApiError, ErrorBody};
use crate::extract::{OptionalJson, Query};
use crate::server::FacadeState;

/// Longest visibility timeout, in seconds: 12 hours.
///
/// These three are SQS's limits, and this facade keeps them so that a queue's legal
/// configuration does not depend on which door it was created through — a queue made here
/// with a 24-hour timeout would be reported over SQS as a value SQS itself would refuse.
///
/// Defined here rather than borrowed from the SQS facade, which should not be a dependency
/// of this one, and kept honest by `the_limits_match_the_sqs_facade` rather than by hoping.
/// If a third consumer ever needs them, that is when they belong in `nexq-core`.
pub const VISIBILITY_TIMEOUT_MAX_SECONDS: u64 = 12 * 60 * 60;

/// Longest delay, in seconds: 15 minutes.
pub const DELAY_MAX_SECONDS: u64 = 15 * 60;

/// Longest default long-poll wait, in seconds.
pub const RECEIVE_WAIT_MAX_SECONDS: u64 = 20;

/// The queue a request is about.
///
/// A struct rather than `Path<String>` because that is what lets the parameter be
/// *documented*: from a bare `String` aide learns a path parameter exists but not what it
/// is called, and generates an operation with no parameters at all. The field name is the
/// parameter name.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct QueuePath {
    /// Name of the queue.
    pub queue: String,
}

/// A queue, as this API represents it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct QueueResponse {
    /// The queue's name, which is also its address: `/api/v1/queues/{name}`.
    pub name: String,

    /// When the queue was created.
    pub created_at: DateTime<Utc>,

    /// When its attributes were last changed, or when it was created if they never have
    /// been. Reported separately because answering with the creation time for both would
    /// be a plausible-looking lie.
    pub last_modified_at: DateTime<Utc>,

    /// How the queue is configured.
    pub attributes: QueueAttributesResponse,
}

/// A queue's settings.
#[derive(Debug, Serialize, JsonSchema)]
pub struct QueueAttributesResponse {
    /// How long a claimed message stays invisible to other consumers before being
    /// redelivered.
    pub visibility_timeout_seconds: u64,

    /// How long a newly sent message waits before it can be received.
    pub delay_seconds: u64,

    /// How long a receive that finds nothing waits by default — the long poll a consumer
    /// gets without asking for one.
    pub receive_wait_time_seconds: u64,
}

/// Settings to create a queue with. Every field is optional and falls back to the
/// server's default. An unrecognised field is refused rather than ignored, so a setting
/// never looks applied when it was dropped.
// Narrower than the engine's own `QueueAttributes`, which also carries `max_receive_count`
// and a dead-letter queue: nothing enforces those yet, and accepting a setting that does
// nothing would tell a client its messages are protected when they are not. Kept out of the
// doc comment because `aide` publishes these — see `published_descriptions_are_not_written_
// for_rustdoc`, which is what caught the first draft of this.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct QueueAttributesBody {
    /// Defaults to 30 seconds.
    #[schemars(range(min = 0, max = 43200))]
    pub visibility_timeout_seconds: Option<u64>,

    /// Defaults to none.
    #[schemars(range(min = 0, max = 900))]
    pub delay_seconds: Option<u64>,

    /// Defaults to none, which makes a receive a plain poll unless it asks to wait.
    #[schemars(range(min = 0, max = 20))]
    pub receive_wait_time_seconds: Option<u64>,
}

/// Which queues to list, and where to resume.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct ListQueuesQuery {
    /// Only queues whose name starts with this.
    pub prefix: Option<String>,

    /// How many to return at most.
    #[schemars(range(min = 1, max = 1000))]
    pub limit: Option<usize>,

    /// Resume after this queue — the `next_cursor` from the previous page.
    pub cursor: Option<String>,
}

/// One page of queues.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListQueuesResponse {
    /// This page, in name order. Empty rather than absent when there are none.
    pub queues: Vec<QueueResponse>,

    /// Pass as `cursor` to fetch the next page, or `null` when this is the last one.
    ///
    /// A **cursor**, not an offset: it names where to resume, so a queue created or
    /// deleted between pages cannot make a caller skip one or see it twice. Present
    /// whenever more remain, including when `limit` was not given.
    pub next_cursor: Option<String>,
}

impl From<Queue> for QueueResponse {
    fn from(queue: Queue) -> Self {
        Self {
            name: queue.name.as_str().to_owned(),
            created_at: queue.created_at.into(),
            last_modified_at: queue.last_modified_at.into(),
            attributes: QueueAttributesResponse {
                visibility_timeout_seconds: queue.attributes.visibility_timeout.as_secs(),
                delay_seconds: queue.attributes.delay.as_secs(),
                receive_wait_time_seconds: queue.attributes.receive_wait_time.as_secs(),
            },
        }
    }
}

impl QueueAttributesBody {
    /// Validate and fill in the defaults.
    ///
    /// Out-of-range values are refused rather than clamped, for the same reason a receive
    /// refuses one: quietly storing something other than what was asked for leaves the
    /// caller believing a setting took effect.
    fn to_attributes(&self) -> Result<QueueAttributes, ApiError> {
        let defaults = QueueAttributes::default();

        Ok(QueueAttributes {
            visibility_timeout: seconds(
                "visibility_timeout_seconds",
                self.visibility_timeout_seconds,
                VISIBILITY_TIMEOUT_MAX_SECONDS,
                defaults.visibility_timeout,
            )?,
            delay: seconds(
                "delay_seconds",
                self.delay_seconds,
                DELAY_MAX_SECONDS,
                defaults.delay,
            )?,
            receive_wait_time: seconds(
                "receive_wait_time_seconds",
                self.receive_wait_time_seconds,
                RECEIVE_WAIT_MAX_SECONDS,
                defaults.receive_wait_time,
            )?,
            // Not settable here while nothing enforces them — see the note on
            // `QueueAttributesBody`. Spelled out rather than `..defaults` so that adding a
            // field to the model is a compile error here, and a decision about whether this
            // facade should expose it, rather than a silent default.
            max_receive_count: defaults.max_receive_count,
            dead_letter_queue: defaults.dead_letter_queue,
        })
    }
}

/// Bound a supplied number of seconds, or fall back to the default.
fn seconds(
    field: &str,
    given: Option<u64>,
    max: u64,
    default: Duration,
) -> Result<Duration, ApiError> {
    match given {
        None => Ok(default),
        Some(value) if value <= max => Ok(Duration::from_secs(value)),
        Some(value) => Err(ApiError::bad_request(
            "invalid_queue_attribute",
            format!("{field} must be between 0 and {max}, got {value}"),
        )),
    }
}

/// `PUT /api/v1/queues/{queue}`
///
/// Idempotent: asking for a queue that already exists **with the same attributes** returns
/// it, because the caller's intent — "there should be a queue called this, configured like
/// this" — is already satisfied. Asking with different attributes is a conflict, since
/// honouring it would mean either ignoring the new attributes or silently reconfiguring a
/// live queue.
pub async fn put_queue(
    State(facade): State<FacadeState>,
    Path(QueuePath { queue }): Path<QueuePath>,
    body: OptionalJson<QueueAttributesBody>,
) -> Result<Json<QueueResponse>, ApiError> {
    let name = QueueName::new(queue)?;
    let attributes = body.unwrap_or_default().to_attributes()?;

    Ok(Json(
        facade.engine.create_queue(name, attributes).await?.into(),
    ))
}

/// `GET /api/v1/queues/{queue}`
pub async fn get_queue(
    State(facade): State<FacadeState>,
    Path(QueuePath { queue }): Path<QueuePath>,
) -> Result<Json<QueueResponse>, ApiError> {
    let name = QueueName::new(queue)?;

    Ok(Json(facade.engine.get_queue(&name).await?.into()))
}

/// `DELETE /api/v1/queues/{queue}`
///
/// Takes the queue's messages with it. `404` when there is no such queue rather than a
/// silent success: a caller deleting something that was never there has a different
/// problem from one whose delete worked.
pub async fn delete_queue(
    State(facade): State<FacadeState>,
    Path(QueuePath { queue }): Path<QueuePath>,
) -> Result<StatusCode, ApiError> {
    let name = QueueName::new(queue)?;
    facade.engine.delete_queue(&name).await?;

    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/v1/queues`
pub async fn list_queues(
    State(facade): State<FacadeState>,
    Query(query): Query<ListQueuesQuery>,
) -> Result<Json<ListQueuesResponse>, ApiError> {
    if let Some(limit) = query.limit
        && !(1..=MAX_QUEUES_PER_PAGE).contains(&limit)
    {
        return Err(ApiError::bad_request(
            "invalid_limit",
            format!("limit must be between 1 and {MAX_QUEUES_PER_PAGE}, got {limit}"),
        ));
    }

    // A cursor is a queue name, so a malformed one is a malformed cursor rather than a
    // malformed queue name — the client did not choose it, this server handed it out.
    let after = match query.cursor {
        Some(cursor) => Some(QueueName::new(cursor).map_err(|error| {
            ApiError::bad_request(
                "invalid_cursor",
                format!("this is not a cursor this server issued: {error}"),
            )
        })?),
        None => None,
    };

    let page = facade
        .engine
        .list_queues(&QueueQuery {
            prefix: query.prefix,
            limit: query.limit,
            after,
        })
        .await?;

    Ok(Json(ListQueuesResponse {
        queues: page.queues.into_iter().map(QueueResponse::from).collect(),
        next_cursor: page.next.map(|name| name.as_str().to_owned()),
    }))
}

/// What the spec says about [`put_queue`].
///
/// Only what the types cannot say. aide already knows the path parameter, the request body,
/// and the success shape from the handler's signature, and `schemars` takes every field
/// description from the doc comments above — so what is added here is the prose a reader
/// needs and the failures worth naming individually.
pub fn put_queue_docs(operation: TransformOperation) -> TransformOperation {
    operation
        .id("putQueue")
        .summary("Create a queue, or confirm one exists")
        .description(
            "Creates the queue named in the path. `PUT` rather than a `POST` to the \
             collection because the operation is idempotent and the client chooses the \
             name: sending the same request twice leaves one queue, not two.\n\n\
             Sending it again with the *same* attributes returns the existing queue, \
             because the intent — \"there should be a queue called this, configured like \
             this\" — is already satisfied. Sending it with *different* attributes is a \
             conflict rather than a reconfiguration, since silently retiming a queue \
             consumers are working would be worse than refusing.\n\n\
             Every attribute is optional and falls back to the server's default. An \
             attribute outside its range is refused rather than clamped, and one this \
             server does not implement is refused rather than ignored, so a setting never \
             looks applied when it was dropped.",
        )
        .response_with::<200, Json<QueueResponse>, _>(|response| {
            response.description("The queue, whether it was just created or already existed.")
        })
        .response_with::<400, Json<ErrorBody>, _>(|response| {
            response.description(
                "The name is not a legal queue name, or an attribute is outside its range \
                 or not one this server implements.",
            )
        })
        .response_with::<409, Json<ErrorBody>, _>(|response| {
            response.description("A queue of that name exists with different attributes.")
        })
        .with(crate::error::needs_a_token)
        .with(crate::error::reads_a_json_body)
}

pub fn get_queue_docs(operation: TransformOperation) -> TransformOperation {
    operation
        .id("getQueue")
        .summary("Read a queue")
        .description(
            "Returns the queue's name, when it was created, when its attributes were last \
             changed, and those attributes.\n\n\
             Message counts are not part of this yet: a listing would otherwise cost one \
             aggregate query per queue on the page, and how cheap that can be made depends \
             on the storage backend. Ask the SQS facade's `GetQueueAttributes` for counts \
             in the meantime.",
        )
        .response_with::<200, Json<QueueResponse>, _>(|response| {
            response.description("The queue and its attributes.")
        })
        .response_with::<400, Json<ErrorBody>, _>(|response| {
            response.description("The name is not a legal queue name.")
        })
        .response_with::<404, Json<ErrorBody>, _>(|response| {
            response.description("No queue by that name.")
        })
        .with(crate::error::needs_a_token)
}

pub fn delete_queue_docs(operation: TransformOperation) -> TransformOperation {
    operation
        .id("deleteQueue")
        .summary("Delete a queue")
        .description(
            "Deletes the queue and everything in it, **including messages a consumer is \
             currently holding** — a receipt handle taken across this stops working, \
             because the message it referred to is gone. Consumers long-polling on the \
             queue are released rather than left waiting on something that no longer \
             exists.\n\n\
             Not idempotent in the way `PUT` is: deleting a queue that was never there is \
             a `404`, because a caller in that position has a different problem from one \
             whose delete worked.",
        )
        .response_with::<204, (), _>(|response| response.description("The queue is gone. No body."))
        .response_with::<400, Json<ErrorBody>, _>(|response| {
            response.description("The name is not a legal queue name.")
        })
        .response_with::<404, Json<ErrorBody>, _>(|response| {
            response.description("No queue by that name.")
        })
        .with(crate::error::needs_a_token)
}

pub fn list_queues_docs(operation: TransformOperation) -> TransformOperation {
    operation
        .id("listQueues")
        .summary("List queues")
        .description(
            "In name order, one page at a time, optionally filtered by `prefix`.\n\n\
             When `next_cursor` is not null there are more: pass it back as `cursor` to \
             continue. It is a **cursor, not an offset** — it names where to resume, so a \
             queue created or deleted between two requests cannot make you skip one or see \
             it twice. That is the storage layer's keyset guarantee carried onto the wire \
             rather than re-derived from it, and it is why there is no \"page number\".\n\n\
             A cursor is this server's to issue. Constructing one yourself is not \
             supported and is refused rather than interpreted. An empty result is a normal \
             answer, not an error.",
        )
        .response_with::<200, Json<ListQueuesResponse>, _>(|response| {
            response.description(
                "One page of queues, with a cursor when more remain. `queues` is an empty \
                 list rather than absent when there are none.",
            )
        })
        .response_with::<400, Json<ErrorBody>, _>(|response| {
            response.description(
                "The limit is outside 1–1000, a query parameter is not one this endpoint \
                 takes, or the cursor was not one this server issued.",
            )
        })
        .with(crate::error::needs_a_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(json: &str) -> QueueAttributesBody {
        serde_json::from_str(json).expect("parses")
    }

    #[test]
    fn an_absent_body_is_the_defaults() {
        let attributes = QueueAttributesBody::default()
            .to_attributes()
            .expect("valid");

        assert_eq!(attributes, QueueAttributes::default());
    }

    #[test]
    fn seconds_become_durations() {
        let attributes = body(
            r#"{"visibility_timeout_seconds": 120, "delay_seconds": 5,
                "receive_wait_time_seconds": 20}"#,
        )
        .to_attributes()
        .expect("valid");

        assert_eq!(attributes.visibility_timeout, Duration::from_secs(120));
        assert_eq!(attributes.delay, Duration::from_secs(5));
        assert_eq!(attributes.receive_wait_time, Duration::from_secs(20));
    }

    /// Naming one attribute must not reset the others to zero — they take the defaults,
    /// which is a different thing from "whatever was left over".
    #[test]
    fn an_unnamed_attribute_takes_its_default() {
        let attributes = body(r#"{"delay_seconds": 5}"#)
            .to_attributes()
            .expect("valid");

        assert_eq!(attributes.delay, Duration::from_secs(5));
        assert_eq!(
            attributes.visibility_timeout,
            QueueAttributes::default().visibility_timeout
        );
    }

    #[test]
    fn out_of_range_values_are_refused_rather_than_clamped() {
        for (json, needle) in [
            (r#"{"visibility_timeout_seconds": 43201}"#, "43200"),
            (r#"{"delay_seconds": 901}"#, "900"),
            (r#"{"receive_wait_time_seconds": 21}"#, "20"),
        ] {
            let error = body(json).to_attributes().expect_err("over the limit");

            assert_eq!(error.code(), "invalid_queue_attribute");
            assert!(
                error.message().contains(needle),
                "the message should name the limit: {}",
                error.message()
            );
        }
    }

    /// The maxima are at their limits, not one past them.
    #[test]
    fn the_limits_themselves_are_accepted() {
        body(
            r#"{"visibility_timeout_seconds": 43200, "delay_seconds": 900,
                 "receive_wait_time_seconds": 20}"#,
        )
        .to_attributes()
        .expect("the boundary values are legal");
    }

    /// A dead-letter setting that nothing enforces would be a promise this server cannot
    /// keep, so it is refused rather than accepted and ignored.
    #[test]
    fn attributes_with_nothing_behind_them_are_refused() {
        for json in [
            r#"{"max_receive_count": 5}"#,
            r#"{"dead_letter_queue": "failed"}"#,
        ] {
            serde_json::from_str::<QueueAttributesBody>(json)
                .expect_err("must not be accepted while it does nothing");
        }
    }

    /// One set of limits, two definitions — this is what keeps them one set. The SQS
    /// facade is a dev-dependency, so nothing in the shipped crate depends on it.
    #[test]
    fn the_limits_match_the_sqs_facade() {
        use nexq_api_aws::attributes;

        assert_eq!(
            VISIBILITY_TIMEOUT_MAX_SECONDS,
            attributes::VISIBILITY_TIMEOUT_MAX
        );
        assert_eq!(DELAY_MAX_SECONDS, attributes::DELAY_SECONDS_MAX);
        assert_eq!(RECEIVE_WAIT_MAX_SECONDS, attributes::RECEIVE_WAIT_TIME_MAX);
    }
}
