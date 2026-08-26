//! The domain model: queues, messages, and the handles that refer to them.
//!
//! These types are shared by the engine, every storage backend, and every protocol
//! facade, so they describe queueing itself rather than any one wire format. Where a
//! rule comes from SQS compatibility rather than from queueing, the doc comment says so.
//!
//! Two ideas worth separating up front:
//!
//! - A [`Message`] is the durable item. It exists from enqueue until it is deleted.
//! - A [`ClaimedMessage`] is a message handed to one consumer for a limited time,
//!   identified by a [`ReceiptHandle`]. The claim expires; the message does not.

use std::fmt;
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use uuid::Uuid;

/// Longest queue name accepted. From SQS, so that a name valid here is valid there.
pub const MAX_QUEUE_NAME_LEN: usize = 80;

/// Largest message body accepted, 256 KiB. Also from SQS.
pub const MAX_BODY_BYTES: usize = 256 * 1024;

/// Default time a claimed message stays invisible to other consumers. SQS's default.
pub const DEFAULT_VISIBILITY_TIMEOUT: Duration = Duration::from_secs(30);

/// A queue's name.
///
/// Validated on construction, so an invalid name cannot reach a backend. The rules are
/// SQS's: 1 to [`MAX_QUEUE_NAME_LEN`] characters, each alphanumeric, `-`, or `_`.
///
/// Note that this rejects names ending in `.fifo`, since `.` is not an accepted
/// character. That is deliberate: FIFO queues carry ordering and deduplication
/// semantics NexQ does not implement, and accepting the name would imply otherwise.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QueueName(String);

/// Why a queue name was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidQueueName {
    Empty,
    TooLong {
        len: usize,
    },
    /// Held as a `char` so the message can point at the offending character.
    InvalidCharacter(char),
}

/// A queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Queue {
    pub name: QueueName,
    pub created_at: SystemTime,
    pub attributes: QueueAttributes,
}

/// The knobs that change how a queue behaves.
///
/// Defaults match SQS's, so a client that sets nothing gets what it would get from AWS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueAttributes {
    /// How long a claimed message stays invisible before being redelivered.
    pub visibility_timeout: Duration,

    /// How long a newly sent message waits before becoming visible.
    pub delay: Duration,

    /// Default wait for a receive that finds nothing — the long-poll duration used
    /// when a request does not ask for its own.
    pub receive_wait_time: Duration,

    /// Deliveries after which a message goes to the dead-letter queue. `None` means
    /// redeliver forever.
    pub max_receive_count: Option<u32>,

    /// Where exhausted messages go. A dead-letter queue is an ordinary queue, so this
    /// is just another name.
    pub dead_letter_queue: Option<QueueName>,
}

/// A message's server-assigned identifier.
///
/// A UUID, because SQS message ids are UUIDs and clients may parse them as such. Held
/// as a string rather than a [`Uuid`] so a backend that mints its own ids — a document
/// store, say — can use them without a conversion that could fail.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MessageId(String);

/// The token a consumer presents to act on the claim it was given.
///
/// Opaque by design: what it encodes is the storage backend's business, and a client
/// must not be able to construct one. A handle refers to *one claim*, not to the
/// message — once the claim ends, whether by deletion or expiry, the handle is spent,
/// and a redelivery of the same message comes with a new one.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ReceiptHandle(String);

/// A message's priority. Higher is served first; the default is the middle of the road.
///
/// NexQ's own concept, not SQS's — the SQS facade has no way to express it, so messages
/// sent through that facade all arrive at [`Priority::DEFAULT`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Priority(i32);

/// A durable message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: MessageId,
    pub body: String,
    pub priority: Priority,

    /// When the message was accepted, not when it becomes visible.
    pub enqueued_at: SystemTime,

    /// How many times this message has been delivered, including the delivery in
    /// progress. Surfaced to SQS clients as `ApproximateReceiveCount`.
    pub receive_count: u32,

    /// When the message was first delivered, or `None` if it never has been.
    pub first_received_at: Option<SystemTime>,
}

/// A message delivered to one consumer, and the claim that came with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimedMessage {
    pub message: Message,
    pub receipt: ReceiptHandle,

    /// When the claim lapses and the message becomes deliverable again.
    pub claim_expires_at: SystemTime,
}

