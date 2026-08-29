//! The dead-letter sweep: what makes a redrive policy true.
//!
//! A message becomes eligible for its dead-letter queue when its last claim **lapses** —
//! a deadline passing, not anything a client did. Nothing observes that on its own, which
//! leaves two possible designs and one of them is wrong:
//!
//! - **A timer per claim.** Correct, and its cost scales with *messages*: every claim
//!   would arm a timer at its `claim_expires_at`, and in the happy path the consumer
//!   acknowledges long before then, so almost every one of those timers would fire to find
//!   nothing to do.
//! - **A periodic sweep.** Its cost scales with *queues*, which is the smaller number by
//!   orders of magnitude and the one that does not grow with traffic. What it costs is
//!   latency: a message may sit exhausted for up to one interval before it moves.
//!
//! The sweep, therefore — and the latency is the right thing to trade away, because a
//! message that has already failed every delivery it was allowed is not urgent. What is
//! not acceptable is the third option, which is to dead-letter messages only when someone
//! happens to call receive: a queue whose consumers have stopped calling is precisely the
//! queue whose messages need moving, so that design fails exactly when it matters.

use std::fmt;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::{MissedTickBehavior, interval};
use tracing::{debug, info, warn};

use crate::engine::{Engine, EngineError};
use crate::model::QueueName;

/// How often the sweep runs by default.
///
/// The upper bound on how long an exhausted message waits before it reaches its
/// dead-letter queue. Fifteen seconds because that is short next to any redrive policy
/// worth having — a policy allowing three deliveries with the default thirty-second
/// visibility timeout takes a minute and a half to exhaust a message — and long enough
/// that the sweep is not a meaningful load on a deployment where nothing is failing.
pub const DEFAULT_SWEEP_INTERVAL: Duration = Duration::from_secs(15);

/// What one pass of the sweep did.
#[derive(Debug, Default)]
pub struct Sweep {
    /// How many messages reached a dead-letter queue.
    pub moved: u64,

    /// How many queues were looked at.
    pub queues: usize,

    /// The queues the sweep could not finish, and why.
    ///
    /// Collected rather than returned as an error, because one unreachable dead-letter
    /// queue must not stop every *other* queue's messages from moving. The usual cause is
    /// a dead-letter queue that has been deleted since the policy naming it was set; the
    /// messages stay where they are, so nothing is lost by trying again next pass.
    pub failures: Vec<(QueueName, EngineError)>,
}

impl Sweep {
    /// Whether anything happened worth reporting.
    pub fn is_quiet(&self) -> bool {
        self.moved == 0 && self.failures.is_empty()
    }
}

impl fmt::Display for Sweep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} message(s) dead-lettered across {} queue(s), {} failure(s)",
            self.moved,
            self.queues,
            self.failures.len()
        )
    }
}

/// Runs [`Engine::sweep_dead_letters`] on a timer until told to stop.
///
/// Owns an `Arc<Engine>` and nothing else, so a deployment that wants a different interval
/// changes one number and a test that wants to sweep once calls the engine directly.
#[derive(Debug)]
pub struct Sweeper {
    engine: Arc<Engine>,
    interval: Duration,
}

impl Sweeper {
    /// A sweeper on the default interval.
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            interval: DEFAULT_SWEEP_INTERVAL,
        }
    }

    /// The same, at a chosen interval. Mostly for tests, which cannot wait fifteen
    /// seconds to find out whether the sweep works.
    pub fn every(engine: Arc<Engine>, interval: Duration) -> Self {
        Self { engine, interval }
    }

    /// Sweep until `shutdown` resolves.
    ///
    /// The first pass is one interval in, not immediate: a server that has just started has
    /// nothing whose claim has lapsed, so sweeping at startup is guaranteed to find
    /// nothing.
    ///
    /// A pass that overruns its interval does **not** queue up the ticks it missed —
    /// stacking sweeps behind a slow backend would turn one slow pass into a backlog of
    /// them. The next tick lands one interval after the pass finishes.
    pub async fn run<S>(self, shutdown: S)
    where
        S: Future<Output = ()>,
    {
        let mut ticker = interval(self.interval);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
        // The first tick of a tokio interval fires immediately; consumed here so the first
        // real pass is one interval in.
        ticker.tick().await;

        info!(
            interval = ?self.interval,
            "dead-letter sweep running"
        );

        let shutdown = std::pin::pin!(shutdown);
        let mut shutdown = shutdown;

        loop {
            tokio::select! {
                _ = ticker.tick() => self.sweep_once().await,
                () = &mut shutdown => {
                    debug!("dead-letter sweep stopping");
                    return;
                }
            }
        }
    }

    /// One pass, logged.
    ///
    /// Quiet when nothing happened, which is almost always: a log line every fifteen
    /// seconds saying nothing failed would bury the one that says something did.
    async fn sweep_once(&self) {
        let sweep = match self.engine.sweep_dead_letters().await {
            Ok(sweep) => sweep,
            Err(error) => {
                warn!("dead-letter sweep could not list queues: {error}");
                return;
            }
        };

        for (queue, error) in &sweep.failures {
            warn!(queue = %queue, "could not dead-letter this queue's messages: {error}");
        }

        if !sweep.is_quiet() {
            info!("dead-letter sweep: {sweep}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sweep_that_did_nothing_is_quiet() {
        assert!(Sweep::default().is_quiet());
        assert!(
            Sweep {
                queues: 12,
                ..Sweep::default()
            }
            .is_quiet(),
            "looking at queues and finding nothing is not news"
        );
        assert!(
            !Sweep {
                moved: 1,
                queues: 1,
                failures: Vec::new(),
            }
            .is_quiet()
        );
    }

    #[test]
    fn a_sweep_reports_what_it_did() {
        let sweep = Sweep {
            moved: 3,
            queues: 7,
            failures: vec![(
                QueueName::new("jobs").expect("valid"),
                EngineError::DeadLetterQueueNotFound(QueueName::new("gone").expect("valid")),
            )],
        };

        assert_eq!(
            sweep.to_string(),
            "3 message(s) dead-lettered across 7 queue(s), 1 failure(s)"
        );
        assert!(!sweep.is_quiet());
    }
}
