// SPDX-License-Identifier: Apache-2.0
//
// Runtime validation of what the API answers: zod mirrors of the JSON Schemas
// in contracts/api-v1, hand-written for the fields the UI reads. Every object
// is "loose": unknown extra fields pass through, so an additive server change
// never breaks the client. The enumerations the honesty rules depend on
// (provenance axes, delivery, flags, states, recommendation kinds) are strict:
// a value outside the contract fails validation instead of being rendered as
// something it is not. Each schema is checked against the generated type
// (`satisfies z.ZodType<...>`), so a mirror that drifts from the contract
// fails `tsc`.

import { z } from "zod";

import type * as T from "./types";

// zod 4 compiles object parsers with `new Function` unless told otherwise.
// The portal runs under a hash-only CSP (`script-src 'self' 'sha256-…'`, no
// 'unsafe-eval'): the probe would be caught by zod but still reported as a
// securitypolicyviolation on every page. Parsing stays interpreted here.
z.config({ jitless: true });

// --- Enumerations ----------------------------------------------------------

export const modelProvenanceSchema = z.enum([
  "scripted",
  "live_meta",
  "mixed",
  "undetermined",
]) satisfies z.ZodType<T.ModelProvenance>;

export const evidenceProvenanceSchema = z.enum([
  "simulated",
  "hyphae_snapshot",
  "hyphae_live",
  "none",
  "mixed",
]) satisfies z.ZodType<T.EvidenceProvenance>;

export const sourceProvenanceSchema = z.enum([
  "this_server",
  "research_archive",
]) satisfies z.ZodType<T.SourceProvenance>;

export const deliverySchema = z.enum([
  "replay",
  "live",
]) satisfies z.ZodType<T.Delivery>;

export const provenanceFlagSchema = z.enum([
  "provenance_inconsistent",
  "unchained",
  "mode_undeclared",
]) satisfies z.ZodType<T.ProvenanceFlag>;

export const runStateSchema = z.enum([
  "created",
  "proposed",
  "evidence_review",
  "evaluated",
  "revised",
  "abstained",
  "awaiting_human_approval",
  "approved",
  "rejected",
  "failed",
]) satisfies z.ZodType<T.RunState>;

export const recommendationKindSchema = z.enum([
  "supported",
  "contested",
  "insufficient_evidence",
  "abstained",
  "blocked",
]) satisfies z.ZodType<T.RecommendationKind>;

export const authorizationStateSchema = z.enum([
  "pending",
  "approved",
  "rejected",
]) satisfies z.ZodType<T.AuthorizationState>;

export const evidenceModeSchema = z.enum([
  "simulated",
  "offline",
  "sealed_snapshot",
  "live_hyphae",
]) satisfies z.ZodType<T.EvidenceMode>;

export const modelModeSchema = z.enum([
  "scripted",
  "meta",
]) satisfies z.ZodType<T.ModelMode>;

export const runOriginSchema = z.enum([
  "cli",
  "operator",
  "public",
]) satisfies z.ZodType<T.RunOrigin>;

export const guidedScenarioSchema = z.enum([
  "retry",
  "offline",
  "invalid_proof",
  "injection",
  "abstain",
]) satisfies z.ZodType<T.GuidedScenario>;

export const agentRoleSchema = z.enum([
  "proposer",
  "critic",
  "evidence_evaluator",
]) satisfies z.ZodType<T.AgentRole>;

export const challengeKindSchema = z.enum([
  "contradictory_evidence",
  "missing_test",
  "unsupported_claim",
  "regression_risk",
  "security_risk",
  "constraint_conflict",
]) satisfies z.ZodType<T.ChallengeKind>;

export const integrityStatusSchema = z.enum([
  "valid",
  "invalid",
  "unavailable",
  "verification_error",
  "not_checked",
]) satisfies z.ZodType<T.IntegrityStatus>;

