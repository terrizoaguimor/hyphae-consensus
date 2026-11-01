// SPDX-License-Identifier: Apache-2.0
//! Single-agent baseline: one agent, one model call, no Critic, no
//! Evaluator, no rounds, no policy. It exists so the project can compare
//! one agent against the three-role consensus on the same tasks, with the
//! same model and the same evidence (`docs/evaluation.md`, "Single-agent
//! baseline").
//!
//! A baseline run:
//! 1. records the task (`task_created`, the domain [`Task`]) and its recall
//!    plan (`recall_planned`: the queries, where they came from, the
//!    per-recall limit and the per-run cap);
//! 2. recalls and records evidence with the consensus runtime's own
//!    evidence stage (`recall_evidence` in [`crate::orchestrator`]): the
//!    same memory port, recall limit, per-run cap, foreign-record drop and
//!    `recall_proven` records, so those records have exactly the shape
//!    they have in a consensus log;
//! 3. checks every record's integrity with the runtime's own pre-check
//!    (`check_integrity`: the upstream `proof_error` rule, then the proof
//!    verifier, e.g. `hyphae proof verify` pinned to the operator's
//!    anchor);
//! 4. makes ONE model call ([`BaselineModel::assess`]): the agent sees the
//!    task and the records with their integrity status, as the consensus
//!    roles do, proposes a change, assesses each of its own claims against
//!    the records, lists the objections it sees, and states its own verdict
//!    with a reason ([`BaselineAssessment`]). The document is checked
//!    against the run's record ([`validate_assessment`]) before it is
//!    recorded (`baseline_assessed`, with the call's invocation: request
//!    id, latency, tokens, prompt and schema SHA-256);
//! 5. records one `evidence_verified` per record (round 1), the integrity
//!    status from step 3 and the factual and relevance status derived from
//!    the agent's own claim assessments by the consensus runtime's function
//!    (`verification_record`);
//! 6. closes with `baseline_completed`, counts the runtime derives from the
//!    records ([`BaselineOutcome`]), or with `baseline_failed` at the stage
//!    that stopped it (the refused call kept, as in a consensus run).
//!
//! The agent's verdict is the measured quantity, recorded as the agent's
//! own statement ([`AgentVerdict`]). It is not a recommendation: the
//! runtime derives none, nothing reads it as one, no human decision is
//! recorded on a baseline run and nothing is executed.
//!
//! The log has the consensus log's format: JSONL, one record per line, the
//! envelope fields of [`consensus_domain::ConsensusEvent`] (`sequence` gap
//! free from 1, `event_id`, `run_id`, `actor`, `occurred_at`, `prev_hash`,
//! `hash`, `payload` tagged by `type`) and the same hash chain: `hash` is
//! the lowercase hex SHA-256 of `prev_hash` (`genesis` for the first)
//! followed by the line with its own hash value blanked. Every line
//! re-serializes byte for byte. Two labels set it apart, on every record:
//! `schema_version` is [`BASELINE_SCHEMA_VERSION`] and `run_kind` is
//! [`RUN_KIND`]. The consensus [`crate::EventLog`] refuses that schema
//! version, so a baseline log can never be read, replayed or served as a
//! consensus run; [`verify_baseline_bytes`] and [`replay_baseline`] read
//! it. One log holds one run, chained from genesis.

use crate::clock::{Clock, IdGen};
use crate::contract::{self, ContractViolation, EvaluationContext};
use crate::control::{RunControl, Stop};
use crate::orchestrator::{
    check_integrity, model_error_code, recall_evidence, verification_record, EvidenceStop,
    RunLimits, TaskInput, TokenBudget, MAX_EVIDENCE_PER_RUN, RECALL_LIMIT,
};
use crate::policy::{explain, PolicyTrace, PolicyVersion};
use crate::ports::{
    MemoryClient, ModelError, ProofCheck, ProofFailure, ProofVerifier, RoleContext,
};
use consensus_domain::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// `schema_version` of every baseline record. Not a consensus schema
/// version: the consensus event log refuses it.
pub const BASELINE_SCHEMA_VERSION: &str = "baseline-1.0";

/// `run_kind` of every baseline record.
pub const RUN_KIND: &str = "single_agent_baseline";

/// Most recall queries one baseline run may plan (twelve claims, the Meta
/// adapter's cap, in each of four proposals: a consensus run's whole set).
pub const MAX_QUERIES: usize = 48;

/// Stable codes of a baseline failure, next to the English reason.
pub mod codes {
    /// The agent's document broke the baseline contract
    /// ([`super::validate_assessment`]).
    pub const BASELINE_CONTRACT: &str = "baseline_contract";
}

/// Who acted. The single agent has a role of its own, so a baseline
/// record can never be read as a consensus role's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BaselineActor {
    Agent { role: BaselineRole },
    Runtime,
    Human { id: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineRole {
    SingleAgent,
}

/// The agent's own verdict on its own proposal. Its values are the
/// consensus recommendation kinds, so the two can be compared task by
/// task; it is the model's statement, never a runtime recommendation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentVerdict {
    Supported,
    Contested,
    InsufficientEvidence,
    Abstained,
    Blocked,
}

impl AgentVerdict {
    /// The serialized name (`insufficient_evidence`, ...).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Contested => "contested",
            Self::InsufficientEvidence => "insufficient_evidence",
            Self::Abstained => "abstained",
            Self::Blocked => "blocked",
        }
    }
}

/// Whether the agent judges one of its own objections still to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectionStatus {
    /// It still applies to the proposal as written.
    Open,
    /// The proposal as written already answers it.
    Addressed,
}

/// One objection the agent raises against its own proposal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineObjection {
    pub id: String,
    pub kind: ChallengeKind,
    /// The claim it targets, when it targets one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_id: Option<ClaimId>,
    pub description: String,
    /// Records it rests on.
    pub evidence_refs: Vec<EvidenceId>,
    pub status: ObjectionStatus,
}

/// The single agent's document: its proposal, its assessment of each of
/// its own claims against the retrieved records, its objections and its
/// verdict. Correlation ids, timestamps and the schema version are the
/// adapter's, never the model's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineAssessment {
    pub schema_version: String,
    pub id: String,
    pub run_id: RunId,
    pub summary: String,
    pub plan_steps: Vec<String>,
    pub claims: Vec<Claim>,
    pub assumptions: Vec<String>,
    pub uncertainties: Vec<String>,
    /// One per claim, in claim order: the consensus Evaluator's shape.
    pub assessments: Vec<ClaimAssessment>,
    pub objections: Vec<BaselineObjection>,
    pub verdict: AgentVerdict,
    pub verdict_reason: String,
    pub created_at: String,
}

impl BaselineAssessment {
    /// The claim assessments as an [`Evaluation`] with no objections under
    /// judgment: what the consensus contract checks the assessments
    /// against, and what the factual and relevance status of each
    /// verification record are derived from.
    pub fn claims_evaluation(&self) -> Evaluation {
        Evaluation {
            schema_version: SCHEMA_VERSION.into(),
            run_id: self.run_id.clone(),
            target_proposal: ProposalId::new(&self.id),
            assessments: self.assessments.clone(),
            unresolved_items: Vec::new(),
            item_verdicts: Vec::new(),
            cleared_contradictions: Vec::new(),
            abstain: self.verdict == AgentVerdict::Abstained,
            abstain_reason: (self.verdict == AgentVerdict::Abstained)
                .then(|| self.verdict_reason.clone()),
            created_at: self.created_at.clone(),
        }
    }

    /// [`claims_evaluation`](Self::claims_evaluation) with the agent's open
    /// objections as unresolved items: the input of
    /// [`BaselineRun::policy_reading`].
    pub fn policy_evaluation(&self) -> Evaluation {
        let mut evaluation = self.claims_evaluation();
        evaluation.unresolved_items = self
            .objections
            .iter()
            .filter(|o| o.status == ObjectionStatus::Open)
            .map(|o| o.id.clone())
            .collect();
        evaluation
    }

    /// The proposal part, as a consensus [`Proposal`] (for the claim-id
    /// rule).
    fn as_proposal(&self) -> Proposal {
        Proposal {
            schema_version: SCHEMA_VERSION.into(),
            id: ProposalId::new(&self.id),
            run_id: self.run_id.clone(),
            author_role: AgentRole::Proposer,
            summary: self.summary.clone(),
            plan_steps: self.plan_steps.clone(),
            claims: self.claims.clone(),
            assumptions: self.assumptions.clone(),
            uncertainties: self.uncertainties.clone(),
            created_at: self.created_at.clone(),
        }
    }
}

