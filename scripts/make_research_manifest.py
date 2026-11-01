# SPDX-License-Identifier: Apache-2.0
# Usage: python -I scripts/make_research_manifest.py research
# Build research/MANIFEST.json and research/SHA256SUMS from the copied
# files. Reads research/ only (never demo-run/), writes only those two files.
# Cross-checks every run record against its event log before writing.
import collections
import copy
import hashlib
import json
import os
import sys

ROOT = sys.argv[1]  # path to research/

LOGS = [
    {
        "path": "2026-10-26/console-events.jsonl",
        "source": "demo-run/events.jsonl",
        "battery": "console-2026-10-26",
        "records_dir": None,
        "description": (
            "Default local event log of the operator console and the scripted CLI demo, 2026-10-26 "
            "(local time). Schema 1.0: written before the "
            "hash chain existed, so its records carry no prev_hash/hash and are not chain-verified "
            "or re-derived on replay. C01, C02 and C05 are scripted (fake model, SIMULATED proofs); "
            "C03 is live Meta with simulated memory and SIMULATED proofs; C04 is live Meta with the "
            "simulated memory "
            "backend offline; C06 and C07 are live Meta with Hyphae records that carried no proof "
            "artifacts (integrity unavailable). Labels C01-C07 are assigned by this archive."
        ),
    },
    {
        "path": "2026-10-26/live-battery-2026-10-26.jsonl",
        "source": "demo-run/live-battery-2026-10-26.jsonl",
        "battery": "live-battery-2026-10-26",
        "records_dir": "2026-10-26/live-battery",
        "description": (
            "Live battery of 2026-10-26 (docs/evaluation.md sections 2 and 3), schema 1.1, "
            "hash-chained from genesis. L01-L10 and I01-I02: live Meta + Hyphae; every record "
            "retrieved carries a proof_error because its proven recall failed upstream (a "
            "deployment misconfiguration, see README), so every verification in those runs is "
            "invalid; L10 (other-repo) retrieved nothing. S01-S03: live Meta with SIMULATED proofs."
        ),
    },
    {
        "path": "2026-10-27/live-battery-2026-10-27-prefix.jsonl",
        "source": "demo-run/live-battery-2026-10-27-prefix.jsonl",
        "battery": "live-battery-2026-10-27-prefix",
        "records_dir": "2026-10-27/live-battery-2026-10-27-prefix",
        "description": (
            "Pre-fix runs P01-P02 (docs/evaluation.md section 5.1; not counted), schema 1.2, "
            "hash-chained from genesis. Live Meta + Hyphae demo store; proofs verified at run time with hyphae proof verify "
            "against the pinned demo-store anchor (see README)."
        ),
    },
    {
        "path": "2026-10-27/live-battery-2026-10-27.jsonl",
        "source": "demo-run/live-battery-2026-10-27.jsonl",
        "battery": "live-battery-2026-10-27",
        "records_dir": "2026-10-27/live-battery-2026-10-27",
        "description": (
            "Battery 1, R01-R08 (docs/evaluation.md section 5.2), schema 1.2, hash-chained from "
            "genesis. Live Meta + Hyphae demo store; proofs verified at run time with hyphae proof verify "
            "against the pinned demo-store anchor (see README)."
        ),
    },
    {
        "path": "2026-10-27/live-battery-2026-10-27-final.jsonl",
        "source": "demo-run/live-battery-2026-10-27-final.jsonl",
        "battery": "live-battery-2026-10-27-final",
        "records_dir": "2026-10-27/live-battery-2026-10-27-final",
        "description": (
            "Final battery F01-F06 (docs/evaluation.md section 5.3), schema 1.2, hash-chained from "
            "genesis. Live Meta + Hyphae demo store; proofs verified at run time with hyphae proof verify "
            "against the pinned demo-store anchor (see README)."
        ),
    },
    {
        "path": "2026-10-27/live-battery-2026-10-27-hostile.jsonl",
        "source": "demo-run/live-battery-2026-10-27-hostile.jsonl",
        "battery": "live-battery-2026-10-27-hostile",
        "records_dir": "2026-10-27/live-battery-2026-10-27-hostile",
        "description": (
            "Hostile-note runs H01-H02 (docs/evaluation.md section 5.4), schema 1.2, hash-chained "
            "from genesis. Live Meta + Hyphae lab store; proofs verified at run time with hyphae proof verify "
            "against the pinned lab-store anchor (see README). Contains the "
            "hostile note's text as retrieved evidence (a prompt-injection example, kept by design)."
        ),
    },
    {
        "path": "2026-10-27/live-battery-2026-10-27-p13-smoke.jsonl",
        "source": "demo-run/live-battery-2026-10-27-p13-smoke.jsonl",
        "battery": "live-battery-2026-10-27-p13-smoke",
        "records_dir": "2026-10-27/live-battery-2026-10-27-p13-smoke",
        "description": (
            "Smoke runs K01-K02 on tag prompt-freeze-1 (docs/evaluation.md section 6.1; superseded, "
            "not counted as validation), schema 1.3, hash-chained from genesis. Live Meta + Hyphae "
            "demo store; proofs verified at run time with hyphae proof verify against the pinned "
            "demo-store anchor (see README). K01 ended insufficient_evidence because the Evaluator "
            "returned empty assessments under that freeze's schema; K02 failed at critique on a "
            "post-send transport error that the adapter of that freeze did not retry."
        ),
    },
    {
        "path": "2026-10-27/live-battery-2026-10-27-pf2-smoke.jsonl",
        "source": "demo-run/live-battery-2026-10-27-pf2-smoke.jsonl",
        "battery": "live-battery-2026-10-27-pf2-smoke",
        "records_dir": "2026-10-27/live-battery-2026-10-27-pf2-smoke",
        "description": (
            "Smoke runs K03-K04 on tag prompt-freeze-2 (docs/evaluation.md section 6.2; not counted "
            "as validation), schema 1.3, hash-chained from genesis. Live Meta + Hyphae demo store; "
            "proofs verified at run time with hyphae proof verify against the pinned demo-store "
            "anchor (see README). Both ended supported for a revised proposal that departs from the "
            "task text (see section 6.2)."
        ),
    },
]

