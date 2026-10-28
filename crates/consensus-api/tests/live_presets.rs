// SPDX-License-Identifier: Apache-2.0
//! Preset tests (ABUSE-8, G10): verbatim equality with the battery script
//! and the absence of any free-text entry point.

use consensus_api::live::presets::{Preset, PresetId, PRESETS, PRESET_PROJECT, PUBLIC_SUBMITTER};
use consensus_domain::{EvidenceMode, ModelMode, RunOrigin};
use std::collections::BTreeSet;

const BATTERY_SCRIPT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../scripts/live_battery.py");
const PRESETS_SOURCE: &str = include_str!("../src/live/presets.rs");

/// Reads one Python string literal starting at `src[i] == '"'`; returns
/// the text and the index after the closing quote.
fn read_literal(src: &[u8], mut i: usize) -> (String, usize) {
    assert_eq!(src[i], b'"');
    i += 1;
    let mut out = Vec::new();
    while src[i] != b'"' {
        if src[i] == b'\\' {
            i += 1;
            match src[i] {
                b'n' => out.push(b'\n'),
                b't' => out.push(b'\t'),
                other => out.push(other),
            }
        } else {
            out.push(src[i]);
        }
        i += 1;
    }
    (String::from_utf8(out).unwrap(), i + 1)
}

/// Parses a Python tuple of string items starting at the `(` at `start`:
/// adjacent literals concatenate, `*NAME` splices `splices[NAME]`.
fn read_tuple(src: &str, start: usize, splices: &[(&str, Vec<String>)]) -> Vec<String> {
    let bytes = src.as_bytes();
    assert_eq!(bytes[start], b'(');
    let mut i = start + 1;
    let mut items = Vec::new();
    let mut current: Option<String> = None;
    loop {
        match bytes[i] {
            b' ' | b'\n' | b'\r' | b'\t' => i += 1,
            b'"' => {
                let (text, next) = read_literal(bytes, i);
                current.get_or_insert_with(String::new).push_str(&text);
                i = next;
            }
            b',' => {
                if let Some(item) = current.take() {
                    items.push(item);
                }
                i += 1;
            }
            b'*' => {
                let end = i
                    + 1
                    + src[i + 1..]
                        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                        .unwrap();
                let name = &src[i + 1..end];
                let (_, values) = splices
                    .iter()
                    .find(|(n, _)| *n == name)
                    .expect("known splice");
                items.extend(values.iter().cloned());
                i = end;
            }
            b')' => {
                if let Some(item) = current.take() {
                    items.push(item);
                }
                return items;
            }
            other => panic!("unexpected byte {other:?} at {i}"),
        }
    }
}

/// `(project, title, description)` for `tag` in the script's `TASKS`.
fn battery_task(src: &str, tag: &str) -> Vec<String> {
    let reference_start =
        src.find("REFERENCE = (").expect("REFERENCE tuple") + "REFERENCE = ".len();
    let reference = read_tuple(src, reference_start, &[]);
    let key = format!("\"{tag}\": (");
    let pos = src.find(&key).unwrap_or_else(|| panic!("{tag} in TASKS"));
    read_tuple(src, pos + key.len() - 1, &[("REFERENCE", reference)])
}

#[test]
fn every_preset_equals_the_battery_script_byte_for_byte() {
    let script = std::fs::read_to_string(BATTERY_SCRIPT).expect("scripts/live_battery.py");
    for id in PresetId::ALL {
        let task = battery_task(&script, id.as_str());
        assert_eq!(task.len(), 3, "{id}: project, title, description");
        let preset = id.preset();
        assert_eq!(task[0], preset.project(), "{id} project");
        assert_eq!(task[0], PRESET_PROJECT);
        assert_eq!(task[1], preset.title(), "{id} title");
        assert_eq!(task[2], preset.description(), "{id} description");
    }
    // The parser itself is checked against a known implicit concatenation.
    let r03 = battery_task(&script, "R03-raise-to-10");
    assert!(r03[2].starts_with(
        "Timeouts persist under load. Increase the payment client's retry attempts to 10 so"
    ));
}