/// Where a run's recall queries came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum QuerySource {
    /// The task's title and description ([`task_text_queries`]).
    TaskText,
    /// Every claim statement of a recorded consensus run, in order, each
    /// once ([`consensus_queries`]): the queries that run recalled with,
    /// so the baseline retrieves the records that run's final round held
    /// in context (given the same store and anchor).
    ConsensusLog { log_sha256: String, run_id: RunId },
}

/// The recall plan, recorded before any recall.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallPlan {
    pub source: QuerySource,
    pub queries: Vec<String>,
    /// Records asked for per recall ([`RECALL_LIMIT`]).
    pub limit_per_query: u32,
    /// Distinct records processed per run ([`MAX_EVIDENCE_PER_RUN`]).
    pub max_records: u32,
}

/// Integrity statuses of a run's verification records.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntegrityCounts {
    pub valid: u32,
    pub invalid: u32,
    pub unavailable: u32,
    pub verification_error: u32,
    pub not_checked: u32,
}

/// What a completed run recorded, counted by the runtime from the records
/// (re-derived on replay).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BaselineOutcome {
    /// The agent's verdict, as its document states it.
    pub agent_verdict: AgentVerdict,
    pub claims: u32,
    pub claims_corroborated: u32,
    /// Claims for which the agent itself listed a contradicting record.
    pub claims_with_contradicting_evidence: u32,
    pub objections: u32,
    pub objections_open: u32,
    /// Records retrieved (and verified).
    pub records: u32,
    pub integrity: IntegrityCounts,
}

/// A baseline record's payload. The four evidence variants have exactly
/// the names and fields of their [`EventPayload`] counterparts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BaselinePayload {
    TaskCreated {
        task: Task,
    },
    RecallPlanned {
        plan: RecallPlan,
    },
    EvidenceRetrieved {
        evidence: EvidenceReference,
    },
    EvidenceUnavailable {
        query: String,
        reason: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
    RecallProven {
        request: ProofRequest,
        evidence_ids: Vec<EvidenceId>,
        proof_ref: String,
        anchor: String,
        empty: bool,
    },
    EvidenceVerified {
        verification: EvidenceVerification,
    },
    BaselineAssessed {
        assessment: BaselineAssessment,
        invocation: ModelInvocation,
    },
    BaselineCompleted {
        outcome: BaselineOutcome,
    },
    BaselineFailed {
        reason: String,
        stage: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        /// The answered call whose output was refused, when there was one.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        invocation: Option<ModelInvocation>,
    },
}

impl BaselinePayload {
    /// The baseline record of an evidence event; `None` for any other
    /// consensus event (the evidence stage emits only these four).
    pub fn from_evidence_event(payload: EventPayload) -> Option<Self> {
        Some(match payload {
            EventPayload::EvidenceRetrieved { evidence } => Self::EvidenceRetrieved { evidence },
            EventPayload::EvidenceUnavailable {
                query,
                reason,
                code,
            } => Self::EvidenceUnavailable {
                query,
                reason,
                code,
            },
            EventPayload::RecallProven {
                request,
                evidence_ids,
                proof_ref,
                anchor,
                empty,
            } => Self::RecallProven {
                request,
                evidence_ids,
                proof_ref,
                anchor,
                empty,
            },
            EventPayload::EvidenceVerified { verification } => {
                Self::EvidenceVerified { verification }
            }
            _ => return None,
        })
    }

    /// The serialized `type` of the payload.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::TaskCreated { .. } => "task_created",
            Self::RecallPlanned { .. } => "recall_planned",
            Self::EvidenceRetrieved { .. } => "evidence_retrieved",
            Self::EvidenceUnavailable { .. } => "evidence_unavailable",
            Self::RecallProven { .. } => "recall_proven",
            Self::EvidenceVerified { .. } => "evidence_verified",
            Self::BaselineAssessed { .. } => "baseline_assessed",
            Self::BaselineCompleted { .. } => "baseline_completed",
            Self::BaselineFailed { .. } => "baseline_failed",
        }
    }
}

/// One line of a baseline log: the consensus envelope plus `run_kind`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BaselineRecord {
    pub schema_version: String,
    pub run_kind: String,
    pub sequence: u64,
    pub event_id: EventId,
    pub run_id: RunId,
    pub actor: BaselineActor,
    pub occurred_at: String,
    pub prev_hash: String,
    pub hash: String,
    pub payload: BaselinePayload,
}

const GENESIS: &str = "genesis";

/// SHA-256 over (`prev_hash` || the record's JSON with `hash` blanked),
/// lowercase hex: the consensus log's recipe.
pub fn record_hash(record: &BaselineRecord) -> Result<String, serde_json::Error> {
    let mut blanked = record.clone();
    blanked.hash.clear();
    let canonical = serde_json::to_string(&blanked)?;
    let mut hasher = Sha256::new();
    hasher.update(record.prev_hash.as_bytes());
    hasher.update(canonical.as_bytes());
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[derive(Debug, Error)]
pub enum BaselineLogError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization failed: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("{} already exists: a baseline log is written once, never appended to", .0.display())]
    Exists(PathBuf),
    #[error("the log handle refuses appends after a failed write: {0}")]
    Poisoned(String),
    #[error("baseline log line {line}: {detail}")]
    Corrupt { line: usize, detail: String },
}

/// A baseline log being written: a new file, one run, chained from
/// genesis, every record fsync'd before it is acknowledged.
#[derive(Debug)]
pub struct BaselineLog {
    path: PathBuf,
    file: File,
    records: Vec<BaselineRecord>,
    poisoned: Option<String>,
}

