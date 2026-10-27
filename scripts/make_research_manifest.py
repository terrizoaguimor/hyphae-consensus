# SPDX-License-Identifier: Apache-2.0
# Usage: python -I scripts/make_research_manifest.py research
# Build research/MANIFEST.json and research/SHA256SUMS from the copied
# files. Reads research/ only (never demo-run/), writes only those two files.
# Cross-checks every run record against its event log before writing.
import collections
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
]

# Run-record file tags that docs/evaluation.md labels differently.
LABEL_OVERRIDES = {
    ("2026-10-27/live-battery-2026-10-27-prefix", "R01-reference"): "P01",
    ("2026-10-27/live-battery-2026-10-27-prefix", "R02-reference-repeat"): "P02",
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
    labels = [r["label"] for r in runs_out]
    assert len(labels) == len(set(labels)), "duplicate label"
    manifest = {
        "schema": "hyphae-consensus-research-manifest-v1",
        "created": "2026-10-27",
        "source": "demo-run/ (git-ignored original, unmodified)",
        "files": files,
        "runs": runs_out,
    }
    with open(os.path.join(ROOT, "MANIFEST.json"), "w", encoding="utf-8", newline="\n") as fh:
        json.dump(manifest, fh, indent=2, ensure_ascii=True)
        fh.write("\n")
    with open(os.path.join(ROOT, "SHA256SUMS"), "w", encoding="utf-8", newline="\n") as fh:
        for f in sorted(files, key=lambda f: f["path"]):
            fh.write(f"{f['sha256']}  {f['path']}\n")
    print(f"files={len(files)} runs={len(runs_out)}")
    print(collections.Counter(r["outcome"] for r in runs_out))


if __name__ == "__main__":
    main()
