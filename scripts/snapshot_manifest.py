# SPDX-License-Identifier: Apache-2.0
"""Build and check the manifest of a sealed Hyphae snapshot directory.

Usage (standard library only; run with -I so nothing is imported from the
snapshot directory or the current directory):

  python -I scripts/snapshot_manifest.py build <snapshot dir> [--captured-at T] [--expect-ids FILE] [REVERIFY]
  python -I scripts/snapshot_manifest.py check <snapshot dir> [--public-only] [--expect-ids FILE] [REVERIFY]
  python -I scripts/snapshot_manifest.py selftest
  REVERIFY: --reverify <hyphae binary or wrapper> [--verifier-dir <dir as the verifier sees it>] [--reverify-timeout SECS]

build reads <snapshot dir>/captures.index and session.json (both written by
scripts/hyphae-container/capture-snapshot.sh inside the container) plus the
capture files under captures/, cross-checks every index line against the
response file (proof_blake3, anchor_hex, memory ids, projects) and the
verifier report (status verified, kind memory, scope semantic_reexecution,
semantic re-execution performed, anchor_digest), requires the sha256 of every
capture file (response, proof, witness, verifier report) to equal the digest
the session recorded in captures.index the moment the file was written (the
only thing that binds a proof or a witness to ITS capture once the directory
has left the container: every proof of one session shares the anchor, so the
verifier alone cannot tell a swapped pair), requires the two required
captures to carry different proofs, computes the sha256 of every file, and
writes manifest.json (keys in the documented order) and SHA256SUMS. It then
runs check.

check recomputes every digest, validates the manifest schema (field presence,
key order, patterns, the two required captures, memory_ids equal to the
response ids, one anchor everywhere), requires captures.index and the
capture-time digests it holds, and exits non-zero with one precise message on
the first mismatch. --public-only skips the digest check of the .proof and
.witness files (they are git-ignored, so a checkout of the committed subset
lacks them) while still requiring their SHA256SUMS lines. With --reverify the
verifier is run again on every capture, exactly as the runtime runs it
(hyphae proof verify --proof F --witness F --anchor HEX), and the fresh
report must equal the captured one field by field (its proof, witness,
request and result digests are deterministic): this is the deployment check
that the proof and witness in hand are the pair verified at capture time.
--verifier-dir names the snapshot directory as the verifier sees it (the
container path when the verifier is scripts/hyphae-docker.cmd); without it
the verifier gets this directory's absolute paths.

selftest builds a synthetic snapshot in a temporary directory (fake proof and
witness bytes, synthetic responses and verifier reports, a fake verifier),
checks it, and asserts that a one-byte change in any file, a missing file, an
extra file, a swapped proof/witness pair, a swapped report or response and
every cross-check violation are detected, and that --reverify accepts the
pristine snapshot and refuses a report that the verifier does not reproduce.

The layout, the manifest schema and their meaning are specified in
docs/architecture/sealed-snapshot.md (ADR-0008). This file is the only
writer of manifest.json and SHA256SUMS; the runtime only reads them.
"""

import argparse
import datetime
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

MANIFEST_SCHEMA = "hyphae-consensus-snapshot-manifest-v1"
SESSION_SCHEMA = "hyphae-consensus-capture-session-v1"
CONTROL_SCHEMA = "hyphae-omarchy-control-v1"
PROOF_SCOPE = "memory-snapshot-v1"
VERIFIED_STATUS = "verified"
VERIFIED_KIND = "memory"
VERIFIED_SCOPE = "semantic_reexecution"
# The snapshot serves the 7 synthetic demo-repo facts (ADR-0008); a list
# capture with any other count is not this snapshot.
REQUIRED_RECORD_COUNT = 7
LIST_CAPTURE_ID = "list-demo-repo"
ISOLATION_CAPTURE_ID = "recall-other-repo-isolation"
ISOLATION_ARGUMENTS = {
    "project": "other-repo",
    "query": "Fix payment client timeouts",
    "limit": 8,
    "prove": True,
}
LIST_LIMIT = 64

MANIFEST_KEYS = [
    "schema", "snapshot_id", "captured_at", "project", "anchor_hex", "hyphae", "store", "captures",
]
HYPHAE_KEYS = ["version", "binary_sha256", "control_schema"]
STORE_KEYS = ["record_count", "root_digest", "visible_csn", "logical_time_micros"]
CAPTURE_KEYS = [
    "id", "operation", "arguments", "response_file", "proof_file", "witness_file", "verify_file",
    "proof_blake3", "proof_sha256", "witness_sha256", "memory_ids", "verification",
]
ARGUMENT_KEYS = ["project", "query", "limit", "prove"]
VERIFICATION_KEYS = ["verified_at", "status", "kind", "scope", "anchor_hex"]
INDEX_KEYS = [
    "id", "operation", "arguments", "files", "sha256", "proof_blake3", "anchor_hex", "memory_ids",
    "captured_at", "verified_at",
]
INDEX_FILE_KEYS = ["response", "proof", "witness", "verify"]
OPERATIONS = ("list", "recall")
# Verifier report fields that are deterministic functions of the proof (and
# therefore differ between the two required captures: a list of 7 records and
# an empty recall seal different requests and different results).
REPORT_PROOF_DIGESTS = ("proof_digest", "request_digest", "result_digest")
# Never handed to a verifier child process (as the runtime does, MA-4).
META_KEY_VARIABLES = ("CONSENSUS_META_API_KEY", "MODEL_API_KEY", "META_API_KEY")
DEFAULT_REVERIFY_TIMEOUT = 600
MISSING = object()

ID_RE = re.compile(r"^[a-z0-9][a-z0-9-]{0,63}$")
HEX64_RE = re.compile(r"^[0-9a-f]{64}$")
HEX_RE = re.compile(r"^[0-9a-f]+$")
DECIMAL_RE = re.compile(r"^[0-9]+$")
RFC3339_RE = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")
PROJECT_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")
SUMS_LINE_RE = re.compile(r"^([0-9a-f]{64})  ([^\s/\\][^\\]*)$")
SUMS_NAME = "SHA256SUMS"
MANIFEST_NAME = "manifest.json"
INDEX_NAME = "captures.index"
SESSION_NAME = "session.json"
PRIVATE_SUFFIXES = (".proof", ".witness")


class CheckError(Exception):
    """A precise, single-sentence reason why the snapshot is not acceptable."""


# ----------------------------------------------------------------- helpers
def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as fh:
        while True:
            chunk = fh.read(1 << 20)
            if not chunk:
                break
            digest.update(chunk)
    return digest.hexdigest()


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def load_json(path, what):
    try:
        with open(path, "rb") as fh:
            data = fh.read()
    except OSError as exc:
        raise CheckError(f"{what}: cannot read {path}: {exc.strerror}") from None
    try:
        return json.loads(data.decode("utf-8"))
    except (UnicodeDecodeError, ValueError) as exc:
        raise CheckError(f"{what}: {path} is not valid UTF-8 JSON: {exc}") from None


def require(condition, message):
    if not condition:
        raise CheckError(message)


def require_keys(obj, keys, what):
    require(isinstance(obj, dict), f"{what} must be a JSON object")
    actual = list(obj.keys())
    require(
        actual == keys,
        f"{what} keys must be exactly {keys} in this order, found {actual}",
    )


def require_hex64(value, what):
    require(isinstance(value, str) and HEX64_RE.match(value), f"{what} must be 64 lowercase hex digits")


def require_rfc3339(value, what):
    require(isinstance(value, str) and RFC3339_RE.match(value), f"{what} must be RFC 3339 UTC with seconds (YYYY-MM-DDTHH:MM:SSZ)")
    try:
        datetime.datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError:
        raise CheckError(f"{what} is not a valid timestamp: {value}") from None


def require_int(value, what, minimum=0):
    require(isinstance(value, int) and not isinstance(value, bool), f"{what} must be an integer")
    require(value >= minimum, f"{what} must be at least {minimum}")


def utc_now():
    return datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def one_line(text, limit):
    flat = " ".join(text.split())
    return flat if len(flat) <= limit else flat[: limit - 3] + "..."


def require_distinct(pairs, what):
    """pairs: (capture id, value); the same value under two captures is a failure."""
    seen = {}
    for cid, value in pairs:
        require(
            value not in seen,
            f"{what} of capture {cid} equals that of capture {seen.get(value)} ({value}): two captures cannot share one proof",
        )
        seen[value] = cid