impl BaselineLog {
    /// Creates `path` (and its directory); an existing file is refused,
    /// never opened, so no earlier log can be appended to or cut.
    pub fn create(path: impl AsRef<Path>) -> Result<Self, BaselineLogError> {
        let path = path.as_ref().to_path_buf();
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(BaselineLogError::Exists(path))
            }
            Err(e) => return Err(e.into()),
        };
        Ok(Self {
            path,
            file,
            records: Vec::new(),
            poisoned: None,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn records(&self) -> &[BaselineRecord] {
        &self.records
    }

    /// Appends one record: sequenced, chained, hashed, written as one line
    /// and synced. A failed write poisons the handle.
    pub fn append(
        &mut self,
        event_id: EventId,
        run_id: RunId,
        actor: BaselineActor,
        occurred_at: String,
        payload: BaselinePayload,
    ) -> Result<&BaselineRecord, BaselineLogError> {
        if let Some(why) = &self.poisoned {
            return Err(BaselineLogError::Poisoned(why.clone()));
        }
        let mut record = BaselineRecord {
            schema_version: BASELINE_SCHEMA_VERSION.into(),
            run_kind: RUN_KIND.into(),
            sequence: self.records.len() as u64 + 1,
            event_id,
            run_id,
            actor,
            occurred_at,
            prev_hash: self
                .records
                .last()
                .map_or_else(|| GENESIS.to_string(), |r| r.hash.clone()),
            hash: String::new(),
            payload,
        };
        record.hash = record_hash(&record)?;
        let mut line = serde_json::to_string(&record)?;
        line.push('\n');
        let written = self
            .file
            .write_all(line.as_bytes())
            .and_then(|_| self.file.sync_data());
        if let Err(e) = written {
            self.poisoned = Some(e.to_string());
            return Err(e.into());
        }
        self.records.push(record);
        Ok(self.records.last().expect("just pushed"))
    }
}

/// Reads a baseline log from its bytes, never writing: every line a
/// baseline record of [`BASELINE_SCHEMA_VERSION`] and [`RUN_KIND`] that
/// re-serializes byte for byte (so the textual chain recipe and this check
/// agree), sequences gap free from 1, event ids unique, one run id, the
/// chain linked from genesis and every hash recomputed, the file ending
/// with a newline.
pub fn verify_baseline_bytes(bytes: &[u8]) -> Result<Vec<BaselineRecord>, BaselineLogError> {
    let corrupt = |line: usize, detail: String| BaselineLogError::Corrupt { line, detail };
    let text = std::str::from_utf8(bytes).map_err(|e| corrupt(0, format!("not UTF-8: {e}")))?;
    if text.is_empty() {
        return Err(corrupt(0, "empty log".into()));
    }
    let Some(body) = text.strip_suffix('\n') else {
        return Err(corrupt(0, "the last record has no final newline".into()));
    };
    let mut records: Vec<BaselineRecord> = Vec::new();
    for (idx, line) in body.split('\n').enumerate() {
        let n = idx + 1;
        let record: BaselineRecord =
            serde_json::from_str(line).map_err(|e| corrupt(n, e.to_string()))?;
        if record.schema_version != BASELINE_SCHEMA_VERSION {
            return Err(corrupt(
                n,
                format!(
                    "schema_version {:?} is not {BASELINE_SCHEMA_VERSION:?}",
                    record.schema_version
                ),
            ));
        }
        if record.run_kind != RUN_KIND {
            return Err(corrupt(
                n,
                format!("run_kind {:?} is not {RUN_KIND:?}", record.run_kind),
            ));
        }
        if serde_json::to_string(&record)? != line {
            return Err(corrupt(
                n,
                "the line does not re-serialize byte for byte".into(),
            ));
        }
        if record.sequence != n as u64 {
            return Err(corrupt(
                n,
                format!("sequence {} where {n} was expected", record.sequence),
            ));
        }
        if records.iter().any(|r| r.event_id == record.event_id) {
            return Err(corrupt(n, format!("event_id {} repeats", record.event_id)));
        }
        if let Some(first) = records.first() {
            if first.run_id != record.run_id {
                return Err(corrupt(n, "a second run id: one log holds one run".into()));
            }
        }
        let expected_prev = records
            .last()
            .map_or_else(|| GENESIS.to_string(), |r| r.hash.clone());
        if record.prev_hash != expected_prev {
            return Err(corrupt(
                n,
                format!(
                    "prev_hash mismatch: stored {:?}, expected {expected_prev:?}",
                    record.prev_hash
                ),
            ));
        }
        let computed = record_hash(&record)?;
        if computed != record.hash {
            return Err(corrupt(
                n,
                format!("hash mismatch: stored {}, computed {computed}", record.hash),
            ));
        }
        records.push(record);
    }
    Ok(records)
}

/// A broken baseline contract rule.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum BaselineViolation {
    /// A consensus contract rule, applied to the agent's claims and claim
    /// assessments (`claim_ids_unique`, `one_assessment_per_claim`,
    /// `cited_evidence_retrieved`) or to its objection ids
    /// (`item_ids_unique`).
    #[error("{0}")]
    Contract(#[from] ContractViolation),
    #[error("baseline_run_id: the document names run {}, not this run", contract::quote(.0.as_str()))]
    OtherRun(RunId),
    #[error("baseline_has_claims: the proposal states no claim")]
    NoClaims,
    #[error(
        "objection_targets_claim: objection {} names {}, not a claim of the proposal",
        contract::quote(.objection),
        contract::quote(.claim.as_str())
    )]
    ObjectionUnknownClaim { objection: String, claim: ClaimId },
    #[error(
        "objection_cites_retrieved: objection {} cites {}, a record this run never retrieved",
        contract::quote(.objection),
        contract::quote(.evidence.as_str())
    )]
    ObjectionUnretrieved {
        objection: String,
        evidence: EvidenceId,
    },
    #[error("verdict_has_reason: the verdict is given without a reason")]
    VerdictWithoutReason,
}

/// Checks the agent's document against what the run recorded: it belongs
/// to `run_id`, states at least one claim with unique claim ids, assesses
/// every claim exactly once citing only `retrieved` records (the consensus
/// evaluation contract with no objections or contradictions to judge),
/// gives its objections unique non-empty ids, names only its own claims
/// and retrieved records in them, and gives its verdict a reason. Nothing
/// is completed or repaired: a document is accepted as written or refused.
pub fn validate_assessment(
    run_id: &RunId,
    retrieved: &[EvidenceId],
    assessment: &BaselineAssessment,
) -> Result<(), BaselineViolation> {
    if &assessment.run_id != run_id {
        return Err(BaselineViolation::OtherRun(assessment.run_id.clone()));
    }
    if assessment.claims.is_empty() {
        return Err(BaselineViolation::NoClaims);
    }
    let proposal = assessment.as_proposal();
    contract::validate_claims(&proposal)?;
    let ctx = EvaluationContext {
        claims: proposal.claims.iter().map(|c| c.id.clone()).collect(),
        expected_items: Vec::new(),
        retrieved: retrieved.to_vec(),
        carried_contradictions: Vec::new(),
    };
    contract::validate_evaluation(&ctx, &assessment.claims_evaluation())?;
    let as_items = Challenge {
        schema_version: SCHEMA_VERSION.into(),
        id: ChallengeId::new(&assessment.id),
        run_id: assessment.run_id.clone(),
        target_proposal: ProposalId::new(&assessment.id),
        items: assessment
            .objections
            .iter()
            .map(|o| ChallengeItem {
                id: o.id.clone(),
                kind: o.kind,
                claim_id: o.claim_id.clone(),
                description: o.description.clone(),
                evidence_refs: o.evidence_refs.clone(),
            })
            .collect(),
        created_at: assessment.created_at.clone(),
    };
    contract::validate_challenge(&[], &as_items)?;
    for o in &assessment.objections {
        if let Some(claim) = &o.claim_id {
            if !proposal.claims.iter().any(|c| &c.id == claim) {
                return Err(BaselineViolation::ObjectionUnknownClaim {
                    objection: o.id.clone(),
                    claim: claim.clone(),
                });
            }
        }
        if let Some(id) = o.evidence_refs.iter().find(|id| !retrieved.contains(id)) {
            return Err(BaselineViolation::ObjectionUnretrieved {
                objection: o.id.clone(),
                evidence: id.clone(),
            });
        }
    }
    if assessment.verdict_reason.trim().is_empty() {
        return Err(BaselineViolation::VerdictWithoutReason);
    }
    Ok(())
}

/// The counts of a completed run, from its document and verifications.
pub fn derive_outcome(
    assessment: &BaselineAssessment,
    verifications: &[EvidenceVerification],
) -> BaselineOutcome {
    let count = |n: usize| n as u32;
    let mut integrity = IntegrityCounts::default();
    for v in verifications {
        let slot = match v.integrity_status {
            IntegrityStatus::Valid => &mut integrity.valid,
            IntegrityStatus::Invalid => &mut integrity.invalid,
            IntegrityStatus::Unavailable => &mut integrity.unavailable,
            IntegrityStatus::VerificationError => &mut integrity.verification_error,
            IntegrityStatus::NotChecked => &mut integrity.not_checked,
        };
        *slot += 1;
    }
    BaselineOutcome {
        agent_verdict: assessment.verdict,
        claims: count(assessment.claims.len()),
        claims_corroborated: count(
            assessment
                .assessments
                .iter()
                .filter(|a| a.corroborated)
                .count(),
        ),
        claims_with_contradicting_evidence: count(
            assessment
                .assessments
                .iter()
                .filter(|a| !a.contradicting_evidence.is_empty())
                .count(),
        ),
        objections: count(assessment.objections.len()),
        objections_open: count(
            assessment
                .objections
                .iter()
                .filter(|o| o.status == ObjectionStatus::Open)
                .count(),
        ),
        records: count(verifications.len()),
        integrity,
    }
}

/// How a baseline run ended, when it failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaselineFailure {
    pub stage: String,
    pub code: Option<String>,
    pub reason: String,
}

/// A baseline run as its records state it, checked by [`replay_baseline`].
#[derive(Debug, Clone, PartialEq)]
pub struct BaselineRun {
    pub run_id: RunId,
    pub task: Task,
    pub plan: RecallPlan,
    pub retrieved: Vec<EvidenceReference>,
    /// `recall_proven` records.
    pub proofs: u32,
    /// `evidence_unavailable` records: (query, code).
    pub unavailable: Vec<(String, Option<String>)>,
    pub assessment: Option<BaselineAssessment>,
    /// Every model call the run recorded (the assessment's, or the refused
    /// call on `baseline_failed`).
    pub invocations: Vec<ModelInvocation>,
    pub verifications: Vec<EvidenceVerification>,
    pub outcome: Option<BaselineOutcome>,
    pub failure: Option<BaselineFailure>,
    pub event_count: u64,
    pub head_hash: String,
}

