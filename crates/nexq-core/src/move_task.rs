//! Redrive tasks: moving a queue's messages to another queue, in the background.
//!
//! Dead-lettering runs in two directions, and they need very different machinery. Moving
//! messages *into* a dead-letter queue is a sweep — see [`crate::dead_letter`] — because
//! nobody asked for it and it must happen whether or not anyone is looking. Moving them
//! back *out* is an operator's deliberate act on a queue that may hold a great many
//! messages, so it is a **task**: something that starts, reports progress while it runs,
//! and can be called off.
//!
//! This module is the bookkeeping only. The loop that does the moving lives on the engine,
//! because moving a message means reading one queue and writing another and those may be
//! on different backends — see [`crate::store::Store::claim_for_move`].
//!
//! **In-process, and lost on restart.** A task is a tokio task plus this registry, so a
//! node that goes down mid-redrive leaves its messages where they were rather than
//! half-moved: each individual message is moved by an enqueue-then-acknowledge pair, so
//! the worst a crash leaves behind is one duplicate. What does not survive is the *task* —
//! its progress and its handle — and an operator restarting a redrive is a far better
//! outcome than one silently resumed under a handle nobody is watching.

use std::fmt;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::{Duration, SystemTime};

use uuid::Uuid;

use crate::model::QueueName;

/// How long a message being moved is held invisible.
///
/// It only needs to outlast one enqueue-and-acknowledge pair, so this is generous. Its job
/// is to bound how long a message stays hidden when the mover dies between the two: the
/// hold lapses, the message becomes claimable again, and a later pass finds it.
pub const MOVE_HOLD: Duration = Duration::from_secs(30);

/// How many messages a redrive takes out of the source at a time.
///
/// One round trip per message would make an unthrottled redrive as slow as the storage
/// latency times the queue depth. Cancellation is still checked per *message*, so a large
/// batch does not make a task slow to stop.
pub const MOVE_BATCH: usize = 100;

/// How many finished tasks are kept so they can still be listed.
///
/// Finished tasks are history, and history has to be bounded or a long-lived server
/// accumulates it forever. The oldest are dropped first, which is the right way round: the
/// task an operator is asking about is the one they just started.
pub const RETAINED_MOVE_TASKS: usize = 100;

/// A redrive task's identifier, as handed to a client.
///
/// Opaque, and a UUID rather than anything derived from the queues involved: two redrives
/// of the same queue are different tasks, and a handle that collided would let a cancel
/// stop the wrong one. SQS's own task handle is a base64 blob carrying the source ARN,
/// which a client is equally not meant to read.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MoveTaskId(String);

/// Where a redrive task has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveTaskStatus {
    /// Still moving messages.
    Running,

    /// A cancel has been asked for and the task has not yet noticed.
    ///
    /// A real state rather than a formality: cancellation is cooperative — the task
    /// finishes the message it is moving rather than abandoning it half-moved — so there is
    /// a moment where the answer to "is it stopped" is honestly "not yet".
    Cancelling,

    /// Stopped because it was cancelled. Whatever it had already moved stays moved.
    Cancelled,

    /// The source had nothing left to move.
    Completed,

    /// Something went wrong; [`MoveTask::failure`] says what.
    Failed,
}