def require_required_captures_distinct(proof_sha256s, proof_blake3s, reports):
    """The two required captures seal different requests and results, so their
    proofs, and the verifier digests derived from them, must differ (the
    witness may be identical: same store, same anchor)."""
    both = (LIST_CAPTURE_ID, ISOLATION_CAPTURE_ID)
    require_distinct([(cid, proof_sha256s[cid]) for cid in both], "proof_sha256")
    require_distinct([(cid, proof_blake3s[cid]) for cid in both], "proof_blake3")
    for key in REPORT_PROOF_DIGESTS:
        values = [(cid, reports[cid].get(key)) for cid in both]
        if all(isinstance(value, str) for _, value in values):
            require_distinct(values, f"verifier report {key}")


class Reverify:
    """How to run the verifier again: the command (one binary or wrapper, as
    the runtime's CONSENSUS_HYPHAE_BIN), the snapshot directory as that
    verifier sees it (None: this directory's absolute paths) and a deadline."""

    def __init__(self, command, verifier_dir=None, timeout=DEFAULT_REVERIFY_TIMEOUT):
        self.command = list(command)
        self.verifier_dir = verifier_dir
        self.timeout = timeout

    def path(self, root, rel):
        if self.verifier_dir is None:
            return os.path.abspath(os.path.join(root, rel))
        return self.verifier_dir.rstrip("/") + "/" + rel


def reverify_capture(root, cid, anchor, captured, reverify):
    """Run `hyphae proof verify` on the capture's proof and witness and require
    the fresh report to equal the captured one on every field."""
    what = f"capture {cid} re-verification"
    paths = capture_paths(cid)
    argv = reverify.command + [
        "proof", "verify",
        "--proof", reverify.path(root, paths["proof"]),
        "--witness", reverify.path(root, paths["witness"]),
        "--anchor", anchor,
    ]
    env = {key: value for key, value in os.environ.items() if key not in META_KEY_VARIABLES}
    try:
        proc = subprocess.run(argv, capture_output=True, timeout=reverify.timeout, env=env)
    except subprocess.TimeoutExpired:
        raise CheckError(f"{what}: hyphae proof verify did not finish within {reverify.timeout} s") from None
    except OSError as exc:
        raise CheckError(f"{what}: the verifier {reverify.command[0]!r} could not run: {exc}") from None
    stdout = proc.stdout.decode("utf-8", errors="replace")
    stderr = proc.stderr.decode("utf-8", errors="replace")
    require(
        proc.returncode == 0,
        f"{what}: hyphae proof verify exited {proc.returncode}: {one_line(stderr or stdout, 300) or '(no output)'}",
    )
    try:
        fresh = json.loads(stdout)
    except ValueError:
        raise CheckError(f"{what}: hyphae proof verify exited 0 but printed no JSON report: {one_line(stdout, 300)!r}") from None
    check_verify_report(fresh, anchor, f"{what}: fresh report")
    for key in sorted(set(captured) | set(fresh)):
        before, now = captured.get(key, MISSING), fresh.get(key, MISSING)
        require(
            before == now,
            f"{what}: the fresh verifier report differs from the captured one at {key!r} "
            f"(captured {'<absent>' if before is MISSING else repr(before)}, now {'<absent>' if now is MISSING else repr(now)}): "
            "the proof or witness in hand is not the pair verified at capture time",
        )


# The layout is flat: files in the root and in one directory level (captures/).
# The runtime refuses anything deeper (layout_too_deep) and every symbolic
# link (symlink_refused); this tool refuses the same, so a directory that
# passes `check` also boots.
MAX_DEPTH = 2


def relative_files(root):
    """Every regular file under root, as sorted forward-slash relative paths, except SHA256SUMS.

    Symbolic links (files or directories) and directories nested below the
    first level are refused with the runtime's codes.
    """
    found = []
    for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
        rel_dir = os.path.relpath(dirpath, root)
        depth = 0 if rel_dir == "." else rel_dir.count(os.sep) + 1
        dirnames.sort()
        for name in dirnames:
            rel = os.path.relpath(os.path.join(dirpath, name), root).replace(os.sep, "/")
            require(not os.path.islink(os.path.join(dirpath, name)), f"symlink_refused: {rel} is a symbolic link")
            require(depth + 1 < MAX_DEPTH, f"layout_too_deep: {rel} is nested below the first directory level")
        for name in filenames:
            full = os.path.join(dirpath, name)
            rel = os.path.relpath(full, root).replace(os.sep, "/")
            require(not os.path.islink(full), f"symlink_refused: {rel} is a symbolic link")
            if rel != SUMS_NAME:
                found.append(rel)
    return sorted(found)


def result_object(response, what):
    """The list/recall result: the response itself, or its `result` member when wrapped."""
    require(isinstance(response, dict), f"{what}: the response is not a JSON object")
    if "memories" in response:
        return response
    inner = response.get("result")
    if isinstance(inner, dict) and "memories" in inner:
        return inner
    code = response.get("code")
    message = response.get("message")
    if code is not None or message is not None:
        raise CheckError(f"{what}: the response is an error, not a result (code {code!r}, message {message!r})")
    raise CheckError(f"{what}: the response has no memories array (neither at the top level nor under result)")


def response_records(result, what):
    memories = result.get("memories")
    require(isinstance(memories, list), f"{what}: memories must be an array")
    ids, projects = [], []
    for n, record in enumerate(memories):
        require(isinstance(record, dict), f"{what}: memories[{n}] is not an object")
        rid = record.get("id")
        require(isinstance(rid, str) and DECIMAL_RE.match(rid), f"{what}: memories[{n}].id is not a decimal string")
        project = record.get("project")
        require(isinstance(project, str) and project, f"{what}: memories[{n}].project is missing")
        ids.append(rid)
        projects.append(project)
    require(len(set(ids)) == len(ids), f"{what}: duplicate memory ids in the response")
    return ids, projects


def response_proof(result, what):
    proof = result.get("proof")
    require(proof is not None, f"{what}: proof is null (no proof was generated)")
    require(isinstance(proof, dict), f"{what}: proof is not an object")
    anchor = proof.get("anchor_hex")
    blake3 = proof.get("proof_blake3")
    require(isinstance(anchor, str), f"{what}: proof.anchor_hex missing")
    require(isinstance(blake3, str), f"{what}: proof.proof_blake3 missing")
    anchor = anchor.lower()
    blake3 = blake3.lower()
    require_hex64(anchor, f"{what}: proof.anchor_hex")
    require_hex64(blake3, f"{what}: proof.proof_blake3")
    require(proof.get("scope") == PROOF_SCOPE, f"{what}: proof.scope is {proof.get('scope')!r}, expected {PROOF_SCOPE!r}")
    require(isinstance(proof.get("proof_path"), str) and proof["proof_path"], f"{what}: proof.proof_path missing")
    require(isinstance(proof.get("witness_path"), str) and proof["witness_path"], f"{what}: proof.witness_path missing")
    return anchor, blake3


def response_snapshot(result, what):
    snapshot = result.get("snapshot")
    require(isinstance(snapshot, dict), f"{what}: snapshot object missing from the response")
    root_digest = snapshot.get("root_digest")
    require(isinstance(root_digest, str) and HEX_RE.match(root_digest), f"{what}: snapshot.root_digest must be lowercase hex")
    require_int(snapshot.get("visible_csn"), f"{what}: snapshot.visible_csn")
    require_int(snapshot.get("logical_time_micros"), f"{what}: snapshot.logical_time_micros")
    return {
        "record_count": None,
        "root_digest": root_digest,
        "visible_csn": snapshot["visible_csn"],
        "logical_time_micros": snapshot["logical_time_micros"],
    }


def check_verify_report(report, anchor, what):
    """What the runtime requires of a `hyphae proof verify` report before a record is Valid."""
    require(isinstance(report, dict), f"{what}: the verifier report is not a JSON object")
    require(report.get("status") == VERIFIED_STATUS, f"{what}: verifier status is {report.get('status')!r}, not {VERIFIED_STATUS!r}")
    require(report.get("kind") == VERIFIED_KIND, f"{what}: verifier kind is {report.get('kind')!r}, not {VERIFIED_KIND!r}")
    require(report.get("scope") == VERIFIED_SCOPE, f"{what}: verifier scope is {report.get('scope')!r}, not {VERIFIED_SCOPE!r}")
    require(report.get("semantic_reexecution_performed") is True, f"{what}: semantic_reexecution_performed is not true")
    digest = report.get("anchor_digest")
    require(isinstance(digest, str) and digest.lower() == anchor, f"{what}: verifier anchor_digest {digest!r} is not the anchor {anchor}")


