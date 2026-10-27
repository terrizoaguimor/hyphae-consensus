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
   to invented support. Field-level confirmation against a real binary is the
   gated `live_hyphae_probe` test (isolated deployment only).
3. **Proof mapping**: exit 0 → `Valid`; nonzero → `Invalid` (stderr excerpt
   kept as detail); spawn failure → `VerificationError`; missing artifacts →
   `Unavailable` without invoking the verifier. `CommandRunner` is a trait so
   tests assert argv and mapping without pretending to run cryptography.
4. **Isolation discipline**: no test or demo may point at the owner's primary
   Agent Memory data. The live probe requires `CONSENSUS_LIVE_TESTS=1` plus an
   explicitly configured isolated endpoint/key file. It has not been run here
   (no Hyphae runtime installed; installation requires owner approval).

## Adversarial proof case (planned with a live isolated runtime)

Corrupt a proof/witness file or present a mismatched anchor to
`hyphae proof verify` on an isolated instance, and show the run degrade to
`blocked`/`insufficient_evidence`. Until that probe runs, only the status
*mapping* is tested; no cryptographic detection is claimed.
