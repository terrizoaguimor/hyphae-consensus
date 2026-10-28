// SPDX-License-Identifier: Apache-2.0
//! Live streaming (B3): a registry of bounded per-run broadcast channels
//! that implements the runtime's [`EventPublisher`], and the SSE stream
//! of one run.
//!
//! The stream subscribes first, replays the durable log from the
//! client's `Last-Event-ID` (or `?after`), then follows the channel. It
//! dedupes by sequence; on `Lagged`, on a gap, or when the channel closes
//! it re-reads the durable log and says so with a `resync` frame. Every
//! `consensus` frame names its delivery, `replay` or `live`, so a client
//! can never label a replayed record as live. The stream ends with
//! `stream_end` once the run awaits a human decision or is terminal, and
//! a heartbeat comment keeps the connection alive meanwhile.
//!
//! A record is published only after [`SharedRunLog`] has stored it
//! (fsync'd), so nothing a subscriber sees is ever absent from the log.
//!
//! [`SharedRunLog`]: consensus_runtime::SharedRunLog

use crate::api::types::*;
use crate::limits::StreamPermit;
use crate::public_view::{self, Surface};
use crate::runs::{ReadError, RunManager, RunSnapshot};
use axum::response::sse::Event;
use consensus_domain::*;
use consensus_runtime::{EventPublisher, Owner, RunAggregate};
use futures_core::Stream;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;

/// SSE `retry` hint sent at the start of every stream.
pub const RETRY_MS: u64 = 3000;

/// Per-run broadcast channels. A channel exists while its run is active;
/// publishing to a run without one is a no-op (nobody can be following).
pub struct Broadcasts {
    channels: Mutex<HashMap<RunId, broadcast::Sender<Arc<ConsensusEvent>>>>,
    capacity: usize,
    /// Records published while no channel was registered.
    unrouted: Mutex<u64>,
}

type Channels = HashMap<RunId, broadcast::Sender<Arc<ConsensusEvent>>>;

impl Broadcasts {
    /// `capacity` records are buffered per subscriber before it lags.
    pub fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            channels: Mutex::new(HashMap::new()),
            capacity: capacity.max(1),
            unrouted: Mutex::new(0),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Channels> {
        self.channels
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Opens the channel of a run about to be written.
    pub fn register(&self, run_id: &RunId) {
        let (tx, _rx) = broadcast::channel(self.capacity);
        self.lock().insert(run_id.clone(), tx);
    }

    /// Closes a run's channel: every subscriber sees `Closed` and
    /// reconciles from the durable log.
    pub fn unregister(&self, run_id: &RunId) {
        self.lock().remove(run_id);
    }

    /// A receiver of the run's records, or None when the run is not
    /// active here.
    pub fn subscribe(&self, run_id: &RunId) -> Option<broadcast::Receiver<Arc<ConsensusEvent>>> {
        self.lock().get(run_id).map(|tx| tx.subscribe())
    }

    pub fn active_channels(&self) -> usize {
        self.lock().len()
    }

    pub fn unrouted(&self) -> u64 {
        *self.unrouted.lock().unwrap_or_else(|p| p.into_inner())
    }
}

impl EventPublisher for Broadcasts {
    fn publish(&self, event: &ConsensusEvent) {
        let sent = self
            .lock()
            .get(&event.run_id)
            .map(|tx| tx.send(Arc::new(event.clone())));
        // No receiver at the moment is fine: late subscribers replay.
        if sent.is_none() {
            *self.unrouted.lock().unwrap_or_else(|p| p.into_inner()) += 1;
        }
    }
}

/// What one stream needs.
pub struct StreamParams {
    pub manager: Arc<RunManager>,
    pub run_id: RunId,
    pub owner: Owner,
    pub surface: Surface,
    pub source: SourceProvenance,
    pub heartbeat: Duration,
    /// Frames start after this sequence.
    pub after: u64,
    /// Released when the stream is dropped.
    pub permit: Option<StreamPermit>,
}

fn end_reason(agg: &RunAggregate) -> Option<StreamEndReason> {
    if agg.state.is_terminal() {
        Some(StreamEndReason::Terminal)
    } else if agg.state == RunState::AwaitingHumanApproval {
        Some(StreamEndReason::AwaitingHumanApproval)
    } else {
        None
    }
}

/// `data` of a JSON frame; serialization of these views cannot fail.
fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".into())
}