# Batteries run through the operator server's run store (consensus-api
# serve-operator): one chained log per run, from genesis, under
# <dir>/runs/<run_id>.jsonl (the store's own file, read after the server
# stopped), and one run record per run, <dir>/<NAME>.json, as
# scripts/live_battery.py writes it (receipt v2). The driver also saved the
# records served by GET /api/v1/runs/{id}/events.ndjson as <NAME>.jsonl;
# that copy had the same sha256 as the store's file for every run, so only
# the store's file is archived.
STORE_BATTERIES = [
    {
        "dir": "2026-10-30/live-battery-2026-10-30-p13",
        "source": "demo-run/battery-2026-10-29",
        "battery": "live-battery-2026-10-30-p13",
        "description": (
            "Protocol 1.3 battery of 2026-10-30, run {label} (task {task}), schema 1.3: this run's "
            "chained records from genesis, as the operator server's run store wrote them "
            "(consensus-api serve-operator built at commit f0f3815); byte-identical to the copy "
            "GET /api/v1/runs/{{id}}/events.ndjson served, {source}/results/{label}.jsonl. Live "
            "Meta muse-wonder-1.0 + Hyphae demo store; proofs verified at run time with hyphae "
            "proof verify against the pinned demo-store anchor (see README)."
        ),
    },
]

# Single-agent baseline batteries (docs/evaluation.md, "Single-agent
# baseline"; scripts/baseline_battery.py): one log per run under
# <dir>/runs/<run_id>.jsonl, written by `consensus-api baseline run`
# (schema_version baseline-1.0, run_kind single_agent_baseline on every
# record, chained from genesis with the consensus recipe), and one run record
# per run, <dir>/<NAME>.json, holding the summary the binary printed. Their
# files are pinned like every other (kinds baseline_log and baseline_record)
# and their runs are listed under "baseline_runs", never under "runs": a
# baseline run is not a consensus run, and the consensus tools (the event
# log, the showcase) refuse or ignore it. The key is written only once a
# baseline battery is archived, so the manifest is unchanged until then.
# Only runs the binary recorded (exit 0, completed or failed) on live Hyphae
# evidence are archived: a record with a non-zero or null exit code (the
# binary could not record the run, or the driver timed it out) and a log
# with no record are kept out of the battery directory and reported
# (docs/evaluation.md §7.2, step 5); the asserts below refuse them, and
# refuse a --smoke run (memory fake, SIMULATED proofs).
# Entry shape (none archived yet):
#   {"dir": "2026-11-0X/baseline-2026-11-0X", "source": "<operator dir>",
#    "battery": "baseline-2026-11-0X", "description": "... {label} ... {task} ..."}
BASELINE_BATTERIES = []

