// SPDX-License-Identifier: Apache-2.0
//! USD ledger tests (plan B9, ABUSE-1, ABUSE-4): caps under concurrency,
//! crash safety, no refunds, trips, lanes, pacing, exact money, locking.

use chrono::{DateTime, TimeDelta, Utc};
use consensus_api::live::budget::{
    AttemptEstimate, AttemptMeter, BudgetConfig, Lane, Ledger, LedgerClock, LedgerError,
    LedgerOptions, Outcome, RecordedOutcome, Refusal, ReservationId, TripReason, Usage, LOCK_FILE,
    SUMMARY_FILE,
};
use consensus_api::live::money::{Ppm, Usd};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A fresh directory for one test, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "hyphae-consensus-live-ledger-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A settable clock shared with the ledger.
#[derive(Clone)]
struct TestClock(Arc<Mutex<DateTime<Utc>>>);

impl TestClock {
    fn at(iso: &str) -> Self {
        Self(Arc::new(Mutex::new(parse(iso))))
    }

    fn set(&self, iso: &str) {
        *self.0.lock().unwrap() = parse(iso);
    }

    fn advance(&self, secs: i64) {
        let mut t = self.0.lock().unwrap();
        *t += TimeDelta::seconds(secs);
    }

    fn clock(&self) -> LedgerClock {
        let shared = self.0.clone();
        Arc::new(move || *shared.lock().unwrap())
    }
}

fn parse(iso: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(iso)
        .unwrap()
        .with_timezone(&Utc)
}

fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap()
}

/// The owner's defaults: $10/day, $60 lifetime, $15 judge slice (and the
/// same per day), one slot, 6 admissions per hour, 25% burst, 70k/16k/8k
/// tokens, $4/$20 per million.
fn owner_cfg() -> BudgetConfig {
    BudgetConfig {
        daily_cap: usd("10.00"),
        lifetime_cap: usd("60.00"),
        judge_slice: usd("15.00"),
        judge_daily_cap: usd("15.00"),
        max_concurrent: 1,
        hourly_admissions: 6,
        burst_fraction: Ppm::parse("0.25").unwrap(),
        run_token_cap: 70_000,
        max_completion_tokens: 16_000,
        input_cap_tokens: 8_000,
        price_input_per_m: usd("4.00"),
        price_output_per_m: usd("20.00"),
        kill_switch_file: None,
    }
}

/// The owner's prices with pacing and slots out of the way, so a test can
/// exercise one rule at a time.
fn unpaced_cfg() -> BudgetConfig {
    BudgetConfig {
        max_concurrent: 100,
        hourly_admissions: 1_000,
        burst_fraction: Ppm::ONE,
        ..owner_cfg()
    }
}

fn fast_options() -> LedgerOptions {
    LedgerOptions {
        lock_wait: Duration::from_millis(300),
        lock_poll: Duration::from_millis(5),
    }
}

fn open(dir: &Path, cfg: BudgetConfig, clock: &TestClock) -> Ledger {
    Ledger::open_with(dir, cfg, clock.clock(), fast_options()).unwrap()
}

/// An attempt with an empty body and the full output bound.
fn full_attempt() -> AttemptEstimate {
    AttemptEstimate {
        request_bytes: 0,
        max_completion_tokens: 16_000,
    }
}

/// Records of one `type` in today's day file.
fn records(dir: &Path, day: &str, kind: &str) -> Vec<serde_json::Value> {
    let text = std::fs::read_to_string(dir.join(format!("{day}.jsonl"))).unwrap();
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .filter(|v| v["type"] == kind)
        .collect()
}

const WORST_CASE: &str = "1.88"; // (70000 + 16000 + 8000) tokens × $20/M
const ATTEMPT_EST: &str = "0.324096"; // 1024 × $4/M + 16000 × $20/M

#[test]
fn fifty_concurrent_reservations_never_exceed_the_cap() {
    let dir = TempDir::new("concurrent");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    // The default lock wait (10 s): fifty threads serialize through the
    // file lock, each admitted one doing two fsyncs and a rename while it
    // holds it, and the claim under test is the cap, not disk timing.
    let ledger = Ledger::open_with(
        dir.path(),
        unpaced_cfg(),
        clock.clock(),
        LedgerOptions::default(),
    )
    .unwrap();
    let admitted = Arc::new(Mutex::new(Vec::new()));
    let refused = Arc::new(Mutex::new(Vec::new()));
    let handles: Vec<_> = (0..50)
        .map(|_| {
            let ledger = ledger.clone();
            let admitted = admitted.clone();
            let refused = refused.clone();
            std::thread::spawn(move || match ledger.reserve(Lane::Public) {
                Ok(r) => admitted.lock().unwrap().push(r),
                Err(e) => refused.lock().unwrap().push(e),
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let admitted = admitted.lock().unwrap();
    let refused = refused.lock().unwrap();
    // $10 holds five $1.88 reservations ($9.40); a sixth would be $11.28.
    assert_eq!(admitted.len(), 5, "{refused:?}");
    assert_eq!(refused.len(), 45);
    assert!(refused
        .iter()
        .all(|r| matches!(r, Refusal::DailyBudget { .. })));
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.daily_committed, usd("9.40"));
    assert!(snap.daily_committed <= snap.daily_cap);
    assert_eq!(snap.daily_remaining, usd("0.60"));
    assert_eq!(snap.unsettled_reservations, 5);
    assert_eq!(
        records(dir.path(), "2026-10-27", "reservation").len(),
        5,
        "a refused admission writes nothing"
    );
}

#[test]
fn crash_between_reserve_and_settle_counts_worst_case_after_reopen() {
    let dir = TempDir::new("crash");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    {
        let ledger = open(dir.path(), owner_cfg(), &clock);
        let reservation = ledger.reserve(Lane::Public).unwrap();
        let _draw = ledger.draw(&reservation, full_attempt()).unwrap();
        // The process dies here: nothing is settled or released.
    }
    let ledger = open(dir.path(), owner_cfg(), &clock);
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.unsettled_reservations, 1);
    assert_eq!(snap.daily_committed, usd(WORST_CASE));
    assert_eq!(snap.lifetime_committed, usd(WORST_CASE));
    assert_eq!(snap.lifetime_remaining, usd("58.12"));
    // The open draw is money spent for sure.
    assert_eq!(snap.lifetime_spent, usd(ATTEMPT_EST));
    assert_eq!(snap.active_public, 0, "slots are per process");
    // A restart does not revive the slot-less reservation; new runs admit.
    assert!(ledger.reserve(Lane::Public).is_ok());
}

#[test]
fn unknown_outcomes_charge_the_full_estimate_and_never_refund() {
    let dir = TempDir::new("no-refund");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), unpaced_cfg(), &clock);
    let reservation = ledger.reserve(Lane::Public).unwrap();
    let mut expected = Usd::ZERO;
    for outcome in [
        Outcome::Timeout,
        Outcome::TransportAfterSend,
        Outcome::Status(500),
        Outcome::Status(503),
        Outcome::SuccessWithoutUsage,
    ] {
        let draw = ledger.draw(&reservation, full_attempt()).unwrap();
        let est = draw.estimate();
        assert_eq!(est, usd(ATTEMPT_EST));
        let settled = ledger.settle(draw, outcome.clone()).unwrap();
        assert_eq!(settled.charged, est, "{outcome:?}");
        assert_eq!(settled.outcome, RecordedOutcome::Unknown);
        assert!(settled.tripped.is_none());
        expected = expected.saturating_add(est);
    }
    let released = ledger.release(&reservation).unwrap();
    assert_eq!(released.spent, expected);
    assert_eq!(released.spent, usd("1.620480"));
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.lifetime_spent, expected);
    assert_eq!(
        snap.daily_committed, expected,
        "released: counts what it spent"
    );
    assert_eq!(snap.unsettled_reservations, 0);
}