/// A redrive task, as a caller sees it.
///
/// A snapshot: it was true when it was taken, and a running task has moved more messages
/// by the time it is read. That is inherent rather than a shortcoming — the alternative is
/// holding a lock across a caller's response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveTask {
    pub id: MoveTaskId,

    /// The queue being emptied — a dead-letter queue, in the case this exists for.
    pub source: QueueName,

    /// Where the messages are going.
    pub destination: QueueName,

    /// Ceiling on how fast messages are moved, or `None` for as fast as the backend
    /// allows. The point of throttling a redrive is that the destination has live
    /// consumers, and dropping a dead-letter queue's worth of messages onto them at once
    /// is how a redrive turns into a second outage.
    pub max_messages_per_second: Option<u32>,

    pub status: MoveTaskStatus,
    pub started_at: SystemTime,

    /// When it stopped, whyever it stopped, or `None` while it is still running.
    pub finished_at: Option<SystemTime>,

    /// How many messages have been moved so far. Exact at the instant it was read.
    pub messages_moved: u64,

    /// How many the source held when the task started.
    ///
    /// A snapshot taken once, so it is what the task set out to do rather than what is
    /// left — which is what makes it useful next to [`MoveTask::messages_moved`]. It can be
    /// wrong in both directions by the time the task ends: producers may add more, and
    /// messages a consumer holds are not moved at all.
    pub messages_to_move: u64,

    /// Why the task failed, for a [`MoveTaskStatus::Failed`] task.
    pub failure: Option<String>,
}

/// Every redrive task this node knows about, running or finished.
#[derive(Debug, Default)]
pub struct MoveTasks {
    /// Oldest first, which is what makes pruning the oldest a truncation from the front.
    tasks: RwLock<Vec<Arc<TaskState>>>,
}

/// One task's live state, shared between the worker moving messages and the requests
/// asking how far it has got.
#[derive(Debug)]
pub(crate) struct TaskState {
    id: MoveTaskId,
    source: QueueName,
    destination: QueueName,
    max_messages_per_second: Option<u32>,
    started_at: SystemTime,
    messages_to_move: u64,

    /// Everything that changes while the task runs, under one lock so a snapshot cannot
    /// show a status and a count from two different moments.
    progress: RwLock<Progress>,
}

#[derive(Debug)]
struct Progress {
    status: MoveTaskStatus,
    messages_moved: u64,
    finished_at: Option<SystemTime>,
    failure: Option<String>,
}

impl MoveTaskId {
    /// Mint an identifier for a new task.
    #[allow(clippy::new_without_default)] // Each call returns a different value.
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// Adopt an identifier from a client, so it can be looked up.
    ///
    /// Not validated: an id this server never issued simply names no task, which is the
    /// same answer as one that has been pruned. A caller cannot tell those apart and does
    /// not need to.
    pub fn from_client(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for MoveTaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl MoveTaskStatus {
    /// Whether the task is still doing something, or expects to.
    ///
    /// [`MoveTaskStatus::Cancelling`] counts as active: the task has not stopped, and
    /// treating it as finished is how a second redrive gets started alongside the first.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Running | Self::Cancelling)
    }

    /// The name SQS gives this status, which is also what the REST facade reports so the
    /// two do not disagree about the word for the same state.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "RUNNING",
            Self::Cancelling => "CANCELLING",
            Self::Cancelled => "CANCELLED",
            Self::Completed => "COMPLETED",
            Self::Failed => "FAILED",
        }
    }
}

