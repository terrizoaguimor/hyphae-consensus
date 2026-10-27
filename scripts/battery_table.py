"""Summarize live battery run records (from live_battery.py) as Markdown.

Every number comes from the saved run detail and receipt; nothing is typed
in by hand. Usage: python scripts/battery_table.py RUN_JSON [RUN_JSON ...]
"""

import json
import sys


def rounds(events):
    """Per-round (critique items, evaluation summary) pairs."""
    out, items = [], None
    for e in events:
        p = e["payload"]
        if p["type"] == "challenge_submitted":
            items = len(p["challenge"]["items"])
        if p["type"] == "evaluation_submitted":
            ev = p["evaluation"]
            corroborated = sum(a["corroborated"] for a in ev["assessments"])
            contradicting = sum(bool(a["contradicting_evidence"]) for a in ev["assessments"])
            out.append(
                {
                    "items": items,
                    "corroborated": corroborated,
                    "claims": len(ev["assessments"]),
                    "unresolved": len(ev["unresolved_items"]),
                    "abstain": ev["abstain"],
                    "contradicting_claims": contradicting,
                }
            )
    return out


def integrity(verifications):
    """Integrity counts over the final round's verification records."""
    counts = {}
    for v in verifications:
        counts[v["integrity_status"]] = counts.get(v["integrity_status"], 0) + 1
    return ", ".join(f"{n} {k}" for k, n in sorted(counts.items())) or "0 retrieved"


def row(path):
    rec = json.load(open(path, encoding="utf-8"))
    detail = rec["detail"]
    events = detail.get("events", [])
    receipt = rec.get("receipt") or {}
    summary = detail.get("summary", {})
    invocations = receipt.get("model_invocations") or detail.get("model_invocations", [])
    failed = next((e["payload"] for e in events if e["payload"]["type"] == "run_failed"), None)
    outcome = summary.get("recommendation") or summary.get("state")
    if failed:
        outcome = f"failed ({failed['stage']}: {failed['reason'][:60]})"
    # Verifications recorded after the last evaluation = the final round.
    last_eval = max(
        (i for i, e in enumerate(events) if e["payload"]["type"] == "evaluation_submitted"),
        default=None,
    )
    final_verifications = [
        e["payload"]["verification"]
        for i, e in enumerate(events)
        if e["payload"]["type"] == "evidence_verified" and last_eval is not None and i > last_eval
    ]
    per_round = rounds(events)
    critic = " → ".join(str(r["items"]) for r in per_round) or "—"
    evaluator = " → ".join(
        f"{r['corroborated']}/{r['claims']}, {r['unresolved']} open"
        + (", abstained" if r["abstain"] else "")
        + (f", {r['contradicting_claims']} contradicted" if r["contradicting_claims"] else "")
        for r in per_round
    ) or "—"
    latency = sum(i.get("latency_ms") or 0 for i in invocations) / 1000
    tokens_in = sum(i.get("input_tokens") or 0 for i in invocations)
    tokens_out = sum(i.get("output_tokens") or 0 for i in invocations)
    task = rec["task"]
    tag = rec["tag"].split("-")[0]
    return (
        f"| {tag} | {task['title']} (`{task['project']}`) | **{outcome}** | "
        f"{detail.get('revision_round', 0)} | {critic} | {evaluator} | "
        f"{integrity(final_verifications)} | {len(invocations)} | {latency:.1f} s | "
        f"{tokens_in:,} / {tokens_out:,} |"
    )


def main():
    print(
        "| Run | Task (project) | Outcome | Revisions | Critic items per round | "
        "Evaluator per round (corroborated/claims, open items) | Final-round integrity | "
        "Calls | Model latency | Tokens in/out |"
    )
    print("|---|---|---|---|---|---|---|---|---|---|")
    for path in sys.argv[1:]:
        print(row(path))


if __name__ == "__main__":
    main()