#[test]
fn retries_take_one_draw_per_attempt() {
    let dir = TempDir::new("retries");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), unpaced_cfg(), &clock);
    let reservation = ledger.reserve(Lane::Public).unwrap();
    let meter: Box<dyn AttemptMeter> = Box::new(ledger.meter(&reservation));

    let first = meter.draw(full_attempt()).unwrap();
    assert_eq!(first.attempt(), 1);
    let s1 = meter.settle(first, Outcome::Status(500)).unwrap();
    assert_eq!(s1.charged, usd(ATTEMPT_EST), "5xx: full estimate");

    let second = meter.draw(full_attempt()).unwrap();
    assert_eq!(second.attempt(), 2);
    let s2 = meter.settle(second, Outcome::Status(429)).unwrap();
    assert_eq!(s2.charged, Usd::ZERO, "429: zero");
    assert_eq!(s2.outcome, RecordedOutcome::Zero);

    let third = meter.draw(full_attempt()).unwrap();
    assert_eq!(third.attempt(), 3);
    // As Meta reports it: completion_tokens inclusive of the reasoning
    // breakdown (R01-reference, 2026-10-27).
    let usage = Usage::from_chat_usage(2_688, 10_692, Some(6_000));
    let s3 = meter.settle(third, Outcome::Success(usage)).unwrap();
    assert_eq!(s3.charged, usd("0.224592"), "2688 × $4/M + 10692 × $20/M");
    assert_eq!(
        s3.outcome,
        RecordedOutcome::Usage {
            input: 2_688,
            completion: 10_692,
            reasoning_included: 6_000
        }
    );
    assert!(
        s3.tripped.is_none(),
        "10,692 inclusive tokens fit the bound"
    );

    let draws = records(dir.path(), "2026-10-27", "draw");
    assert_eq!(draws.len(), 3);
    assert_eq!(
        draws
            .iter()
            .map(|d| d["attempt"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    assert_eq!(records(dir.path(), "2026-10-27", "settlement").len(), 3);
    let released = ledger.release(&reservation).unwrap();
    assert_eq!(released.spent, usd("0.548688"));
}

#[test]
fn completion_over_max_trips_and_the_trip_survives_reopen() {
    let dir = TempDir::new("trip-completion");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), unpaced_cfg(), &clock);
    let reservation = ledger.reserve(Lane::Public).unwrap();
    let draw = ledger.draw(&reservation, full_attempt()).unwrap();
    let settled = ledger
        .settle(
            draw,
            Outcome::Success(Usage {
                input: 10,
                completion: 16_001,
                reasoning_included: 1,
            }),
        )
        .unwrap();
    let trip = settled.tripped.expect("tripped");
    assert_eq!(trip.code, TripReason::OutputBound);
    let reason = trip.detail;
    assert!(
        reason.starts_with("completion_tokens_exceeded_max"),
        "{reason}"
    );
    match ledger.reserve(Lane::Public) {
        Err(Refusal::Tripped { code, reason: r }) => {
            assert_eq!(code, TripReason::OutputBound);
            assert_eq!(r, reason);
        }
        other => panic!("{other:?}"),
    }
    assert!(
        matches!(
            ledger.draw(&reservation, full_attempt()),
            Err(Refusal::Tripped { .. })
        ),
        "in-flight runs stop at their next call"
    );
    assert!(matches!(
        ledger.reserve(Lane::Judge),
        Err(Refusal::Tripped { .. })
    ));
    drop(reservation);
    drop(ledger);

    let reopened = open(dir.path(), unpaced_cfg(), &clock);
    let snap = reopened.snapshot().unwrap();
    assert!(snap.tripped);
    assert_eq!(snap.trip_code, Some(TripReason::OutputBound));
    assert_eq!(snap.trip_reason.as_deref(), Some(reason.as_str()));
    assert!(matches!(
        reopened.reserve(Lane::Public),
        Err(Refusal::Tripped { .. })
    ));
    // Only the operator's explicit reset clears it.
    reopened.reset("operator").unwrap();
    assert!(!reopened.snapshot().unwrap().tripped);
    assert!(reopened.reserve(Lane::Public).is_ok());
}

#[test]
fn reasoning_included_changes_neither_the_charge_nor_the_trip() {
    let dir = TempDir::new("reasoning");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), unpaced_cfg(), &clock);
    let reservation = ledger.reserve(Lane::Public).unwrap();

    // The same completion total with and without a reasoning breakdown
    // costs the same: reasoning is inside completion_tokens, not added.
    let plain = Usage::from_chat_usage(2_688, 10_692, None);
    let with_reasoning = Usage::from_chat_usage(2_688, 10_692, Some(6_000));
    assert_eq!(ledger.config().charge(&plain), usd("0.224592"));
    assert_eq!(ledger.config().charge(&with_reasoning), usd("0.224592"));
    assert_eq!(
        plain.completion_tokens(),
        with_reasoning.completion_tokens()
    );

    let draw = ledger.draw(&reservation, full_attempt()).unwrap();
    let s1 = ledger.settle(draw, Outcome::Success(plain)).unwrap();
    assert_eq!(s1.charged, usd("0.224592"));
    assert!(s1.tripped.is_none());

    // 10,692 completion tokens of which 6,000 are reasoning: a naive sum
    // would read 16,692 > 16,000 and trip; the inclusive total does not.
    let draw = ledger.draw(&reservation, full_attempt()).unwrap();
    let s2 = ledger
        .settle(draw, Outcome::Success(with_reasoning))
        .unwrap();
    assert_eq!(s2.charged, usd("0.224592"));
    assert!(s2.tripped.is_none(), "{:?}", s2.tripped);
    assert_eq!(
        s2.outcome,
        RecordedOutcome::Usage {
            input: 2_688,
            completion: 10_692,
            reasoning_included: 6_000
        }
    );

    // Exactly at the bound, all of it reasoning: still no trip.
    let draw = ledger.draw(&reservation, full_attempt()).unwrap();
    let s3 = ledger
        .settle(
            draw,
            Outcome::Success(Usage::from_chat_usage(10, 16_000, Some(16_000))),
        )
        .unwrap();
    assert!(s3.tripped.is_none());

    // A breakdown larger than its total is not a usage report at all: the
    // attempt is charged at its full estimate, like a 2xx without usage.
    let draw = ledger.draw(&reservation, full_attempt()).unwrap();
    let s4 = ledger
        .settle(
            draw,
            Outcome::Success(Usage::from_chat_usage(10, 100, Some(101))),
        )
        .unwrap();
    assert_eq!(s4.charged, usd(ATTEMPT_EST));
    assert_eq!(s4.outcome, RecordedOutcome::Unknown);
    assert!(s4.tripped.is_none());
    let settlements = records(dir.path(), "2026-10-27", "settlement");
    assert_eq!(settlements.len(), 4);
    assert_eq!(settlements[3]["outcome"]["kind"], "unknown");
    assert!(settlements[3]["detail"]
        .as_str()
        .unwrap()
        .contains("inconsistent usage"));
    assert!(ledger.release(&reservation).is_ok());
}

#[test]
fn a_charge_above_the_attempt_draw_trips() {
    let dir = TempDir::new("trip-charge");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), unpaced_cfg(), &clock);
    let reservation = ledger.reserve(Lane::Public).unwrap();
    let draw = ledger.draw(&reservation, full_attempt()).unwrap();
    // Input far above the 1,024-token bound: the bound did not hold.
    let settled = ledger
        .settle(
            draw,
            Outcome::Success(Usage {
                input: 100_000,
                completion: 100,
                reasoning_included: 0,
            }),
        )
        .unwrap();
    assert_eq!(settled.charged, usd("0.402"), "the real charge is kept");
    let trip = settled.tripped.expect("tripped");
    assert_eq!(trip.code, TripReason::ChargeBound);
    assert!(
        trip.detail.starts_with("charge_exceeded_attempt_draw"),
        "{}",
        trip.detail
    );
}

