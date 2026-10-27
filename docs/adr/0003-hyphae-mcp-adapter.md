# ADR-0003: Hyphae MCP adapter and proof verification

Date: 2026-10-26 · Status: accepted · Scope: milestone M3

## Verified upstream contract (pinned commit, see README)

- Memory MCP surface: `hyphae mcp --profile memory
  (--endpoint <LOCAL> | --base-url <ORIGIN>) --native-api-key-file <PATH>`
  runs MCP revision `2025-06-18` as newline-delimited JSON-RPC 2.0 over stdio.
- Without `--allow-write`, the memory profile advertises only
  `hyphae_memory_recall` and `hyphae_memory_status` — exactly the read-only
  authority this project grants agents. We never pass `--allow-write`.
- The credential is accepted only from a restricted file (or first stdin
  line), never argv/env values. Our config therefore carries only the path
  (`HYPHAE_NATIVE_API_KEY_FILE`), passed through to the child argv as
  documented; the key never enters our process.
- Recall with `prove` produces sealed proof artifacts; offline verification is
  `hyphae proof verify --proof <FILE> --witness <FILE> --anchor <64_HEX>`.
- Sources: `docs/product/agent-memory.md`, `docs/cli/reference.md` (§`mcp`,
  §`proof`), `mcp/README.md` at the pinned commit (read 2026-10-26).

## Decisions

1. **Injectable transport** (`McpTransport`): `StdioTransport` (child process,
   one in-flight request, 30 s response timeout, killed on drop) and
   `InMemoryTransport` for tests. Handshake (`initialize` →
   `notifications/initialized` → `tools/list`) happens at connect; the client
   refuses surfaces that lack `hyphae_memory_recall`.
2. **Defensive result parsing**: accepts `structuredContent` or JSON text
   content; maps records conservatively; unknown/absent fields degrade to
   empty content/`note` kind and `integrity: unavailable` downstream — never
   to invented support. A tool-error envelope inside `structuredContent`
   (`schema: hyphae-native-mcp-tool-error-v2`, even with `isError: false`)
   is an error, and its `category` is kept in the message. Field-level
   confirmation against a real binary is the gated `live_hyphae_probe` test
   (isolated deployment only).
3. **Proof mapping** (`hyphae proof verify`, amended 2026-10-27): `Valid`
   requires exit 0 **and** the verifier's JSON report saying
   `status: verified` with `semantic_reexecution_performed: true` (the
   recall was re-run against the witness and reproduced the sealed result);
   exit 0 without that report is a `VerificationError`. A nonzero exit is
   classified by the structured error's `category` on stderr: `corruption`
   (tampered or inconsistent artifacts, exit 9 live) and `invalid-request`
   (an anchor the proof does not match, exit 2 live) → `Invalid`; `io`
   (missing file, exit 11 live) and every other category →
   `VerificationError`; unstructured stderr falls back to the earlier
   keyword check. Exit 125–127 from `docker exec` and spawn failures are
   always `VerificationError`, so infrastructure faults cannot masquerade
   as tampering; missing artifacts → `Unavailable` without invoking the
   verifier. `CommandRunner` captures stdout and stderr and is a trait, so
   tests assert argv and mapping (with outputs captured live) without
   pretending to run cryptography.
4. **Isolation discipline**: no test or demo may point at the owner's primary
   Agent Memory data. The live probe requires `CONSENSUS_LIVE_TESTS=1` plus an
   explicitly configured isolated endpoint/key file. It **has been run and
   passes** against the pinned Linux 4.0.0 build in the local
   `hyphae-consensus` Docker container (`scripts/hyphae-container/`); the
   Windows build's Agent Memory surface does not start on this host.
5. **Failed proof generation is recorded, never dropped.** If
   `recall(prove=true)` fails, the adapter retries unproven and stamps every
   returned record with `proof_error` (the upstream reason, category
   included). The runtime maps a corruption-class reason to `Invalid`
   (→ `blocked`) and any other to `Unavailable`; it never verifies such a
   record. A proven call that returns no artifacts is recorded the same way
   (`proof_error`, → `Unavailable`).
6. **Anchor trust.** By default the anchor comes from the same recall
   response that delivers the proof, so a `Valid` result proves internal
   consistency against a server-supplied anchor, not against an
   independently trusted one. Operators can pin an anchor obtained
   out-of-band (`CONSENSUS_HYPHAE_EXPECTED_ANCHOR`); records anchored
   elsewhere are then `Invalid` without running the verifier. Live, the
   anchor is stable while the store does not change: every proof over an
   unchanged store shares it, and a write or a new checkpoint (including
   `agent backup`) changes it. The 2026-10-27 battery ran pinned to the
   anchor the operator observed before it started.
7. **One proof per recall (2026-10-27).** A proven recall returns a single
   sealed proof for its whole ordered result
   (`proof: {proof_path, witness_path, anchor_hex}` at the top level), not
   one per record. The adapter attaches it to every record of that recall;
   the verifier checks each distinct proof once per run and reuses the
   result. The proof file seals each memory's envelope (including its text),
   but the adapter does not byte-compare the text it hands to models against
   the proof: both come from the same `hyphae mcp` response, the same trust
   class as decision 6.

## Why live proofs failed, and the fix (2026-10-27)

Every proven recall had answered `{category: "corruption", message: "native
durable state is invalid", retry: "after-recovery"}` while plain recall
worked. With owner authorization, recovery and diagnostics ran inside the
container only (procedure and every mutating command in
`docs/reviews/2026-10-27-claude-fix-report-2.md`). `hyphae agent doctor`
reported the store **healthy**: it was not corrupt. Reading Hyphae's source
at the pinned commit (read-only) showed that `corruption` is the generic code
for *any* proof-generation failure and for an oversized response. Two causes
were then isolated:

1. **Socket inside the data directory.** The container daemon had been
   started as `hyphae serve --data-dir D` (the operate hint `agent setup`
   prints), so its socket was `D/hyphae.sock`. Proof generation bundles the
   whole data directory into the witness and rejects special files. Fix:
   serve with `--endpoint` outside `D`, exactly as the systemd unit written by
   `agent setup` does (`scripts/hyphae-container/serve.sh`). Proofs verified
   immediately afterwards.
2. **Store size versus the 4 MiB response cap.** The proof carries the
   complete directory witness inline, and `hyphae mcp` caps each response at
   4 MiB. After new memories were seeded, the store passed about 3.9 MB and
   every proven recall failed again, reported once more as `corruption`. On a
   restored copy, proofs kept working through three new stores and failed
   after the next, whatever the content. The available roles cannot `vacuum`,
   so the store was replaced by a fresh one with only the needed records
   (about 3.3 MB), and the hostile-note case moved to a separate isolated lab
   store. Keep stores small (`hyphae_store_bytes`).

## Adversarial proof case (demonstrated 2026-10-27)

With proofs working, the verifier's detection was exercised against the real
binary in the container: the verified recall proof checks out against its own
anchor (exit 0, `semantic_reexecution`); one flipped witness byte is rejected
(exit 9, `corruption` → `Invalid`); a different anchor is rejected (exit 2,
`invalid-request` → `Invalid`); a missing file is an infrastructure error
(exit 11, `io` → `VerificationError`). The gated `live_hyphae_probe` repeats
the anchor case end to end through this adapter on every run. The full
pipeline consequence (an `Invalid` record → `blocked`) is the tested policy
path (`invalid_proof_blocks_without_revision_attempt`).