impl BaselineRun {
    /// `<verdict>` for a completed run, `failed:<stage>` for a failed one:
    /// the archive's outcome label.
    pub fn outcome_label(&self) -> String {
        match (&self.outcome, &self.failure) {
            (Some(o), _) => o.agent_verdict.as_str().to_string(),
            (None, Some(f)) => format!("failed:{}", f.stage),
            (None, None) => "incomplete".into(),
        }
    }

    /// For comparison only, never recorded and never a recommendation:
    /// what the consensus runtime's policy rules (current version) would
    /// derive from the agent's own claim assessments, its open objections
    /// as unresolved items and the run's verification records. It separates
    /// "the agent saw the problem and approved anyway" from "the agent never
    /// saw it".
    pub fn policy_reading(&self) -> Option<PolicyTrace> {
        let assessment = self.assessment.as_ref()?;
        self.outcome.as_ref()?;
        Some(explain(
            PolicyVersion::CURRENT,
            &assessment.policy_evaluation(),
            &self.verifications,
        ))
    }
}

#[derive(Debug, Error)]
pub enum BaselineReplayError {
    #[error(transparent)]
    Log(#[from] BaselineLogError),
    #[error("record {sequence} ({kind}): {detail}")]
    Invalid {
        sequence: u64,
        kind: &'static str,
        detail: String,
    },
}

/// Where a run is in its fixed record order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Start,
    Created,
    Recalling,
    Assessed,
    Ended,
}

/// Replays a baseline run from its verified records ([`verify_baseline_bytes`])
/// and checks it, so a forged or edited log is refused even when it was
/// re-hashed: `task_created` (by a human, for this run) then
/// `recall_planned` (the plan within its bounds); then only runtime
/// evidence records (no record retrieved twice, at most
/// [`MAX_EVIDENCE_PER_RUN`]); then `baseline_assessed` by the single agent,
/// its document checked again ([`validate_assessment`]); then one
/// `evidence_verified` per retrieved record, in round 1, with the factual
/// and relevance status the document gives it (the consensus rule
/// `verification_derived`, and `round_fully_verified` before the end);
/// then `baseline_completed`, whose counts must equal those derived from
/// the records ([`derive_outcome`]). `baseline_failed` may end the run at
/// any point after the task. Nothing follows the end, and a run without
/// one is refused.
pub fn replay_baseline(records: &[BaselineRecord]) -> Result<BaselineRun, BaselineReplayError> {
    let Some(first) = records.first() else {
        return Err(BaselineLogError::Corrupt {
            line: 0,
            detail: "empty log".into(),
        }
        .into());
    };
    let mut run = BaselineRun {
        run_id: first.run_id.clone(),
        task: Task {
            schema_version: String::new(),
            id: TaskId::new(""),
            run_id: first.run_id.clone(),
            project_id: ProjectId::new(""),
            title: String::new(),
            description: String::new(),
            created_at: String::new(),
            mode: None,
        },
        plan: RecallPlan {
            source: QuerySource::TaskText,
            queries: Vec::new(),
            limit_per_query: 0,
            max_records: 0,
        },
        retrieved: Vec::new(),
        proofs: 0,
        unavailable: Vec::new(),
        assessment: None,
        invocations: Vec::new(),
        verifications: Vec::new(),
        outcome: None,
        failure: None,
        event_count: records.len() as u64,
        head_hash: records.last().map(|r| r.hash.clone()).unwrap_or_default(),
    };
    let mut phase = Phase::Start;
    for r in records {
        let kind = r.payload.type_name();
        let bad = |detail: String| BaselineReplayError::Invalid {
            sequence: r.sequence,
            kind,
            detail,
        };
        if r.run_id != run.run_id {
            return Err(bad("another run's record".into()));
        }
        if phase == Phase::Ended {
            return Err(bad("a record after the run ended".into()));
        }
        let runtime = r.actor == BaselineActor::Runtime;
        match &r.payload {
            BaselinePayload::TaskCreated { task } => {
                if phase != Phase::Start {
                    return Err(bad("the task is not the first record".into()));
                }
                if !matches!(r.actor, BaselineActor::Human { .. }) {
                    return Err(bad("the task is not submitted by a human".into()));
                }
                if task.run_id != run.run_id {
                    return Err(bad("the task names another run".into()));
                }
                run.task = task.clone();
                phase = Phase::Created;
            }
            BaselinePayload::RecallPlanned { plan } => {
                if phase != Phase::Created || !runtime {
                    return Err(bad(
                        "the recall plan must follow the task, by the runtime".into()
                    ));
                }
                if plan.queries.is_empty() || plan.queries.len() > MAX_QUERIES {
                    return Err(bad(format!(
                        "{} queries planned (1 to {MAX_QUERIES} allowed)",
                        plan.queries.len()
                    )));
                }
                if plan.limit_per_query != RECALL_LIMIT as u32
                    || plan.max_records != MAX_EVIDENCE_PER_RUN as u32
                {
                    return Err(bad("the plan's bounds are not the runtime's".into()));
                }
                run.plan = plan.clone();
                phase = Phase::Recalling;
            }
            BaselinePayload::EvidenceRetrieved { evidence } => {
                if phase != Phase::Recalling || !runtime {
                    return Err(bad("evidence outside the recall stage".into()));
                }
                if run.retrieved.iter().any(|e| e.id == evidence.id) {
                    return Err(bad(format!("{} is retrieved twice", evidence.id)));
                }
                if run.retrieved.len() >= MAX_EVIDENCE_PER_RUN {
                    return Err(bad("past the per-run evidence cap".into()));
                }
                run.retrieved.push(evidence.clone());
            }
            BaselinePayload::EvidenceUnavailable { query, code, .. } => {
                if phase != Phase::Recalling || !runtime {
                    return Err(bad("evidence outside the recall stage".into()));
                }
                run.unavailable.push((query.clone(), code.clone()));
            }
            BaselinePayload::RecallProven { .. } => {
                if phase != Phase::Recalling || !runtime {
                    return Err(bad("evidence outside the recall stage".into()));
                }
                run.proofs += 1;
            }
            BaselinePayload::BaselineAssessed {
                assessment,
                invocation,
            } => {
                if phase != Phase::Recalling
                    || r.actor
                        != (BaselineActor::Agent {
                            role: BaselineRole::SingleAgent,
                        })
                {
                    return Err(bad(
                        "the assessment must follow the recall stage, by the single agent".into(),
                    ));
                }
                let ids: Vec<EvidenceId> = run.retrieved.iter().map(|e| e.id.clone()).collect();
                validate_assessment(&run.run_id, &ids, assessment)
                    .map_err(|v| bad(v.to_string()))?;
                run.assessment = Some(assessment.clone());
                run.invocations.push(invocation.clone());
                phase = Phase::Assessed;
            }
            BaselinePayload::EvidenceVerified { verification } => {
                let Some(assessment) = (phase == Phase::Assessed && runtime)
                    .then_some(run.assessment.as_ref())
                    .flatten()
                else {
                    return Err(bad("a verification outside the verification stage".into()));
                };
                contract::validate_verification(
                    1,
                    &run.retrieved,
                    &run.verifications,
                    &assessment.claims_evaluation(),
                    verification,
                )
                .map_err(|v| bad(v.to_string()))?;
                run.verifications.push(verification.clone());
            }
            BaselinePayload::BaselineCompleted { outcome } => {
                let Some(assessment) = (phase == Phase::Assessed && runtime)
                    .then_some(run.assessment.as_ref())
                    .flatten()
                else {
                    return Err(bad("the run completes only after its assessment".into()));
                };
                contract::validate_round_verified(&run.retrieved, &run.verifications)
                    .map_err(|v| bad(v.to_string()))?;
                let derived = derive_outcome(assessment, &run.verifications);
                if &derived != outcome {
                    return Err(bad(format!(
                        "the recorded counts {outcome:?} are not those of the records {derived:?}"
                    )));
                }
                run.outcome = Some(outcome.clone());
                phase = Phase::Ended;
            }
            BaselinePayload::BaselineFailed {
                reason,
                stage,
                code,
                invocation,
            } => {
                if phase == Phase::Start || !runtime {
                    return Err(bad(
                        "a failure before the task, or not by the runtime".into()
                    ));
                }
                if let Some(call) = invocation {
                    run.invocations.push(call.clone());
                }
                run.failure = Some(BaselineFailure {
                    stage: stage.clone(),
                    code: code.clone(),
                    reason: reason.clone(),
                });
                phase = Phase::Ended;
            }
        }
    }
    if phase != Phase::Ended {
        return Err(BaselineReplayError::Invalid {
            sequence: run.event_count,
            kind: records.last().map_or("none", |r| r.payload.type_name()),
            detail: "the run has no end (baseline_completed or baseline_failed)".into(),
        });
    }
    Ok(run)
}

