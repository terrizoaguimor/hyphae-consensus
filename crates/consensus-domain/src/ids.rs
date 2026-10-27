// SPDX-License-Identifier: Apache-2.0
//! Stable identifier types. IDs are opaque strings; generation is the
//! runtime's responsibility so tests can use deterministic sequences.

macro_rules! id_type {
    ($name:ident, $doc:expr) => {
        #[doc = $doc]
        #[derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            Hash,
            PartialOrd,
            Ord,
            serde::Serialize,
            serde::Deserialize,
            schemars::JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id_type!(
    RunId,
    "Correlates every event and artifact of one consensus run."
);
id_type!(
    TaskId,
    "Identifies the user-submitted task under deliberation."
);
id_type!(ProposalId, "Identifies one proposal or revision document.");
id_type!(ClaimId, "Identifies one explicit claim inside a proposal.");
id_type!(EvidenceId, "Identifies one retrieved evidence record.");
id_type!(ChallengeId, "Identifies one critic challenge document.");
id_type!(EventId, "Idempotency key for one event-log record.");
id_type!(
    ProjectId,
    "Stable memory-isolation identity (mirrors the Hyphae project string)."
);
