// SPDX-License-Identifier: Apache-2.0
//! Cooperative stop signal for one run: cancellation by its owner,
//! process shutdown, and an optional wall-clock deadline.
//!
//! The orchestrator checks a [`RunControl`] before every model call
//! (propose, critique, evaluate, revise), before every memory recall,
//! and before issuing the recommendation. A call already in progress is
//! not interrupted: its outcome is handled as usual first (a returned
//! document is recorded, since its tokens were spent), and the run stops
//! at the next checkpoint with `RunFailed` and the stage of
//! [`Stop::stage`]. No new event type is involved, so record shapes and
//! the schema version are unchanged.

use std::sync::{Arc, OnceLock};
use std::time::Instant;

/// Why a run was asked to stop early.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelReason {
    /// The run's owner cancelled it.
    Owner,
    /// The process is shutting down.
    Shutdown,
}

/// What a tripped [`RunControl`] makes the orchestrator record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// Cancelled by the run's owner: stage `cancelled`.
    Cancelled,
    /// Process shutdown: stage `shutdown`.
    Shutdown,
    /// The deadline was reached: stage `deadline`.
    Deadline,
}

impl Stop {
    /// The `RunFailed.stage` recorded for this stop.
    pub fn stage(self) -> &'static str {
        match self {
            Stop::Cancelled => "cancelled",
            Stop::Shutdown => "shutdown",
            Stop::Deadline => "deadline",
        }
    }

    /// Short `RunFailed.reason` naming the checkpoint that saw the stop.
    pub fn reason(self, checkpoint: &str) -> String {
        match self {
            Stop::Cancelled => format!("cancelled by the run's owner before the {checkpoint}"),
            Stop::Shutdown => format!("stopped by shutdown before the {checkpoint}"),
            Stop::Deadline => format!("run deadline passed before the {checkpoint}"),
        }
    }
}

/// Cloneable stop handle for one run. Every clone shares one set-once
/// cancellation flag (the role of an `Arc<AtomicBool>`, plus the
/// reason), so a handle kept elsewhere can stop a run that another
/// thread is deliberating. The deadline is fixed at construction, so
/// all clones carry the same one.
#[derive(Debug, Clone, Default)]
pub struct RunControl {
    cancelled: Arc<OnceLock<CancelReason>>,
    deadline: Option<Instant>,
}

impl RunControl {
    /// No deadline; trips only when [`cancel`](Self::cancel) is called.
    pub fn new() -> Self {
        Self::default()
    }

    /// A control that also trips once `deadline` is reached.
    pub fn with_deadline(deadline: Instant) -> Self {
        Self {
            cancelled: Arc::default(),
            deadline: Some(deadline),
        }
    }

    /// The deadline this control was built with, if any.
    pub fn deadline(&self) -> Option<Instant> {
        self.deadline
    }

    /// Asks the run to stop at its next checkpoint. The first request
    /// wins: a later one changes nothing and returns `false`.
    pub fn cancel(&self, reason: CancelReason) -> bool {
        self.cancelled.set(reason).is_ok()
    }

    /// The cancellation request, if one was made.
    pub fn cancel_reason(&self) -> Option<CancelReason> {
        self.cancelled.get().copied()
    }

    /// The stop to record now, if any. A cancellation request outranks
    /// the deadline.
    pub fn check(&self) -> Option<Stop> {
        match self.cancel_reason() {
            Some(CancelReason::Owner) => Some(Stop::Cancelled),
            Some(CancelReason::Shutdown) => Some(Stop::Shutdown),
            None => self
                .deadline
                .filter(|deadline| Instant::now() >= *deadline)
                .map(|_| Stop::Deadline),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn clones_share_one_flag_and_the_first_request_wins() {
        let control = RunControl::new();
        let handle = control.clone();
        assert_eq!(control.check(), None);
        assert!(handle.cancel(CancelReason::Shutdown));
        assert!(!control.cancel(CancelReason::Owner), "a second request");
        assert_eq!(control.cancel_reason(), Some(CancelReason::Shutdown));
        assert_eq!(control.check(), Some(Stop::Shutdown));
        assert_eq!(handle.check(), Some(Stop::Shutdown));
    }

    #[test]
    fn deadline_trips_once_reached_and_cancellation_outranks_it() {
        let later = RunControl::with_deadline(Instant::now() + Duration::from_secs(3600));
        assert_eq!(later.check(), None);

        let reached = RunControl::with_deadline(Instant::now());
        assert_eq!(reached.check(), Some(Stop::Deadline));
        assert_eq!(reached.clone().deadline(), reached.deadline());
        reached.cancel(CancelReason::Owner);
        assert_eq!(reached.check(), Some(Stop::Cancelled));
    }
}