#[test]
fn the_table_and_the_id_list_agree() {
    assert_eq!(PRESETS.len(), PresetId::ALL.len());
    for (preset, id) in PRESETS.iter().zip(PresetId::ALL) {
        assert_eq!(preset.id(), id);
        assert_eq!(PresetId::parse(id.as_str()), Ok(id));
    }
    let ids: BTreeSet<&str> = PresetId::ALL.iter().map(|p| p.as_str()).collect();
    assert_eq!(
        ids,
        BTreeSet::from([
            "R01-reference",
            "R03-raise-to-10",
            "R05-add-jitter",
            "R06-timeout-30s",
            "R07-exp-backoff"
        ])
    );
    let titles: BTreeSet<&str> = PRESETS.iter().map(Preset::title).collect();
    assert_eq!(titles.len(), 5, "distinct titles");
}

#[test]
fn task_input_declares_the_public_live_mode_and_a_fixed_submitter() {
    for preset in &PRESETS {
        let input = preset.task_input();
        assert_eq!(input.project_id.as_str(), "demo-repo");
        assert_eq!(input.title, preset.title());
        assert_eq!(input.description, preset.description());
        assert_eq!(input.submitted_by, PUBLIC_SUBMITTER);
        let mode = input.mode.expect("declared mode");
        assert_eq!(mode.model, ModelMode::Meta);
        assert_eq!(mode.evidence, EvidenceMode::SealedSnapshot);
        assert_eq!(mode.origin, RunOrigin::Public);
        assert_eq!(mode.preset.as_deref(), Some(preset.id().as_str()));
    }
}

#[test]
fn the_module_exposes_no_way_to_carry_visitor_text() {
    // Every public function in the module, by name.
    let public_fns: BTreeSet<&str> = PRESETS_SOURCE
        .lines()
        .map(str::trim_start)
        .filter_map(|l| {
            l.strip_prefix("pub fn ")
                .or_else(|| l.strip_prefix("pub const fn "))
        })
        .map(|rest| rest.split(['(', '<']).next().unwrap())
        .collect();
    assert_eq!(
        public_fns,
        BTreeSet::from([
            "as_str",
            "parse",
            "preset",
            "id",
            "title",
            "description",
            "project",
            "task_mode",
            "task_input",
        ]),
        "a new public function must be reviewed: none may accept visitor text"
    );
    // The only one taking a string is `parse`, which matches five constants.
    for line in PRESETS_SOURCE.lines() {
        let l = line.trim_start();
        if let Some(sig) = l.strip_prefix("pub fn ") {
            let takes_text =
                sig.contains("&str") || sig.contains("String") || sig.contains("impl Into");
            assert!(
                !takes_text || sig.starts_with("parse(id: &str)"),
                "public function takes text: {sig}"
            );
        }
    }
    // `Preset` has no public field and no constructor: its fields are
    // private and the only values are the `PRESETS` constants.
    let header = "pub struct Preset {";
    let struct_start = PRESETS_SOURCE.find(header).unwrap() + header.len();
    let struct_body = &PRESETS_SOURCE[struct_start..];
    let struct_body = &struct_body[..struct_body.find('}').unwrap()];
    assert!(!struct_body.contains("pub "), "{struct_body}");
    assert!(!PRESETS_SOURCE.contains("impl From<"));
    assert!(!PRESETS_SOURCE.contains("Default for Preset"));
    assert!(
        !PRESETS_SOURCE.contains("Deserialize"),
        "no preset is ever read from input"
    );
    // And arbitrary ids are refused, so nothing but the five tags select a preset.
    for junk in [
        "R01-reference ",
        "r01-reference",
        "R04-remove-cap",
        "",
        "Fix payment client timeouts",
    ] {
        assert!(PresetId::parse(junk).is_err(), "{junk:?}");
    }
}
