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
3. **Proof mapping** (`hyphae proof verify`): exit 0 → `Valid`; nonzero
   with a verifier rejection (stderr mentions mismatch/invalid/corrupt/
   digest) → `Invalid`; transport or usage failures (exit 125–127 from
   `docker exec`, unknown flags, missing files) → `VerificationError`, never
   `Invalid`, so infrastructure faults cannot masquerade as tampering;
   spawn failure → `VerificationError`; missing artifacts → `Unavailable`
   without invoking the verifier. `CommandRunner` is a trait so tests
   assert argv and mapping without pretending to run cryptography.
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
   record. Live, the pinned build currently answers every proven recall
   with `{category: "corruption", message: "native durable state is
   invalid", retry: "after-recovery"}` while plain recall works, so every
   live run that retrieves evidence is **blocked**. This is the fail-closed
   reading of the backend's own corruption report, not a verdict that the
   records were tampered with.
6. **Anchor trust.** By default the anchor comes from the same recall
   response that delivers the proof, so a `Valid` result proves internal
   consistency against a server-supplied anchor, not against an
   independently trusted one. Operators can pin an anchor obtained
   out-of-band (`CONSENSUS_HYPHAE_EXPECTED_ANCHOR`); records anchored
   elsewhere are then `Invalid` without running the verifier.

## Adversarial proof case (still undemonstrated live)

Corrupt a proof/witness file or present a mismatched anchor to
`hyphae proof verify` on an isolated instance, and show the run degrade to
`blocked`. This needs working proof generation, which the pinned build does
not provide today (decision 5), so only the status *mapping* is tested;
no cryptographic detection is claimed. What *is* demonstrated live is the
corruption-signal path: real backend error → `proof_error` → `Invalid` →
`blocked`.