impl fmt::Display for MoveTaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl MoveTasks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a task and return its state, for the worker to report through.
    pub(crate) fn start(
        &self,
        source: QueueName,
        destination: QueueName,
        max_messages_per_second: Option<u32>,
        messages_to_move: u64,
    ) -> Arc<TaskState> {
        let state = Arc::new(TaskState {
            id: MoveTaskId::new(),
            source,
            destination,
            max_messages_per_second,
            started_at: SystemTime::now(),
            messages_to_move,
            progress: RwLock::new(Progress {
                status: MoveTaskStatus::Running,
                messages_moved: 0,
                finished_at: None,
                failure: None,
            }),
        });

        let mut tasks = self.write();
        tasks.push(Arc::clone(&state));
        prune(&mut tasks);

        state
    }

    /// Whether a task is already moving this queue's messages.
    ///
    /// Two redrives of one queue would race for every message and report half the progress
    /// each, so the second is refused rather than allowed to interleave.
    pub fn is_moving(&self, source: &QueueName) -> bool {
        self.read()
            .iter()
            .any(|state| &state.source == source && state.is_active())
    }

    /// Every task, newest first, optionally only those draining one queue.
    ///
    /// Newest first because the task an operator is asking about is almost always the one
    /// they just started — the opposite of the order tasks are stored in, which is the
    /// order that makes pruning cheap.
    pub fn list(&self, source: Option<&QueueName>) -> Vec<MoveTask> {
        self.read()
            .iter()
            .rev()
            .filter(|state| source.is_none_or(|wanted| &state.source == wanted))
            .map(|state| state.snapshot())
            .collect()
    }

    /// One task, or `None` if this node never had it or has since pruned it.
    pub fn get(&self, id: &MoveTaskId) -> Option<MoveTask> {
        self.read()
            .iter()
            .find(|state| &state.id == id)
            .map(|state| state.snapshot())
    }

    /// Ask a task to stop, returning it as it now is.
    ///
    /// `None` when no such task. Asking a task that has already stopped to stop is
    /// reported by the returned status rather than by an error here: whether that is worth
    /// refusing is the caller's decision, and it differs by protocol.
    ///
    /// Cooperative: this sets [`MoveTaskStatus::Cancelling`] and the task stops at its next
    /// message boundary. It does not abandon the message in flight, because a message
    /// abandoned between its enqueue and its acknowledgement is a duplicate for no reason.
    pub fn cancel(&self, id: &MoveTaskId) -> Option<MoveTask> {
        let tasks = self.read();
        let state = tasks.iter().find(|state| &state.id == id)?;

        {
            let mut progress = state.write();
            if progress.status == MoveTaskStatus::Running {
                progress.status = MoveTaskStatus::Cancelling;
            }
        }

        Some(state.snapshot())
    }

    /// How many tasks are held, running and finished. For tests and metrics.
    pub fn len(&self) -> usize {
        self.read().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn read(&self) -> RwLockReadGuard<'_, Vec<Arc<TaskState>>> {
        self.tasks.read().unwrap_or_else(PoisonRecovery::recover)
    }

    fn write(&self) -> RwLockWriteGuard<'_, Vec<Arc<TaskState>>> {
        self.tasks.write().unwrap_or_else(PoisonRecovery::recover)
    }
}

/// Drop the oldest finished tasks once there are more than [`RETAINED_MOVE_TASKS`].
///
/// Only finished ones: a running task is not history, and forgetting it would lose the
/// handle that cancels it. So the registry can exceed the cap, but only by however many
/// redrives are actually in flight — which is one per source queue.
fn prune(tasks: &mut Vec<Arc<TaskState>>) {
    if tasks.len() <= RETAINED_MOVE_TASKS {
        return;
    }

    let mut over = tasks.len() - RETAINED_MOVE_TASKS;
    tasks.retain(|state| {
        if over > 0 && !state.is_active() {
            over -= 1;
            return false;
        }
        true
    });
}

impl TaskState {
    pub(crate) fn id(&self) -> &MoveTaskId {
        &self.id
    }

    pub(crate) fn source(&self) -> &QueueName {
        &self.source
    }

    pub(crate) fn destination(&self) -> &QueueName {
        &self.destination
    }

    /// Whether the task has not stopped. Cheaper than a snapshot, which is what the
    /// hot paths — pruning, and the check that refuses a second redrive — need.
    fn is_active(&self) -> bool {
        self.read().status.is_active()
    }

    /// How long to pause between messages to stay under the configured rate, or `None`
    /// for an unthrottled task.
    pub(crate) fn pace(&self) -> Option<Duration> {
        self.max_messages_per_second
            .filter(|rate| *rate > 0)
            .map(|rate| Duration::from_secs_f64(1.0 / f64::from(rate)))
    }