def load_expected_ids(path):
    ids = set()
    with open(path, "r", encoding="utf-8") as fh:
        for line in fh:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            require(DECIMAL_RE.match(line), f"expected ids file {path}: {line!r} is not a decimal id")
            ids.add(line)
    require(ids, f"expected ids file {path} lists no ids")
    return ids


def capture_paths(capture_id):
    return {
        "response": f"captures/{capture_id}.response.json",
        "proof": f"captures/{capture_id}.proof",
        "witness": f"captures/{capture_id}.witness",
        "verify": f"captures/{capture_id}.verify.json",
    }


# ------------------------------------------------------------------- check
def validate_manifest(manifest, root, public_only=False, expect_ids=None, reverify=None):
    """Raise CheckError on the first problem; return a short summary dict."""
    what = MANIFEST_NAME
    require_keys(manifest, MANIFEST_KEYS, what)
    require(manifest["schema"] == MANIFEST_SCHEMA, f"{what}: schema is {manifest['schema']!r}, expected {MANIFEST_SCHEMA!r}")
    snapshot_id = manifest["snapshot_id"]
    require(isinstance(snapshot_id, str) and ID_RE.match(snapshot_id), f"{what}: snapshot_id must match ^[a-z0-9][a-z0-9-]{{0,63}}$")
    dir_name = os.path.basename(os.path.normpath(os.path.abspath(root)))
    require(snapshot_id == dir_name, f"{what}: snapshot_id {snapshot_id!r} differs from the directory name {dir_name!r}")
    require_rfc3339(manifest["captured_at"], f"{what}: captured_at")
    project = manifest["project"]
    require(isinstance(project, str) and PROJECT_RE.match(project), f"{what}: project must be a plain identifier")
    anchor = manifest["anchor_hex"]
    require_hex64(anchor, f"{what}: anchor_hex")

    hyphae = manifest["hyphae"]
    require_keys(hyphae, HYPHAE_KEYS, f"{what}: hyphae")
    version = hyphae["version"]
    require(isinstance(version, str) and version.strip() and all(ch.isprintable() for ch in version), f"{what}: hyphae.version must be printable text")
    require_hex64(hyphae["binary_sha256"], f"{what}: hyphae.binary_sha256")
    require(hyphae["control_schema"] == CONTROL_SCHEMA, f"{what}: hyphae.control_schema is {hyphae['control_schema']!r}, expected {CONTROL_SCHEMA!r}")

    store = manifest["store"]
    require_keys(store, STORE_KEYS, f"{what}: store")
    require_int(store["record_count"], f"{what}: store.record_count")
    require(store["record_count"] == REQUIRED_RECORD_COUNT, f"{what}: store.record_count is {store['record_count']}, this snapshot must hold exactly {REQUIRED_RECORD_COUNT} records")
    require(isinstance(store["root_digest"], str) and HEX_RE.match(store["root_digest"]), f"{what}: store.root_digest must be lowercase hex")
    require_int(store["visible_csn"], f"{what}: store.visible_csn")
    require_int(store["logical_time_micros"], f"{what}: store.logical_time_micros")

    captures = manifest["captures"]
    require(isinstance(captures, list) and captures, f"{what}: captures must be a non-empty array")
    seen = set()
    by_id = {}
    for n, capture in enumerate(captures):
        cw = f"{what}: captures[{n}]"
        require_keys(capture, CAPTURE_KEYS, cw)
        cid = capture["id"]
        require(isinstance(cid, str) and ID_RE.match(cid), f"{cw}: id must match ^[a-z0-9][a-z0-9-]{{0,63}}$")
        require(cid not in seen, f"{cw}: duplicate capture id {cid!r}")
        seen.add(cid)
        cw = f"{what}: capture {cid}"
        require(capture["operation"] in OPERATIONS, f"{cw}: operation must be list or recall")
        arguments = capture["arguments"]
        require_keys(arguments, ARGUMENT_KEYS, f"{cw}: arguments")
        require(isinstance(arguments["project"], str) and PROJECT_RE.match(arguments["project"]), f"{cw}: arguments.project must be a plain identifier")
        require(isinstance(arguments["query"], str), f"{cw}: arguments.query must be a string")
        require_int(arguments["limit"], f"{cw}: arguments.limit", 1)
        require(arguments["limit"] <= 64, f"{cw}: arguments.limit must be 1..64")
        require(arguments["prove"] is True, f"{cw}: arguments.prove must be true (only proven captures are evidence)")
        paths = capture_paths(cid)
        for key, field in (("response", "response_file"), ("proof", "proof_file"), ("witness", "witness_file"), ("verify", "verify_file")):
            require(capture[field] == paths[key], f"{cw}: {field} must be {paths[key]!r}, found {capture[field]!r}")
        require_hex64(capture["proof_blake3"], f"{cw}: proof_blake3")
        require_hex64(capture["proof_sha256"], f"{cw}: proof_sha256")
        require_hex64(capture["witness_sha256"], f"{cw}: witness_sha256")
        memory_ids = capture["memory_ids"]
        require(isinstance(memory_ids, list), f"{cw}: memory_ids must be an array")
        for mid in memory_ids:
            require(isinstance(mid, str) and DECIMAL_RE.match(mid), f"{cw}: memory_ids entries must be decimal strings")
        require(len(set(memory_ids)) == len(memory_ids), f"{cw}: duplicate memory ids")
        verification = capture["verification"]
        require_keys(verification, VERIFICATION_KEYS, f"{cw}: verification")
        require_rfc3339(verification["verified_at"], f"{cw}: verification.verified_at")
        require(verification["status"] == VERIFIED_STATUS, f"{cw}: verification.status must be {VERIFIED_STATUS!r}")
        require(verification["kind"] == VERIFIED_KIND, f"{cw}: verification.kind must be {VERIFIED_KIND!r}")
        require(verification["scope"] == VERIFIED_SCOPE, f"{cw}: verification.scope must be {VERIFIED_SCOPE!r}")
        require(verification["anchor_hex"] == anchor, f"{cw}: verification.anchor_hex differs from the manifest anchor (one anchor for every capture)")
        by_id[cid] = capture

    # The two required captures, exactly as the runtime expects them.
    require(LIST_CAPTURE_ID in by_id, f"{what}: required capture {LIST_CAPTURE_ID!r} is missing")
    require(ISOLATION_CAPTURE_ID in by_id, f"{what}: required capture {ISOLATION_CAPTURE_ID!r} is missing")
    listing = by_id[LIST_CAPTURE_ID]
    expected_list_args = {"project": project, "query": "", "limit": LIST_LIMIT, "prove": True}
    require(listing["operation"] == "list", f"{what}: capture {LIST_CAPTURE_ID} must be a list operation")
    require(listing["arguments"] == expected_list_args, f"{what}: capture {LIST_CAPTURE_ID} arguments must be {expected_list_args}, found {listing['arguments']}")
    require(len(listing["memory_ids"]) == store["record_count"], f"{what}: capture {LIST_CAPTURE_ID} lists {len(listing['memory_ids'])} ids but store.record_count is {store['record_count']}")
    isolation = by_id[ISOLATION_CAPTURE_ID]
    require(isolation["operation"] == "recall", f"{what}: capture {ISOLATION_CAPTURE_ID} must be a recall operation")
    require(isolation["arguments"] == ISOLATION_ARGUMENTS, f"{what}: capture {ISOLATION_CAPTURE_ID} arguments must be {ISOLATION_ARGUMENTS}, found {isolation['arguments']}")
    require(isolation["arguments"]["project"] != project, f"{what}: the isolation capture must target another project than {project!r}")
    require(isolation["memory_ids"] == [], f"{what}: capture {ISOLATION_CAPTURE_ID} must be empty (isolation evidence), found {len(isolation['memory_ids'])} ids")
    if expect_ids is not None:
        got = set(listing["memory_ids"])
        require(got == expect_ids, f"{what}: the list capture ids differ from the expected ids (missing {sorted(expect_ids - got)}, unexpected {sorted(got - expect_ids)})")

    # Cross-check every capture against its files.
    files_checked = 0
    reports = {}
    recomputed = {}
    for capture in captures:
        cid = capture["id"]
        cw = f"capture {cid}"
        paths = capture_paths(cid)
        response = load_json(os.path.join(root, paths["response"]), f"{cw} response")
        result = result_object(response, f"{cw} response")
        r_anchor, r_blake3 = response_proof(result, f"{cw} response")
        require(r_anchor == anchor, f"{cw}: response proof.anchor_hex {r_anchor} differs from the manifest anchor {anchor}")
        require(r_blake3 == capture["proof_blake3"], f"{cw}: response proof.proof_blake3 {r_blake3} differs from the manifest value {capture['proof_blake3']}")
        ids, projects = response_records(result, f"{cw} response")
        require(ids == capture["memory_ids"], f"{cw}: manifest memory_ids differ from the response ids (manifest {capture['memory_ids']}, response {ids})")
        for n, rp in enumerate(projects):
            require(rp == capture["arguments"]["project"], f"{cw}: memories[{n}] belongs to project {rp!r}, not the requested {capture['arguments']['project']!r}")
        if cid == LIST_CAPTURE_ID:
            snap = response_snapshot(result, f"{cw} response")
            for key in ("root_digest", "visible_csn", "logical_time_micros"):
                require(store[key] == snap[key], f"{MANIFEST_NAME}: store.{key} {store[key]!r} differs from the list response snapshot.{key} {snap[key]!r}")
        report = load_json(os.path.join(root, paths["verify"]), f"{cw} verifier report")
        check_verify_report(report, anchor, f"{cw} verifier report")
        reports[cid] = report
        recomputed[cid] = {
            "response": sha256_file(os.path.join(root, paths["response"])),
            "verify": sha256_file(os.path.join(root, paths["verify"])),
        }
        files_checked += 2
        for key, field in (("proof", "proof_sha256"), ("witness", "witness_sha256")):
            path = os.path.join(root, paths[key])
            if public_only and not os.path.isfile(path):
                continue
            require(os.path.isfile(path), f"{cw}: {paths[key]} is missing")
            actual = sha256_file(path)
            require(actual == capture[field], f"{cw}: sha256 of {paths[key]} is {actual}, manifest says {capture[field]}")
            files_checked += 1
    require_required_captures_distinct(
        {c["id"]: c["proof_sha256"] for c in captures},
        {c["id"]: c["proof_blake3"] for c in captures},
        reports,
    )

    # captures.index is required: it holds the digests the session recorded
    # the moment each file was written, the only binding of a proof, a witness
    # or a report to ITS capture (every proof of the session shares the anchor,
    # so a swapped pair would verify just as well).
    index_path = os.path.join(root, INDEX_NAME)
    require(os.path.isfile(index_path), f"{INDEX_NAME} is missing: it holds the capture-time digests that bind every file to its capture")
    entries = read_index(index_path)
    require(set(entries) == set(by_id), f"{INDEX_NAME} lists captures {sorted(entries)} but the manifest has {sorted(by_id)}")
    for cid, entry in entries.items():
        capture = by_id[cid]
        require(entry["operation"] == capture["operation"], f"{INDEX_NAME}: capture {cid} operation differs from the manifest")
        require(entry["arguments"] == capture["arguments"], f"{INDEX_NAME}: capture {cid} arguments differ from the manifest")
        require(entry["proof_blake3"].lower() == capture["proof_blake3"], f"{INDEX_NAME}: capture {cid} proof_blake3 differs from the manifest")
        require(entry["anchor_hex"].lower() == anchor, f"{INDEX_NAME}: capture {cid} anchor_hex differs from the manifest")
        require(entry["memory_ids"] == capture["memory_ids"], f"{INDEX_NAME}: capture {cid} memory_ids differ from the manifest")
        require(entry["verified_at"] == capture["verification"]["verified_at"], f"{INDEX_NAME}: capture {cid} verified_at differs from the manifest")
        recorded = entry["sha256"]
        for key, field in (("proof", "proof_sha256"), ("witness", "witness_sha256")):
            require(
                recorded[key] == capture[field],
                f"{INDEX_NAME}: capture {cid} {field} {capture[field]} differs from the digest recorded at capture time {recorded[key]}: the {key} file is not the one the session copied for this capture",
            )
        for key in ("response", "verify"):
            require(
                recorded[key] == recomputed[cid][key],
                f"capture {cid}: sha256 of {capture_paths(cid)[key]} is {recomputed[cid][key]}, the session recorded {recorded[key]} at capture time: the file is not the one the session wrote for this capture",
            )

    # SHA256SUMS: strict format, exact file set, every digest.
    sums_path = os.path.join(root, SUMS_NAME)
    require(os.path.isfile(sums_path), f"{SUMS_NAME} is missing")
    with open(sums_path, "rb") as fh:
        sums_data = fh.read()
    try:
        sums_text = sums_data.decode("utf-8")
    except UnicodeDecodeError:
        raise CheckError(f"{SUMS_NAME} is not UTF-8") from None
    require(sums_text.endswith("\n"), f"{SUMS_NAME} must end with a newline")
    listed = {}
    for n, line in enumerate(sums_text[:-1].split("\n"), 1):
        match = SUMS_LINE_RE.match(line)
        require(match, f"{SUMS_NAME} line {n} is not '<64 hex>  <relative path>': {line!r}")
        digest, rel = match.group(1), match.group(2)
        require(rel != SUMS_NAME, f"{SUMS_NAME} line {n} must not list {SUMS_NAME} itself")
        require(".." not in rel.split("/"), f"{SUMS_NAME} line {n} path escapes the directory: {rel!r}")
        require(rel not in listed, f"{SUMS_NAME} line {n} lists {rel!r} twice")
        listed[rel] = digest
    require(MANIFEST_NAME in listed, f"{SUMS_NAME} does not list {MANIFEST_NAME}")
    on_disk = relative_files(root)
    for rel in on_disk:
        require(rel in listed, f"file {rel!r} is present but not listed in {SUMS_NAME}")
    for rel in listed:
        if rel in on_disk:
            continue
        if public_only and rel.endswith(PRIVATE_SUFFIXES):
            continue
        raise CheckError(f"{SUMS_NAME} lists {rel!r} but the file is missing")
    for rel in on_disk:
        actual = sha256_file(os.path.join(root, rel))
        require(actual == listed[rel], f"sha256 of {rel!r} is {actual}, {SUMS_NAME} says {listed[rel]}")
        files_checked += 1

    # Re-verification (optional, the expensive step last): the same verifier
    # invocation as the runtime's, and the fresh report must equal the
    # captured one.
    reverified = 0
    if reverify is not None:
        for capture in captures:
            reverify_capture(root, capture["id"], anchor, reports[capture["id"]], reverify)
            reverified += 1
    return {
        "snapshot_id": snapshot_id,
        "anchor_hex": anchor,
        "captures": len(captures),
        "files": files_checked,
        "record_ids": listing["memory_ids"],
        "reverified": reverified,
    }