BASELINE_SCHEMA = "baseline-1.0"
BASELINE_RUN_KIND = "single_agent_baseline"
BASELINE_SUMMARY = "hyphae-consensus-baseline-summary-v1"

RECEIPT_V2 = "hyphae-consensus-run-receipt-v2"

# Run-record file tags that docs/evaluation.md labels differently.
LABEL_OVERRIDES = {
    ("2026-10-27/live-battery-2026-10-27-prefix", "R01-reference"): "P01",
    ("2026-10-27/live-battery-2026-10-27-prefix", "R02-reference-repeat"): "P02",
    ("2026-10-27/live-battery-2026-10-27-p13-smoke", "R03-raise-to-10"): "K01",
    ("2026-10-27/live-battery-2026-10-27-p13-smoke", "R06-timeout-30s"): "K02",
    ("2026-10-27/live-battery-2026-10-27-pf2-smoke", "R03-raise-to-10"): "K03",
    ("2026-10-27/live-battery-2026-10-27-pf2-smoke", "R06-timeout-30s"): "K04",
}


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def read(rel):
    with open(os.path.join(ROOT, rel), "rb") as fh:
        return fh.read()


def parse_log(data):
    text = data.decode("utf-8")
    assert text.endswith("\n")
    return [json.loads(line) for line in text[:-1].split("\n")]


def run_outcomes(events):
    runs = collections.OrderedDict()
    for ev in events:
        r = runs.setdefault(ev["run_id"], {"events": [], "versions": set()})
        r["events"].append(ev)
        r["versions"].add(ev["schema_version"])
    for rid, r in runs.items():
        assert len(r["versions"]) == 1, rid
        r["schema_version"] = next(iter(r["versions"]))
        outcome, auth, title, project = None, "pending", None, None
        for ev in r["events"]:
            p = ev["payload"]
            if p["type"] == "task_created":
                title, project = p["task"]["title"], p["task"]["project_id"]
            elif p["type"] == "recommendation_issued":
                outcome = p["recommendation"]["kind"]
            elif p["type"] == "run_failed":
                outcome = "failed:" + p["stage"]
            elif p["type"] == "human_decision_recorded":
                auth = "approved" if p["authorization"]["approved"] else "rejected"
        assert outcome is not None, rid
        r.update(outcome=outcome, authorization=auth, title=title, project=project)
    return runs


def record_versions(rec):
    versions = {ev["schema_version"] for ev in rec["detail"]["events"]}
    versions.add(rec["receipt"]["schema_version"])
    return sorted(versions)


# Fields the API v1 view adds to a stored event (display only, each false
# here) and fields of the stored event it leaves out.
VIEW_FLAGS = (("invocation", "scripted"), ("evidence", "proof_error_reported"), ("verification", "simulated"))
STORED_ONLY = (("invocation", "safety_identifier_sent"), ("evidence", "schema_version"), ("verification", "schema_version"))


def shown_event(event, rel):
    """An API v1 event without its display fields (each checked first)."""
    e = copy.deepcopy(event)
    p = e["payload"]
    if "message" in p:
        assert p["type"] == "run_failed" and isinstance(p["message"], str), rel
        del p["message"]
    for obj, field in VIEW_FLAGS:
        if isinstance(p.get(obj), dict) and field in p[obj]:
            assert p[obj].pop(field) is False, (rel, obj, field)
    return e


def stored_event(event):
    """A stored event without the fields the API v1 view leaves out."""
    e = copy.deepcopy(event)
    p = e["payload"]
    for obj, field in STORED_ONLY:
        if isinstance(p.get(obj), dict):
            p[obj].pop(field, None)
    return e