/// The `consensus` frame for one event, after applying it to `agg`.
pub fn consensus_frame(
    ev: &ConsensusEvent,
    agg: &mut RunAggregate,
    events_so_far: &[ConsensusEvent],
    delivery: Delivery,
    surface: Surface,
    source: SourceProvenance,
) -> ConsensusFrameV1 {
    if let Err(e) = agg.apply(ev) {
        if agg.failure_reason.is_none() {
            agg.failure_reason = Some(format!("replay error at seq {}: {e}", ev.sequence));
            agg.state = RunState::Failed;
        }
    }
    ConsensusFrameV1 {
        event: public_view::event_v1(ev, surface),
        delivery,
        summary: public_view::summary(events_so_far, agg, source, true, surface),
    }
}

fn consensus_event(frame: &ConsensusFrameV1) -> Event {
    Event::default()
        .id(frame.event.sequence.to_string())
        .event("consensus")
        .data(json(frame))
}

fn end_event(reason: StreamEndReason, state: RunState, last: u64) -> Event {
    Event::default()
        .event("stream_end")
        .data(json(&StreamEndV1 {
            reason,
            state,
            last_sequence: last,
        }))
}

fn resync_event(reason: ResyncReason, from: u64) -> Event {
    Event::default().event("resync").data(json(&ResyncV1 {
        reason,
        from_sequence: from,
    }))
}

/// The stream's position: the aggregate and the records applied so far.
struct Cursor {
    manager: Arc<RunManager>,
    run_id: RunId,
    owner: Owner,
    surface: Surface,
    source: SourceProvenance,
    after: u64,
    agg: RunAggregate,
    seen: Vec<ConsensusEvent>,
    last: u64,
}

/// What one re-read of the durable log produced.
struct Replay {
    frames: Vec<Event>,
    ended: Option<StreamEndReason>,
    /// The run could not be read (gone, or no longer owned).
    unreadable: bool,
}

impl Cursor {
    async fn load(&self) -> Result<RunSnapshot, ReadError> {
        let manager = self.manager.clone();
        let id = self.run_id.to_string();
        let owner = self.owner.clone();
        tokio::task::spawn_blocking(move || manager.snapshot(&id, &owner))
            .await
            .map_err(|e| {
                ReadError::Store(consensus_runtime::RunStoreError::Io(std::io::Error::other(
                    e.to_string(),
                )))
            })?
    }

    /// Applies one stored record, emitting a frame unless the client
    /// already has it (sequence at or below `after`).
    fn apply(&mut self, ev: &ConsensusEvent, delivery: Delivery, frames: &mut Vec<Event>) {
        self.seen.push(ev.clone());
        if ev.sequence <= self.after {
            let _ = self.agg.apply(ev);
            self.last = ev.sequence;
            return;
        }
        let frame = consensus_frame(
            ev,
            &mut self.agg,
            &self.seen,
            delivery,
            self.surface,
            self.source,
        );
        self.last = ev.sequence;
        frames.push(consensus_event(&frame));
    }

    /// Re-reads the durable log and returns the frames after `last`.
    async fn replay(&mut self, reason: Option<ResyncReason>) -> Replay {
        let snapshot = match self.load().await {
            Ok(snapshot) => snapshot,
            Err(e) => {
                eprintln!("[sse] run {}: cannot read: {e}", self.run_id);
                return Replay {
                    frames: Vec::new(),
                    ended: None,
                    unreadable: true,
                };
            }
        };
        let mut frames = Vec::new();
        if let Some(reason) = reason {
            frames.push(resync_event(reason, self.last));
        }
        // Everything stored is replayed: a decision recorded after the
        // run rested belongs to its history. Nothing follows a terminal
        // record.
        for ev in &snapshot.events {
            if ev.sequence <= self.last {
                continue;
            }
            self.apply(ev, Delivery::Replay, &mut frames);
            if self.agg.state.is_terminal() {
                break;
            }
        }
        let ended = end_reason(&self.agg);
        Replay {
            frames,
            ended,
            unreadable: false,
        }
    }
}