def check(root, public_only=False, expect_ids=None, reverify=None):
    require(os.path.isdir(root), f"{root} is not a directory")
    manifest = load_json(os.path.join(root, MANIFEST_NAME), MANIFEST_NAME)
    return validate_manifest(manifest, root, public_only=public_only, expect_ids=expect_ids, reverify=reverify)


# ------------------------------------------------------------------- build
def read_index(path):
    """captures.index: one JSON object per line, keyed by capture id."""
    entries = {}
    with open(path, "rb") as fh:
        data = fh.read()
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError:
        raise CheckError(f"{INDEX_NAME} is not UTF-8") from None
    require(text.endswith("\n"), f"{INDEX_NAME} must end with a newline")
    for n, line in enumerate(text[:-1].split("\n"), 1):
        try:
            entry = json.loads(line)
        except ValueError as exc:
            raise CheckError(f"{INDEX_NAME} line {n} is not JSON: {exc}") from None
        iw = f"{INDEX_NAME} line {n}"
        require(isinstance(entry, dict), f"{iw} is not an object")
        for key in INDEX_KEYS:
            require(key in entry, f"{iw} lacks {key!r}")
        cid = entry["id"]
        require(isinstance(cid, str) and ID_RE.match(cid), f"{iw}: id must match ^[a-z0-9][a-z0-9-]{{0,63}}$")
        require(cid not in entries, f"{iw}: duplicate capture id {cid!r}")
        require(entry["operation"] in OPERATIONS, f"{iw}: operation must be list or recall")
        require(isinstance(entry["arguments"], dict), f"{iw}: arguments must be an object")
        files = entry["files"]
        require(isinstance(files, dict) and list(files.keys()) == INDEX_FILE_KEYS, f"{iw}: files must have keys {INDEX_FILE_KEYS}")
        expected = capture_paths(cid)
        for key in INDEX_FILE_KEYS:
            require(files[key] == expected[key], f"{iw}: files.{key} must be {expected[key]!r}")
        digests = entry["sha256"]
        require(isinstance(digests, dict) and list(digests.keys()) == INDEX_FILE_KEYS, f"{iw}: sha256 must have keys {INDEX_FILE_KEYS} (the capture-time digests)")
        for key in INDEX_FILE_KEYS:
            require_hex64(digests[key], f"{iw}: sha256.{key}")
        require(isinstance(entry["proof_blake3"], str), f"{iw}: proof_blake3 must be a string")
        require(isinstance(entry["anchor_hex"], str), f"{iw}: anchor_hex must be a string")
        require(isinstance(entry["memory_ids"], list) and all(isinstance(m, str) for m in entry["memory_ids"]), f"{iw}: memory_ids must be an array of strings")
        for key in ("captured_at", "verified_at"):
            require_rfc3339(entry[key], f"{iw}: {key}")
        entries[cid] = entry
    require(entries, f"{INDEX_NAME} is empty")
    return entries


