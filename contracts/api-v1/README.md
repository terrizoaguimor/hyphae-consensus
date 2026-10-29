# API v1 contract

Generated: do not edit. `*.schema.json` is one JSON Schema per request, response or SSE frame type of `/api/v1`; `fixtures/` holds responses produced by the real view builders (fixed clock, sequential ids): the detail, summary and receipt of each of the five guided scenarios, plus the SSE frames, the creation response and the operator detail of `retry`; a public live run at creation with preset `R01-reference` (`run-*-live-R01-reference`: only its task exists, no model is called); the research showcase entry F03 (`showcase-entry-F03`); the public capabilities under the default configuration (`capabilities-public`); `health`, `scenarios` and `session`; and sample error bodies (`api-error-samples`). `contract-sha256.txt` is the SHA-256 over the sorted schema files, reported by `GET /api/v1/health`.

Regenerate with `cargo run -p consensus-api --example export_contract`; `cargo test -p consensus-api --test api_contract` fails on drift.