/// Builds the SSE stream of one run. The caller has already checked the
/// id, the owner and the caps.
pub fn run_stream(params: StreamParams) -> impl Stream<Item = Result<Event, Infallible>> + Send {
    async_stream::stream! {
        let StreamParams { manager, run_id, owner, surface, source, heartbeat, after, permit } = params;
        let _permit = permit;
        yield Ok(Event::default().comment("stream-start").retry(Duration::from_millis(RETRY_MS)));

        // Subscribe before reading, so nothing published meanwhile is missed.
        let subscription = manager.broadcasts().subscribe(&run_id);
        let mut cursor = Cursor {
            agg: RunAggregate::new(run_id.clone()),
            seen: Vec::new(),
            last: 0,
            manager,
            run_id,
            owner,
            surface,
            source,
            after,
        };

        let replay = cursor.replay(None).await;
        for frame in replay.frames {
            yield Ok(frame);
        }
        if replay.unreadable {
            yield Ok(end_event(StreamEndReason::Terminal, cursor.agg.state, cursor.last));
            return;
        }
        if let Some(reason) = replay.ended {
            yield Ok(end_event(reason, cursor.agg.state, cursor.last));
            return;
        }

        let Some(mut receiver) = subscription else {
            // Not active here, yet not at rest: the writer finished between
            // the subscription and the read. The durable log is the truth.
            let replay = cursor.replay(Some(ResyncReason::Closed)).await;
            for frame in replay.frames {
                yield Ok(frame);
            }
            let reason = replay.ended.unwrap_or(StreamEndReason::Terminal);
            yield Ok(end_event(reason, cursor.agg.state, cursor.last));
            return;
        };

        let mut ticker = tokio::time::interval(heartbeat);
        ticker.tick().await; // the first tick completes immediately
        loop {
            let mut resync: Option<ResyncReason> = None;
            let mut closed = false;
            tokio::select! {
                _ = ticker.tick() => {
                    yield Ok(Event::default().comment("heartbeat"));
                }
                received = receiver.recv() => {
                    match received {
                        Ok(ev) => {
                            if ev.sequence <= cursor.last {
                                // Already delivered: the replay and the
                                // channel overlapped.
                            } else if ev.sequence != cursor.last + 1 {
                                resync = Some(ResyncReason::Gap);
                            } else {
                                let mut frames = Vec::new();
                                cursor.apply(&ev, Delivery::Live, &mut frames);
                                for frame in frames {
                                    yield Ok(frame);
                                }
                                if let Some(reason) = end_reason(&cursor.agg) {
                                    yield Ok(end_event(reason, cursor.agg.state, cursor.last));
                                    return;
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {
                            resync = Some(ResyncReason::Lagged);
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            resync = Some(ResyncReason::Closed);
                            closed = true;
                        }
                    }
                }
            }
            if let Some(reason) = resync {
                let replay = cursor.replay(Some(reason)).await;
                for frame in replay.frames {
                    yield Ok(frame);
                }
                if replay.unreadable {
                    yield Ok(end_event(StreamEndReason::Terminal, cursor.agg.state, cursor.last));
                    return;
                }
                if let Some(reason) = replay.ended {
                    yield Ok(end_event(reason, cursor.agg.state, cursor.last));
                    return;
                }
                if closed {
                    // The run left the active set without resting: its
                    // writer failed. Whatever is stored is final.
                    yield Ok(end_event(StreamEndReason::Terminal, cursor.agg.state, cursor.last));
                    return;
                }
            }
        }
    }
}

/// Frames of a finished run as one array, for the contract fixtures and
/// for tests: a full replay followed by `stream_end`.
pub fn replay_frames(
    events: &[ConsensusEvent],
    surface: Surface,
    source: SourceProvenance,
) -> Vec<SseFrameV1> {
    let Some(first) = events.first() else {
        return Vec::new();
    };
    let mut agg = RunAggregate::new(first.run_id.clone());
    let mut seen = Vec::new();
    let mut frames = Vec::new();
    let mut last = 0;
    for ev in events {
        seen.push(ev.clone());
        let frame = consensus_frame(ev, &mut agg, &seen, Delivery::Replay, surface, source);
        last = ev.sequence;
        frames.push(SseFrameV1::Consensus(Box::new(frame)));
        if agg.state.is_terminal() {
            break;
        }
    }
    if let Some(reason) = end_reason(&agg) {
        frames.push(SseFrameV1::StreamEnd(StreamEndV1 {
            reason,
            state: agg.state,
            last_sequence: last,
        }));
    }
    frames
}
