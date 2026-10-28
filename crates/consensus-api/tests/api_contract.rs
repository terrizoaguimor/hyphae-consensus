// SPDX-License-Identifier: Apache-2.0
//! The committed contract (`contracts/api-v1/`) must be exactly what the
//! code renders: every schema, every fixture, the digest.

use consensus_api::api::contract;

#[test]
fn committed_contract_matches_the_code() {
    let dir = contract::contracts_dir();
    let problems = contract::drift(&dir).expect("contract renders");
    assert!(
        problems.is_empty(),
        "contracts/api-v1 drifted from the code ({}); {}:\n  {}",
        dir.display(),
        contract::EXPORT_HINT,
        problems.join("\n  ")
    );
}

#[test]
fn health_reports_the_digest_of_the_committed_schemas() {
    let dir = contract::contracts_dir();
    let committed = std::fs::read_to_string(dir.join("contract-sha256.txt"))
        .expect("contract-sha256.txt")
        .trim()
        .to_string();
    assert_eq!(
        committed,
        contract::contract_sha256(),
        "{}",
        contract::EXPORT_HINT
    );
}