export const factualStatusSchema = z.enum([
  "corroborated",
  "uncorroborated",
  "contradicted",
  "not_assessed",
]) satisfies z.ZodType<T.FactualStatus>;

export const relevanceStatusSchema = z.enum([
  "relevant",
  "irrelevant",
  "not_assessed",
]) satisfies z.ZodType<T.RelevanceStatus>;

export const itemStatusSchema = z.enum([
  "resolved",
  "unresolved",
]) satisfies z.ZodType<T.ItemStatus>;

export const challengeItemStatusSchema = z.enum([
  "open",
  "resolved",
  "never_rejudged",
]) satisfies z.ZodType<T.ChallengeItemStatus>;

export const retrievalSchema = z.enum([
  "live_recall",
  "sealed_snapshot",
  "simulated",
]) satisfies z.ZodType<T.Retrieval>;

export const chainScopeSchema = z.enum([
  "run",
  "log",
  "partly_chained",
  "unchained",
]) satisfies z.ZodType<T.ChainScope>;

export const policyRuleSchema = z.enum([
  "invalid_proof",
  "recorded_contradiction",
  "abstention",
  "unresolved_items",
  "insufficient_evidence",
  "all_checks_passed",
]) satisfies z.ZodType<T.PolicyRule>;

export const policyRulesSchema = z.enum([
  "1.1",
  "1.2",
]) satisfies z.ZodType<T.PolicyRules>;

export const streamEndReasonSchema = z.enum([
  "awaiting_human_approval",
  "terminal",
]) satisfies z.ZodType<T.StreamEndReason>;

export const resyncReasonSchema = z.enum([
  "lagged",
  "gap",
  "closed",
]) satisfies z.ZodType<T.ResyncReason>;

export const surfaceSchema = z.enum([
  "public",
  "operator",
]) satisfies z.ZodType<T.SurfaceV1>;

export const authoritySchema = z.enum([
  "operator",
  "public_demo",
]) satisfies z.ZodType<T.Authority>;

// --- Scalars ---------------------------------------------------------------

const count = z.number().int().nonnegative();
const optionalText = z.string().nullable().optional();
const optionalCount = count.nullable().optional();
const texts = z.array(z.string());

// --- Provenance ------------------------------------------------------------

export const taskModeSchema = z.looseObject({
  evidence: evidenceModeSchema,
  model: modelModeSchema,
  origin: runOriginSchema,
  preset: optionalText,
}) satisfies z.ZodType<T.TaskMode>;

export const provenanceSchema = z.looseObject({
  model: modelProvenanceSchema,
  evidence: evidenceProvenanceSchema,
  source: sourceProvenanceSchema,
  declared: taskModeSchema.nullable().optional(),
  flags: z.array(provenanceFlagSchema).optional(),
  meta_request_ids: texts.optional(),
}) satisfies z.ZodType<T.ProvenanceV1>;

// --- Run summary -----------------------------------------------------------

export const failureSchema = z.looseObject({
  kind: z.string(),
  stage: z.string(),
  code: optionalText,
  message: z.string(),
  reason: optionalText,
}) satisfies z.ZodType<T.FailureV1>;

export const runSummarySchema = z.looseObject({
  run_id: z.string(),
  state: runStateSchema,
  recommendation: recommendationKindSchema.nullable().optional(),
  authorization: authorizationStateSchema,
  title: optionalText,
  provenance: provenanceSchema,
  created_at: z.string(),
  updated_at: z.string(),
  event_count: count,
  last_sequence: count,
  revision_round: count,
  failure: failureSchema.nullable().optional(),
  override: z.boolean(),
  active: z.boolean(),
}) satisfies z.ZodType<T.RunSummaryV1>;

export const runLinksSchema = z.looseObject({
  self: z.string(),
  events: z.string(),
  events_ndjson: z.string(),
  receipt: z.string(),
  decision: z.string(),
  cancel: z.string(),
}) satisfies z.ZodType<T.RunLinksV1>;