/// [`verify_baseline_bytes`] then [`replay_baseline`].
pub fn read_baseline(bytes: &[u8]) -> Result<BaselineRun, BaselineReplayError> {
    let records = verify_baseline_bytes(bytes)?;
    replay_baseline(&records)
}

/// `format` of [`baseline_summary`].
pub const SUMMARY_FORMAT: &str = "hyphae-consensus-baseline-summary-v1";

/// What `consensus-api baseline run` and `baseline verify` print and the
/// baseline driver archives as a run's summary: everything in it is read
/// from the run's records (`log_sha256` is the digest of the log's bytes).
/// The agent's verdict is the agent's own statement; `policy_reading` is
/// the comparison-only reading of [`BaselineRun::policy_reading`]. Neither
/// is a recommendation.
pub fn baseline_summary(run: &BaselineRun, log_sha256: &str) -> serde_json::Value {
    use serde_json::json;
    let assessment = run.assessment.as_ref();
    let verifications: Vec<serde_json::Value> = run
        .verifications
        .iter()
        .map(|v| {
            json!({
                "evidence_id": v.evidence_id,
                "integrity_status": v.integrity_status,
                "factual_status": v.factual_status,
                "relevance_status": v.relevance_status,
                "verifier": v.verifier,
                "verified_anchor": v.verified_anchor,
            })
        })
        .collect();
    let unavailable: Vec<serde_json::Value> = run
        .unavailable
        .iter()
        .map(|(query, code)| json!({"query": query, "code": code}))
        .collect();
    json!({
        "format": SUMMARY_FORMAT,
        "run_kind": RUN_KIND,
        "schema_version": BASELINE_SCHEMA_VERSION,
        "run_id": run.run_id,
        "event_count": run.event_count,
        "head_hash": run.head_hash,
        "log_sha256": log_sha256,
        "task": {
            "project_id": run.task.project_id,
            "title": run.task.title,
            "description": run.task.description,
            "mode": run.task.mode,
        },
        "recall": {
            "source": run.plan.source,
            "queries": run.plan.queries.len(),
            "limit_per_query": run.plan.limit_per_query,
            "max_records": run.plan.max_records,
        },
        "evidence": {
            "retrieved": run.retrieved.iter().map(|e| e.id.clone()).collect::<Vec<_>>(),
            "recall_proven": run.proofs,
            "unavailable": unavailable,
        },
        "verifications": verifications,
        "model_invocations": run.invocations,
        "outcome": run.outcome_label(),
        "agent_verdict": assessment.map(|a| a.verdict),
        "verdict_reason": assessment.map(|a| a.verdict_reason.clone()),
        "counts": run.outcome,
        "failure": run.failure.as_ref().map(|f| json!({
            "stage": f.stage,
            "code": f.code,
            "reason": f.reason,
        })),
        "policy_reading": run.policy_reading().map(|t| json!({
            "kind": t.kind,
            "rule": t.rule,
        })),
        "note": "Single-agent baseline: one model call, no Critic, no Evaluator, no policy rounds. The agent's verdict is its own statement and policy_reading is a comparison aid; neither is a recommendation, nothing was executed and no human decision is recorded.",
    })
}

/// The single agent's one call. The document returned must keep
/// [`validate_assessment`] for the run's retrieved records; the runner
/// checks it before recording anything and fails the run on a violation,
/// keeping the call on the record. A client that refuses a document after
/// the provider answered returns the error with the call's invocation
/// ([`ModelError::after_call`]) or keeps it for
/// [`take_failed_invocation`](Self::take_failed_invocation), as a
/// consensus [`crate::ports::ModelClient`] does.
pub trait BaselineModel {
    /// `ctx` holds every retrieved record with its integrity status; it has
    /// no objections or contradictions (nothing came before this call).
    fn assess(
        &mut self,
        task: &Task,
        ctx: &RoleContext,
    ) -> Result<(BaselineAssessment, ModelInvocation), ModelError>;

    fn take_failed_invocation(&mut self) -> Option<ModelInvocation> {
        None
    }
}

/// The queries of [`QuerySource::TaskText`]: the title and the
/// description, each once and non-empty.
pub fn task_text_queries(title: &str, description: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for text in [title, description] {
        let text = text.trim();
        if !text.is_empty() && !out.iter().any(|q| q == text) {
            out.push(text.to_string());
        }
    }
    out
}

