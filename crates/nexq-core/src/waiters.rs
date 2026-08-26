//! The long-poll waiter registry.
//!
//! A consumer asking to wait for a message should be woken when one arrives, not put on
//! a polling loop. This is what makes that possible: one [`Notify`] per queue that has
//! ever been waited on, signalled by the enqueue path.
//!
//! **In-process only, and deliberately so.** A waiter registered here is woken by an
//! enqueue *on this node*. That is exactly right for a single node, and for a cluster it
//! is the reason the design routes a queue's traffic to one primary — a waiter and the
//! enqueue that should wake it have to meet somewhere, and a shared backend that every
//! node polls is the thing this avoids.
//!
//! # Why this cannot lose a wake
//!
//! The hazard is a message arriving between a consumer finding the queue empty and that
//! consumer starting to wait — a wake with nobody registered to receive it, and a
//! consumer that then sleeps through a message already sitting there.
//!
//! It is avoided by ordering rather than by locking: a waiter takes its [`Notified`]
//! future and *enables* it — which is what registers interest — **before** looking at the
//! queue. Anything enqueued from that moment on either finds the waiter registered and
//! wakes it, or has not happened yet and will be seen by the look itself.
//!
//! [`Notified`]: tokio::sync::futures::Notified

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use tokio::sync::Notify;

use crate::model::QueueName;

/// Who is waiting for a message, per queue.
#[derive(Debug, Default)]
pub struct Waiters {
    /// Only holds queues that have actually been waited on, so a deployment whose
    /// consumers never long-poll carries nothing. Entries are dropped with the queue.
    queues: RwLock<HashMap<QueueName, Arc<Notify>>>,
}

impl Waiters {
    pub fn new() -> Self {
        Self::default()
    }

    /// The handle to wait on for a queue, creating it if this is the first waiter.
    ///
    /// Must be called — and the returned handle armed — *before* checking the queue, or
    /// a wake can be lost. See the module docs.
    pub fn register(&self, queue: &QueueName) -> Arc<Notify> {
        // Checked under the read lock first: after the first waiter on a queue, which is
        // the common case, this never takes the write lock.
        if let Some(existing) = self.read().get(queue) {
            return Arc::clone(existing);
        }

        match self.write().entry(queue.clone()) {
            // Someone else got there between the two locks.
            Entry::Occupied(existing) => Arc::clone(existing.get()),
            Entry::Vacant(slot) => Arc::clone(slot.insert(Arc::new(Notify::new()))),
        }
    }

    /// Wake one waiter, because one message arrived.
    ///
    /// One message can only go to one consumer, so waking every waiter would have them
    /// all query the backend for a message all but one of them cannot have. Which
    /// *message* the woken consumer then gets is decided by its own claim, at wake time,
    /// so nothing about the ordering is fixed when a waiter registers.
    ///
    /// A wake with nobody waiting is not wasted: `Notify` keeps a permit, so the next
    /// waiter returns immediately rather than sleeping.
    pub fn notify_one(&self, queue: &QueueName) {
        if let Some(notify) = self.read().get(queue) {
            notify.notify_one();
        }
    }

    /// Wake every waiter on a queue, because something happened to the queue itself
    /// rather than to one message — it was deleted, say.
    ///
    /// Unlike [`Waiters::notify_one`] this stores no permit, since it is about a change
    /// every current waiter should re-examine, not a message someone can take.
    pub fn notify_all(&self, queue: &QueueName) {
        if let Some(notify) = self.read().get(queue) {
            notify.notify_waiters();
        }
    }

    /// Wake every waiter on every queue.
    ///
    /// For a change that is about the process rather than about any one queue — it is
    /// shutting down, or handing its queues to another node — where leaving consumers
    /// blocked would hold that up for as long as their deadlines allow.
    pub fn notify_everything(&self) {
        for notify in self.read().values() {
            notify.notify_waiters();
        }
    }

    /// Drop a queue's entry, since the queue is gone.
    ///
    /// Callers should wake the waiters first: a consumer blocked on a queue that no
    /// longer exists should find that out rather than wait out its full timeout.
    pub fn forget(&self, queue: &QueueName) {
        self.write().remove(queue);
    }

    /// How many queues have an entry. For tests and metrics.
    pub fn tracked_queues(&self) -> usize {
        self.read().len()
    }