export const runCreatedSchema = z.looseObject({
  run: runSummarySchema,
  links: runLinksSchema,
}) satisfies z.ZodType<T.RunCreatedV1>;

export const runListSchema = z.looseObject({
  runs: z.array(runSummarySchema),
  next_cursor: optionalText,
}) satisfies z.ZodType<T.RunListV1>;

// --- Domain documents ------------------------------------------------------

export const taskSchema = z.looseObject({
  id: z.string(),
  run_id: z.string(),
  schema_version: z.string(),
  title: z.string(),
  description: z.string(),
  project_id: z.string(),
  created_at: z.string(),
  mode: taskModeSchema.nullable().optional(),
}) satisfies z.ZodType<T.Task>;

export const claimSchema = z.looseObject({
  id: z.string(),
  statement: z.string(),
  evidence_refs: texts,
}) satisfies z.ZodType<T.Claim>;

export const proposalSchema = z.looseObject({
  id: z.string(),
  run_id: z.string(),
  schema_version: z.string(),
  author_role: agentRoleSchema,
  summary: z.string(),
  claims: z.array(claimSchema),
  plan_steps: texts,
  assumptions: texts,
  uncertainties: texts,
  created_at: z.string(),
}) satisfies z.ZodType<T.Proposal>;

export const challengeItemSchema = z.looseObject({
  id: z.string().optional(),
  kind: challengeKindSchema,
  claim_id: optionalText,
  description: z.string(),
  evidence_refs: texts,
}) satisfies z.ZodType<T.ChallengeItem>;

export const challengeSchema = z.looseObject({
  id: z.string(),
  run_id: z.string(),
  schema_version: z.string(),
  target_proposal: z.string(),
  items: z.array(challengeItemSchema),
  created_at: z.string(),
}) satisfies z.ZodType<T.Challenge>;

export const claimAssessmentSchema = z.looseObject({
  claim_id: z.string(),
  corroborated: z.boolean(),
  note: z.string(),
  relevant_evidence: texts,
  contradicting_evidence: texts.optional(),
}) satisfies z.ZodType<T.ClaimAssessment>;

export const itemVerdictSchema = z.looseObject({
  item_id: z.string(),
  status: itemStatusSchema,
  reason: z.string(),
}) satisfies z.ZodType<T.ItemVerdict>;

export const clearedContradictionSchema = z.looseObject({
  evidence_id: z.string(),
  reason: z.string(),
}) satisfies z.ZodType<T.ClearedContradiction>;

export const evaluationSchema = z.looseObject({
  run_id: z.string(),
  schema_version: z.string(),
  target_proposal: z.string(),
  abstain: z.boolean(),
  abstain_reason: optionalText,
  assessments: z.array(claimAssessmentSchema),
  unresolved_items: texts.optional(),
  item_verdicts: z.array(itemVerdictSchema).optional(),
  cleared_contradictions: z.array(clearedContradictionSchema).optional(),
  created_at: z.string(),
}) satisfies z.ZodType<T.Evaluation>;

export const revisionSchema = z.looseObject({
  schema_version: z.string(),
  proposal: proposalSchema,
  supersedes: z.string(),
  addressed_items: texts.optional(),
}) satisfies z.ZodType<T.Revision>;

export const recommendationSchema = z.looseObject({
  schema_version: z.string(),
  kind: recommendationKindSchema,
  rationale: z.string(),
  unresolved_assumptions: texts,
  unresolved_uncertainties: texts.optional(),
  issued_at: z.string(),
}) satisfies z.ZodType<T.Recommendation>;

export const proofRequestSchema = z.looseObject({
  operation: z.string(),
  project: z.string(),
  query: optionalText,
  limit: optionalCount,
}) satisfies z.ZodType<T.ProofRequest>;