impl QueueName {
    /// Validate and wrap a queue name.
    pub fn new(name: impl Into<String>) -> Result<Self, InvalidQueueName> {
        let name = name.into();

        if name.is_empty() {
            return Err(InvalidQueueName::Empty);
        }

        // Counted in characters rather than bytes: the accepted set is ASCII, so the
        // two agree for any valid name, and this reports the length a user would count
        // for an invalid one.
        let len = name.chars().count();
        if len > MAX_QUEUE_NAME_LEN {
            return Err(InvalidQueueName::TooLong { len });
        }

        if let Some(character) = name
            .chars()
            .find(|c| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_'))
        {
            return Err(InvalidQueueName::InvalidCharacter(character));
        }

        Ok(Self(name))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for QueueName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for QueueName {
    type Err = InvalidQueueName;

    fn from_str(name: &str) -> Result<Self, Self::Err> {
        Self::new(name)
    }
}

impl fmt::Display for InvalidQueueName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "a queue name must not be empty"),
            Self::TooLong { len } => write!(
                f,
                "a queue name must be at most {MAX_QUEUE_NAME_LEN} characters, got {len}"
            ),
            Self::InvalidCharacter(character) => write!(
                f,
                "a queue name may only contain letters, digits, hyphens, and \
                 underscores, but contains {character:?}"
            ),
        }
    }
}

impl std::error::Error for InvalidQueueName {}

impl Queue {
    /// A queue with default attributes, created now.
    pub fn new(name: QueueName) -> Self {
        Self {
            name,
            created_at: SystemTime::now(),
            attributes: QueueAttributes::default(),
        }
    }
}

impl Default for QueueAttributes {
    fn default() -> Self {
        Self {
            visibility_timeout: DEFAULT_VISIBILITY_TIMEOUT,
            delay: Duration::ZERO,
            receive_wait_time: Duration::ZERO,
            max_receive_count: None,
            dead_letter_queue: None,
        }
    }
}

impl MessageId {
    /// Mint a new identifier.
    #[allow(clippy::new_without_default)] // Each call returns a different value.
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Adopt an identifier a backend produced.
    pub fn from_backend(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl ReceiptHandle {
    /// Mint a handle for a new claim.
    ///
    /// Unguessable on purpose: presenting a handle is what authorizes deleting or
    /// extending a claim, so one must not be derivable from a message id.
    #[allow(clippy::new_without_default)] // Each call returns a different value.
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Adopt a handle a backend encoded itself.
    pub fn from_backend(handle: impl Into<String>) -> Self {
        Self(handle.into())
    }

    /// The handle as sent to, and received from, a client.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Prints as a placeholder. A handle is a bearer token for a claim, so it should not
/// end up in a log line by accident; [`ReceiptHandle::as_str`] is the way to read it.
impl fmt::Debug for ReceiptHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReceiptHandle(..)")
    }
}

impl Priority {
    /// What a message gets when nothing sets a priority.
    pub const DEFAULT: Self = Self(0);

    /// Served before everything else.
    pub const MAX: Self = Self(i32::MAX);

    /// Served after everything else.
    pub const MIN: Self = Self(i32::MIN);

    pub fn new(priority: i32) -> Self {
        Self(priority)
    }

    pub fn get(self) -> i32 {
        self.0
    }
}

impl Default for Priority {
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl fmt::Display for Priority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<i32> for Priority {
    fn from(priority: i32) -> Self {
        Self::new(priority)
    }
}

impl Message {
    /// A message as first accepted: never delivered, so no receive count and no first
    /// delivery time.
    pub fn new(body: impl Into<String>, priority: Priority) -> Self {
        Self {
            id: MessageId::new(),
            body: body.into(),
            priority,
            enqueued_at: SystemTime::now(),
            receive_count: 0,
            first_received_at: None,
        }
    }

    /// Whether the body is within [`MAX_BODY_BYTES`].
    ///
    /// Measured in bytes, since that is what the limit is about, and a multi-byte
    /// character therefore counts for more than one.
    pub fn body_within_limit(&self) -> bool {
        self.body.len() <= MAX_BODY_BYTES
    }
}

/// Milliseconds since the Unix epoch, which is how SQS reports every timestamp.
///
/// Saturates at zero for a time before the epoch rather than failing: a clock set that
/// far wrong is not something a queue can report its way out of.
pub fn epoch_millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|since| u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_queue_names_are_accepted() {
        for name in ["jobs", "a", "Jobs-2", "jobs_dlq", "0", &"q".repeat(80)] {
            QueueName::new(name).unwrap_or_else(|error| panic!("{name:?}: {error}"));
        }
    }

    #[test]
    fn an_empty_queue_name_is_rejected() {
        assert_eq!(QueueName::new(""), Err(InvalidQueueName::Empty));
    }

    #[test]
    fn an_overlong_queue_name_is_rejected() {
        let name = "q".repeat(MAX_QUEUE_NAME_LEN + 1);

        assert_eq!(
            QueueName::new(name),
            Err(InvalidQueueName::TooLong {
                len: MAX_QUEUE_NAME_LEN + 1
            })
        );
    }

