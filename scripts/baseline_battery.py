# SPDX-License-Identifier: Apache-2.0
"""Drive the single-agent baseline battery (stdlib only).

Each task is one `consensus-api baseline run` (one Meta call, Hyphae
evidence recalled and verified by the runtime): the same task texts as the
consensus battery (TASKS of scripts/live_battery.py, loaded from that file,
never copied), the same model (the operator's Meta configuration, read by
the binary from the environment) and, by default, the same recall queries as
the archived consensus run of the same task: every claim statement of that
run, so the baseline recalls the records that run's final round held in
context, given the same store and anchor.

The planned battery (docs/evaluation.md, "Single-agent baseline", not run
yet) pairs each task with its 2026-10-30 consensus run:

  B01=R01-reference@M01  B02=R05-add-jitter@M02
  B03=R07-exp-backoff@M03  B04=R04-remove-cap@M04

For a pair, the consensus run's log is located through research/MANIFEST.json
(its label), its bytes checked against the pinned sha256 and copied to
OUT_DIR/consensus/<label>.jsonl; the binary reads only that copy, never
research/. A task given without @LABEL recalls with its title and description
instead.

Per task the script writes the run's log to OUT_DIR/runs/<run_id>.jsonl (the
binary's own file) and the run record OUT_DIR/<NAME>.json: the task key and
text, the consensus pairing (label, archived path, sha256), the binary's exit
code, the summary it printed (everything read back from the log), the
model-settings parity with the paired consensus run and the wall time. These
two are what scripts/make_research_manifest.py archives (BASELINE_BATTERIES).
The script never records a decision, never prints or writes a credential,
and stops at the first task whose run could not be recorded: a non-zero exit
gives a record with the binary's error, a run past RUN_TIMEOUT_SECONDS a
record with exit_code null and error "timeout" (the binary may have left a
partial log in OUT_DIR/runs/). Neither is archived (the manifest script
refuses both, and a log without a record); they are reported instead.

Parity: `consensus-api baseline run --queries-from` refuses, before its call,
a Meta configuration whose model (and, when recorded, max_completion_tokens
and reasoning_effort) differs from the paired consensus run's calls. The
record's "parity" block shows both sides as recorded, read from the paired
log and from the baseline's invocation, and the script stops on a mismatch.

The Meta and Hyphae settings are the ones `consensus-api serve-operator`
reads (scripts/hyphae-container/live-env.ps1, with a pinned
CONSENSUS_HYPHAE_EXPECTED_ANCHOR). The baseline is not metered by the live
ledger: the per-run token cap CONSENSUS_MAX_RUN_TOKENS applies, and each run
makes exactly one call. Its spend is outside the ledger's daily and total
caps, so the script prints each run's input and output tokens and the
running totals, to reconcile by hand against the owner's approval.

Usage: python -I scripts/baseline_battery.py [--smoke] CONSENSUS_API OUT_DIR [TASK ...]
  --smoke        memory=fake: the scripted reference records with SIMULATED
                 proofs, to check the driver against a fake model endpoint;
                 never for the battery (its records say memory simulated)
  CONSENSUS_API  the consensus-api binary built at the commit under test
  TASK           NAME=KEY or NAME=KEY@LABEL (a TASKS key of live_battery.py,
                 a consensus run label of research/MANIFEST.json); none: the
                 planned battery above
"""

import hashlib
import importlib.util
import json
import os
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(HERE)
RESEARCH = os.path.join(REPO, "research")

PLANNED = [
    "B01=R01-reference@M01",
    "B02=R05-add-jitter@M02",
    "B03=R07-exp-backoff@M03",
    "B04=R04-remove-cap@M04",
]

# One run is one model call plus recall and verification; the binary's own
# Meta call budget is shorter.
RUN_TIMEOUT_SECONDS = 30 * 60