export const verifierReportSchema = z.looseObject({
  kind: optionalText,
  status: optionalText,
  scope: optionalText,
  anchor_digest: optionalText,
  proof_digest: optionalText,
  result_digest: optionalText,
}) satisfies z.ZodType<T.VerifierReport>;

export const evidenceReferenceSchema = z.looseObject({
  id: z.string(),
  project_id: z.string(),
  kind: z.string(),
  content: z.string(),
  retrieved_at: z.string(),
  anchor: optionalText,
  retrieval: retrievalSchema.nullable().optional(),
  snapshot_id: optionalText,
  captured_at: optionalText,
  proof_ref: optionalText,
  proof_sha256: optionalText,
  proof_request: proofRequestSchema.nullable().optional(),
  backend_project: optionalText,
  backend_scope: optionalText,
  proof_error_reported: z.boolean(),
  proof_error: optionalText,
  backend_ref: optionalText,
  proof_file: optionalText,
  witness_file: optionalText,
}) satisfies z.ZodType<T.EvidenceReferenceV1>;

export const verificationSchema = z.looseObject({
  evidence_id: z.string(),
  integrity_status: integrityStatusSchema,
  factual_status: factualStatusSchema,
  relevance_status: relevanceStatusSchema,
  verifier: optionalText,
  verified_anchor: optionalText,
  checked_at: z.string(),
  round: optionalCount,
  proof_ref: optionalText,
  report: verifierReportSchema.nullable().optional(),
  simulated: z.boolean(),
  detail: optionalText,
}) satisfies z.ZodType<T.VerificationV1>;

export const modelInvocationSchema = z.looseObject({
  provider: z.string(),
  model: z.string(),
  request_id: optionalText,
  latency_ms: optionalCount,
  input_tokens: optionalCount,
  output_tokens: optionalCount,
  reasoning_tokens: optionalCount,
  finish_reason: optionalText,
  scripted: z.boolean(),
  max_completion_tokens: optionalCount,
  schema_name: optionalText,
  schema_sha256: optionalText,
  prompt_sha256: optionalText,
  attempts: optionalCount,
}) satisfies z.ZodType<T.ModelInvocationV1>;

export const decisionSchema = z.looseObject({
  approved: z.boolean(),
  authority: z.string(),
  authorized_by: z.string(),
  decided_at: z.string(),
  note: optionalText,
  notice: optionalText,
  idempotency_key: optionalText,
}) satisfies z.ZodType<T.DecisionV1>;

// --- Events ----------------------------------------------------------------

export const actorSchema = z.discriminatedUnion("kind", [
  z.looseObject({ kind: z.literal("agent"), role: agentRoleSchema }),
  z.looseObject({ kind: z.literal("runtime") }),
  z.looseObject({ kind: z.literal("human"), id: z.string() }),
]) satisfies z.ZodType<T.Actor>;

export const eventPayloadSchema = z.discriminatedUnion("type", [
  z.looseObject({ type: z.literal("task_created"), task: taskSchema }),
  z.looseObject({
    type: z.literal("proposal_submitted"),
    proposal: proposalSchema,
    invocation: modelInvocationSchema,
  }),
  z.looseObject({
    type: z.literal("evidence_retrieved"),
    evidence: evidenceReferenceSchema,
  }),
  z.looseObject({
    type: z.literal("evidence_unavailable"),
    query: z.string(),
    code: optionalText,
    message: z.string(),
    reason: optionalText,
  }),
  z.looseObject({
    type: z.literal("recall_proven"),
    request: proofRequestSchema,
    evidence_ids: texts,
    proof_ref: z.string(),
    anchor: z.string(),
    empty: z.boolean(),
  }),
  z.looseObject({
    type: z.literal("evidence_verified"),
    verification: verificationSchema,
  }),
  z.looseObject({
    type: z.literal("challenge_submitted"),
    challenge: challengeSchema,
    invocation: modelInvocationSchema,
  }),
  z.looseObject({
    type: z.literal("evaluation_submitted"),
    evaluation: evaluationSchema,
    invocation: modelInvocationSchema,
  }),
  z.looseObject({
    type: z.literal("revision_submitted"),
    revision: revisionSchema,
    invocation: modelInvocationSchema,
  }),
  z.looseObject({
    type: z.literal("recommendation_issued"),
    recommendation: recommendationSchema,
  }),
  z.looseObject({
    type: z.literal("human_decision_recorded"),
    decision: decisionSchema,
  }),
  z.looseObject({
    type: z.literal("run_failed"),
    stage: z.string(),
    code: optionalText,
    message: z.string(),
    reason: optionalText,
    invocation: modelInvocationSchema.nullable().optional(),
  }),
]) satisfies z.ZodType<T.EventPayloadV1>;