def store_battery(store, files, runs_out):
    """One STORE_BATTERIES entry: a log and a run record per run, cross-checked."""
    base = store["dir"]
    names = sorted(
        n for n in os.listdir(os.path.join(ROOT, base)) if os.path.isfile(os.path.join(ROOT, base, n))
    )
    logs_on_disk = set(os.listdir(os.path.join(ROOT, base, "runs")))
    logs_used = set()
    for name in names:
        assert name.endswith(".json"), name
        rel = base + "/" + name
        rdata = read(rel)
        rec = json.loads(rdata.decode("utf-8"))
        label = rec["name"]
        assert label == name[:-5], rel
        rid = rec["detail"]["summary"]["run_id"]
        log_name = rid + ".jsonl"
        log_rel = base + "/runs/" + log_name
        data = read(log_rel)
        events = parse_log(data)
        assert events[0]["prev_hash"] == "genesis" and events[0]["sequence"] == 1, log_rel
        runs = run_outcomes(events)
        assert list(runs) == [rid], log_rel
        r = runs[rid]
        receipt = rec["receipt"]
        assert receipt["format"] == RECEIPT_V2, rel
        assert receipt["run_id"] == rid, rel
        assert receipt["event_count"] == len(r["events"]), rel
        assert receipt["chain"]["head_hash"] == r["events"][-1]["hash"], rel
        assert receipt["authorization"]["state"] == r["authorization"], rel
        # The detail is the API v1 view of the events: whole events, once
        # the view's display fields and the stored-only fields are dropped.
        shown = [shown_event(e, rel) for e in rec["detail"]["events"]]
        stored = [stored_event(e) for e in r["events"]]
        assert shown == stored, rel
        kind = (receipt.get("recommendation") or {}).get("kind")
        expected = r["outcome"] if not r["outcome"].startswith("failed:") else None
        assert kind == expected, (rel, kind, expected)
        assert log_name not in logs_used, log_rel
        logs_used.add(log_name)
        files.append({
            "path": log_rel,
            "source": store["source"] + "/store/runs/" + log_name,
            "bytes": len(data),
            "sha256": sha256(data),
            "kind": "event_log",
            "schema_versions": sorted({ev["schema_version"] for ev in events}),
            "runs": 1,
            "events": len(events),
            "description": store["description"].format(
                label=label, task=rec["task_key"], source=store["source"]
            ),
        })
        files.append({
            "path": rel,
            "source": store["source"] + "/results/" + name,
            "bytes": len(rdata),
            "sha256": sha256(rdata),
            "kind": "run_record",
            "schema_versions": record_versions(rec),
            "runs": 1,
            "description": (
                f"Run record for {label} (task {rec['task_key']}), in the format "
                "scripts/live_battery.py writes for the operator server: the task key and text, "
                "the status and body of POST /api/v1/operator/runs, the run detail polled until "
                "the run rested (summary, events, model invocations), the receipt (v2) downloaded "
                "after the run, the status of the events.ndjson download, and the wall time."
            ),
        })
        runs_out.append({
            "run_id": rid,
            "label": label,
            "battery": store["battery"],
            "task_title": r["title"],
            "project": r["project"],
            "outcome": r["outcome"],
            "authorization": r["authorization"],
            "schema_version": r["schema_version"],
            "file": log_rel,
            "record": rel,
        })
    assert logs_used == logs_on_disk, (base, logs_on_disk - logs_used)


def chain_ok(data):
    """The consensus recipe on raw lines: sequence n on line n, prev_hash
    genesis then the previous hash, hash = sha256(prev_hash + the line with
    its own hash value blanked)."""
    prev = "genesis"
    for n, line in enumerate(data.decode("utf-8")[:-1].split("\n"), 1):
        rec = json.loads(line)
        assert rec["sequence"] == n and rec["prev_hash"] == prev, n
        field = '"hash":"%s"' % rec["hash"]
        assert line.count(field) == 1, n
        blanked = line.replace(field, '"hash":""', 1)
        assert sha256((prev + blanked).encode("utf-8")) == rec["hash"], n
        prev = rec["hash"]
    return True


def baseline_outcome(records):
    """The agent's verdict of a completed run, failed:<stage> of a failed one."""
    last = records[-1]["payload"]
    if last["type"] == "baseline_completed":
        return last["outcome"]["agent_verdict"]
    assert last["type"] == "baseline_failed", last["type"]
    return "failed:" + last["stage"]