/// The queries of [`QuerySource::ConsensusLog`]: every claim statement of
/// run `run_id` in a verified consensus log, from its proposal and every
/// revision, in record order, each once. `None` when the run has no
/// proposal.
pub fn consensus_queries(events: &[ConsensusEvent], run_id: &RunId) -> Option<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for ev in events.iter().filter(|e| &e.run_id == run_id) {
        let claims = match &ev.payload {
            EventPayload::ProposalSubmitted { proposal, .. } => &proposal.claims,
            EventPayload::RevisionSubmitted { revision, .. } => &revision.proposal.claims,
            _ => continue,
        };
        for claim in claims {
            if !out.contains(&claim.statement) {
                out.push(claim.statement.clone());
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

#[derive(Debug, Error)]
pub enum BaselineError {
    #[error("baseline log: {0}")]
    Log(#[from] BaselineLogError),
    #[error("baseline replay: {0}")]
    Replay(#[from] BaselineReplayError),
    #[error("the recall plan has {0} queries (1 to {MAX_QUERIES} allowed)")]
    Queries(usize),
}

/// Runs one single-agent baseline: see the module documentation for the
/// record order. The ports are the consensus runtime's.
pub struct BaselineRunner<M, Mem, V, C, I> {
    pub model: M,
    pub memory: Mem,
    pub verifier: V,
    pub clock: C,
    pub ids: I,
    pub limits: RunLimits,
}

impl<M: BaselineModel, Mem: MemoryClient, V: ProofVerifier, C: Clock, I: IdGen>
    BaselineRunner<M, Mem, V, C, I>
{
    /// A new baseline run id, `baseline-<id>`: never a consensus run id.
    pub fn new_run_id(&mut self) -> RunId {
        RunId::new(self.ids.next("baseline"))
    }

    /// Runs the task as run `run_id` ([`new_run_id`](Self::new_run_id))
    /// with `queries` (from `source`) into `log`, a new log, and returns the
    /// run as [`replay_baseline`] reads it back from the records written.
    /// `control` is checked before every recall, every proof verification
    /// and the model call, as in a consensus run. A plan without queries,
    /// or with more than [`MAX_QUERIES`], is refused before anything is
    /// recorded.
    pub fn run(
        &mut self,
        log: &mut BaselineLog,
        run_id: RunId,
        input: TaskInput,
        source: QuerySource,
        queries: Vec<String>,
        control: &RunControl,
    ) -> Result<BaselineRun, BaselineError> {
        if queries.is_empty() || queries.len() > MAX_QUERIES {
            return Err(BaselineError::Queries(queries.len()));
        }
        let task = Task {
            schema_version: SCHEMA_VERSION.into(),
            id: TaskId::new(self.ids.next("task")),
            run_id: run_id.clone(),
            project_id: input.project_id.clone(),
            title: input.title,
            description: input.description,
            created_at: self.clock.now(),
            mode: input.mode,
        };
        self.emit(
            log,
            &run_id,
            BaselineActor::Human {
                id: input.submitted_by,
            },
            BaselinePayload::TaskCreated { task: task.clone() },
        )?;
        self.emit(
            log,
            &run_id,
            BaselineActor::Runtime,
            BaselinePayload::RecallPlanned {
                plan: RecallPlan {
                    source,
                    queries: queries.clone(),
                    limit_per_query: RECALL_LIMIT as u32,
                    max_records: MAX_EVIDENCE_PER_RUN as u32,
                },
            },
        )?;
        self.deliberate(log, &run_id, &task, &queries, control)?;
        Ok(replay_baseline(log.records())?)
    }

    fn deliberate(
        &mut self,
        log: &mut BaselineLog,
        run_id: &RunId,
        task: &Task,
        queries: &[String],
        control: &RunControl,
    ) -> Result<(), BaselineError> {
        // The consensus evidence stage, recorded in this log.
        let mut proof_failures: Vec<(EvidenceId, ProofFailure)> = Vec::new();
        let recalled = {
            let (ids, clock) = (&mut self.ids, &self.clock);
            let mut record = |payload: EventPayload| -> Result<(), BaselineLogError> {
                let payload = BaselinePayload::from_evidence_event(payload)
                    .expect("the evidence stage emits only evidence records");
                append(ids, clock, log, run_id, BaselineActor::Runtime, payload)
            };
            recall_evidence(
                &mut self.memory,
                &task.project_id,
                queries,
                &[],
                control,
                &mut proof_failures,
                &mut record,
            )
        };
        match recalled {
            Ok(()) => {}
            Err(EvidenceStop::Stopped(stop, checkpoint)) => {
                return self.stop(log, run_id, stop, checkpoint)
            }
            Err(EvidenceStop::Record(e)) => return Err(e.into()),
        }
        let evidence: Vec<EvidenceReference> = log
            .records()
            .iter()
            .filter_map(|r| match &r.payload {
                BaselinePayload::EvidenceRetrieved { evidence } => Some(evidence.clone()),
                _ => None,
            })
            .collect();
        let checks: Vec<ProofCheck> =
            match check_integrity(&mut self.verifier, &evidence, &proof_failures, control) {
                Ok(checks) => checks,
                Err((stop, checkpoint)) => return self.stop(log, run_id, stop, checkpoint),
            };
        let ctx = RoleContext {
            integrity: evidence
                .iter()
                .zip(&checks)
                .map(|(ev, check)| (ev.id.clone(), check.status))
                .collect(),
            evidence: evidence.clone(),
            open_items: Vec::new(),
            carried_contradictions: Vec::new(),
        };

        // The one model call.
        if let Some(stop) = control.check() {
            return self.stop(log, run_id, stop, "assess call");
        }
        let mut budget = TokenBudget::new(&self.limits);
        let (assessment, invocation) = match self.model.assess(task, &ctx) {
            Ok(v) => v,
            Err(e) => {
                // Always taken, so a kept call can never surface later.
                let kept = self.model.take_failed_invocation();
                let invocation = e.invocation().cloned().or(kept);
                return self.fail(
                    log,
                    run_id,
                    "assess",
                    model_error_code(&e),
                    &e.to_string(),
                    invocation,
                );
            }
        };
        let over_budget = budget.charge("assess", &invocation);
        let ids: Vec<EvidenceId> = evidence.iter().map(|e| e.id.clone()).collect();
        if let Err(violation) = validate_assessment(run_id, &ids, &assessment) {
            let reason = format!("assess output refused by the baseline contract: {violation}");
            return self.fail(
                log,
                run_id,
                "assess",
                codes::BASELINE_CONTRACT,
                &reason,
                Some(invocation),
            );
        }
        self.emit(
            log,
            run_id,
            BaselineActor::Agent {
                role: BaselineRole::SingleAgent,
            },
            BaselinePayload::BaselineAssessed {
                assessment: assessment.clone(),
                invocation,
            },
        )?;
        if let Some((code, reason)) = over_budget {
            return self.fail(log, run_id, "token_budget", code, &reason, None);
        }

        // One verification per record, round 1, as a consensus round
        // records them after its evaluation.
        let evaluation = assessment.claims_evaluation();
        let mut verifications = Vec::with_capacity(evidence.len());
        for (ev, check) in evidence.iter().zip(checks) {
            let verification = verification_record(ev, check, &evaluation, 1, self.clock.now());
            verifications.push(verification.clone());
            self.emit(
                log,
                run_id,
                BaselineActor::Runtime,
                BaselinePayload::EvidenceVerified { verification },
            )?;
        }
        let outcome = derive_outcome(&assessment, &verifications);
        self.emit(
            log,
            run_id,
            BaselineActor::Runtime,
            BaselinePayload::BaselineCompleted { outcome },
        )?;
        Ok(())
    }

    fn emit(
        &mut self,
        log: &mut BaselineLog,
        run_id: &RunId,
        actor: BaselineActor,
        payload: BaselinePayload,
    ) -> Result<(), BaselineError> {
        Ok(append(
            &mut self.ids,
            &self.clock,
            log,
            run_id,
            actor,
            payload,
        )?)
    }

    fn fail(
        &mut self,
        log: &mut BaselineLog,
        run_id: &RunId,
        stage: &str,
        code: &str,
        reason: &str,
        invocation: Option<ModelInvocation>,
    ) -> Result<(), BaselineError> {
        self.emit(
            log,
            run_id,
            BaselineActor::Runtime,
            BaselinePayload::BaselineFailed {
                reason: reason.to_string(),
                stage: stage.to_string(),
                code: Some(code.to_string()),
                invocation,
            },
        )
    }

    fn stop(
        &mut self,
        log: &mut BaselineLog,
        run_id: &RunId,
        stop: Stop,
        checkpoint: &str,
    ) -> Result<(), BaselineError> {
        self.fail(
            log,
            run_id,
            stop.stage(),
            stop.stage(),
            &stop.reason(checkpoint),
            None,
        )
    }
}

fn append<C: Clock, I: IdGen>(
    ids: &mut I,
    clock: &C,
    log: &mut BaselineLog,
    run_id: &RunId,
    actor: BaselineActor,
    payload: BaselinePayload,
) -> Result<(), BaselineLogError> {
    log.append(
        EventId::new(ids.next("evt")),
        run_id.clone(),
        actor,
        clock.now(),
        payload,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{FixedClock, SequentialIdGen};
    use crate::control::CancelReason;
    use crate::event_log::{verify_bytes, EventLog, EventLogError};
    use crate::fakes::{FakeMemoryClient, FakeProofVerifier, ScriptedBaselineModel};
    use crate::orchestrator::Orchestrator;
    use crate::scenarios::{
        baseline_document, baseline_scenario, evidence, retry_constraint_scenario, TS,
    };

    type Runner = BaselineRunner<
        ScriptedBaselineModel,
        FakeMemoryClient,
        FakeProofVerifier,
        FixedClock,
        SequentialIdGen,
    >;

    fn temp_path(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("hyphae-consensus-baseline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let _ = std::fs::remove_file(&path);
        path
    }

    fn runner(model: ScriptedBaselineModel, memory: FakeMemoryClient) -> Runner {
        BaselineRunner {
            model,
            memory,
            verifier: FakeProofVerifier::all(IntegrityStatus::Valid),
            clock: FixedClock::new(TS),
            ids: SequentialIdGen::new(),
            limits: RunLimits::default(),
        }
    }

    /// The consensus reference run, recorded with the scripted doubles:
    /// its log and run id.
    fn consensus_run(name: &str) -> (EventLog, RunId) {
        let s = retry_constraint_scenario();
        let mut log = EventLog::open(temp_path(name)).unwrap();
        let mut orch = Orchestrator {
            model: s.model,
            memory: s.memory,
            verifier: s.verifier,
            clock: FixedClock::new(TS),
            ids: SequentialIdGen::new(),
            limits: RunLimits::default(),
        };
        let agg = orch.run(&mut log, s.input).unwrap();
        (log, agg.run_id)
    }

    /// The baseline reference run with the consensus run's own queries.
    fn reference(name: &str) -> (BaselineRun, PathBuf, EventLog, RunId) {
        let (consensus, consensus_id) = consensus_run(&format!("{name}-consensus.jsonl"));
        let queries = consensus_queries(consensus.all_events(), &consensus_id).unwrap();
        let s = baseline_scenario();
        let path = temp_path(&format!("{name}.jsonl"));
        let mut log = BaselineLog::create(&path).unwrap();
        let mut r = runner(s.model, s.memory);
        let source = QuerySource::ConsensusLog {
            log_sha256: "0".repeat(64),
            run_id: consensus_id.clone(),
        };
        let id = r.new_run_id();
        let run = r
            .run(&mut log, id, s.input, source, queries, &RunControl::new())
            .unwrap();
        (run, path, consensus, consensus_id)
    }

    fn kinds(records: &[BaselineRecord]) -> Vec<&'static str> {
        records.iter().map(|r| r.payload.type_name()).collect()
    }

    #[test]
    fn a_baseline_run_records_its_one_call_in_a_chained_log_of_its_own_kind() {
        let (run, path, _, _) = reference("reference");
        let bytes = std::fs::read(&path).unwrap();
        let records = verify_baseline_bytes(&bytes).unwrap();
        assert_eq!(
            kinds(&records),
            [
                "task_created",
                "recall_planned",
                "evidence_retrieved",
                "evidence_retrieved",
                "baseline_assessed",
                "evidence_verified",
                "evidence_verified",
                "baseline_completed",
            ]
        );
        assert!(records
            .iter()
            .all(|r| r.schema_version == BASELINE_SCHEMA_VERSION && r.run_kind == RUN_KIND));
        assert!(
            run.run_id.as_str().starts_with("baseline-"),
            "{}",
            run.run_id
        );
        // Exactly one model call, with its invocation on the record.
        assert_eq!(run.invocations.len(), 1);
        assert_eq!(
            run.invocations[0].request_id.as_deref(),
            Some("fake-assess-1")
        );
        assert_eq!(run.event_count, 8);
        assert_eq!(run.head_hash, records[7].hash);
        // The agent's own verdict is what the outcome says, and the counts
        // are the records'.
        let outcome = run.outcome.clone().unwrap();
        assert_eq!(outcome.agent_verdict, AgentVerdict::Supported);
        assert_eq!(run.outcome_label(), "supported");
        assert_eq!(
            (
                outcome.claims,
                outcome.claims_corroborated,
                outcome.claims_with_contradicting_evidence
            ),
            (2, 1, 1)
        );
        assert_eq!((outcome.objections, outcome.objections_open), (1, 1));
        assert_eq!(outcome.records, 2);
        assert_eq!(outcome.integrity.valid, 2);
        // Factual and relevance status come from the agent's assessments,
        // by the consensus rule: ev-constraint contradicted, ev-fact
        // corroborated.
        let status: Vec<(&str, FactualStatus)> = run
            .verifications
            .iter()
            .map(|v| (v.evidence_id.as_str(), v.factual_status))
            .collect();
        assert_eq!(
            status,
            [
                ("ev-constraint", FactualStatus::Contradicted),
                ("ev-fact", FactualStatus::Corroborated)
            ]
        );
        assert!(run.verifications.iter().all(|v| v.round == Some(1)));
        // The consensus rules, applied to the agent's own findings, would
        // not have called it supported: the comparison's second measure.
        let reading = run.policy_reading().unwrap();
        assert_eq!(reading.kind, RecommendationKind::Contested);
        // Reading the file back gives the same run.
        assert_eq!(read_baseline(&bytes).unwrap(), run);
    }

    #[test]
    fn the_chain_is_the_consensus_recipe_on_the_raw_lines() {
        let (_, path, _, _) = reference("raw-chain");
        let text = std::fs::read_to_string(&path).unwrap();
        let mut prev = "genesis".to_string();
        for (n, line) in text.strip_suffix('\n').unwrap().split('\n').enumerate() {
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            assert_eq!(value["sequence"].as_u64(), Some(n as u64 + 1));
            assert_eq!(value["prev_hash"], prev.as_str());
            let hash = value["hash"].as_str().unwrap().to_string();
            let field = format!(r#""hash":"{hash}""#);
            assert_eq!(line.matches(&field).count(), 1);
            let blanked = line.replacen(&field, r#""hash":"""#, 1);
            let digest: String = Sha256::digest(format!("{prev}{blanked}").as_bytes())
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            assert_eq!(digest, hash, "line {}", n + 1);
            prev = hash;
        }
    }

    #[test]
    fn the_consensus_log_refuses_a_baseline_log() {
        let (_, path, _, _) = reference("refused");
        let bytes = std::fs::read(&path).unwrap();
        assert!(matches!(
            verify_bytes(&bytes),
            Err(EventLogError::SchemaVersion { line: 1, .. })
        ));
        assert!(EventLog::open_read_only(&path).is_err());
        // And the baseline reader refuses a consensus log.
        let (consensus, _) = consensus_run("refused-other.jsonl");
        let consensus_bytes: String = consensus
            .all_events()
            .iter()
            .map(|e| serde_json::to_string(e).unwrap() + "\n")
            .collect();
        assert!(verify_bytes(consensus_bytes.as_bytes()).is_ok());
        assert!(verify_baseline_bytes(consensus_bytes.as_bytes()).is_err());
    }

    #[test]
    fn with_the_consensus_queries_the_baseline_retrieves_the_consensus_evidence() {
        let (run, path, consensus, consensus_id) = reference("same-evidence");
        // Every record the consensus run retrieved, each with the same
        // evidence record, and nothing else.
        let consensus_records: Vec<serde_json::Value> = consensus
            .events_for(&consensus_id)
            .filter(|e| matches!(e.payload, EventPayload::EvidenceRetrieved { .. }))
            .map(|e| serde_json::to_value(&e.payload).unwrap())
            .collect();
        let records = verify_baseline_bytes(&std::fs::read(&path).unwrap()).unwrap();
        let baseline_records: Vec<serde_json::Value> = records
            .iter()
            .filter(|r| matches!(r.payload, BaselinePayload::EvidenceRetrieved { .. }))
            .map(|r| serde_json::to_value(&r.payload).unwrap())
            .collect();
        assert_eq!(consensus_records.len(), 2);
        let mut a = consensus_records.clone();
        let mut b = baseline_records.clone();
        let key = |v: &serde_json::Value| v["evidence"]["id"].as_str().unwrap().to_string();
        a.sort_by_key(key);
        b.sort_by_key(key);
        assert_eq!(a, b, "the same payloads, byte for byte in JSON");
        // The integrity checks are the consensus run's: the same verifier,
        // the same verifier fields.
        let consensus_checks: Vec<(String, Option<String>, Option<String>)> = consensus
            .events_for(&consensus_id)
            .filter_map(|e| match &e.payload {
                EventPayload::EvidenceVerified { verification }
                    if verification.round == Some(1) =>
                {
                    Some((
                        verification.evidence_id.to_string(),
                        verification.verifier.clone(),
                        verification.detail.clone(),
                    ))
                }
                _ => None,
            })
            .collect();
        let baseline_checks: Vec<(String, Option<String>, Option<String>)> = run
            .verifications
            .iter()
            .map(|v| {
                (
                    v.evidence_id.to_string(),
                    v.verifier.clone(),
                    v.detail.clone(),
                )
            })
            .collect();
        assert_eq!(consensus_checks, baseline_checks);
        // The plan says where the queries came from.
        assert!(matches!(run.plan.source, QuerySource::ConsensusLog { .. }));
        assert_eq!(run.plan.limit_per_query, RECALL_LIMIT as u32);
    }

    #[test]
    fn evidence_payloads_serialize_exactly_as_the_consensus_ones() {
        let ev = evidence("ev-1", "demo-repo", "fact", "Fact: x.");
        let payloads = vec![
            EventPayload::EvidenceRetrieved { evidence: ev },
            EventPayload::EvidenceUnavailable {
                query: "q".into(),
                reason: "r".into(),
                code: Some("memory_unavailable".into()),
            },
            EventPayload::EvidenceUnavailable {
                query: "q".into(),
                reason: "r".into(),
                code: None,
            },
            EventPayload::RecallProven {
                request: ProofRequest {
                    operation: "list".into(),
                    project: "demo-repo".into(),
                    query: None,
                    limit: Some(20),
                },
                evidence_ids: vec![EvidenceId::new("ev-1")],
                proof_ref: "proof-1".into(),
                anchor: "a".repeat(64),
                empty: false,
            },
        ];
        for p in payloads {
            let consensus = serde_json::to_string(&p).unwrap();
            let baseline =
                serde_json::to_string(&BaselinePayload::from_evidence_event(p).unwrap()).unwrap();
            assert_eq!(consensus, baseline);
        }
        assert!(
            BaselinePayload::from_evidence_event(EventPayload::RunFailed {
                reason: "x".into(),
                stage: "y".into(),
                code: None,
                invocation: None
            })
            .is_none()
        );
    }

    fn task_text_run(
        name: &str,
        model: ScriptedBaselineModel,
        limits: RunLimits,
    ) -> (BaselineRun, PathBuf) {
        let s = baseline_scenario();
        let path = temp_path(&format!("{name}.jsonl"));
        let mut log = BaselineLog::create(&path).unwrap();
        let mut r = runner(model, s.memory);
        r.limits = limits;
        let queries = task_text_queries(&s.input.title, &s.input.description);
        let id = r.new_run_id();
        let run = r
            .run(
                &mut log,
                id,
                s.input,
                QuerySource::TaskText,
                queries,
                &RunControl::new(),
            )
            .unwrap();
        (run, path)
    }

    #[test]
    fn a_document_citing_an_unretrieved_record_fails_the_run_and_keeps_the_call() {
        // The task text recalls only ev-fact from the scripted memory; the
        // document cites ev-constraint, which this run never retrieved.
        let mut model = ScriptedBaselineModel::new();
        model.assessments.push_back(Ok(baseline_document()));
        let (run, path) = task_text_run("contract", model, RunLimits::default());
        assert_eq!(run.retrieved.len(), 1);
        assert_eq!(run.retrieved[0].id.as_str(), "ev-fact");
        let failure = run.failure.clone().unwrap();
        assert_eq!(failure.stage, "assess");
        assert_eq!(failure.code.as_deref(), Some(codes::BASELINE_CONTRACT));
        assert!(
            failure.reason.contains("cited_evidence_retrieved"),
            "{}",
            failure.reason
        );
        assert_eq!(
            run.invocations.len(),
            1,
            "the refused call stays on the record"
        );
        assert_eq!(run.outcome_label(), "failed:assess");
        assert!(run.policy_reading().is_none());
        let records = verify_baseline_bytes(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(kinds(&records).last(), Some(&"baseline_failed"));
        assert!(!kinds(&records).contains(&"baseline_assessed"));
    }

    #[test]
    fn a_model_failure_fails_the_run_at_assess_with_its_code() {
        let mut model = ScriptedBaselineModel::new();
        model
            .assessments
            .push_back(Err(ModelError::Unavailable("down".into())));
        let (run, _) = task_text_run("model-error", model, RunLimits::default());
        let failure = run.failure.unwrap();
        assert_eq!(failure.stage, "assess");
        assert_eq!(failure.code.as_deref(), Some("model_unavailable"));
        assert!(run.invocations.is_empty());
    }

    #[test]
    fn a_call_past_the_token_cap_is_recorded_then_fails_the_run() {
        let mut doc = baseline_document();
        // Cite only what the task text retrieves.
        doc.assessments[1].contradicting_evidence.clear();
        doc.objections[0].evidence_refs.clear();
        let mut model = ScriptedBaselineModel::new();
        model.tokens_per_call = (900, 200);
        model.assessments.push_back(Ok(doc));
        let (run, path) = task_text_run(
            "budget",
            model,
            RunLimits {
                max_run_tokens: 1000,
            },
        );
        assert_eq!(run.failure.unwrap().stage, "token_budget");
        assert!(run.assessment.is_some(), "the document is recorded first");
        let records = verify_baseline_bytes(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(
            kinds(&records)[kinds(&records).len() - 2..],
            ["baseline_assessed", "baseline_failed"]
        );
    }

    #[test]
    fn a_tripped_control_stops_the_run_before_any_recall() {
        let s = baseline_scenario();
        let path = temp_path("cancelled.jsonl");
        let mut log = BaselineLog::create(&path).unwrap();
        let mut r = runner(s.model, s.memory);
        let control = RunControl::new();
        control.cancel(CancelReason::Owner);
        let queries = task_text_queries(&s.input.title, &s.input.description);
        let id = r.new_run_id();
        let run = r
            .run(
                &mut log,
                id,
                s.input,
                QuerySource::TaskText,
                queries,
                &control,
            )
            .unwrap();
        assert_eq!(run.outcome_label(), "failed:cancelled");
        assert!(r.model.contexts.is_empty(), "no model call");
        assert_eq!(
            kinds(log.records()),
            ["task_created", "recall_planned", "baseline_failed"]
        );
    }

    /// Rewrites `records` with `edit` applied, re-chained from genesis:
    /// what a forger who recomputes every hash would write.
    fn forged(records: &[BaselineRecord], edit: impl Fn(&mut BaselineRecord)) -> Vec<u8> {
        let mut prev = GENESIS.to_string();
        let mut out = String::new();
        for r in records {
            let mut r = r.clone();
            edit(&mut r);
            r.prev_hash = prev.clone();
            r.hash = record_hash(&r).unwrap();
            prev = r.hash.clone();
            out.push_str(&serde_json::to_string(&r).unwrap());
            out.push('\n');
        }
        out.into_bytes()
    }

    #[test]
    fn edited_logs_are_refused_even_when_re_hashed() {
        let (_, path, _, _) = reference("forged");
        let bytes = std::fs::read(&path).unwrap();
        let records = verify_baseline_bytes(&bytes).unwrap();
        // An edit without re-hashing breaks the chain.
        let text = String::from_utf8(bytes.clone()).unwrap();
        let tampered = text.replacen("Raise the payment", "Lower the payment", 1);
        assert!(verify_baseline_bytes(tampered.as_bytes()).is_err());
        // Re-hashed: the chain holds, replay refuses.
        let flipped = forged(&records, |r| {
            if let BaselinePayload::EvidenceVerified { verification } = &mut r.payload {
                verification.factual_status = FactualStatus::Corroborated;
            }
        });
        let err = read_baseline(&flipped).unwrap_err().to_string();
        assert!(err.contains("verification_derived"), "{err}");
        let recounted = forged(&records, |r| {
            if let BaselinePayload::BaselineCompleted { outcome } = &mut r.payload {
                outcome.agent_verdict = AgentVerdict::Contested;
            }
        });
        let err = read_baseline(&recounted).unwrap_err().to_string();
        assert!(err.contains("recorded counts"), "{err}");
        let relabelled = forged(&records, |r| r.schema_version = "1.3".into());
        assert!(verify_baseline_bytes(&relabelled).is_err());
        // A verification dropped before the end.
        let dropped: Vec<BaselineRecord> = records
            .iter()
            .filter(|r| {
                !matches!(&r.payload, BaselinePayload::EvidenceVerified { verification }
                    if verification.evidence_id.as_str() == "ev-fact")
            })
            .cloned()
            .map(|mut r| {
                if r.sequence > 6 {
                    r.sequence -= 1;
                }
                r
            })
            .collect();
        let err = read_baseline(&forged(&dropped, |_| {}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("round_fully_verified"), "{err}");
        // A log cut before its end.
        let cut = forged(&records[..5], |_| {});
        let err = read_baseline(&cut).unwrap_err().to_string();
        assert!(err.contains("has no end"), "{err}");
    }

    #[test]
    fn a_log_is_written_once() {
        let path = temp_path("once.jsonl");
        drop(BaselineLog::create(&path).unwrap());
        assert!(matches!(
            BaselineLog::create(&path),
            Err(BaselineLogError::Exists(_))
        ));
    }

    #[test]
    fn query_plans_are_bounded_and_deduplicated() {
        assert_eq!(task_text_queries("  a ", "a"), ["a"]);
        assert_eq!(task_text_queries("a", " "), ["a"]);
        let (consensus, id) = consensus_run("queries.jsonl");
        let queries = consensus_queries(consensus.all_events(), &id).unwrap();
        // The proposal's two claims, then the revision's two new ones.
        assert_eq!(queries.len(), 4, "{queries:?}");
        assert!(consensus_queries(consensus.all_events(), &RunId::new("other")).is_none());
        let s = baseline_scenario();
        let mut log = BaselineLog::create(temp_path("no-queries.jsonl")).unwrap();
        let mut r = runner(s.model, s.memory);
        let id = r.new_run_id();
        assert!(matches!(
            r.run(
                &mut log,
                id,
                s.input.clone(),
                QuerySource::TaskText,
                vec![],
                &RunControl::new()
            ),
            Err(BaselineError::Queries(0))
        ));
        assert!(log.records().is_empty(), "nothing recorded");
    }
}