export const eventSchema = z.looseObject({
  sequence: count,
  event_id: z.string(),
  run_id: z.string(),
  schema_version: z.string(),
  actor: actorSchema,
  occurred_at: z.string(),
  prev_hash: z.string(),
  hash: z.string(),
  payload: eventPayloadSchema,
}) satisfies z.ZodType<T.EventV1>;

// --- Run detail ------------------------------------------------------------

export const roundSchema = z.looseObject({
  round: count,
  proposal_id: optionalText,
  challenge: challengeSchema.nullable().optional(),
  evaluation: evaluationSchema.nullable().optional(),
  verifications: z.array(verificationSchema),
}) satisfies z.ZodType<T.RoundV1>;

export const challengeItemViewSchema = z.looseObject({
  id: z.string(),
  kind: challengeKindSchema,
  claim_id: optionalText,
  description: z.string(),
  status: challengeItemStatusSchema,
  raised_in_round: count,
  resolution_reason: optionalText,
}) satisfies z.ZodType<T.ChallengeItemV1>;

export const traceCountsSchema = z.looseObject({
  claims: count,
  corroborated: count,
  proofs_invalid: count,
  proofs_unavailable_or_error: count,
  proofs_valid: count,
  unresolved_items: count,
}) satisfies z.ZodType<T.TraceCounts>;

export const traceReasonSchema = z.discriminatedUnion("reason", [
  z.looseObject({ reason: z.literal("invalid_proofs"), evidence_ids: texts }),
  z.looseObject({
    reason: z.literal("contradicted_evidence"),
    evidence_ids: texts,
  }),
  z.looseObject({
    reason: z.literal("evaluator_abstained"),
    abstain_reason: optionalText,
  }),
  z.looseObject({ reason: z.literal("open_items"), item_ids: texts }),
  z.looseObject({ reason: z.literal("no_assessments") }),
  z.looseObject({ reason: z.literal("uncorroborated_claims"), claim_ids: texts }),
  z.looseObject({
    reason: z.literal("claims_without_own_evidence"),
    claim_ids: texts,
  }),
  z.looseObject({
    reason: z.literal("cited_evidence_not_valid"),
    evidence_ids: texts,
  }),
  z.looseObject({ reason: z.literal("no_checked_evidence") }),
]) satisfies z.ZodType<T.TraceReason>;

export const policyTraceSchema = z.looseObject({
  counts: traceCountsSchema,
  kind: recommendationKindSchema,
  reasons: z.array(traceReasonSchema),
  rule: policyRuleSchema,
  version: policyRulesSchema,
}) satisfies z.ZodType<T.PolicyTrace>;

export const recommendationViewSchema = z.looseObject({
  kind: recommendationKindSchema,
  rationale: z.string(),
  rationale_code: optionalText,
  unresolved_assumptions: texts,
  unresolved_uncertainties: texts.optional(),
  issued_at: z.string(),
  trace: policyTraceSchema.nullable().optional(),
  protocol: z.string(),
}) satisfies z.ZodType<T.RecommendationV1>;

export const chainSchema = z.looseObject({
  scope: chainScopeSchema,
  head_hash: optionalText,
  first_sequence: count,
  last_sequence: count,
}) satisfies z.ZodType<T.ChainV1>;