def baseline_battery(entry, files, baseline_runs, consensus_runs):
    """One BASELINE_BATTERIES entry: a log and a run record per run, cross-checked."""
    base = entry["dir"]
    names = sorted(
        n for n in os.listdir(os.path.join(ROOT, base)) if os.path.isfile(os.path.join(ROOT, base, n))
    )
    logs_on_disk = set(os.listdir(os.path.join(ROOT, base, "runs")))
    logs_used = set()
    for name in names:
        assert name.endswith(".json"), name
        rel = base + "/" + name
        rdata = read(rel)
        rec = json.loads(rdata.decode("utf-8"))
        label = rec["name"]
        assert label == name[:-5], rel
        summary = rec["summary"]
        assert rec["exit_code"] == 0 and summary["format"] == BASELINE_SUMMARY, rel
        rid = summary["run_id"]
        log_name = rid + ".jsonl"
        log_rel = base + "/runs/" + log_name
        data = read(log_rel)
        records = parse_log(data)
        assert chain_ok(data), log_rel
        assert all(
            r["schema_version"] == BASELINE_SCHEMA and r["run_kind"] == BASELINE_RUN_KIND
            and r["run_id"] == rid
            for r in records
        ), log_rel
        task = records[0]["payload"]
        assert task["type"] == "task_created", log_rel
        # Live Hyphae evidence judged by the Meta model, never a --smoke run
        # (memory fake, SIMULATED proofs) or a run without a recorded mode.
        mode = task["task"].get("mode") or {}
        assert rec["memory"] == "hyphae", (rel, rec["memory"])
        assert mode.get("evidence") == "live_hyphae" and mode.get("model") == "meta", (log_rel, mode)
        assert summary["evidence_mode"] == "live_hyphae" and summary["evidence_warning"] is None, rel
        outcome = baseline_outcome(records)
        assert summary["run_kind"] == BASELINE_RUN_KIND, rel
        assert summary["event_count"] == len(records), rel
        assert summary["head_hash"] == records[-1]["hash"], rel
        assert summary["log_sha256"] == sha256(data), rel
        assert summary["outcome"] == outcome, (rel, summary["outcome"], outcome)
        assert summary["log"] == "runs/" + log_name, rel
        consensus = rec.get("consensus")
        if consensus:
            # The paired consensus run is archived, of the same task, and
            # its pinned log is the one the baseline recalled with.
            paired = [r for r in consensus_runs if r["label"] == consensus["label"]]
            assert len(paired) == 1 and paired[0]["run_id"] == consensus["run_id"], rel
            assert paired[0]["task_title"] == task["task"]["title"], rel
            source = summary["recall"]["source"]
            assert source == {
                "kind": "consensus_log",
                "log_sha256": consensus["sha256"],
                "run_id": consensus["run_id"],
            }, rel
            pinned = [f for f in files if f["path"] == paired[0]["file"]]
            assert len(pinned) == 1 and pinned[0]["sha256"] == consensus["sha256"], rel
        assert log_name not in logs_used, log_rel
        logs_used.add(log_name)
        files.append({
            "path": log_rel,
            "source": entry["source"] + "/runs/" + log_name,
            "bytes": len(data),
            "sha256": sha256(data),
            "kind": "baseline_log",
            "schema_versions": [BASELINE_SCHEMA],
            "runs": 1,
            "events": len(records),
            "description": entry["description"].format(label=label, task=rec["task_key"]),
        })
        files.append({
            "path": rel,
            "source": entry["source"] + "/" + name,
            "bytes": len(rdata),
            "sha256": sha256(rdata),
            "kind": "baseline_record",
            "schema_versions": [BASELINE_SCHEMA],
            "runs": 1,
            "description": (
                f"Run record for the single-agent baseline run {label} (task {rec['task_key']}), "
                "as scripts/baseline_battery.py writes it: the task key and text, the paired "
                "consensus run (label, archived log, sha256), the exit code of consensus-api "
                "baseline run, the summary it printed (read back from the run's log) and the "
                "wall time. Not a consensus run: no recommendation, no decision."
            ),
        })
        baseline_runs.append({
            "run_id": rid,
            "label": label,
            "battery": entry["battery"],
            "run_kind": BASELINE_RUN_KIND,
            "task_title": task["task"]["title"],
            "project": task["task"]["project_id"],
            "outcome": outcome,
            "consensus_label": consensus["label"] if consensus else None,
            "evidence": mode["evidence"],
            "schema_version": BASELINE_SCHEMA,
            "file": log_rel,
            "record": rel,
        })
    assert logs_used == logs_on_disk, (base, logs_on_disk - logs_used)