#[test]
fn provider_key_refusals_trip_and_other_no_retry_statuses_charge_zero() {
    for status in [400u16, 404, 413, 429] {
        let dir = TempDir::new(&format!("status-{status}"));
        let clock = TestClock::at("2026-10-27T12:00:00Z");
        let ledger = open(dir.path(), unpaced_cfg(), &clock);
        let reservation = ledger.reserve(Lane::Public).unwrap();
        let draw = ledger.draw(&reservation, full_attempt()).unwrap();
        let settled = ledger.settle(draw, Outcome::Status(status)).unwrap();
        assert_eq!(settled.charged, Usd::ZERO, "{status}");
        assert!(settled.tripped.is_none(), "{status}");
    }
    for status in [401u16, 402, 403] {
        let dir = TempDir::new(&format!("status-{status}"));
        let clock = TestClock::at("2026-10-27T12:00:00Z");
        let ledger = open(dir.path(), unpaced_cfg(), &clock);
        let reservation = ledger.reserve(Lane::Public).unwrap();
        let draw = ledger.draw(&reservation, full_attempt()).unwrap();
        let settled = ledger.settle(draw, Outcome::Status(status)).unwrap();
        assert_eq!(settled.charged, Usd::ZERO, "{status}");
        let trip = settled.tripped.expect("tripped");
        assert_eq!(trip.code, TripReason::ProviderRefusedKey);
        assert_eq!(trip.detail, format!("provider_refused_key: http {status}"));
        assert!(matches!(
            ledger.reserve(Lane::Public),
            Err(Refusal::Tripped { .. })
        ));
        // The visitor-facing projection carries the coarse code only: no
        // upstream status, no detail text, no paths.
        let snap = ledger.snapshot().unwrap();
        assert_eq!(snap.trip_code, Some(TripReason::ProviderRefusedKey));
        assert!(snap.trip_reason.as_deref().unwrap().contains("http"));
        let public = serde_json::to_string(&snap.public()).unwrap();
        assert!(!public.contains("http"), "{public}");
        assert!(!public.contains(&status.to_string()), "{public}");
        assert!(!public.contains('/') && !public.contains('\\'), "{public}");
        assert!(
            public.contains("\"trip_code\":\"provider_refused_key\""),
            "{public}"
        );
        assert!(public.contains("\"tripped\":true"), "{public}");
    }
}

#[test]
fn lifetime_cap_reached_trips_across_reopen() {
    // Lifetime = one run's worst case, so spending a whole reservation
    // reaches the cap. Ten draws whose estimates sum to exactly $1.88:
    // 9 × (1024 × $4/M + 10000 × $20/M) + (1024 × $4/M + 1952 × $20/M).
    let cfg = BudgetConfig {
        daily_cap: usd(WORST_CASE),
        lifetime_cap: usd(WORST_CASE),
        judge_slice: Usd::ZERO,
        judge_daily_cap: Usd::ZERO,
        ..unpaced_cfg()
    };
    let dir = TempDir::new("lifetime");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), cfg.clone(), &clock);
    let reservation = ledger.reserve(Lane::Public).unwrap();
    let mut spent = Usd::ZERO;
    let mut tripped = None;
    for mct in [10_000u64; 9].into_iter().chain([1_952]) {
        let draw = ledger
            .draw(
                &reservation,
                AttemptEstimate {
                    request_bytes: 0,
                    max_completion_tokens: mct,
                },
            )
            .unwrap();
        spent = spent.saturating_add(draw.estimate());
        tripped = ledger.settle(draw, Outcome::Timeout).unwrap().tripped;
    }
    assert_eq!(spent, usd(WORST_CASE));
    let trip = tripped.expect("tripped");
    assert_eq!(trip.code, TripReason::LifetimeCap);
    assert_eq!(trip.detail, "lifetime_cap_reached");
    drop(reservation);
    drop(ledger);

    let reopened = open(dir.path(), cfg, &clock);
    let snap = reopened.snapshot().unwrap();
    assert!(snap.tripped);
    assert_eq!(snap.trip_code, Some(TripReason::LifetimeCap));
    assert_eq!(snap.lifetime_spent, usd(WORST_CASE));
    assert_eq!(snap.lifetime_remaining, Usd::ZERO);
    assert!(matches!(
        reopened.reserve(Lane::Public),
        Err(Refusal::Tripped { .. })
    ));
    // Even after a reset, nothing fits the lifetime cap.
    reopened.reset("operator").unwrap();
    assert!(matches!(
        reopened.reserve(Lane::Public),
        Err(Refusal::LifetimeBudget)
    ));
}

#[test]
fn kill_switch_file_refuses_everything_until_an_operator_reset() {
    let dir = TempDir::new("kill-switch");
    let switch = dir.path().join("kill-switch");
    let cfg = BudgetConfig {
        kill_switch_file: Some(switch.clone()),
        ..unpaced_cfg()
    };
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), cfg.clone(), &clock);
    let in_flight = ledger.reserve(Lane::Public).unwrap();

    std::fs::write(&switch, b"stop\n").unwrap();
    match ledger.reserve(Lane::Public) {
        Err(Refusal::Tripped { code, reason }) => {
            assert_eq!(code, TripReason::KillSwitch);
            assert_eq!(reason, "kill_switch_file_present");
        }
        other => panic!("{other:?}"),
    }
    assert!(
        matches!(
            ledger.draw(&in_flight, full_attempt()),
            Err(Refusal::Tripped { .. })
        ),
        "the in-flight run stops at its next call"
    );
    assert!(matches!(
        ledger.reserve(Lane::Operator),
        Err(Refusal::Tripped { .. })
    ));

    // Removing the file is not enough: the trip is persisted.
    std::fs::remove_file(&switch).unwrap();
    assert!(matches!(
        ledger.reserve(Lane::Public),
        Err(Refusal::Tripped { .. })
    ));
    drop(ledger);
    let reopened = open(dir.path(), cfg, &clock);
    assert!(reopened.snapshot().unwrap().tripped);
    reopened.reset("operator").unwrap();
    assert!(reopened.reserve(Lane::Public).is_ok());

    // A reset with the file still present trips again at once.
    std::fs::write(&switch, b"stop\n").unwrap();
    reopened.reset("operator").unwrap();
    assert!(matches!(
        reopened.reserve(Lane::Public),
        Err(Refusal::Tripped { .. })
    ));
    assert!(reopened.snapshot().unwrap().tripped);
}

#[test]
fn judge_lane_admits_while_public_is_exhausted_and_never_beyond_its_slice() {
    let dir = TempDir::new("judge");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), unpaced_cfg(), &clock);
    let mut public = Vec::new();
    loop {
        match ledger.reserve(Lane::Public) {
            Ok(r) => public.push(r),
            Err(Refusal::DailyBudget { .. }) => break,
            Err(other) => panic!("{other:?}"),
        }
    }
    assert_eq!(public.len(), 5, "the public daily budget is exhausted");

    // The judge lane has its own slice and is not paced by the daily cap.
    let first_judge = ledger.reserve(Lane::Judge).unwrap();
    assert_eq!(first_judge.lane(), Lane::Judge);
    assert!(
        matches!(
            ledger.reserve(Lane::Judge),
            Err(Refusal::Concurrency { .. })
        ),
        "one judge slot"
    );
    let snap = ledger.snapshot().unwrap();
    assert_eq!(
        snap.daily_committed,
        usd("9.40"),
        "judge runs do not touch the public day"
    );
    assert_eq!(snap.judge_committed, usd(WORST_CASE));
    assert_eq!(snap.active_judge, 1);

    // Dropping the handle frees the slot but keeps the worst case: a
    // $15 slice holds seven such reservations ($13.16), never an eighth.
    drop(first_judge);
    let mut judges = 1;
    loop {
        match ledger.reserve(Lane::Judge) {
            Ok(r) => {
                judges += 1;
                drop(r);
            }
            Err(Refusal::JudgeSlice) => break,
            Err(other) => panic!("{other:?}"),
        }
    }
    assert_eq!(judges, 7);
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.judge_committed, usd("13.16"));
    assert_eq!(snap.judge_remaining, usd("1.84"));
    assert!(snap.judge_committed <= snap.judge_slice);
    assert_eq!(snap.lifetime_committed, usd("22.56"), "5 public + 7 judge");
}