    #[test]
    fn a_queue_name_may_only_use_the_accepted_characters() {
        for (name, offender) in [
            ("with space", ' '),
            ("with/slash", '/'),
            ("with.dot", '.'),
            ("emoji🎉", '🎉'),
        ] {
            assert_eq!(
                QueueName::new(name),
                Err(InvalidQueueName::InvalidCharacter(offender)),
                "{name}"
            );
        }
    }

    #[test]
    fn a_fifo_queue_name_is_rejected_since_fifo_is_not_supported() {
        // Falls out of the character rules, but worth pinning: silently accepting the
        // name would promise ordering guarantees that do not exist.
        assert_eq!(
            QueueName::new("jobs.fifo"),
            Err(InvalidQueueName::InvalidCharacter('.'))
        );
    }

    #[test]
    fn a_queue_name_parses_from_a_string() {
        let name: QueueName = "jobs".parse().expect("valid");

        assert_eq!(name.as_str(), "jobs");
        assert_eq!(name.to_string(), "jobs");
        "not valid".parse::<QueueName>().expect_err("has a space");
    }

    #[test]
    fn default_queue_attributes_match_sqs() {
        let attributes = QueueAttributes::default();

        assert_eq!(attributes.visibility_timeout, Duration::from_secs(30));
        assert_eq!(attributes.delay, Duration::ZERO);
        assert_eq!(attributes.receive_wait_time, Duration::ZERO);
        assert_eq!(attributes.max_receive_count, None);
        assert_eq!(attributes.dead_letter_queue, None);
    }

    #[test]
    fn a_new_queue_uses_default_attributes() {
        let queue = Queue::new(QueueName::new("jobs").expect("valid"));

        assert_eq!(queue.name.as_str(), "jobs");
        assert_eq!(queue.attributes, QueueAttributes::default());
    }

    #[test]
    fn message_ids_are_unique_uuids() {
        let first = MessageId::new();
        let second = MessageId::new();

        assert_ne!(first, second);
        assert_eq!(first.as_str().len(), 36, "hyphenated uuid");
        Uuid::parse_str(first.as_str()).expect("a uuid");
    }

    #[test]
    fn a_backend_may_supply_its_own_message_id() {
        assert_eq!(MessageId::from_backend("es-doc-7").as_str(), "es-doc-7");
    }

    #[test]
    fn receipt_handles_are_unique_and_do_not_print_themselves() {
        let handle = ReceiptHandle::new();

        assert_ne!(handle, ReceiptHandle::new());
        assert_eq!(format!("{handle:?}"), "ReceiptHandle(..)");
        assert!(
            !format!("{handle:?}").contains(handle.as_str()),
            "a handle authorizes deleting a message, so it should not be logged by \
             accident"
        );
    }

    #[test]
    fn priorities_order_highest_first() {
        let mut priorities = [
            Priority::new(-5),
            Priority::MAX,
            Priority::DEFAULT,
            Priority::MIN,
            Priority::new(10),
        ];
        priorities.sort();

        assert_eq!(
            priorities,
            [
                Priority::MIN,
                Priority::new(-5),
                Priority::DEFAULT,
                Priority::new(10),
                Priority::MAX,
            ]
        );
        assert_eq!(Priority::default(), Priority::DEFAULT);
        assert_eq!(Priority::DEFAULT.get(), 0);
        assert_eq!(Priority::from(3), Priority::new(3));
    }

    #[test]
    fn a_new_message_has_never_been_delivered() {
        let message = Message::new("hello", Priority::DEFAULT);

        assert_eq!(message.body, "hello");
        assert_eq!(message.receive_count, 0);
        assert_eq!(message.first_received_at, None);
        assert!(message.body_within_limit());
    }

    #[test]
    fn the_body_limit_is_counted_in_bytes() {
        let at_limit = Message::new("x".repeat(MAX_BODY_BYTES), Priority::DEFAULT);
        assert!(at_limit.body_within_limit());

        let over_by_one = Message::new("x".repeat(MAX_BODY_BYTES + 1), Priority::DEFAULT);
        assert!(!over_by_one.body_within_limit());

        // Two bytes per character, so half as many characters reach the limit.
        let multibyte = Message::new("é".repeat(MAX_BODY_BYTES / 2 + 1), Priority::DEFAULT);
        assert!(!multibyte.body_within_limit());
    }

    #[test]
    fn timestamps_convert_to_epoch_millis() {
        assert_eq!(epoch_millis(UNIX_EPOCH), 0);
        assert_eq!(
            epoch_millis(UNIX_EPOCH + Duration::from_millis(1_760_000_000_123)),
            1_760_000_000_123
        );
        // Before the epoch, rather than panicking on a backwards clock.
        assert_eq!(epoch_millis(UNIX_EPOCH - Duration::from_secs(1)), 0);
    }
}