export const runDetailSchema = z.looseObject({
  summary: runSummarySchema,
  task: taskSchema.nullable().optional(),
  current_proposal: proposalSchema.nullable().optional(),
  rounds: z.array(roundSchema),
  challenge_items: z.array(challengeItemViewSchema),
  recommendation: recommendationViewSchema.nullable().optional(),
  decision: decisionSchema.nullable().optional(),
  model_invocations: z.array(modelInvocationSchema),
  events: z.array(eventSchema),
  chain: chainSchema,
}) satisfies z.ZodType<T.RunDetailV1>;

// --- Decisions and cancellation -------------------------------------------

export const decisionResponseSchema = z.looseObject({
  run: runSummarySchema,
  decision: decisionSchema,
}) satisfies z.ZodType<T.DecisionResponseV1>;

export const cancelResponseSchema = z.looseObject({
  run_id: z.string(),
  cancelling: z.boolean(),
}) satisfies z.ZodType<T.CancelResponseV1>;

// --- SSE frames ------------------------------------------------------------

export const consensusFrameSchema = z.looseObject({
  event: eventSchema,
  delivery: deliverySchema,
  summary: runSummarySchema,
}) satisfies z.ZodType<T.ConsensusFrameV1>;

export const resyncSchema = z.looseObject({
  reason: resyncReasonSchema,
  from_sequence: count,
}) satisfies z.ZodType<T.ResyncV1>;

export const streamEndSchema = z.looseObject({
  reason: streamEndReasonSchema,
  state: runStateSchema,
  last_sequence: count,
}) satisfies z.ZodType<T.StreamEndV1>;

/** The frames as the contract fixture lists them (`frame` tagged). */
export const sseFrameSchema = z.discriminatedUnion("frame", [
  consensusFrameSchema.extend({ frame: z.literal("consensus") }),
  resyncSchema.extend({ frame: z.literal("resync") }),
  streamEndSchema.extend({ frame: z.literal("stream_end") }),
]) satisfies z.ZodType<T.SseFrameV1>;

// --- Sessions, scenarios, capabilities, health -----------------------------

export const sessionSchema = z.looseObject({
  created: z.boolean(),
  expires_at: z.string(),
  pseudonym: z.string(),
}) satisfies z.ZodType<T.SessionV1>;

export const scenarioSchema = z.looseObject({
  id: guidedScenarioSchema,
  title: z.string(),
  description: z.string(),
  expected_outcome: recommendationKindSchema,
  simulated: z.boolean(),
  model: modelProvenanceSchema,
  evidence: evidenceProvenanceSchema,
}) satisfies z.ZodType<T.ScenarioV1>;

export const scenariosSchema = z.looseObject({
  scenarios: z.array(scenarioSchema),
}) satisfies z.ZodType<T.ScenariosV1>;

export const limitsSchema = z.looseObject({
  body_limit_bytes: count,
  guided_concurrency: count,
  live_concurrency: count,
  run_deadline_secs: count,
  max_run_tokens: count,
  sse_max_per_run: count,
  sse_max_per_client: count,
  sse_max_global: count,
  sse_heartbeat_secs: count,
  rate_session_per_min: count,
  rate_runs_per_min: count,
  rate_sse_per_min: count,
  session_ttl_secs: count,
  session_max: count,
  guided_ring: count,
  visitor_access_hours: count,
  visitor_delete_days: count,
  list_page_max: count,
  request_timeout_secs: count,
}) satisfies z.ZodType<T.LimitsV1>;

/**
 * One curated live preset, as the live-gate lane will list it under
 * `capabilities.live.presets`. Not in the contract yet: only the id is
 * required, every other field is read defensively.
 */
export const livePresetSchema = z.looseObject({
  id: z.string(),
  title: optionalText,
  description: optionalText,
  evidence: optionalText,
  model: optionalText,
});