#[test]
fn the_judge_lane_has_its_own_daily_cap() {
    // Two judge runs a day ($3.76 of the $15 slice).
    let cfg = BudgetConfig {
        judge_daily_cap: usd("3.76"),
        ..unpaced_cfg()
    };
    let dir = TempDir::new("judge-daily");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), cfg, &clock);
    let first = ledger.reserve(Lane::Judge).unwrap();
    drop(first);
    let second = ledger.reserve(Lane::Judge).unwrap();
    drop(second);
    match ledger.reserve(Lane::Judge) {
        Err(Refusal::JudgeDaily { retry_after }) => {
            assert_eq!(
                retry_after,
                Duration::from_secs(43_200),
                "until the next UTC day"
            );
        }
        other => panic!("{other:?}"),
    }
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.judge_daily_cap, usd("3.76"));
    assert_eq!(snap.judge_daily_committed, usd("3.76"));
    assert_eq!(snap.judge_daily_remaining, Usd::ZERO);
    assert_eq!(snap.judge_committed, usd("3.76"));
    assert_eq!(snap.judge_remaining, usd("11.24"));
    assert_eq!(
        snap.daily_committed,
        Usd::ZERO,
        "judge runs stay outside the public day"
    );
    assert!(
        ledger.reserve(Lane::Public).is_ok(),
        "the public lane is untouched"
    );
    let refusal = Refusal::JudgeDaily {
        retry_after: Duration::from_secs(1),
    };
    assert_eq!(refusal.code(), "live_budget_judge_daily");
    assert_eq!(refusal.retry_after(), Some(Duration::from_secs(1)));
    // A new UTC day admits judges again; the slice counts across days.
    clock.set("2026-10-28T00:00:00Z");
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.judge_daily_committed, Usd::ZERO);
    assert_eq!(snap.judge_committed, usd("3.76"));
    let third = ledger.reserve(Lane::Judge).unwrap();
    drop(third);
    assert_eq!(ledger.snapshot().unwrap().judge_committed, usd("5.64"));
}

#[test]
fn public_lane_never_reserves_into_the_judge_slice() {
    // Daily cap equal to the lifetime cap, so only the lifetime binds.
    let cfg = BudgetConfig {
        daily_cap: usd("60.00"),
        ..unpaced_cfg()
    };
    let dir = TempDir::new("public-share");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), cfg, &clock);
    let mut admitted = Vec::new();
    loop {
        match ledger.reserve(Lane::Public) {
            Ok(r) => admitted.push(r),
            Err(Refusal::LifetimeBudget) => break,
            Err(other) => panic!("{other:?}"),
        }
    }
    // $60 − $15 slice = $45 holds 23 × $1.88 = $43.24, not 24.
    assert_eq!(admitted.len(), 23);
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.lifetime_committed, usd("43.24"));
    assert_eq!(snap.judge_remaining, usd("15.00"));
    assert!(ledger.reserve(Lane::Judge).is_ok(), "the slice is intact");
    assert!(matches!(
        ledger.reserve(Lane::Operator),
        Err(Refusal::LifetimeBudget)
    ));
}

#[test]
fn hourly_admission_cap_and_even_daily_refill_with_burst() {
    let cfg = BudgetConfig {
        hourly_admissions: 1,
        max_concurrent: 10,
        ..owner_cfg()
    };
    let dir = TempDir::new("pacing");
    let clock = TestClock::at("2026-10-27T00:00:00Z");
    let ledger = open(dir.path(), cfg, &clock);
    let snap = ledger.snapshot().unwrap();
    assert_eq!(
        snap.daily_available_now,
        usd("2.50"),
        "25% burst at midnight"
    );

    let first = ledger.reserve(Lane::Public).unwrap();
    match ledger.reserve(Lane::Public) {
        Err(Refusal::HourlyAdmissions { retry_after }) => {
            assert_eq!(retry_after, Duration::from_secs(3600));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(ledger.snapshot().unwrap().hourly_public, 1);

    // An hour later the window is clear; the budget is still refilling.
    clock.advance(3600);
    let retry_after = match ledger.reserve(Lane::Public) {
        Err(Refusal::DailyBudget { retry_after }) => retry_after,
        other => panic!("{other:?}"),
    };
    // Need $3.76 of $2.50 + $10 × t/86400 => t = ceil(1.26 × 8640) = 10,887 s.
    assert_eq!(retry_after, Duration::from_secs(10_887 - 3600));
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.daily_available_now, usd("1.036666"));
    assert_eq!(snap.hourly_public, 0);

    clock.advance(10_887 - 3600 - 1);
    assert!(matches!(
        ledger.reserve(Lane::Public),
        Err(Refusal::DailyBudget { .. })
    ));
    clock.advance(1);
    let second = ledger.reserve(Lane::Public).unwrap();
    assert_eq!(ledger.snapshot().unwrap().daily_committed, usd("3.76"));

    // The judge lane has its own hourly window.
    let judge = ledger.reserve(Lane::Judge).unwrap();
    assert!(matches!(
        ledger.reserve(Lane::Judge),
        Err(Refusal::Concurrency { .. })
    ));
    drop(judge);
    assert!(matches!(
        ledger.reserve(Lane::Judge),
        Err(Refusal::HourlyAdmissions { .. })
    ));

    // A new UTC day starts from the burst again.
    clock.set("2026-10-28T00:00:00Z");
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.day, "2026-10-28");
    assert_eq!(snap.daily_committed, Usd::ZERO);
    assert_eq!(snap.daily_available_now, usd("2.50"));
    assert_eq!(
        snap.lifetime_committed,
        usd("5.64"),
        "3 × $1.88 across days"
    );
    let third = ledger.reserve(Lane::Public).unwrap();
    drop((first, second, third));
}

#[test]
fn operator_runs_debit_the_same_ledger_without_pacing() {
    let cfg = BudgetConfig {
        hourly_admissions: 1,
        ..owner_cfg()
    };
    let dir = TempDir::new("operator");
    let clock = TestClock::at("2026-10-27T00:00:00Z");
    let ledger = open(dir.path(), cfg, &clock);
    // No burst pacing, no hourly window, no slot for the operator...
    let a = ledger.reserve(Lane::Operator).unwrap();
    let b = ledger.reserve(Lane::Operator).unwrap();
    let c = ledger.reserve(Lane::Operator).unwrap();
    let d = ledger.reserve(Lane::Operator).unwrap();
    let e = ledger.reserve(Lane::Operator).unwrap();
    // ...but the same daily cap.
    match ledger.reserve(Lane::Operator) {
        Err(Refusal::DailyBudget { retry_after }) => {
            assert_eq!(
                retry_after,
                Duration::from_secs(86_400),
                "until the next UTC day"
            );
        }
        other => panic!("{other:?}"),
    }
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.daily_committed, usd("9.40"));
    assert_eq!(snap.active_public, 0, "operator runs take no public slot");
    // And the public lane sees the day as spent.
    assert!(matches!(
        ledger.reserve(Lane::Public),
        Err(Refusal::DailyBudget { .. })
    ));
    drop((a, b, c, d, e));
}

