# API v1 contract

Generated: do not edit. `*.schema.json` is one JSON Schema per request, response or SSE frame type of `/api/v1`; `fixtures/` holds responses produced by the real view builders over the five guided scenarios (fixed clock, sequential ids). `contract-sha256.txt` is the SHA-256 over the sorted schema files, reported by `GET /api/v1/health`.

Regenerate with `cargo run -p consensus-api --example export_contract`; `cargo test -p consensus-api --test api_contract` fails on drift.