def read_session(root):
    path = os.path.join(root, SESSION_NAME)
    if not os.path.isfile(path):
        return {}
    session = load_json(path, SESSION_NAME)
    require(isinstance(session, dict), f"{SESSION_NAME} is not an object")
    require(session.get("schema") == SESSION_SCHEMA, f"{SESSION_NAME}: schema is {session.get('schema')!r}, expected {SESSION_SCHEMA!r}")
    return session


def read_binary_sha256(root, session):
    value = session.get("binary_sha256")
    if value is None:
        path = os.path.join(root, "hyphae-binary.sha256")
        require(os.path.isfile(path), f"neither {SESSION_NAME} binary_sha256 nor hyphae-binary.sha256 is available")
        with open(path, "r", encoding="utf-8") as fh:
            value = fh.read().strip().split(" ", 1)[0]
    require(isinstance(value, str), "binary_sha256 must be a string")
    value = value.lower()
    require_hex64(value, "hyphae binary sha256")
    return value


def read_version(root, session):
    value = session.get("hyphae_version")
    if value is None:
        path = os.path.join(root, "hyphae-version.txt")
        require(os.path.isfile(path), f"neither {SESSION_NAME} hyphae_version nor hyphae-version.txt is available")
        with open(path, "r", encoding="utf-8") as fh:
            value = fh.read().strip()
    require(isinstance(value, str) and value.strip(), "hyphae version text is empty")
    return value.strip()


def build(root, captured_at=None, expect_ids=None, reverify=None):
    require(os.path.isdir(root), f"{root} is not a directory")
    snapshot_id = os.path.basename(os.path.normpath(os.path.abspath(root)))
    require(ID_RE.match(snapshot_id), f"the directory name {snapshot_id!r} must match ^[a-z0-9][a-z0-9-]{{0,63}}$ (it becomes snapshot_id)")
    index_path = os.path.join(root, INDEX_NAME)
    require(os.path.isfile(index_path), f"{INDEX_NAME} is missing from {root}")
    entries = read_index(index_path)
    session = read_session(root)
    if "snapshot_id" in session:
        require(session["snapshot_id"] == snapshot_id, f"{SESSION_NAME} snapshot_id {session['snapshot_id']!r} differs from the directory name {snapshot_id!r}")
    require(LIST_CAPTURE_ID in entries, f"{INDEX_NAME} has no {LIST_CAPTURE_ID!r} capture")
    require(ISOLATION_CAPTURE_ID in entries, f"{INDEX_NAME} has no {ISOLATION_CAPTURE_ID!r} capture")
    listing = entries[LIST_CAPTURE_ID]
    project = session.get("project") or listing["arguments"].get("project")
    require(isinstance(project, str) and PROJECT_RE.match(project), "the served project is unknown or not a plain identifier")
    if captured_at is None:
        captured_at = session.get("captured_at") or listing.get("captured_at")
    require(captured_at is not None, f"captured_at is unknown: pass --captured-at or provide it in {SESSION_NAME}")
    require_rfc3339(captured_at, "captured_at")

    anchor = None
    store = None
    captures = []
    reports = {}
    for cid, entry in entries.items():
        cw = f"capture {cid}"
        paths = capture_paths(cid)
        digests = {}
        for key in INDEX_FILE_KEYS:
            path = os.path.join(root, paths[key])
            require(os.path.isfile(path), f"{cw}: {paths[key]} is missing")
            # The binding: the file must be byte-identical to the one the
            # session wrote for this capture (digest recorded in the container
            # right after the file was written and compared with its source).
            digests[key] = sha256_file(path)
            require(
                digests[key] == entry["sha256"][key],
                f"{cw}: sha256 of {paths[key]} is {digests[key]}, {INDEX_NAME} recorded {entry['sha256'][key]} at capture time: the file is not the one the session wrote for this capture",
            )
        response = load_json(os.path.join(root, paths["response"]), f"{cw} response")
        result = result_object(response, f"{cw} response")
        r_anchor, r_blake3 = response_proof(result, f"{cw} response")
        require(entry["anchor_hex"].lower() == r_anchor, f"{cw}: {INDEX_NAME} anchor_hex {entry['anchor_hex']} differs from the response {r_anchor}")
        require(entry["proof_blake3"].lower() == r_blake3, f"{cw}: {INDEX_NAME} proof_blake3 {entry['proof_blake3']} differs from the response {r_blake3}")
        if anchor is None:
            anchor = r_anchor
        require(r_anchor == anchor, f"{cw}: anchor_hex {r_anchor} differs from the first capture's anchor {anchor} (one anchor for every capture)")
        ids, projects = response_records(result, f"{cw} response")
        require(ids == entry["memory_ids"], f"{cw}: {INDEX_NAME} memory_ids differ from the response ids")
        arguments = entry["arguments"]
        require(list(arguments.keys()) == ARGUMENT_KEYS, f"{cw}: arguments keys must be {ARGUMENT_KEYS} in this order, found {list(arguments.keys())}")
        for n, rp in enumerate(projects):
            require(rp == arguments["project"], f"{cw}: memories[{n}] belongs to project {rp!r}, not the requested {arguments['project']!r}")
        if cid == LIST_CAPTURE_ID:
            require(arguments["project"] == project, f"{cw}: arguments.project {arguments['project']!r} differs from the served project {project!r}")
            require(len(ids) == REQUIRED_RECORD_COUNT, f"{cw}: {len(ids)} records in the list response, this snapshot must hold exactly {REQUIRED_RECORD_COUNT}")
            store = response_snapshot(result, f"{cw} response")
            store["record_count"] = len(ids)
        if cid == ISOLATION_CAPTURE_ID:
            require(ids == [], f"{cw}: the isolation recall returned {len(ids)} records, it must be empty")
        report = load_json(os.path.join(root, paths["verify"]), f"{cw} verifier report")
        check_verify_report(report, anchor, f"{cw} verifier report")
        reports[cid] = report
        captures.append({
            "id": cid,
            "operation": entry["operation"],
            "arguments": {key: arguments[key] for key in ARGUMENT_KEYS},
            "response_file": paths["response"],
            "proof_file": paths["proof"],
            "witness_file": paths["witness"],
            "verify_file": paths["verify"],
            "proof_blake3": r_blake3,
            "proof_sha256": digests["proof"],
            "witness_sha256": digests["witness"],
            "memory_ids": list(ids),
            "verification": {
                "verified_at": entry["verified_at"],
                "status": VERIFIED_STATUS,
                "kind": VERIFIED_KIND,
                "scope": VERIFIED_SCOPE,
                "anchor_hex": anchor,
            },
        })
    require_required_captures_distinct(
        {c["id"]: c["proof_sha256"] for c in captures},
        {c["id"]: c["proof_blake3"] for c in captures},
        reports,
    )
    manifest = {
        "schema": MANIFEST_SCHEMA,
        "snapshot_id": snapshot_id,
        "captured_at": captured_at,
        "project": project,
        "anchor_hex": anchor,
        "hyphae": {
            "version": read_version(root, session),
            "binary_sha256": read_binary_sha256(root, session),
            "control_schema": CONTROL_SCHEMA,
        },
        "store": {key: store[key] for key in STORE_KEYS},
        "captures": captures,
    }
    manifest_path = os.path.join(root, MANIFEST_NAME)
    sums_path = os.path.join(root, SUMS_NAME)
    for stale in (manifest_path, sums_path):
        if os.path.exists(stale):
            os.remove(stale)
    with open(manifest_path, "w", encoding="utf-8", newline="\n") as fh:
        json.dump(manifest, fh, indent=2, ensure_ascii=True)
        fh.write("\n")
    with open(sums_path, "w", encoding="utf-8", newline="\n") as fh:
        for rel in relative_files(root):
            fh.write(f"{sha256_file(os.path.join(root, rel))}  {rel}\n")
    return check(root, expect_ids=expect_ids, reverify=reverify)