#[test]
fn exact_decimal_money_over_ten_thousand_settlements() {
    let cfg = unpaced_cfg();
    let usage = Usage {
        input: 123,
        completion: 463,
        reasoning_included: 7,
    };
    // 123 × 4 + 463 × 20 = 492 + 9,260 = 9,752 micro-dollars.
    assert_eq!(cfg.charge(&usage), Usd::from_micros(9_752));
    let mut total = Usd::ZERO;
    for _ in 0..10_000 {
        total = total.saturating_add(cfg.charge(&usage));
    }
    assert_eq!(total, Usd::from_micros(97_520_000));
    assert_eq!(total.to_fixed(), "97.520000");
    assert_eq!(total, usd("97.52"));

    // Through the files: every record is a decimal string and the ledger's
    // totals equal the integer arithmetic exactly.
    let big = BudgetConfig {
        daily_cap: usd("100000"),
        lifetime_cap: usd("1000000"),
        judge_slice: usd("15"),
        run_token_cap: 1_000_000_000,
        ..cfg
    };
    let dir = TempDir::new("exact");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), big, &clock);
    let reservation = ledger.reserve(Lane::Public).unwrap();
    let n = 200u64;
    for _ in 0..n {
        let draw = ledger.draw(&reservation, full_attempt()).unwrap();
        ledger.settle(draw, Outcome::Success(usage)).unwrap();
    }
    let released = ledger.release(&reservation).unwrap();
    assert_eq!(released.spent, Usd::from_micros(9_752 * n));
    assert_eq!(released.spent.to_fixed(), "1.950400");
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.lifetime_spent, released.spent);
    let settlements = records(dir.path(), "2026-10-27", "settlement");
    assert_eq!(settlements.len(), n as usize);
    let summed = settlements
        .iter()
        .map(|s| Usd::parse(s["charged_usd"].as_str().unwrap()).unwrap())
        .fold(Usd::ZERO, Usd::saturating_add);
    assert_eq!(summed, released.spent);
    assert!(settlements.iter().all(|s| s["charged_usd"] == "0.009752"));
}

#[test]
fn two_handles_on_one_directory_serialize_through_the_lock() {
    let dir = TempDir::new("two-handles");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let a = open(dir.path(), unpaced_cfg(), &clock);
    let b = open(dir.path(), unpaced_cfg(), &clock);
    let ra = a.reserve(Lane::Public).unwrap();
    assert_eq!(
        b.snapshot().unwrap().daily_committed,
        usd(WORST_CASE),
        "b sees a's reservation"
    );
    let rb = b.reserve(Lane::Public).unwrap();
    assert_eq!(
        a.snapshot().unwrap().daily_committed,
        usd("3.76"),
        "a sees b's reservation"
    );
    assert_eq!(
        a.snapshot().unwrap().active_public,
        1,
        "slots are per handle group"
    );
    let draw = a.draw(&ra, full_attempt()).unwrap();
    a.settle(draw, Outcome::Timeout).unwrap();
    assert_eq!(b.snapshot().unwrap().lifetime_spent, usd(ATTEMPT_EST));
    drop(rb);

    // A writer elsewhere holding the lock makes every operation wait, then
    // fail closed when the wait runs out.
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.path().join(LOCK_FILE))
        .unwrap();
    lock.try_lock().unwrap();
    let started = std::time::Instant::now();
    match a.reserve(Lane::Public) {
        Err(Refusal::Ledger(LedgerError::Locked { lock_file, waited })) => {
            assert!(lock_file.ends_with(LOCK_FILE));
            assert!(waited >= Duration::from_millis(300), "{waited:?}");
        }
        other => panic!("{other:?}"),
    }
    assert!(started.elapsed() >= Duration::from_millis(300));
    assert!(matches!(b.snapshot(), Err(LedgerError::Locked { .. })));
    lock.unlock().unwrap();
    assert!(a.reserve(Lane::Public).is_ok());
    assert_eq!(records(dir.path(), "2026-10-27", "reservation").len(), 3);
}

#[test]
fn input_and_output_caps_refuse_before_anything_is_written() {
    let dir = TempDir::new("caps");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), unpaced_cfg(), &clock);
    let reservation = ledger.reserve(Lane::Public).unwrap();
    match ledger.draw(
        &reservation,
        AttemptEstimate {
            request_bytes: 7_000,
            max_completion_tokens: 16_000,
        },
    ) {
        Err(Refusal::InputCap {
            bound_tokens,
            cap_tokens,
        }) => {
            assert_eq!((bound_tokens, cap_tokens), (8_024, 8_000));
        }
        other => panic!("{other:?}"),
    }
    match ledger.draw(
        &reservation,
        AttemptEstimate {
            request_bytes: 10,
            max_completion_tokens: 16_001,
        },
    ) {
        Err(Refusal::OutputCap { requested, cap }) => {
            assert_eq!((requested, cap), (16_001, 16_000))
        }
        other => panic!("{other:?}"),
    }
    assert!(records(dir.path(), "2026-10-27", "draw").is_empty());
    // Exactly at the bound is fine: 6,976 + 1,024 = 8,000.
    let draw = ledger
        .draw(
            &reservation,
            AttemptEstimate {
                request_bytes: 6_976,
                max_completion_tokens: 16_000,
            },
        )
        .unwrap();
    assert_eq!(draw.input_bound_tokens(), 8_000);
    assert_eq!(draw.estimate(), usd("0.352"));
}

#[test]
fn the_reservation_is_a_hard_ceiling() {
    let dir = TempDir::new("ceiling");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), unpaced_cfg(), &clock);
    let reservation = ledger.reserve(Lane::Public).unwrap();
    assert_eq!(reservation.worst_case(), usd(WORST_CASE));
    let mut drawn = Usd::ZERO;
    let mut attempts = 0;
    let refusal = loop {
        match ledger.draw(&reservation, full_attempt()) {
            Ok(draw) => {
                drawn = drawn.saturating_add(draw.estimate());
                attempts += 1;
                ledger.settle(draw, Outcome::Timeout).unwrap();
            }
            Err(refusal) => break refusal,
        }
    };
    assert_eq!(attempts, 5);
    assert_eq!(drawn, usd("1.620480"));
    match refusal {
        Refusal::ReservationExhausted { remaining, needed } => {
            assert_eq!(remaining, usd("0.259520"));
            assert_eq!(needed, usd(ATTEMPT_EST));
            // A draw-time refusal: the run fails with the adapter's
            // `budget_refused`; as an admission code it is the generic one.
            assert_eq!(refusal.code(), "live_unavailable");
        }
        other => panic!("{other:?}"),
    }
    // A smaller attempt still fits the remainder.
    let small = ledger
        .draw(
            &reservation,
            AttemptEstimate {
                request_bytes: 0,
                max_completion_tokens: 1_000,
            },
        )
        .unwrap();
    assert_eq!(small.estimate(), usd("0.024096"));
    ledger.settle(small, Outcome::Status(500)).unwrap();
    let released = ledger.release(&reservation).unwrap();
    assert!(released.spent <= usd(WORST_CASE));
    assert_eq!(released.spent, usd("1.644576"));
}

#[test]
fn release_frees_the_day_down_to_what_was_spent() {
    let dir = TempDir::new("release");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), unpaced_cfg(), &clock);
    let mut held = Vec::new();
    for _ in 0..5 {
        held.push(ledger.reserve(Lane::Public).unwrap());
    }
    assert!(matches!(
        ledger.reserve(Lane::Public),
        Err(Refusal::DailyBudget { .. })
    ));
    let run = held.pop().unwrap();
    let draw = ledger.draw(&run, full_attempt()).unwrap();
    ledger
        .settle(
            draw,
            Outcome::Success(Usage {
                input: 1_000,
                completion: 2_000,
                reasoning_included: 0,
            }),
        )
        .unwrap();
    let released = ledger.release(&run).unwrap();
    assert_eq!(released.spent, usd("0.044"));
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.daily_committed, usd("7.564"), "4 × 1.88 + 0.044");
    assert!(
        ledger.reserve(Lane::Public).is_ok(),
        "the freed budget admits again"
    );
}

#[test]
fn draws_and_settlements_are_checked_against_the_record() {
    let dir = TempDir::new("checks");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), unpaced_cfg(), &clock);
    let reservation = ledger.reserve(Lane::Public).unwrap();
    let meter = ledger.meter(&reservation);
    let draw = meter.draw(full_attempt()).unwrap();
    ledger.release(&reservation).unwrap();
    // The run is over: settling its last attempt still counts, drawing more does not.
    assert!(matches!(
        meter.draw(full_attempt()),
        Err(Refusal::Ledger(LedgerError::Released(_)))
    ));
    let settled = meter.settle(draw, Outcome::Status(429)).unwrap();
    assert_eq!(settled.charged, Usd::ZERO);
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.lifetime_spent, Usd::ZERO);
    assert_eq!(snap.unsettled_reservations, 0);
}