/**
 * The `live` section of the capabilities (live-gate lane): the presets and
 * the public budget. Absent or null until that lane lands; every field is
 * optional, and the budget is shown as the key/value pairs it carries.
 */
export const capabilitiesLiveSchema = z.looseObject({
  presets: z.array(livePresetSchema).optional(),
  budget: z.record(z.string(), z.unknown()).nullable().optional(),
  judge_codes_enabled: z.boolean().nullable().optional(),
  turnstile_required: z.boolean().nullable().optional(),
});

export type LivePreset = z.infer<typeof livePresetSchema>;
export type CapabilitiesLive = z.infer<typeof capabilitiesLiveSchema>;

export const capabilitiesSchema = z.looseObject({
  surface: surfaceSchema,
  live_enabled: z.boolean(),
  guided_enabled: z.boolean(),
  showcase_enabled: z.boolean(),
  limits: limitsSchema,
  schema_version: z.string(),
  policy_version: z.string(),
  protocol: z.string(),
  decision_authority: authoritySchema,
  live: capabilitiesLiveSchema.nullable().optional(),
}) satisfies z.ZodType<T.CapabilitiesV1>;

/** `CapabilitiesV1` plus the optional `live` block. */
export type Capabilities = z.infer<typeof capabilitiesSchema>;

export const healthSchema = z.looseObject({
  status: z.string(),
  version: z.string(),
  schema_version: z.string(),
  policy_version: z.string(),
  contract_sha256: z.string(),
  executor: z.string(),
}) satisfies z.ZodType<T.HealthV1>;

// --- Showcase --------------------------------------------------------------

export const showcaseEntrySchema = z.looseObject({
  run_id: z.string(),
  label: z.string(),
  battery: z.string(),
  title: z.string(),
  outcome: z.string(),
  authorization: z.string(),
  schema_version: z.string(),
  provenance: provenanceSchema,
  file: z.string(),
  sha256: z.string(),
  head_hash: optionalText,
  record_file: optionalText,
  event_count: count,
}) satisfies z.ZodType<T.ShowcaseEntryV1>;

export const showcaseSchema = z.looseObject({
  entries: z.array(showcaseEntrySchema),
  manifest_file: z.string(),
  manifest_sha256: z.string(),
}) satisfies z.ZodType<T.ShowcaseV1>;

// --- Receipt v2 ------------------------------------------------------------

export const receiptAssessmentSchema = z.looseObject({
  contradicting_evidence: texts,
  corroborated: z.boolean(),
  note: z.string(),
  relevant_evidence: texts,
}) satisfies z.ZodType<T.ReceiptAssessment>;

export const receiptClaimSchema = z.looseObject({
  id: z.string(),
  statement: z.string(),
  evidence_refs: texts,
  assessment: receiptAssessmentSchema.nullable().optional(),
}) satisfies z.ZodType<T.ReceiptClaim>;

export const openItemSchema = z.looseObject({
  id: z.string(),
  kind: challengeKindSchema.nullable().optional(),
  claim_id: optionalText,
  description: optionalText,
  raised_in_round: optionalCount,
}) satisfies z.ZodType<T.OpenItem>;

export const unrejudgedObjectionSchema = z.looseObject({
  id: z.string(),
  kind: challengeKindSchema,
  claim_id: optionalText,
  description: z.string(),
  raised_in_round: count,
  unresolved_in_its_round: z.boolean(),
}) satisfies z.ZodType<T.UnrejudgedObjection>;

export const receiptRevisionSchema = z.looseObject({
  proposal_id: z.string(),
  supersedes: z.string(),
  summary: z.string(),
  addressed_items: texts,
  after_round: count,
}) satisfies z.ZodType<T.ReceiptRevision>;