# ---------------------------------------------------------------- selftest
SELFTEST_ANCHOR = "a" * 60 + "0001"
SELFTEST_IDS = [
    "186670067796945807561772405393844769735",
    "8864874238917673930644756936982232317",
    "328114099256634798832222840773545327795",
    "295729039551322334710406856728557559035",
    "287567405605917891851290763137356115373",
    "237412834382378687445660534362297845298",
    "161896895843658017381255677281559699870",
]


def synthetic_response(ids, project, blake3, anchor, wrapped, visible_csn):
    memories = [
        {
            "id": mid, "score": 0.016, "project": project, "scope": "project", "kind": "fact",
            "layer": "work", "agent": None, "harness": None, "model": None,
            "text": f"Synthetic record {n} for the selftest.", "expires_at_micros": None,
        }
        for n, mid in enumerate(ids)
    ]
    result = {
        "memories": memories,
        "expired_filtered": 0,
        "proof": {
            "scope": PROOF_SCOPE,
            "proof_path": "/x/.local/state/hyphae/proofs/selftest.proof",
            "witness_path": "/x/.local/state/hyphae/proofs/selftest.witness",
            "proof_blake3": blake3,
            "anchor_hex": anchor,
        },
        "retrieval_mode": "lexical",
        "semantic_status": "disabled",
        "snapshot": {"visible_csn": visible_csn, "root_digest": "c" * 64, "logical_time_micros": 1793080919911004},
    }
    if wrapped:
        return {"schema": CONTROL_SCHEMA, "id": "capture-selftest", "result": result}
    return result


def fake_report(anchor, proof, witness):
    """The report shape of the live 2026-10-27 verifier, with every digest a
    deterministic function of the proof and witness bytes (as the real ones
    are, by an algorithm this script does not know); FAKE_VERIFIER_SOURCE
    computes the same values from the files it is given."""
    return {
        "anchor_digest": anchor, "directory_count": 5,
        "evidence_digest": sha256_bytes(b"evidence\n" + proof + witness), "file_count": 5,
        "kind": VERIFIED_KIND, "proof_digest": sha256_bytes(proof),
        "request_digest": sha256_bytes(b"request\n" + proof), "result_digest": sha256_bytes(b"result\n" + proof),
        "scope": VERIFIED_SCOPE, "semantic_reexecution_performed": True, "status": VERIFIED_STATUS,
        "total_file_bytes": len(witness), "witness_digest": sha256_bytes(witness),
    }


# A stand-in for `hyphae proof verify` (run as `python -I <file> proof verify
# --proof P --witness W --anchor A`): prints fake_report of the files it gets.
FAKE_VERIFIER_SOURCE = '''
import hashlib, json, sys
args = sys.argv[1:]
if args[:2] != ["proof", "verify"]:
    sys.exit(2)
def opt(name):
    return args[args.index(name) + 1]
def h(data):
    return hashlib.sha256(data).hexdigest()
with open(opt("--proof"), "rb") as fh:
    proof = fh.read()
with open(opt("--witness"), "rb") as fh:
    witness = fh.read()
report = {
    "anchor_digest": opt("--anchor"), "directory_count": 5,
    "evidence_digest": h(b"evidence\\n" + proof + witness), "file_count": 5,
    "kind": "memory", "proof_digest": h(proof),
    "request_digest": h(b"request\\n" + proof), "result_digest": h(b"result\\n" + proof),
    "scope": "semantic_reexecution", "semantic_reexecution_performed": True, "status": "verified",
    "total_file_bytes": len(witness), "witness_digest": h(witness),
}
print(json.dumps(report, indent=2, sort_keys=True))
'''
# A verifier that rejects (the structured failure the real one prints).
FAKE_REJECTING_VERIFIER_SOURCE = '''
import sys
print('{"error":{"category":"corruption","message":"proof digest mismatch"},"exit_class":9}', file=sys.stderr)
sys.exit(9)
'''
# A verifier that exits 0 without a report.
FAKE_SILENT_VERIFIER_SOURCE = '''
print("verified, trust me")
'''