#[test]
fn a_torn_tail_is_cut_and_a_corrupt_line_fails_closed() {
    let dir = TempDir::new("torn");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let day_file = dir.path().join("2026-10-27.jsonl");
    {
        let ledger = open(dir.path(), unpaced_cfg(), &clock);
        let r = ledger.reserve(Lane::Public).unwrap();
        ledger.release(&r).unwrap();
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&day_file)
            .unwrap();
        use std::io::Write;
        f.write_all(br#"{"type":"draw","v":1,"reservation":"x","att"#)
            .unwrap();
    }
    let ledger = open(dir.path(), unpaced_cfg(), &clock);
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.reservations, 1);
    assert_eq!(snap.lifetime_committed, Usd::ZERO);
    let r = ledger.reserve(Lane::Public).unwrap();
    drop(r);
    // Every line of the file parses again: the torn bytes were truncated.
    let text = std::fs::read_to_string(&day_file).unwrap();
    assert_eq!(text.lines().count(), 3);
    for line in text.lines() {
        serde_json::from_str::<serde_json::Value>(line).unwrap();
    }
    drop(ledger);

    // A damaged line in the middle is not recoverable: the ledger refuses
    // to open rather than guess what it meant.
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    lines.insert(1, "{\"type\":\"reservation\",\"v\":1}".to_string());
    std::fs::write(&day_file, lines.join("\n") + "\n").unwrap();
    match Ledger::open_with(dir.path(), unpaced_cfg(), clock.clock(), fast_options()) {
        Err(LedgerError::Corrupt { file, .. }) => assert_eq!(file, day_file),
        other => panic!("{:?}", other.map(|_| ())),
    }
}

#[test]
fn a_record_with_an_unreadable_timestamp_fails_closed() {
    let dir = TempDir::new("bad-timestamp");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let day_file = dir.path().join("2026-10-27.jsonl");
    {
        let ledger = open(dir.path(), unpaced_cfg(), &clock);
        let r = ledger.reserve(Lane::Public).unwrap();
        drop(r);
    }
    // An edited reservation time could otherwise drop out of today's
    // budget while still looking like a valid record.
    let text = std::fs::read_to_string(&day_file).unwrap();
    assert!(text.contains("\"at\":\"2026-10-27T12:00:00.000Z\""));
    let edited = text.replace("2026-10-27T12:00:00.000Z", "yesterday-ish");
    std::fs::write(&day_file, edited).unwrap();
    match Ledger::open_with(dir.path(), unpaced_cfg(), clock.clock(), fast_options()) {
        Err(LedgerError::Corrupt { file, reason, .. }) => {
            assert_eq!(file, day_file);
            assert!(reason.contains("bad timestamp"), "{reason}");
        }
        other => panic!("{:?}", other.map(|_| ())),
    }
}

#[test]
fn the_lifetime_summary_is_a_floor_when_day_files_go_missing() {
    let dir = TempDir::new("summary-floor");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    {
        let ledger = open(dir.path(), unpaced_cfg(), &clock);
        let r = ledger.reserve(Lane::Public).unwrap();
        let draw = ledger.draw(&r, full_attempt()).unwrap();
        ledger.settle(draw, Outcome::Timeout).unwrap();
        ledger.release(&r).unwrap();
    }
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join(SUMMARY_FILE)).unwrap()).unwrap();
    assert_eq!(summary["lifetime_spent_usd"], ATTEMPT_EST);
    assert_eq!(summary["tripped"], false);

    std::fs::remove_file(dir.path().join("2026-10-27.jsonl")).unwrap();
    let ledger = open(dir.path(), unpaced_cfg(), &clock);
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.reservations, 0);
    assert_eq!(
        snap.lifetime_spent,
        usd(ATTEMPT_EST),
        "the summary's figure is kept"
    );
    assert_eq!(snap.lifetime_committed, usd(ATTEMPT_EST));
    assert!(snap.inconsistent.is_some());
    assert!(!snap.tripped);
    // The operator sees the inconsistency; the public projection does not.
    let full = serde_json::to_string(&snap).unwrap();
    assert!(full.contains("inconsistent"), "{full}");
    let public = serde_json::to_string(&snap.public()).unwrap();
    assert!(!public.contains("inconsistent"), "{public}");
    assert!(!public.contains("summary"), "{public}");
    assert!(!public.contains('/') && !public.contains('\\'), "{public}");
    assert!(!public.contains("trip_code"), "no trip, no code: {public}");
}

#[test]
fn snapshot_and_refusals_expose_stable_codes_and_string_money() {
    let dir = TempDir::new("snapshot");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), owner_cfg(), &clock);
    let snap = ledger.snapshot().unwrap();
    let json = serde_json::to_value(&snap).unwrap();
    assert_eq!(json["daily_cap"], "10.000000");
    assert_eq!(json["lifetime_remaining"], "60.000000");
    assert_eq!(json["judge_remaining"], "15.000000");
    assert_eq!(json["run_worst_case"], "1.880000");
    assert_eq!(json["tripped"], false);
    assert_eq!(json["day"], "2026-10-27");
    let codes: Vec<&str> = [
        Refusal::Tripped {
            code: TripReason::KillSwitch,
            reason: "x".into(),
        }
        .code(),
        Refusal::DailyBudget {
            retry_after: Duration::from_secs(1),
        }
        .code(),
        Refusal::LifetimeBudget.code(),
        Refusal::JudgeSlice.code(),
        Refusal::HourlyAdmissions {
            retry_after: Duration::from_secs(1),
        }
        .code(),
        Refusal::Concurrency {
            retry_after: Duration::from_secs(1),
        }
        .code(),
    ]
    .to_vec();
    // The wire strings of the API's ErrorCode (gate::refusal_code): one set
    // of codes for the UI, a tripped ledger never reading as "live mode
    // off" (`live_disabled` is the route's code for a server without a
    // gate).
    assert_eq!(
        codes,
        vec![
            "live_tripped",
            "live_budget_daily",
            "live_budget_lifetime",
            "live_budget_judge_slice",
            "live_hourly_admissions",
            "live_concurrency"
        ]
    );
    assert_eq!(Refusal::LifetimeBudget.retry_after(), None);
    assert_eq!(
        Refusal::DailyBudget {
            retry_after: Duration::from_secs(7)
        }
        .retry_after(),
        Some(Duration::from_secs(7))
    );
}

#[test]
fn a_failed_release_keeps_the_handle_and_the_slot_until_it_succeeds() {
    let dir = TempDir::new("release-retry");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), owner_cfg(), &clock);
    let reservation = ledger.reserve(Lane::Public).unwrap();
    assert!(
        matches!(
            ledger.reserve(Lane::Public),
            Err(Refusal::Concurrency { .. })
        ),
        "one slot"
    );

    // Another writer holds the lock: the release fails closed...
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.path().join(LOCK_FILE))
        .unwrap();
    lock.try_lock().unwrap();
    assert!(matches!(
        ledger.release(&reservation),
        Err(LedgerError::Locked { .. })
    ));
    // ...and nothing changed: the reservation still counts at its worst
    // case and the handle still holds its slot (read without the lock).
    let snap = ledger.snapshot_cached(Duration::from_secs(60)).unwrap();
    assert_eq!(snap.unsettled_reservations, 1);
    assert_eq!(snap.active_public, 1);
    assert_eq!(snap.daily_committed, usd(WORST_CASE));
    lock.unlock().unwrap();
    assert!(
        matches!(
            ledger.reserve(Lane::Public),
            Err(Refusal::Concurrency { .. })
        ),
        "the slot is held until the release is durable"
    );

    // The same handle releases once the lock is free.
    let released = ledger.release(&reservation).unwrap();
    assert_eq!(released.spent, Usd::ZERO);
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.unsettled_reservations, 0);
    assert_eq!(snap.active_public, 0);
    assert_eq!(snap.daily_committed, Usd::ZERO);
    // Idempotent: a second release changes nothing and writes nothing.
    assert_eq!(ledger.release(&reservation).unwrap().spent, Usd::ZERO);
    assert_eq!(records(dir.path(), "2026-10-27", "release").len(), 1);
    assert!(ledger.reserve(Lane::Public).is_ok());
}