    /// A poisoned lock is recovered from rather than reported.
    ///
    /// The opposite of the choice a storage backend makes, and for a reason: this map
    /// holds no invariant a panic could break — a `Notify` is either there or it is not
    /// — so the worst a recovered lock can do is cost a wake, which degrades a long poll
    /// to its timeout. Failing requests instead would turn a lost wake into an outage.
    fn read(&self) -> RwLockReadGuard<'_, HashMap<QueueName, Arc<Notify>>> {
        self.queues
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, HashMap<QueueName, Arc<Notify>>> {
        self.queues
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::timeout;

    use super::*;

    fn name(name: &str) -> QueueName {
        QueueName::new(name).expect("valid queue name")
    }

    /// Long enough that a missed wake fails the test, short enough not to stall it.
    const PATIENCE: Duration = Duration::from_millis(500);

    #[tokio::test]
    async fn nothing_is_tracked_until_someone_waits() {
        let waiters = Waiters::new();

        // Notifying a queue nobody waits on is a no-op, not an entry.
        waiters.notify_one(&name("jobs"));
        waiters.notify_all(&name("jobs"));

        assert_eq!(waiters.tracked_queues(), 0);
    }

    #[tokio::test]
    async fn every_waiter_on_a_queue_shares_one_handle() {
        let waiters = Waiters::new();

        let first = waiters.register(&name("jobs"));
        let second = waiters.register(&name("jobs"));

        assert!(
            Arc::ptr_eq(&first, &second),
            "two waiters on one queue must be woken by the same notification"
        );
        assert_eq!(waiters.tracked_queues(), 1);

        waiters.register(&name("emails"));
        assert_eq!(waiters.tracked_queues(), 2);
    }

    #[tokio::test]
    async fn a_waiter_is_woken_by_a_notification() {
        let waiters = Waiters::new();
        let notify = waiters.register(&name("jobs"));

        let notified = notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        waiters.notify_one(&name("jobs"));

        timeout(PATIENCE, notified)
            .await
            .expect("the waiter should have been woken");
    }

    #[tokio::test]
    async fn a_notification_arriving_before_the_wait_is_not_lost() {
        // The race the ordering rule exists for: the wake happens after the waiter is
        // armed but before it starts awaiting, which must not sleep.
        let waiters = Waiters::new();
        let notify = waiters.register(&name("jobs"));

        let notified = notify.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();

        waiters.notify_one(&name("jobs"));
        // ... a claim attempt would happen here, finding nothing ...

        timeout(PATIENCE, notified)
            .await
            .expect("an armed waiter must not miss a wake that already happened");
    }

    #[tokio::test]
    async fn a_notification_with_nobody_waiting_is_kept_for_the_next_waiter() {
        let waiters = Waiters::new();
        let notify = waiters.register(&name("jobs"));

        // Nobody is armed yet.
        waiters.notify_one(&name("jobs"));

        timeout(PATIENCE, notify.notified())
            .await
            .expect("the permit should let the next waiter straight through");
    }

    #[tokio::test]
    async fn one_message_wakes_one_waiter() {
        let waiters = Waiters::new();
        let notify = waiters.register(&name("jobs"));

        let first = notify.notified();
        let second = notify.notified();
        tokio::pin!(first, second);
        first.as_mut().enable();
        second.as_mut().enable();

        waiters.notify_one(&name("jobs"));

        timeout(PATIENCE, first).await.expect("the first waiter");
        timeout(PATIENCE, second)
            .await
            .expect_err("the second must keep waiting: there was only one message");
    }

    #[tokio::test]
    async fn a_change_to_the_queue_wakes_everyone() {
        let waiters = Waiters::new();
        let notify = waiters.register(&name("jobs"));

        let first = notify.notified();
        let second = notify.notified();
        tokio::pin!(first, second);
        first.as_mut().enable();
        second.as_mut().enable();

        waiters.notify_all(&name("jobs"));

        timeout(PATIENCE, first).await.expect("the first waiter");
        timeout(PATIENCE, second).await.expect("and the second");
    }

    #[tokio::test]
    async fn waiters_on_one_queue_are_not_woken_by_another() {
        let waiters = Waiters::new();
        let jobs = waiters.register(&name("jobs"));
        let emails = waiters.register(&name("emails"));

        let waiting = emails.notified();
        tokio::pin!(waiting);
        waiting.as_mut().enable();

        waiters.notify_one(&name("jobs"));

        timeout(PATIENCE, waiting)
            .await
            .expect_err("a message for jobs must not wake a consumer of emails");
        drop(jobs);
    }

    #[tokio::test]
    async fn forgetting_a_queue_drops_its_entry() {
        let waiters = Waiters::new();
        waiters.register(&name("jobs"));

        waiters.forget(&name("jobs"));

        assert_eq!(waiters.tracked_queues(), 0);
    }

    #[tokio::test]
    async fn a_waiter_already_woken_is_unaffected_by_forgetting_the_queue() {
        // What a queue deletion looks like: wake everyone, then drop the entry. A
        // waiter holding the old handle still gets its wake.
        let waiters = Waiters::new();
        let notify = waiters.register(&name("jobs"));

        let waiting = notify.notified();
        tokio::pin!(waiting);
        waiting.as_mut().enable();

        waiters.notify_all(&name("jobs"));
        waiters.forget(&name("jobs"));

        timeout(PATIENCE, waiting)
            .await
            .expect("the wake was already delivered");
        assert_eq!(waiters.tracked_queues(), 0);
    }
}