def write_text(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(text)


def write_bytes(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "wb") as fh:
        fh.write(data)


def make_synthetic_snapshot(
    root, list_ids=None, isolation_ids=None, report_status=VERIFIED_STATUS, isolation_anchor=None,
    identical_proofs=False, wrong_result_digest=False, old_index=False,
):
    """A complete capture-session output as capture-snapshot.sh would leave it (no manifest yet)."""
    list_ids = SELFTEST_IDS if list_ids is None else list_ids
    isolation_ids = [] if isolation_ids is None else isolation_ids
    isolation_anchor = SELFTEST_ANCHOR if isolation_anchor is None else isolation_anchor
    proof_l = bytes((i * 7) % 256 for i in range(4096))
    witness_l = bytes((i * 13 + 1) % 256 for i in range(6000))
    proof_i = proof_l if identical_proofs else bytes((i * 11 + 2) % 256 for i in range(3000))
    witness_i = bytes((i * 17 + 3) % 256 for i in range(5000))
    blake_l, blake_i = "1" * 64, ("1" * 64 if identical_proofs else "2" * 64)
    captures_dir = os.path.join(root, "captures")
    write_bytes(os.path.join(captures_dir, f"{LIST_CAPTURE_ID}.proof"), proof_l)
    write_bytes(os.path.join(captures_dir, f"{LIST_CAPTURE_ID}.witness"), witness_l)
    write_bytes(os.path.join(captures_dir, f"{ISOLATION_CAPTURE_ID}.proof"), proof_i)
    write_bytes(os.path.join(captures_dir, f"{ISOLATION_CAPTURE_ID}.witness"), witness_i)
    list_response = synthetic_response(list_ids, "demo-repo", blake_l, SELFTEST_ANCHOR, False, 26)
    isolation_response = synthetic_response(isolation_ids, "other-repo", blake_i, isolation_anchor, True, 26)
    write_text(os.path.join(captures_dir, f"{LIST_CAPTURE_ID}.response.json"), json.dumps(list_response, separators=(",", ":")) + "\n")
    write_text(os.path.join(captures_dir, f"{ISOLATION_CAPTURE_ID}.response.json"), json.dumps(isolation_response, separators=(",", ":")) + "\n")
    report_l = fake_report(SELFTEST_ANCHOR, proof_l, witness_l)
    report_l["status"] = report_status
    if wrong_result_digest:
        # A report that is well-formed and verified but was not produced from
        # this proof: only a fresh verifier run can tell.
        report_l["result_digest"] = "0" * 64
    report_i = fake_report(isolation_anchor, proof_i, witness_i)
    write_text(os.path.join(captures_dir, f"{LIST_CAPTURE_ID}.verify.json"), json.dumps(report_l, indent=2, sort_keys=True) + "\n")
    write_text(os.path.join(captures_dir, f"{ISOLATION_CAPTURE_ID}.verify.json"), json.dumps(report_i, indent=2, sort_keys=True) + "\n")
    list_args = {"project": "demo-repo", "query": "", "limit": LIST_LIMIT, "prove": True}
    index_lines = []
    for cid, op, args, blake, anchor, ids in (
        (LIST_CAPTURE_ID, "list", list_args, blake_l, SELFTEST_ANCHOR, list_ids),
        (ISOLATION_CAPTURE_ID, "recall", ISOLATION_ARGUMENTS, blake_i, isolation_anchor, isolation_ids),
    ):
        paths = capture_paths(cid)
        entry = {
            "id": cid, "operation": op, "arguments": args, "files": paths,
            "sha256": {key: sha256_file(os.path.join(root, paths[key])) for key in INDEX_FILE_KEYS},
            "proof_blake3": blake, "anchor_hex": anchor, "memory_ids": ids,
            "captured_at": "2026-10-29T15:04:05Z", "verified_at": "2026-10-29T15:04:09Z",
        }
        if old_index:
            del entry["sha256"]
        index_lines.append(json.dumps(entry, separators=(",", ":")))
    write_text(os.path.join(root, INDEX_NAME), "\n".join(index_lines) + "\n")
    session = {
        "schema": SESSION_SCHEMA, "snapshot_id": os.path.basename(root), "project": "demo-repo",
        "captured_at": "2026-10-29T15:04:05Z", "finished_at": "2026-10-29T15:05:00Z",
        "anchor_hex": SELFTEST_ANCHOR, "record_count": len(list_ids), "hyphae_version": "hyphae 4.0.0",
        "binary_path": "/opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae", "binary_sha256": "5" * 64,
        "control_schema": CONTROL_SCHEMA, "ui_mode": "daemon", "instance_home": "/x",
    }
    write_text(os.path.join(root, SESSION_NAME), json.dumps(session, separators=(",", ":")) + "\n")
    write_text(os.path.join(root, "hyphae-version.txt"), "hyphae 4.0.0\n")
    write_text(os.path.join(root, "hyphae-binary.sha256"), "5" * 64 + "  /opt/hyphae/hyphae-4.0.0-x86_64-unknown-linux-gnu/hyphae\n")
    write_text(os.path.join(root, "capture.log"), "2026-10-29T15:04:05Z step 0: session start (synthetic selftest log)\n2026-10-29T15:05:00Z DONE\n")


def expect_failure(label, fn, failures):
    try:
        fn()
    except CheckError as exc:
        return str(exc)
    failures.append(label)
    return None


def selftest():
    failures = []
    passed = []
    base = tempfile.mkdtemp(prefix="snapshot-selftest-")
    name = "snap-selftest"

    # Every variant keeps the snapshot directory name (snapshot_id must equal
    # it), under its own parent, so a failure can only come from the change
    # the case makes.
    def fresh_dir(label):
        path = os.path.join(base, label, name)
        if os.path.exists(path):
            shutil.rmtree(path)
        return path

    def fresh_copy(label):
        path = fresh_dir(label)
        shutil.copytree(root, path)
        return path

    try:
        root = os.path.join(base, "origin", name)
        make_synthetic_snapshot(root)
        summary = build(root)
        passed.append(f"build+check: {summary['captures']} captures, {summary['files']} files verified")
        check(root, expect_ids=set(SELFTEST_IDS))
        passed.append("check --expect-ids with the right ids")
        detail = expect_failure("expect-ids mismatch", lambda: check(root, expect_ids=set(SELFTEST_IDS[:-1]) | {"42"}), failures)
        if detail:
            passed.append(f"expect-ids mismatch detected: {detail[:90]}")
        pristine = fresh_copy("pristine")
        check(pristine)
        passed.append("an untouched copy under another parent passes")

        # One-byte changes in every file (first, middle and last byte), each in a fresh copy.
        files = relative_files(root) + [SUMS_NAME]
        detected = 0
        for rel in files:
            data_path = os.path.join(root, rel)
            with open(data_path, "rb") as fh:
                original = fh.read()
            for offset in sorted({0, len(original) // 2, len(original) - 1}):
                copy = fresh_copy("flip")
                mutated = bytearray(original)
                mutated[offset] ^= 0x01
                with open(os.path.join(copy, rel), "wb") as fh:
                    fh.write(bytes(mutated))
                label = f"one-byte change in {rel} at offset {offset} not detected"
                if expect_failure(label, lambda c=copy: check(c), failures):
                    detected += 1
        passed.append(f"one-byte changes detected: {detected} mutations across {len(files)} files")

        # A missing file, an extra file, a missing private file in public-only mode.
        copy = fresh_copy("missing")
        os.remove(os.path.join(copy, "captures", f"{LIST_CAPTURE_ID}.witness"))
        if expect_failure("missing witness not detected", lambda: check(copy), failures):
            passed.append("missing witness detected")
        check(copy, public_only=True)
        passed.append("public-only check passes without the private files")
        os.remove(os.path.join(copy, "captures", f"{LIST_CAPTURE_ID}.verify.json"))
        if expect_failure("missing verify report not detected in public-only mode", lambda: check(copy, public_only=True), failures):
            passed.append("missing verifier report detected in public-only mode")
        copy = fresh_copy("extra")
        write_text(os.path.join(copy, "captures", "stray.txt"), "not in SHA256SUMS\n")
        if expect_failure("extra file not detected", lambda: check(copy), failures):
            passed.append("extra file detected")

        # Cross-check violations that build must refuse.
        copy = fresh_dir("six")
        make_synthetic_snapshot(copy, list_ids=SELFTEST_IDS[:-1])
        if expect_failure("6-record list accepted", lambda: build(copy), failures):
            passed.append("list with 6 records refused")
        copy = fresh_dir("leak")
        make_synthetic_snapshot(copy, isolation_ids=["99"])
        if expect_failure("non-empty isolation recall accepted", lambda: build(copy), failures):
            passed.append("non-empty isolation recall refused")
        copy = fresh_dir("unverified")
        make_synthetic_snapshot(copy, report_status="rejected")
        if expect_failure("unverified report accepted", lambda: build(copy), failures):
            passed.append("verifier report without status verified refused")
        copy = fresh_dir("anchors")
        make_synthetic_snapshot(copy, isolation_anchor="b" * 64)
        if expect_failure("two anchors accepted", lambda: build(copy), failures):
            passed.append("captures with two different anchors refused")
        copy = fresh_dir("index-ids")
        make_synthetic_snapshot(copy)
        index_path = os.path.join(copy, INDEX_NAME)
        with open(index_path, "r", encoding="utf-8") as fh:
            lines = fh.read().split("\n")
        first = json.loads(lines[0])
        first["memory_ids"] = list(reversed(first["memory_ids"]))
        lines[0] = json.dumps(first, separators=(",", ":"))
        write_text(index_path, "\n".join(lines))
        if expect_failure("index/response id mismatch accepted", lambda: build(copy), failures):
            passed.append("captures.index ids that disagree with the response refused")
        copy = fresh_copy("manifest-ids")
        manifest_path = os.path.join(copy, MANIFEST_NAME)
        manifest = load_json(manifest_path, MANIFEST_NAME)
        manifest["captures"][0]["memory_ids"][0] = "7"
        with open(manifest_path, "w", encoding="utf-8", newline="\n") as fh:
            json.dump(manifest, fh, indent=2)
            fh.write("\n")
        if expect_failure("manifest/response id mismatch accepted", lambda: validate_manifest(manifest, copy), failures):
            passed.append("manifest memory_ids that disagree with the response refused (before any digest)")
        reordered = {key: manifest[key] for key in reversed(MANIFEST_KEYS)}
        if expect_failure("manifest key order accepted", lambda: validate_manifest(reordered, copy), failures):
            passed.append("manifest key order enforced")

        # Binding of each file to ITS capture: swaps between the two captures
        # keep every digest valid somewhere, share the anchor, and verify;
        # only the capture-time record in captures.index tells them apart.
        def swap(copy, suffix):
            a = os.path.join(copy, "captures", f"{LIST_CAPTURE_ID}{suffix}")
            b = os.path.join(copy, "captures", f"{ISOLATION_CAPTURE_ID}{suffix}")
            tmp = a + ".swap"
            os.replace(a, tmp)
            os.replace(b, a)
            os.replace(tmp, b)

        copy = fresh_dir("swap-before-build")
        make_synthetic_snapshot(copy)
        swap(copy, ".proof")
        swap(copy, ".witness")
        detail = expect_failure("swapped proof+witness pairs accepted by build", lambda: build(copy), failures)
        if detail:
            passed.append(f"swapped proof+witness pairs refused by build: {detail[:80]}")
        copy = fresh_copy("swap-after-build")
        swap(copy, ".proof")
        swap(copy, ".witness")
        if expect_failure("swapped proof+witness pairs accepted by check", lambda: check(copy), failures):
            passed.append("swapped proof+witness pairs refused by check")
        copy = fresh_copy("swap-reports")
        swap(copy, ".verify.json")
        if expect_failure("swapped verifier reports accepted", lambda: check(copy), failures):
            passed.append("swapped verifier reports refused")
        copy = fresh_dir("swap-reports-before-build")
        make_synthetic_snapshot(copy)
        swap(copy, ".verify.json")
        if expect_failure("swapped verifier reports accepted by build", lambda: build(copy), failures):
            passed.append("swapped verifier reports refused by build")
        copy = fresh_copy("swap-responses")
        swap(copy, ".response.json")
        if expect_failure("swapped responses accepted", lambda: check(copy), failures):
            passed.append("swapped responses refused")
        copy = fresh_copy("nested-dir")
        deep = os.path.join(copy, "captures", "deep")
        os.makedirs(deep)
        write_text(os.path.join(deep, "x.txt"), "x\n")
        detail = expect_failure("nested directory accepted", lambda: check(copy), failures)
        if detail and "layout_too_deep" in detail:
            passed.append(f"nested directory refused as the runtime does: {detail[:70]}")
        elif detail:
            failures.append(f"nested directory refused for another reason: {detail[:70]}")
        copy = fresh_copy("symlinked-file")
        link = os.path.join(copy, "captures", "link.response.json")
        try:
            os.symlink(os.path.join(copy, "captures", "list-demo-repo.response.json"), link)
        except (OSError, NotImplementedError) as exc:
            passed.append(f"symbolic link case SKIPPED: the OS refused to create a link ({exc.__class__.__name__})")
        else:
            detail = expect_failure("symbolic link accepted", lambda: check(copy), failures)
            if detail and "symlink_refused" in detail:
                passed.append(f"symbolic link refused as the runtime does: {detail[:70]}")
            elif detail:
                failures.append(f"symbolic link refused for another reason: {detail[:70]}")
        copy = fresh_copy("no-index")
        os.remove(os.path.join(copy, INDEX_NAME))
        detail = expect_failure("missing captures.index accepted", lambda: check(copy), failures)
        if detail:
            passed.append(f"missing captures.index refused: {detail[:70]}")
        copy = fresh_dir("old-index")
        make_synthetic_snapshot(copy, old_index=True)
        detail = expect_failure("captures.index without capture-time digests accepted", lambda: build(copy), failures)
        if detail:
            passed.append(f"captures.index without sha256 refused: {detail[:70]}")
        copy = fresh_dir("identical-proofs")
        make_synthetic_snapshot(copy, identical_proofs=True)
        detail = expect_failure("identical proofs for the two required captures accepted", lambda: build(copy), failures)
        if detail:
            passed.append(f"two required captures sharing one proof refused: {detail[:70]}")

        # Re-verification with a fake verifier that reproduces the captured
        # reports from the files: the pristine snapshot passes (with and
        # without --verifier-dir), a report the verifier does not reproduce,
        # a rejecting verifier and a silent one fail.
        fake = os.path.join(base, "fake_hyphae.py")
        write_text(fake, FAKE_VERIFIER_SOURCE)
        rejecting = os.path.join(base, "fake_rejecting_hyphae.py")
        write_text(rejecting, FAKE_REJECTING_VERIFIER_SOURCE)
        silent = os.path.join(base, "fake_silent_hyphae.py")
        write_text(silent, FAKE_SILENT_VERIFIER_SOURCE)
        python = [sys.executable, "-I"]
        summary = check(root, reverify=Reverify(python + [fake]))
        require(summary["reverified"] == 2, f"expected 2 re-verified captures, got {summary['reverified']}")
        passed.append("re-verification with a verifier that reproduces the reports passes")
        check(root, reverify=Reverify(python + [fake], verifier_dir=os.path.abspath(root).replace(os.sep, "/")))
        passed.append("re-verification with --verifier-dir passes")
        copy = fresh_dir("foreign-report")
        make_synthetic_snapshot(copy, wrong_result_digest=True)
        build(copy)
        passed.append("a well-formed report the verifier did not produce is not detectable offline (build passes)")
        detail = expect_failure("report not reproduced by the verifier accepted", lambda: check(copy, reverify=Reverify(python + [fake])), failures)
        if detail:
            passed.append(f"re-verification refuses it: {detail[:90]}")
        detail = expect_failure("rejecting verifier accepted", lambda: check(root, reverify=Reverify(python + [rejecting])), failures)
        if detail:
            passed.append(f"re-verification with a rejecting verifier fails: {detail[:80]}")
        detail = expect_failure("verifier without a report accepted", lambda: check(root, reverify=Reverify(python + [silent])), failures)
        if detail:
            passed.append(f"re-verification with a verifier that prints no report fails: {detail[:80]}")
        detail = expect_failure("missing verifier accepted", lambda: check(root, reverify=Reverify([os.path.join(base, "no-such-hyphae")])), failures)
        if detail:
            passed.append("re-verification with a verifier that cannot run fails")
    finally:
        shutil.rmtree(base, ignore_errors=True)
    for line in passed:
        print(f"ok: {line}")
    for line in failures:
        print(f"FAILED: {line}")
    if failures:
        print(f"selftest FAILED ({len(failures)} problem(s))")
        return 1
    print(f"selftest OK ({len(passed)} checks)")
    return 0


# -------------------------------------------------------------------- main
def main(argv):
    parser = argparse.ArgumentParser(prog="snapshot_manifest.py", description=__doc__.split("\n\n")[0])
    sub = parser.add_subparsers(dest="command", required=True)
    p_build = sub.add_parser("build", help="write manifest.json and SHA256SUMS from captures.index, then check")
    p_build.add_argument("snapshot_dir")
    p_build.add_argument("--captured-at", help="RFC 3339 UTC seconds; default: session.json or the list capture")
    p_build.add_argument("--expect-ids", help="file with one expected memory id per line (the recorded research ids)")
    p_check = sub.add_parser("check", help="validate the manifest, the files and every digest")
    p_check.add_argument("snapshot_dir")
    p_check.add_argument("--public-only", action="store_true", help="tolerate missing .proof/.witness files (git checkout)")
    p_check.add_argument("--expect-ids", help="file with one expected memory id per line")
    for p in (p_build, p_check):
        p.add_argument("--reverify", metavar="HYPHAE", help="run this hyphae binary or wrapper (as CONSENSUS_HYPHAE_BIN) on every capture and require the captured verifier report to be reproduced")
        p.add_argument("--verifier-dir", metavar="DIR", help="the snapshot directory as the verifier sees it (the container path with scripts/hyphae-docker.cmd); default: this directory")
        p.add_argument("--reverify-timeout", type=int, default=DEFAULT_REVERIFY_TIMEOUT, metavar="SECS", help=f"deadline of one verifier run (default {DEFAULT_REVERIFY_TIMEOUT})")
    sub.add_parser("selftest", help="build and check a synthetic snapshot, assert tampering is detected")
    args = parser.parse_args(argv)
    if args.command == "selftest":
        return selftest()
    reverify = None
    if args.reverify:
        if args.reverify_timeout < 1:
            parser.error("--reverify-timeout must be at least 1")
        reverify = Reverify([args.reverify], args.verifier_dir, args.reverify_timeout)
    elif args.verifier_dir:
        parser.error("--verifier-dir needs --reverify")
    try:
        expect_ids = load_expected_ids(args.expect_ids) if args.expect_ids else None
        if args.command == "build":
            summary = build(args.snapshot_dir, captured_at=args.captured_at, expect_ids=expect_ids, reverify=reverify)
            verb = "built and checked"
        else:
            summary = check(args.snapshot_dir, public_only=args.public_only, expect_ids=expect_ids, reverify=reverify)
            verb = "checked" + (" (public files only)" if args.public_only else "")
    except CheckError as exc:
        print(f"snapshot check failed: {exc}", file=sys.stderr)
        return 1
    reverified = ""
    if reverify is not None:
        reverified = f", {summary['reverified']} captures re-verified with {os.path.basename(reverify.command[0])} (fresh reports equal the captured ones)"
    print(
        f"OK: {verb} {args.snapshot_dir}: snapshot {summary['snapshot_id']}, anchor {summary['anchor_hex']}, "
        f"{summary['captures']} captures, {summary['files']} file digests verified, "
        f"{len(summary['record_ids'])} record ids: {' '.join(summary['record_ids'])}{reverified}"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