#[test]
fn release_by_id_reconciles_a_crashed_reservation() {
    let dir = TempDir::new("release-by-id");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let id = {
        let ledger = open(dir.path(), owner_cfg(), &clock);
        let reservation = ledger.reserve(Lane::Public).unwrap();
        let draw = ledger.draw(&reservation, full_attempt()).unwrap();
        ledger
            .settle(
                draw,
                Outcome::Success(Usage::from_chat_usage(1_000, 2_000, None)),
            )
            .unwrap();
        let _open_attempt = ledger.draw(&reservation, full_attempt()).unwrap();
        reservation.id().clone()
        // The process dies here: no release, one attempt still open.
    };
    let ledger = open(dir.path(), owner_cfg(), &clock);
    let before = ledger.snapshot().unwrap();
    assert_eq!(before.unsettled_reservations, 1);
    assert_eq!(before.daily_committed, usd(WORST_CASE));

    let released = ledger
        .release_by_id(&id, "operator: dashboard shows one call")
        .unwrap();
    // The settled charge plus the open attempt at its estimate: never less.
    assert_eq!(
        released.spent,
        usd("0.044").saturating_add(usd(ATTEMPT_EST))
    );
    let after = ledger.snapshot().unwrap();
    assert_eq!(after.unsettled_reservations, 0);
    assert_eq!(after.daily_committed, released.spent);
    assert_eq!(after.lifetime_committed, released.spent);
    let releases = records(dir.path(), "2026-10-27", "release");
    assert_eq!(releases.len(), 1);
    assert_eq!(releases[0]["by"], "operator: dashboard shows one call");
    assert_eq!(releases[0]["reservation"], id.as_str());
    // Idempotent; an unknown id is an error and writes nothing.
    assert_eq!(
        ledger.release_by_id(&id, "again").unwrap().spent,
        released.spent
    );
    let unknown: ReservationId = serde_json::from_str("\"not-a-reservation\"").unwrap();
    assert!(matches!(
        ledger.release_by_id(&unknown, "typo"),
        Err(LedgerError::UnknownReservation(_))
    ));
    assert_eq!(records(dir.path(), "2026-10-27", "release").len(), 1);
    // A plain release writes no `by`.
    let r = ledger.reserve(Lane::Public).unwrap();
    ledger.release(&r).unwrap();
    let releases = records(dir.path(), "2026-10-27", "release");
    assert_eq!(releases.len(), 2);
    assert!(releases[1].get("by").is_none());
}

#[test]
fn snapshot_cached_never_waits_for_the_lock_and_never_writes() {
    let dir = TempDir::new("snapshot-cached");
    let switch = dir.path().join("kill-switch");
    let cfg = BudgetConfig {
        kill_switch_file: Some(switch.clone()),
        ..unpaced_cfg()
    };
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let a = open(dir.path(), cfg.clone(), &clock);
    let b = open(dir.path(), cfg, &clock);
    let ra = a.reserve(Lane::Public).unwrap();
    // When the lock is free a stale handle reads the others' records.
    assert_eq!(
        b.snapshot_cached(Duration::ZERO).unwrap().daily_committed,
        usd(WORST_CASE)
    );
    // Time-dependent figures are computed for now, even from the cache.
    clock.set("2026-10-28T00:00:00Z");
    let snap = b.snapshot_cached(Duration::from_secs(3600)).unwrap();
    assert_eq!(snap.day, "2026-10-28");
    assert_eq!(snap.daily_committed, Usd::ZERO);
    assert_eq!(snap.lifetime_committed, usd(WORST_CASE));
    clock.set("2026-10-27T12:00:00Z");

    // Another writer holds the lock for a long time: the blocking read
    // fails closed after its wait; the cached read keeps answering from
    // the in-process state without waiting at all.
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(dir.path().join(LOCK_FILE))
        .unwrap();
    lock.try_lock().unwrap();
    assert!(matches!(b.snapshot(), Err(LedgerError::Locked { .. })));
    let started = std::time::Instant::now();
    for _ in 0..1_000 {
        let snap = b.snapshot_cached(Duration::from_secs(60)).unwrap();
        assert_eq!(snap.daily_committed, usd(WORST_CASE));
    }
    for _ in 0..10 {
        // Stale and the lock is busy: still answered, still no wait.
        assert_eq!(
            b.snapshot_cached(Duration::ZERO).unwrap().daily_committed,
            usd(WORST_CASE)
        );
    }
    // 1,010 blocking reads would have waited 1,010 × lock_wait (300 ms).
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "{:?}",
        started.elapsed()
    );

    // The kill-switch file is noticed by write paths only: the cached read
    // reports no trip and writes no record, with or without the lock...
    std::fs::write(&switch, b"stop\n").unwrap();
    assert!(!a.snapshot_cached(Duration::ZERO).unwrap().tripped);
    lock.unlock().unwrap();
    assert!(!a.snapshot_cached(Duration::ZERO).unwrap().tripped);
    assert!(records(dir.path(), "2026-10-27", "trip").is_empty());
    // ...the next admission trips; the handle's own writes refresh its
    // state at once and the other handle sees the trip when it refreshes.
    assert!(matches!(
        a.reserve(Lane::Public),
        Err(Refusal::Tripped {
            code: TripReason::KillSwitch,
            ..
        })
    ));
    assert!(a.snapshot_cached(Duration::from_secs(60)).unwrap().tripped);
    let fresh = b.snapshot_cached(Duration::ZERO).unwrap();
    assert!(fresh.tripped);
    assert_eq!(fresh.trip_code, Some(TripReason::KillSwitch));
    assert_eq!(records(dir.path(), "2026-10-27", "trip").len(), 1);
    drop(ra);
}

#[test]
fn the_summary_floor_covers_the_judge_lane_and_a_reached_cap_trips_at_open() {
    let dir = TempDir::new("summary-judge");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    {
        let ledger = open(dir.path(), unpaced_cfg(), &clock);
        let public = ledger.reserve(Lane::Public).unwrap();
        let draw = ledger.draw(&public, full_attempt()).unwrap();
        ledger.settle(draw, Outcome::Timeout).unwrap();
        ledger.release(&public).unwrap();
        // A judge run is admitted and never released: it counts at its
        // worst case, in the files and in the summary.
        let judge = ledger.reserve(Lane::Judge).unwrap();
        drop(judge);
    }
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join(SUMMARY_FILE)).unwrap()).unwrap();
    assert_eq!(summary["lifetime_spent_usd"], ATTEMPT_EST);
    assert_eq!(summary["judge_committed_usd"], "1.880000");
    assert_eq!(summary["nonjudge_committed_usd"], ATTEMPT_EST);
    assert_eq!(summary["lifetime_committed_usd"], "2.204096");

    std::fs::remove_file(dir.path().join("2026-10-27.jsonl")).unwrap();
    let ledger = open(dir.path(), unpaced_cfg(), &clock);
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.reservations, 0);
    assert_eq!(snap.lifetime_spent, usd(ATTEMPT_EST));
    assert_eq!(
        snap.judge_committed,
        usd(WORST_CASE),
        "the lost judge run keeps its worst case"
    );
    assert_eq!(snap.judge_remaining, usd("13.12"));
    assert_eq!(snap.lifetime_committed, usd("2.204096"));
    assert_eq!(snap.lifetime_remaining, usd("57.795904"));
    assert!(snap.inconsistent.is_some());
    assert!(!snap.tripped);
    // What is left of the slice bounds the lane: $13.12 holds six more
    // $1.88 runs ($11.28), not seven.
    let mut judges = 0;
    loop {
        match ledger.reserve(Lane::Judge) {
            Ok(r) => {
                judges += 1;
                drop(r);
            }
            Err(Refusal::JudgeSlice) => break,
            Err(other) => panic!("{other:?}"),
        }
    }
    assert_eq!(judges, 6);
    assert_eq!(ledger.snapshot().unwrap().judge_committed, usd("13.16"));
    drop(ledger);
    // The offsets went into the new summary: a reopen with the new day
    // file intact still counts the lost run.
    let reopened = open(dir.path(), unpaced_cfg(), &clock);
    assert_eq!(reopened.snapshot().unwrap().judge_committed, usd("13.16"));
    assert!(matches!(
        reopened.reserve(Lane::Judge),
        Err(Refusal::JudgeSlice)
    ));
    drop(reopened);

    // A floor that reaches the lifetime cap trips at open: the owner
    // lowers the caps below what the summary already remembers as spent.
    let lowered_caps = BudgetConfig {
        daily_cap: usd("0.30"),
        lifetime_cap: usd("0.30"),
        judge_slice: Usd::ZERO,
        judge_daily_cap: Usd::ZERO,
        run_token_cap: 1_000,
        max_completion_tokens: 1_000,
        input_cap_tokens: 1_000,
        ..unpaced_cfg()
    };
    let lowered = open(dir.path(), lowered_caps, &clock);
    let snap = lowered.snapshot().unwrap();
    assert!(snap.tripped);
    assert_eq!(snap.trip_code, Some(TripReason::LifetimeCap));
    assert_eq!(snap.lifetime_spent, usd(ATTEMPT_EST));
    assert!(matches!(
        lowered.reserve(Lane::Public),
        Err(Refusal::Tripped {
            code: TripReason::LifetimeCap,
            ..
        })
    ));
    assert_eq!(
        records(dir.path(), "2026-10-27", "trip").len(),
        1,
        "the trip is persisted at open"
    );
}

