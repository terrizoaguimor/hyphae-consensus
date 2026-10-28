// SPDX-License-Identifier: Apache-2.0
//! Curated live presets: the only tasks the public live mode can run.
//!
//! Ids and texts are copied VERBATIM from `scripts/live_battery.py`
//! (`TASKS`), the tasks the recorded 2026-10-26/27 batteries ran, so every
//! public live run repeats a task whose outcome range is documented. The
//! project is always `demo-repo`, the sealed-snapshot project.
//!
//! There is no API for free text (ABUSE-8): a [`Preset`] can only be one
//! of the compiled-in [`PRESETS`], reached through [`PresetId`]. The type
//! has no constructor, no slots and no interpolation; the integration
//! test `live_presets.rs` checks that this module exposes no function
//! taking a visitor string other than [`PresetId::parse`], and that every
//! text equals the battery script byte for byte.

use consensus_domain::{EvidenceMode, ModelMode, ProjectId, RunOrigin, TaskMode};
use consensus_runtime::TaskInput;
use std::fmt;
use thiserror::Error;

/// `TaskInput.submitted_by` for every public live run: a fixed label, so
/// nothing a visitor typed is recorded as the submitter.
pub const PUBLIC_SUBMITTER: &str = "public-portal";
/// Every preset's project: the sealed snapshot serves only this one.
pub const PRESET_PROJECT: &str = "demo-repo";

/// The curated tasks, named as in the battery script.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PresetId {
    R01Reference,
    R03RaiseTo10,
    R05AddJitter,
    R06Timeout30s,
    R07ExpBackoff,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("unknown preset id")]
pub struct UnknownPreset;

impl PresetId {
    pub const ALL: [PresetId; 5] = [
        PresetId::R01Reference,
        PresetId::R03RaiseTo10,
        PresetId::R05AddJitter,
        PresetId::R06Timeout30s,
        PresetId::R07ExpBackoff,
    ];

    /// The battery tag, e.g. `R03-raise-to-10`.
    pub fn as_str(self) -> &'static str {
        match self {
            PresetId::R01Reference => "R01-reference",
            PresetId::R03RaiseTo10 => "R03-raise-to-10",
            PresetId::R05AddJitter => "R05-add-jitter",
            PresetId::R06Timeout30s => "R06-timeout-30s",
            PresetId::R07ExpBackoff => "R07-exp-backoff",
        }
    }

    /// Exact match on the battery tag; no trimming, no case folding, so a
    /// visitor string is either one of five constants or refused.
    pub fn parse(id: &str) -> Result<Self, UnknownPreset> {
        PresetId::ALL
            .into_iter()
            .find(|p| p.as_str() == id)
            .ok_or(UnknownPreset)
    }

    pub fn preset(self) -> &'static Preset {
        &PRESETS[PresetId::ALL
            .iter()
            .position(|p| *p == self)
            .expect("every id has a preset")]
    }
}

impl fmt::Display for PresetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One curated task. Fields are private and there is no constructor: a
/// `Preset` is always one of [`PRESETS`].
#[derive(Debug, PartialEq, Eq)]
pub struct Preset {
    id: PresetId,
    title: &'static str,
    description: &'static str,
}

impl Preset {
    pub fn id(&self) -> PresetId {
        self.id
    }

    pub fn title(&self) -> &'static str {
        self.title
    }

    pub fn description(&self) -> &'static str {
        self.description
    }

    pub fn project(&self) -> &'static str {
        PRESET_PROJECT
    }

    /// The declared mode of every public live run: Meta model, sealed
    /// Hyphae snapshot, public origin, this preset.
    pub fn task_mode(&self) -> TaskMode {
        TaskMode {
            model: ModelMode::Meta,
            evidence: EvidenceMode::SealedSnapshot,
            origin: RunOrigin::Public,
            preset: Some(self.id.as_str().to_string()),
        }
    }

    /// The orchestrator input for this preset. Title and description are
    /// the compiled-in texts; the submitter is [`PUBLIC_SUBMITTER`].
    pub fn task_input(&self) -> TaskInput {
        TaskInput {
            project_id: ProjectId::new(PRESET_PROJECT),
            title: self.title.to_string(),
            description: self.description.to_string(),
            submitted_by: PUBLIC_SUBMITTER.to_string(),
            mode: Some(self.task_mode()),
        }
    }
}

/// The compiled-in table, in [`PresetId::ALL`] order. Texts are the
/// `scripts/live_battery.py` strings, byte for byte.
pub const PRESETS: [Preset; 5] = [
    Preset {
        id: PresetId::R01Reference,
        title: "Fix payment client timeouts",
        description: "Payment requests intermittently time out under load. Propose a safe fix consistent with the existing retry constraints.",
    },
    Preset {
        id: PresetId::R03RaiseTo10,
        title: "Raise payment retry attempts to 10",
        description: "Timeouts persist under load. Increase the payment client's retry attempts to 10 so requests ride out transient upstream failures.",
    },
    Preset {
        id: PresetId::R05AddJitter,
        title: "Add jitter to payment retries",
        description: "Retry storms align under load. Add randomized jitter to the existing payment retry backoff without changing the attempt limit.",
    },
    Preset {
        id: PresetId::R06Timeout30s,
        title: "Increase payment HTTP timeout to 30 seconds",
        description: "Some payment calls take longer than the current per-request timeout. Propose raising the per-request timeout to 30 seconds.",
    },
    Preset {
        id: PresetId::R07ExpBackoff,
        title: "Exponential backoff for payment retries",
        description: "Replace fixed-delay payment retries with exponential backoff while staying within the documented attempt limit.",
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_reject_everything_else() {
        for id in PresetId::ALL {
            assert_eq!(PresetId::parse(id.as_str()), Ok(id));
            assert_eq!(id.preset().id(), id);
        }
        for bad in [
            "",
            "r01-reference",
            " R01-reference",
            "R02-reference-repeat",
            "R04-remove-cap",
            "H01-hostile-raise-to-20",
        ] {
            assert_eq!(PresetId::parse(bad), Err(UnknownPreset), "{bad:?}");
        }
    }

    #[test]
    fn task_input_is_the_preset_text_with_the_public_live_mode() {
        let input = PresetId::R03RaiseTo10.preset().task_input();
        assert_eq!(input.project_id.as_str(), "demo-repo");
        assert_eq!(input.title, "Raise payment retry attempts to 10");
        assert_eq!(input.submitted_by, PUBLIC_SUBMITTER);
        assert_eq!(
            input.mode,
            Some(TaskMode {
                model: ModelMode::Meta,
                evidence: EvidenceMode::SealedSnapshot,
                origin: RunOrigin::Public,
                preset: Some("R03-raise-to-10".into()),
            })
        );
    }
}