    /// How many messages to take out of the source at once.
    ///
    /// [`MOVE_BATCH`] for an unthrottled task, because one round trip per message would
    /// make a redrive as slow as the storage latency times the queue depth.
    ///
    /// For a throttled one, few enough that the *last* message of a batch is still held
    /// when its turn comes. A batch is claimed all at once and moved one at a time, so one
    /// message per second with a batch of a hundred would have the hundredth message's
    /// [`MOVE_HOLD`] lapse some seventy seconds before it was reached — another consumer
    /// could claim it in the meantime, and the acknowledgement that finishes the move would
    /// fail. Half the hold rather than all of it, leaving room for the moves themselves to
    /// take time.
    pub(crate) fn batch_size(&self) -> usize {
        let Some(pace) = self.pace() else {
            return MOVE_BATCH;
        };

        let fits = MOVE_HOLD.as_secs_f64() / 2.0 / pace.as_secs_f64();

        // At least one, or a very slow rate would claim nothing and never finish.
        (fits as usize).clamp(1, MOVE_BATCH)
    }

    /// Whether a cancel has been asked for, and if so, record that the task has stopped.
    ///
    /// The observation and the transition are one step so that a task cannot notice a
    /// cancel and then fail to record it.
    pub(crate) fn stop_if_cancelled(&self) -> bool {
        let mut progress = self.write();
        if progress.status != MoveTaskStatus::Cancelling {
            return false;
        }

        progress.status = MoveTaskStatus::Cancelled;
        progress.finished_at = Some(SystemTime::now());

        true
    }

    /// One more message moved.
    pub(crate) fn record_moved(&self) {
        self.write().messages_moved += 1;
    }

    /// The source had nothing left.
    pub(crate) fn complete(&self) {
        self.finish(MoveTaskStatus::Completed, None);
    }

    /// Something went wrong, and the task is over.
    pub(crate) fn fail(&self, why: impl fmt::Display) {
        self.finish(MoveTaskStatus::Failed, Some(why.to_string()));
    }

    fn finish(&self, status: MoveTaskStatus, failure: Option<String>) {
        let mut progress = self.write();

        // A cancel that arrived while the last message was in flight already recorded the
        // outcome, and it is the truthful one: the task stopped because it was told to.
        if !progress.status.is_active() {
            return;
        }

        progress.status = status;
        progress.failure = failure;
        progress.finished_at = Some(SystemTime::now());
    }

    pub(crate) fn snapshot(&self) -> MoveTask {
        let progress = self.read();

        MoveTask {
            id: self.id.clone(),
            source: self.source.clone(),
            destination: self.destination.clone(),
            max_messages_per_second: self.max_messages_per_second,
            status: progress.status,
            started_at: self.started_at,
            finished_at: progress.finished_at,
            messages_moved: progress.messages_moved,
            messages_to_move: self.messages_to_move,
            failure: progress.failure.clone(),
        }
    }

    fn read(&self) -> RwLockReadGuard<'_, Progress> {
        self.progress.read().unwrap_or_else(PoisonRecovery::recover)
    }

    fn write(&self) -> RwLockWriteGuard<'_, Progress> {
        self.progress.write().unwrap_or_else(PoisonRecovery::recover)
    }
}

/// Take the guard out of a poisoned lock rather than failing.
///
/// Unlike the storage backends, which report a poisoned lock as a backend failure because
/// the data behind it may be half-written, everything under these locks is a progress
/// counter and a status. A panic cannot leave either in a state that is wrong to read, and
/// making a redrive unreportable — or uncancellable — because something unrelated panicked
/// would be the worse outcome.
trait PoisonRecovery<T> {
    fn recover(self) -> T;
}

