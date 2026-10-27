// SPDX-License-Identifier: Apache-2.0
//! Writes JSON Schema files for every public contract into `schemas/`.
//! Run: `cargo run -p consensus-domain --example write_schemas`

use consensus_domain::*;
use schemars::schema_for;
use std::path::{Path, PathBuf};

fn write<T: schemars::JsonSchema>(dir: &Path, name: &str) {
    let schema = schema_for!(T);
    let json = serde_json::to_string_pretty(&schema).expect("schema serializes");
    std::fs::write(dir.join(format!("{name}.schema.json")), json + "\n").expect("write schema");
    println!("wrote {name}.schema.json");
}

fn main() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas");
    std::fs::create_dir_all(&dir).expect("create schemas dir");
    write::<Task>(&dir, "task");
    write::<Proposal>(&dir, "proposal");
    write::<Claim>(&dir, "claim");
    write::<Revision>(&dir, "revision");
    write::<Challenge>(&dir, "challenge");
    write::<Evaluation>(&dir, "evaluation");
    write::<EvidenceReference>(&dir, "evidence-reference");
    write::<EvidenceVerification>(&dir, "evidence-verification");
    write::<Recommendation>(&dir, "recommendation");
    write::<HumanAuthorization>(&dir, "human-authorization");
    write::<ConsensusEvent>(&dir, "consensus-event");
    write::<RunReceipt>(&dir, "run-receipt");
}
