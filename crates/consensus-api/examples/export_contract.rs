// SPDX-License-Identifier: Apache-2.0
//! Writes the frozen `/api/v1` contract to `contracts/api-v1/`: one JSON
//! Schema per type, fixtures from the five guided scenarios, and the
//! contract digest `/health` reports.
//!
//! Run: `cargo run -p consensus-api --example export_contract`
//! (`cargo test -p consensus-api --test api_contract` fails on drift).

use consensus_api::api::contract;

fn main() {
    let dir = contract::contracts_dir();
    match contract::export(&dir) {
        Ok(written) => {
            for rel in &written {
                println!("wrote {}", dir.join(rel).display());
            }
            println!(
                "{} files; contract_sha256 {}",
                written.len(),
                contract::contract_sha256()
            );
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