#[test]
fn a_stale_summary_does_not_override_newer_day_files() {
    use std::io::Write;
    let append_line = |path: &Path, line: &str| {
        let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
        f.write_all(line.as_bytes()).unwrap();
        f.write_all(b"\n").unwrap();
    };

    // A reset written one second after the trip, then a crash before the
    // summary rewrite: the summary still says tripped, the files know
    // better.
    let dir = TempDir::new("stale-trip");
    let day_file = dir.path().join("2026-10-27.jsonl");
    let switch = dir.path().join("kill-switch");
    let cfg = BudgetConfig {
        kill_switch_file: Some(switch.clone()),
        ..unpaced_cfg()
    };
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    {
        let ledger = open(dir.path(), cfg.clone(), &clock);
        let r = ledger.reserve(Lane::Public).unwrap();
        ledger.release(&r).unwrap();
        std::fs::write(&switch, b"stop\n").unwrap();
        assert!(matches!(
            ledger.reserve(Lane::Public),
            Err(Refusal::Tripped { .. })
        ));
        std::fs::remove_file(&switch).unwrap();
    }
    let summary: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join(SUMMARY_FILE)).unwrap()).unwrap();
    assert_eq!(summary["tripped"], true);
    assert_eq!(summary["updated_at"], "2026-10-27T12:00:00.000Z");
    append_line(
        &day_file,
        r#"{"type":"reset","v":1,"by":"operator","at":"2026-10-27T12:00:01.000Z"}"#,
    );
    clock.set("2026-10-27T12:00:02Z");
    let ledger = open(dir.path(), cfg.clone(), &clock);
    let snap = ledger.snapshot().unwrap();
    assert!(!snap.tripped, "the files are newer and show the reset");
    assert!(snap.inconsistent.is_none(), "{:?}", snap.inconsistent);
    assert!(ledger.reserve(Lane::Public).is_ok());
    drop(ledger);

    // The same rule for amounts: a release record newer than the summary
    // ends the reservation instead of flooring it at its worst case.
    let dir = TempDir::new("stale-release");
    let day_file = dir.path().join("2026-10-27.jsonl");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let id = {
        let ledger = open(dir.path(), unpaced_cfg(), &clock);
        let r = ledger.reserve(Lane::Public).unwrap();
        r.id().clone()
    };
    append_line(
        &day_file,
        &format!(
            r#"{{"type":"release","v":1,"reservation":"{id}","at":"2026-10-27T12:00:01.000Z"}}"#
        ),
    );
    clock.set("2026-10-27T12:00:02Z");
    let ledger = open(dir.path(), unpaced_cfg(), &clock);
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.reservations, 1);
    assert_eq!(snap.unsettled_reservations, 0);
    assert_eq!(snap.lifetime_committed, Usd::ZERO);
    assert!(snap.inconsistent.is_none(), "{:?}", snap.inconsistent);
    drop(ledger);

    // Without a newer record the summary is trusted: a lost day file with
    // a remembered trip reopens tripped, with the remembered code.
    let dir = TempDir::new("lost-trip");
    let switch = dir.path().join("kill-switch");
    let cfg = BudgetConfig {
        kill_switch_file: Some(switch.clone()),
        ..unpaced_cfg()
    };
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    {
        let ledger = open(dir.path(), cfg.clone(), &clock);
        std::fs::write(&switch, b"stop\n").unwrap();
        assert!(matches!(
            ledger.reserve(Lane::Public),
            Err(Refusal::Tripped { .. })
        ));
        std::fs::remove_file(&switch).unwrap();
    }
    std::fs::remove_file(dir.path().join("2026-10-27.jsonl")).unwrap();
    clock.set("2026-10-27T12:00:02Z");
    let ledger = open(dir.path(), cfg, &clock);
    let snap = ledger.snapshot().unwrap();
    assert!(snap.tripped);
    assert_eq!(snap.trip_code, Some(TripReason::KillSwitch));
    assert!(
        snap.trip_reason
            .as_deref()
            .unwrap()
            .starts_with("summary_trip_not_in_day_files"),
        "{:?}",
        snap.trip_reason
    );
    assert!(snap.inconsistent.is_some());
    assert!(matches!(
        ledger.reserve(Lane::Public),
        Err(Refusal::Tripped { .. })
    ));
}

#[test]
fn an_attempt_that_was_never_sent_settles_at_zero_and_is_recorded_as_such() {
    // A connection that could not be opened (or a request that could not
    // be built) reached no provider: the draw is settled at zero, like a
    // non-retryable 4xx, with its own detail so the operator can tell the
    // two apart. Nothing trips, and the run keeps its ceiling for the
    // retry that follows.
    let dir = TempDir::new("not-sent");
    let clock = TestClock::at("2026-10-27T12:00:00Z");
    let ledger = open(dir.path(), unpaced_cfg(), &clock);
    let reservation = ledger.reserve(Lane::Public).unwrap();
    let meter: Box<dyn AttemptMeter> = Box::new(ledger.meter(&reservation));

    let first = meter.draw(full_attempt()).unwrap();
    assert_eq!(first.estimate(), usd(ATTEMPT_EST));
    let settled = meter.settle(first, Outcome::NotSent).unwrap();
    assert_eq!(settled.charged, Usd::ZERO);
    assert_eq!(settled.outcome, RecordedOutcome::Zero);
    assert!(settled.tripped.is_none());

    // The retry draws again on the same reservation, at the full ceiling.
    let second = meter.draw(full_attempt()).unwrap();
    assert_eq!(second.attempt(), 2);
    let settled = meter
        .settle(
            second,
            Outcome::Success(Usage::from_chat_usage(1_000, 2_000, None)),
        )
        .unwrap();
    assert_eq!(settled.charged, usd("0.044"));

    let settlements = records(dir.path(), "2026-10-27", "settlement");
    assert_eq!(settlements.len(), 2);
    assert_eq!(settlements[0]["outcome"]["kind"], "zero");
    assert_eq!(settlements[0]["charged_usd"], "0.000000");
    assert_eq!(settlements[0]["detail"], "not_sent");
    assert_eq!(settlements[1]["outcome"]["kind"], "usage");
    let released = ledger.release(&reservation).unwrap();
    assert_eq!(
        released.spent,
        usd("0.044"),
        "the unsent attempt cost nothing"
    );
    let snap = ledger.snapshot().unwrap();
    assert_eq!(snap.lifetime_spent, usd("0.044"));
    assert_eq!(snap.unsettled_reservations, 0);
}
