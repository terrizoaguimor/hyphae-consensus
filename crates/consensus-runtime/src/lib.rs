// SPDX-License-Identifier: Apache-2.0
//! Consensus runtime: the only component allowed to change run state.
//!
//! Models and memory backends are untrusted participants reached through
//! the ports in [`ports`]. Every state change is an event in the
//! append-only log ([`event_log`]) applied through the deterministic
//! state machine ([`state_machine`]). Replaying the log rebuilds state
//! exactly and performs no external effects.

pub mod clock;
pub mod control;
pub mod event_log;
pub mod fakes;
pub mod orchestrator;
pub mod policy;
pub mod ports;
pub mod scenarios;
pub mod sink;
pub mod state_machine;

pub use clock::{Clock, FixedClock, IdGen, SequentialIdGen, SystemClock, UuidIdGen};
pub use control::{CancelReason, RunControl, Stop};
pub use event_log::{EventLog, EventLogError};
pub use orchestrator::{Orchestrator, OrchestratorError, RunLimits, TaskInput};
pub use sink::EventSink;
pub use state_machine::{RunAggregate, TransitionError, MAX_REVISION_ROUNDS};