def load_tasks():
    """TASKS of scripts/live_battery.py, loaded by path (python -I keeps the
    script's directory off sys.path)."""
    spec = importlib.util.spec_from_file_location(
        "live_battery", os.path.join(HERE, "live_battery.py")
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.TASKS


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def parse_task(arg, tasks):
    name, sep, rest = arg.partition("=")
    if not sep or not name or not rest:
        sys.exit(f"task {arg!r}: expected NAME=KEY or NAME=KEY@LABEL")
    key, _, label = rest.partition("@")
    if key not in tasks:
        sys.exit(f"unknown task {key!r}; known: {', '.join(sorted(tasks))}")
    return name, key, label or None


def consensus_log(label, task):
    """(archived path, sha256, bytes) of the consensus run `label`, checked
    against MANIFEST.json and against the task."""
    with open(os.path.join(RESEARCH, "MANIFEST.json"), encoding="utf-8") as fh:
        manifest = json.load(fh)
    runs = [r for r in manifest["runs"] if r["label"] == label]
    if len(runs) != 1:
        sys.exit(f"consensus run {label!r}: not exactly one run with that label in MANIFEST.json")
    run = runs[0]
    project, title, _ = task
    if run["task_title"] != title or run["project"] != project:
        sys.exit(f"consensus run {label!r} is of another task ({run['task_title']!r})")
    pinned = [f for f in manifest["files"] if f["path"] == run["file"]]
    if len(pinned) != 1:
        sys.exit(f"consensus run {label!r}: its log is not pinned in MANIFEST.json")
    with open(os.path.join(RESEARCH, run["file"]), "rb") as fh:
        data = fh.read()
    if sha256(data) != pinned[0]["sha256"]:
        sys.exit(f"{run['file']}: sha256 differs from MANIFEST.json")
    return run["file"], pinned[0]["sha256"], data, run["run_id"]


SETTINGS = ("model", "max_completion_tokens", "reasoning_effort")


def call_settings(data, run_id):
    """The distinct (model, max_completion_tokens, reasoning_effort) of every
    model call run `run_id` recorded in a consensus log."""
    seen = []
    for line in data.decode("utf-8").splitlines():
        event = json.loads(line)
        if event["run_id"] != run_id:
            continue
        invocation = event["payload"].get("invocation")
        if isinstance(invocation, dict):
            settings = {k: invocation.get(k) for k in SETTINGS}
            if settings not in seen:
                seen.append(settings)
    return seen


def parity(consensus_data, run_id, summary):
    """Both sides of the model settings, and whether they agree."""
    consensus = call_settings(consensus_data, run_id)
    calls = summary.get("model_invocations") or []
    baseline = [{k: c.get(k) for k in SETTINGS} for c in calls]
    return {
        "consensus": consensus,
        "baseline": baseline,
        "equal": len(consensus) == 1 and all(b == consensus[0] for b in baseline),
    }


def main():
    args = sys.argv[1:]
    memory = "hyphae"
    if args[:1] == ["--smoke"]:
        memory, args = "fake", args[1:]
    if len(args) < 2:
        sys.exit(__doc__)
    binary, out_dir = args[0], args[1]
    tasks = load_tasks()
    planned = [parse_task(arg, tasks) for arg in (args[2:] or PLANNED)]
    names = [name for name, _, _ in planned]
    if len(names) != len(set(names)):
        sys.exit("task names repeat")
    runs_dir = os.path.join(out_dir, "runs")
    os.makedirs(runs_dir, exist_ok=True)
    totals = [0, 0]
    for name, key, label in planned:
        record_path = os.path.join(out_dir, f"{name}.json")
        if os.path.exists(record_path):
            sys.exit(f"{record_path} exists: one record per name, never overwritten")
        project, title, description = tasks[key]
        argv = [
            binary, "baseline", "run",
            "--project", project,
            "--title", title,
            "--description", description,
            "--out-dir", runs_dir,
            "--memory", memory,
        ]
        consensus = None
        if label:
            path, digest, data, run_id = consensus_log(label, tasks[key])
            copy_dir = os.path.join(out_dir, "consensus")
            os.makedirs(copy_dir, exist_ok=True)
            copy = os.path.join(copy_dir, f"{label}.jsonl")
            with open(copy, "wb") as fh:
                fh.write(data)
            argv += ["--queries-from", copy, "--run", run_id]
            consensus = {"label": label, "file": path, "sha256": digest, "run_id": run_id}
        started = time.monotonic()
        try:
            done = subprocess.run(argv, capture_output=True, timeout=RUN_TIMEOUT_SECONDS)
            exit_code = done.returncode
        except subprocess.TimeoutExpired:
            done, exit_code = None, None
        record = {
            "name": name,
            "task_key": key,
            "task": {"project": project, "title": title, "description": description},
            "consensus": consensus,
            "memory": memory,
            "exit_code": exit_code,
            "summary": None,
            "wall_seconds": round(time.monotonic() - started, 1),
        }
        if exit_code == 0:
            summary = json.loads(done.stdout.decode("utf-8"))
            # The log's path relative to OUT_DIR: an absolute local path
            # names the operator's machine and is not archived.
            summary["log"] = "runs/" + os.path.basename(summary["log"])
            record["summary"] = summary
            if label:
                record["parity"] = parity(data, run_id, summary)
        elif done is None:
            record["error"] = "timeout"
        else:
            # The binary's own message (fixed texts, no credential).
            record["error"] = done.stderr.decode("utf-8", errors="replace")[-2000:]
        with open(record_path, "w", encoding="utf-8", newline="\n") as fh:
            json.dump(record, fh, indent=1)
            fh.write("\n")
        summary = record["summary"] or {}
        calls = summary.get("model_invocations") or []
        tokens_in = sum(c.get("input_tokens") or 0 for c in calls)
        tokens_out = sum(c.get("output_tokens") or 0 for c in calls)
        totals[0] += tokens_in
        totals[1] += tokens_out
        evidence = summary.get("evidence_mode") or f"memory {memory}"
        print(
            f"{'SIMULATED ' if memory != 'hyphae' else ''}"
            f"{name} ({key}{'@' + label if label else ''}): exit {exit_code}, "
            f"evidence {evidence}, run {summary.get('run_id')}, {record['wall_seconds']} s, "
            f"outcome {summary.get('outcome')}, policy reading "
            f"{(summary.get('policy_reading') or {}).get('kind')}, "
            f"tokens in/out {tokens_in}/{tokens_out} (totals {totals[0]}/{totals[1]})",
            flush=True,
        )
        if exit_code != 0:
            sys.exit(f"{name}: the run could not be recorded ({record['error'][:200]}); stopping")
        if label and not record["parity"]["equal"]:
            sys.exit(f"{name}: model settings differ from {label}'s: {record['parity']}; stopping")


if __name__ == "__main__":
    main()