impl<T> PoisonRecovery<T> for std::sync::PoisonError<T> {
    fn recover(self) -> T {
        self.into_inner()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(name: &str) -> QueueName {
        QueueName::new(name).expect("valid queue name")
    }

    fn tasks() -> MoveTasks {
        MoveTasks::new()
    }

    fn start(registry: &MoveTasks) -> Arc<TaskState> {
        registry.start(name("jobs_dlq"), name("jobs"), None, 3)
    }

    #[test]
    fn a_new_task_is_running_and_has_moved_nothing() {
        let registry = tasks();
        let state = start(&registry);

        let task = state.snapshot();
        assert_eq!(task.status, MoveTaskStatus::Running);
        assert_eq!(task.messages_moved, 0);
        assert_eq!(task.messages_to_move, 3);
        assert_eq!(task.finished_at, None);
        assert_eq!(task.failure, None);
        assert_eq!(registry.get(&task.id).expect("registered"), task);
    }

    #[test]
    fn ids_are_unique_and_opaque() {
        let registry = tasks();

        assert_ne!(start(&registry).id, start(&registry).id);
    }

    #[test]
    fn an_id_this_server_never_issued_names_no_task() {
        assert_eq!(tasks().get(&MoveTaskId::from_client("nope")), None);
        assert_eq!(tasks().cancel(&MoveTaskId::from_client("nope")), None);
    }

    #[test]
    fn progress_is_visible_while_the_task_runs() {
        let registry = tasks();
        let state = start(&registry);

        state.record_moved();
        state.record_moved();

        let task = registry.get(state.id()).expect("registered");
        assert_eq!(task.messages_moved, 2);
        assert_eq!(task.status, MoveTaskStatus::Running, "still going");
    }

    #[test]
    fn completing_stops_the_task_and_stamps_when() {
        let registry = tasks();
        let state = start(&registry);

        state.complete();

        let task = registry.get(state.id()).expect("registered");
        assert_eq!(task.status, MoveTaskStatus::Completed);
        assert!(task.finished_at.is_some());
        assert!(!task.status.is_active());
    }

    #[test]
    fn failing_keeps_the_reason() {
        let registry = tasks();
        let state = start(&registry);

        state.fail("the destination is gone");

        let task = registry.get(state.id()).expect("registered");
        assert_eq!(task.status, MoveTaskStatus::Failed);
        assert_eq!(task.failure.as_deref(), Some("the destination is gone"));
    }

    /// Cancellation is two steps, and both are observable: asked for, then acted on. A
    /// design collapsing them would report a task as stopped while it was still moving a
    /// message.
    #[test]
    fn cancelling_asks_first_and_the_task_stops_when_it_notices() {
        let registry = tasks();
        let state = start(&registry);

        let asked = registry.cancel(state.id()).expect("registered");
        assert_eq!(asked.status, MoveTaskStatus::Cancelling);
        assert!(asked.status.is_active(), "it has not stopped yet");
        assert_eq!(asked.finished_at, None);

        assert!(state.stop_if_cancelled(), "the task notices");

        let stopped = registry.get(state.id()).expect("registered");
        assert_eq!(stopped.status, MoveTaskStatus::Cancelled);
        assert!(stopped.finished_at.is_some());
    }

    #[test]
    fn a_task_that_was_not_cancelled_keeps_going() {
        let state = start(&tasks());

        assert!(!state.stop_if_cancelled());
        assert_eq!(state.snapshot().status, MoveTaskStatus::Running);
    }

    /// The race worth pinning: a cancel arrives while the last message is in flight, so
    /// the task would otherwise report `Completed` for a run that was called off.
    #[test]
    fn a_cancel_that_lands_during_the_last_move_wins() {
        let registry = tasks();
        let state = start(&registry);

        registry.cancel(state.id()).expect("registered");
        assert!(state.stop_if_cancelled());
        state.complete();

        assert_eq!(
            registry.get(state.id()).expect("registered").status,
            MoveTaskStatus::Cancelled,
            "it stopped because it was told to, not because it ran out"
        );
    }

    #[test]
    fn cancelling_a_finished_task_leaves_it_finished() {
        let registry = tasks();
        let state = start(&registry);
        state.complete();

        let task = registry.cancel(state.id()).expect("registered");

        assert_eq!(task.status, MoveTaskStatus::Completed);
    }

    #[test]
    fn one_queue_can_only_be_drained_by_one_task_at_a_time() {
        let registry = tasks();
        assert!(!registry.is_moving(&name("jobs_dlq")));

        let state = start(&registry);
        assert!(registry.is_moving(&name("jobs_dlq")));
        assert!(
            !registry.is_moving(&name("other_dlq")),
            "a different queue is unaffected"
        );

        registry.cancel(state.id());
        assert!(
            registry.is_moving(&name("jobs_dlq")),
            "asked to stop is not stopped"
        );

        state.stop_if_cancelled();
        assert!(!registry.is_moving(&name("jobs_dlq")));
    }

    #[test]
    fn tasks_are_listed_newest_first_and_can_be_filtered_by_source() {
        let registry = tasks();
        let first = registry.start(name("a_dlq"), name("a"), None, 0);
        let second = registry.start(name("b_dlq"), name("b"), None, 0);

        let all = registry.list(None);
        assert_eq!(
            all.iter().map(|task| &task.id).collect::<Vec<_>>(),
            [second.id(), first.id()],
            "newest first"
        );

        let filtered = registry.list(Some(&name("a_dlq")));
        assert_eq!(filtered.len(), 1);
        assert_eq!(&filtered[0].id, first.id());

        assert!(registry.list(Some(&name("nope"))).is_empty());
    }

    #[test]
    fn a_pace_is_only_set_for_a_throttled_task() {
        let registry = tasks();

        assert_eq!(start(&registry).pace(), None, "unthrottled");
        assert_eq!(
            registry
                .start(name("a_dlq"), name("a"), Some(4), 0)
                .pace(),
            Some(Duration::from_millis(250))
        );
        assert_eq!(
            registry
                .start(name("b_dlq"), name("b"), Some(0), 0)
                .pace(),
            None,
            "zero is not a rate anyone can be held to, so it is no limit"
        );
    }

    /// A throttled batch has to be small enough that its last message is still held when
    /// the task reaches it, or the acknowledgement that finishes the move fails.
    #[test]
    fn a_throttled_batch_is_bounded_by_how_long_a_message_stays_held() {
        let registry = tasks();

        assert_eq!(start(&registry).batch_size(), MOVE_BATCH, "unthrottled");

        for (rate, expected) in [
            // 15 seconds of headroom at one per second.
            (1, 15),
            (2, 30),
            // Fast enough that the batch cap binds first.
            (1_000, MOVE_BATCH),
        ] {
            let state = registry.start(
                name(&format!("q{rate}")),
                name("destination"),
                Some(rate),
                0,
            );

            assert_eq!(state.batch_size(), expected, "{rate} per second");
        }

        // The lower clamp is unreachable through config — the slowest rate expressible is
        // one message per second, and half of a 30-second hold fits fifteen of those — so
        // it is there to keep the arithmetic honest if either constant changes, not
        // because a caller can provoke it.
    }

    /// History is bounded, and it is the *finished* tasks that get dropped — forgetting a
    /// running one would lose the handle that cancels it.
    #[test]
    fn finished_tasks_are_pruned_and_running_ones_are_not() {
        let registry = tasks();

        // One running task per source queue, since a second on the same source would be
        // refused by the engine anyway.
        let running = registry.start(name("kept"), name("destination"), None, 0);

        for index in 0..RETAINED_MOVE_TASKS + 10 {
            registry
                .start(name(&format!("q{index}")), name("destination"), None, 0)
                .complete();
        }

        assert!(
            registry.len() <= RETAINED_MOVE_TASKS + 1,
            "history must be bounded, got {}",
            registry.len()
        );
        assert!(
            registry.get(running.id()).is_some(),
            "a running task must not be pruned"
        );
        assert!(
            registry.list(None).first().is_some_and(|task| task
                .source
                .as_str()
                .starts_with('q')),
            "the newest is kept"
        );
    }
}