def main():
    files, runs_out = [], []
    for log in LOGS:
        data = read(log["path"])
        events = parse_log(data)
        runs = run_outcomes(events)
        files.append({
            "path": log["path"],
            "source": log["source"],
            "bytes": len(data),
            "sha256": sha256(data),
            "kind": "event_log",
            "schema_versions": sorted({ev["schema_version"] for ev in events}),
            "runs": len(runs),
            "events": len(events),
            "description": log["description"],
        })
        by_run = {}
        if log["records_dir"]:
            names = sorted(os.listdir(os.path.join(ROOT, log["records_dir"])))
            for name in names:
                rel = log["records_dir"] + "/" + name
                rdata = read(rel)
                rec = json.loads(rdata.decode("utf-8"))
                tag = rec["tag"]
                assert tag == name[:-5], rel
                label = LABEL_OVERRIDES.get((log["records_dir"], tag), tag[:3])
                rid = rec["detail"]["summary"]["run_id"]
                assert rid in runs, rel
                receipt = rec["receipt"]
                r = runs[rid]
                kind = (receipt.get("recommendation") or {}).get("kind")
                expected = r["outcome"] if not r["outcome"].startswith("failed:") else None
                assert kind == expected, (rel, kind, expected)
                assert receipt["run_id"] == rid
                assert receipt["event_count"] == len(r["events"]), rel
                assert receipt["head_hash"] == r["events"][-1]["hash"], rel
                assert rid not in by_run, rel
                by_run[rid] = (label, rel)
                desc = (
                    f"Run record for {label} ({tag}), in the format scripts/live_battery.py "
                    "writes: the task, the HTTP status and wall time of POST /api/runs, the "
                    "console's run detail (summary, events, model invocations, verifications) "
                    "and the receipt downloaded after the run."
                )
                if "note" in rec:
                    desc += (
                        " The driver stopped before this run returned, so http_status and "
                        "wall_seconds are null; detail and receipt were fetched from the API "
                        "afterwards (see the record's note field)."
                    )
                files.append({
                    "path": rel,
                    "source": "demo-run/" + rel.split("/", 1)[1],
                    "bytes": len(rdata),
                    "sha256": sha256(rdata),
                    "kind": "run_record",
                    "schema_versions": record_versions(rec),
                    "runs": 1,
                    "description": desc,
                })
            assert set(by_run) == set(runs), log["path"]
        for n, (rid, r) in enumerate(runs.items(), 1):
            if log["records_dir"]:
                label, record = by_run[rid]
            else:
                label, record = f"C{n:02d}", None
            runs_out.append({
                "run_id": rid,
                "label": label,
                "battery": log["battery"],
                "task_title": r["title"],
                "project": r["project"],
                "outcome": r["outcome"],
                "authorization": r["authorization"],
                "schema_version": r["schema_version"],
                "file": log["path"],
                "record": record,
            })
    for store in STORE_BATTERIES:
        store_battery(store, files, runs_out)
    baseline_runs = []
    for entry in BASELINE_BATTERIES:
        baseline_battery(entry, files, baseline_runs, runs_out)
    labels = [r["label"] for r in runs_out + baseline_runs]
    assert len(labels) == len(set(labels)), "duplicate label"
    manifest = {
        "schema": "hyphae-consensus-research-manifest-v1",
        "created": "2026-10-30",
        "source": "demo-run/ (git-ignored original, unmodified)",
        "files": files,
        "runs": runs_out,
    }
    if baseline_runs:
        manifest["baseline_runs"] = baseline_runs
    with open(os.path.join(ROOT, "MANIFEST.json"), "w", encoding="utf-8", newline="\n") as fh:
        json.dump(manifest, fh, indent=2, ensure_ascii=True)
        fh.write("\n")
    with open(os.path.join(ROOT, "SHA256SUMS"), "w", encoding="utf-8", newline="\n") as fh:
        for f in sorted(files, key=lambda f: f["path"]):
            fh.write(f"{f['sha256']}  {f['path']}\n")
    print(f"files={len(files)} runs={len(runs_out)}")
    print(collections.Counter(r["outcome"] for r in runs_out))
    if baseline_runs:
        print(f"baseline_runs={len(baseline_runs)}")
        print(collections.Counter(r["outcome"] for r in baseline_runs))


if __name__ == "__main__":
    main()