export const deliberationSchema = z.looseObject({
  final_proposal_id: optionalText,
  final_proposal_summary: optionalText,
  evaluated_proposal_id: optionalText,
  claims: z.array(receiptClaimSchema),
  plan_steps: texts,
  assumptions: texts,
  uncertainties: texts,
  open_items: z.array(openItemSchema),
  missing_tests: z.array(openItemSchema),
  unevaluated_items: z.array(openItemSchema),
  earlier_round_objections_never_rejudged: z.array(unrejudgedObjectionSchema),
  unmatched_assessments: z.array(claimAssessmentSchema),
  revisions: z.array(receiptRevisionSchema),
  rounds: count,
}) satisfies z.ZodType<T.Deliberation>;

export const evidenceVerificationSchema = z.looseObject({
  schema_version: z.string(),
  evidence_id: z.string(),
  integrity_status: integrityStatusSchema,
  factual_status: factualStatusSchema,
  relevance_status: relevanceStatusSchema,
  verifier: optionalText,
  verified_anchor: optionalText,
  detail: optionalText,
  checked_at: z.string(),
  round: optionalCount,
  proof_ref: optionalText,
  report: verifierReportSchema.nullable().optional(),
}) satisfies z.ZodType<T.EvidenceVerification>;

export const roundVerificationSchema = z.looseObject({
  round: count,
  verification: evidenceVerificationSchema,
}) satisfies z.ZodType<T.RoundVerification>;

export const receiptModelInvocationSchema = z.looseObject({
  provider: z.string(),
  model: z.string(),
  request_id: optionalText,
  latency_ms: optionalCount,
  input_tokens: optionalCount,
  output_tokens: optionalCount,
  reasoning_tokens: optionalCount,
  finish_reason: optionalText,
  max_completion_tokens: optionalCount,
  reasoning_effort: optionalText,
  safety_identifier_sent: z.boolean().nullable().optional(),
  schema_name: optionalText,
  schema_sha256: optionalText,
  prompt_sha256: optionalText,
  attempts: optionalCount,
}) satisfies z.ZodType<T.ModelInvocation>;

export const receiptAuthorizationSchema = z.looseObject({
  state: authorizationStateSchema,
  authority: optionalText,
  decided_at: optionalText,
  note: optionalText,
}) satisfies z.ZodType<T.ReceiptAuthorization>;

export const receiptChainSchema = z.looseObject({
  scope: chainScopeSchema,
  head_hash: optionalText,
  first_sequence: count,
  last_sequence: count,
  note: z.string(),
}) satisfies z.ZodType<T.ReceiptChain>;

export const receiptFailureSchema = z.looseObject({
  stage: z.string(),
  reason: z.string(),
  code: optionalText,
}) satisfies z.ZodType<T.ReceiptFailure>;

export const receiptSchema = z.looseObject({
  format: z.string(),
  schema_version: z.string(),
  run_schema_version: z.string(),
  run_id: z.string(),
  generated_at: z.string(),
  protocol: z.string(),
  task: taskSchema.nullable().optional(),
  final_state: runStateSchema,
  event_count: count,
  recommendation: recommendationSchema.nullable().optional(),
  policy_trace: policyTraceSchema.nullable().optional(),
  deliberation: deliberationSchema,
  verifications: z.array(roundVerificationSchema),
  model_invocations: z.array(receiptModelInvocationSchema),
  authorization: receiptAuthorizationSchema,
  failure: receiptFailureSchema.nullable().optional(),
  chain: receiptChainSchema,
  proof_limitations: z.string(),
}) satisfies z.ZodType<T.RunReceiptV2>;

// --- Errors ----------------------------------------------------------------

/**
 * The body of every error response. `code` is kept as a string: a code this
 * client does not know yet is still an error to show, with its generic
 * sentence.
 */
export const apiErrorBodySchema = z.looseObject({
  code: z.string(),
  message: z.string(),
  retry_after_secs: optionalCount,
});

export type ApiErrorBody = z.infer<typeof apiErrorBodySchema>;
