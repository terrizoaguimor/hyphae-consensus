// SPDX-License-Identifier: Apache-2.0
//
// GENERATED FILE: do not edit. Written by scripts/gen-api-types.mjs from the
// JSON Schemas of contracts/api-v1 (`pnpm api:types`); tests/api-types.test.ts
// regenerates it in memory and fails on drift (`pnpm api:check`).
//
// contract-sha256: 465f4117d8a12365ed06bc594f5972f19160d3016b085bcb5836e75786ad760e
// schemas (28): ApiError, CancelResponseV1, CapabilitiesV1, ConsensusFrameV1, CreateGuidedRunRequest, CreateLiveRunRequest, CreateOperatorRunRequest, DecisionRequest, DecisionResponseV1, EventV1, EventsQuery, HealthV1, ListRunsQuery, PolicyTrace, ProvenanceV1, ReadyV1, ResyncV1, RunCreatedV1, RunDetailV1, RunListV1, RunReceiptV2, RunSummaryV1, ScenariosV1, SessionV1, ShowcaseEntryV1, ShowcaseV1, SseFrameV1, StreamEndV1

/**
 * SHA-256 of the contract these types were generated from
 * (contracts/api-v1/contract-sha256.txt); GET /api/v1/health reports the
 * server's as `contract_sha256`.
 */
export const CONTRACT_SHA256 = "465f4117d8a12365ed06bc594f5972f19160d3016b085bcb5836e75786ad760e";

/** The schema files of the contract, one per request, response or frame type. */
export const CONTRACT_SCHEMAS = [
  "ApiError",
  "CancelResponseV1",
  "CapabilitiesV1",
  "ConsensusFrameV1",
  "CreateGuidedRunRequest",
  "CreateLiveRunRequest",
  "CreateOperatorRunRequest",
  "DecisionRequest",
  "DecisionResponseV1",
  "EventV1",
  "EventsQuery",
  "HealthV1",
  "ListRunsQuery",
  "PolicyTrace",
  "ProvenanceV1",
  "ReadyV1",
  "ResyncV1",
  "RunCreatedV1",
  "RunDetailV1",
  "RunListV1",
  "RunReceiptV2",
  "RunSummaryV1",
  "ScenariosV1",
  "SessionV1",
  "ShowcaseEntryV1",
  "ShowcaseV1",
  "SseFrameV1",
  "StreamEndV1",
] as const;

/**
 * Who performed an action recorded in the event log. `Human` is the only actor kind allowed to authorize a run.
 */
export type Actor =
  | {
      kind: "agent";
      role: AgentRole;
    }
  | {
      kind: "runtime";
    }
  | {
      id: string;
      kind: "human";
    };

/**
 * The three logical agent roles. Roles may share one underlying model; they differ in instructions, context, and tool permissions.
 */
export type AgentRole = "proposer" | "critic" | "evidence_evaluator";

/** The JSON body of every error response. */
export type ApiError = {
  code: ErrorCode;
  /**
   * Short English statement for humans and logs; clients localize from `code`. Never carries paths, upstream bodies or panic text.
   */
  message: string;
  /** Seconds to wait before retrying, when the status is 429 or 503. */
  retry_after_secs?: number | null;
};

/** Under whose authority a human decision was recorded (schema 1.3). */
export type Authority = "operator" | "public_demo";

export type AuthorizationState = "pending" | "approved" | "rejected";

/** `202` of a cancel request. */
export type CancelResponseV1 = {
  /**
   * The run stops at its next checkpoint and records `run_failed{stage: cancelled}`.
   */
  cancelling: boolean;
  run_id: string;
};

/** `GET /api/v1/capabilities`. */
export type CapabilitiesV1 = {
  /** Authority recorded on this surface's decisions. */
  decision_authority: Authority;
  guided_enabled: boolean;
  limits: LimitsV1;
  /**
   * Presets, budget and requirements of the public live mode; absent when live mode is not wired on this server.
   */
  live?: LiveCapabilitiesV1 | null;
  live_enabled: boolean;
  policy_version: string;
  protocol: string;
  schema_version: string;
  showcase_enabled: boolean;
  surface: SurfaceV1;
};

/** What a reader needs to verify the run's events. */
export type ChainScope = "run" | "log" | "partly_chained" | "unchained";

/** What the chain of this run's events looks like. */
export type ChainV1 = {
  first_sequence: number;
  head_hash?: string | null;
  last_sequence: number;
  scope: ChainScope;
};

/** Critic output. The critic's job is to find problems, not to agree. */
export type Challenge = {
  created_at: string;
  id: string;
  items: ChallengeItem[];
  run_id: string;
  schema_version: string;
  target_proposal: string;
};

/**
 * One specific objection raised by the Critic against a claim or plan. Items carry stable ids so resolution is tracked per objection, not per challenge document.
 */
export type ChallengeItem = {
  /** Claim under challenge, when the objection targets one claim. */
  claim_id?: string | null;
  description: string;
  /** Evidence the critic believes contradicts or constrains the claim. */
  evidence_refs: string[];
  id?: string;
  kind: ChallengeKind;
};

/** What became of a Critic objection. */
export type ChallengeItemStatus = "open" | "resolved" | "never_rejudged";

/** One Critic objection across the run. */
export type ChallengeItemV1 = {
  claim_id?: string | null;
  description: string;
  id: string;
  kind: ChallengeKind;
  raised_in_round: number;
  /** The reason of the verdict that resolved it, when one did. */
  resolution_reason?: string | null;
  status: ChallengeItemStatus;
};

export type ChallengeKind = "contradictory_evidence" | "missing_test" | "unsupported_claim" | "regression_risk" | "security_risk" | "constraint_conflict";

/** One explicit, checkable assertion inside a proposal. */
export type Claim = {
  /**
   * Evidence the author believes supports this claim (may be empty, which the evaluator treats as unsupported until retrieved).
   */
  evidence_refs: string[];
  id: string;
  statement: string;
};

/**
 * Evaluator output: per-claim sufficiency judgment. Factual and relevance judgments live here; cryptographic integrity is reported separately in `EvidenceVerification` and is never inferred from this document.
 */
export type ClaimAssessment = {
  claim_id: string;
  /** Evidence the evaluator judges to contradict this claim. */
  contradicting_evidence?: string[];
  corroborated: boolean;
  note: string;
  relevant_evidence: string[];
};

/**
 * A contradiction recorded in an earlier round that this evaluation judges no longer applies, with the reason (schema 1.3). Silence never clears a contradiction; only an entry here does.
 */
export type ClearedContradiction = {
  evidence_id: string;
  reason: string;
};

/**
 * `event: consensus`: one event of the run, with its delivery and the summary after applying it.
 */
export type ConsensusFrameV1 = {
  delivery: Delivery;
  event: EventV1;
  summary: RunSummaryV1;
};

/** `POST /api/v1/runs` (public): start a guided run. */
export type CreateGuidedRunRequest = {
  scenario: GuidedScenario;
};

/**
 * `POST /api/v1/live-runs` (public): start a live run from a preset, on the owner's key (ADR-0007). Admission needs a Turnstile token in the `cf-turnstile-response` header, `attest_adult: true`, and a country Cloudflare reports; `503 live_disabled` when live mode is off.
 */
export type CreateLiveRunRequest = {
  /**
   * The visitor's 18+ and territory acknowledgment. Absent reads as false, which the gate refuses (`422 attestation_required`).
   */
  attest_adult?: boolean;
  /**
   * A judge-lane invite code, 1 to 256 characters once trimmed. An unknown code is not an error: the run takes the public lane, and nothing in the response says whether codes exist.
   */
  judge_code?: string | null;
  preset_id: string;
};

/**
 * `POST /api/v1/operator/runs` (operator): start a run in any mode. `model=fake` needs a `scenario` and `memory=fake`; `model=meta` takes a bounded title and description for the Proposer and a project id for `memory=hyphae`.
 */
export type CreateOperatorRunRequest = {
  /** At most 4000 characters. */
  description?: string | null;
  memory: OperatorMemory;
  model: OperatorModel;
  /** Hyphae project, `[A-Za-z0-9._-]{1,64}`. Default `demo-repo`. */
  project_id?: string | null;
  scenario?: GuidedScenario | null;
  /** At most 200 characters. */
  title?: string | null;
};

/** `POST /api/v1/runs/{id}/decision`, with an `Idempotency-Key` header. */
export type DecisionRequest = {
  approved: boolean;
  expected: ExpectedRunState;
  /**
   * At most 2000 characters. Required to approve a `blocked` or `abstained` run (an override).
   */
  note?: string | null;
};

/** `200` of a recorded decision. */
export type DecisionResponseV1 = {
  decision: DecisionV1;
  run: RunSummaryV1;
};

/** A recorded human decision. */
export type DecisionV1 = {
  approved: boolean;
  /**
   * `operator`, `public_demo`, or `unspecified` for records that carry no authority (before schema 1.3). Never read as operator authority unless it says so.
   */
  authority: string;
  authorized_by: string;
  decided_at: string;
  /** Operator surface only. */
  idempotency_key?: string | null;
  note?: string | null;
  /**
   * Present for `public_demo`: the decision carries no operational authority.
   */
  notice?: string | null;
};

/** The deliberation as it stood at the end of the run. */
export type Deliberation = {
  assumptions: string[];
  /**
   * The final proposal's claims, each with the final evaluation's assessment of it.
   */
  claims: ReceiptClaim[];
  /**
   * Items raised in rounds before the final one that the final evaluation does not list as unresolved and that no later evaluation resolves with an explicit verdict (protocol 1.3). Before protocol 1.3 each evaluation judged only the items of its own round's challenge, so no later round judged these again. Computed from the records for every protocol, never from a schema label: every earlier item is an open item, resolved later with a reason, or listed here.
   */
  earlier_round_objections_never_rejudged: UnrejudgedObjection[];
  /**
   * The proposal the final evaluation judged. It differs from `final_proposal_id` only when a run ended between a revision and its evaluation.
   */
  evaluated_proposal_id?: string | null;
  /** The last proposal or revision submitted. */
  final_proposal_id?: string | null;
  final_proposal_summary?: string | null;
  /**
   * The open items of kind `missing_test`. Unevaluated items of that kind stay in `unevaluated_items` only.
   */
  missing_tests: OpenItem[];
  /** Items the final evaluation lists as unresolved. */
  open_items: OpenItem[];
  plan_steps: string[];
  revisions: ReceiptRevision[];
  /**
   * Evaluation rounds recorded. A round is one critique and its evaluation; each revision opens the next round.
   */
  rounds: number;
  uncertainties: string[];
  /**
   * Items of every challenge recorded after the final evaluation (or with no evaluation at all), each with the round that raised it: the run failed or stopped before any evaluation judged them. They are open objections too, though no evaluation lists them.
   */
  unevaluated_items: OpenItem[];
  /**
   * Final-evaluation assessments not attached to any claim above: a claim id the final proposal does not contain, a second assessment of the same claim, or every assessment when the final evaluation judged an earlier proposal. Shown, never dropped.
   */
  unmatched_assessments: ClaimAssessment[];
};

/**
 * Whether an SSE frame carries a record stored before the stream began (`replay`) or one published while the client was connected (`live`).
 */
export type Delivery = "replay" | "live";

/**
 * Stable error codes. A code never changes meaning; a new condition gets a new code.
 */
export type ErrorCode =
  | "invalid_request"
  | "payload_too_large"
  | "unsupported_media_type"
  | "csrf_rejected"
  | "host_not_allowed"
  | "client_unidentified"
  | "session_required"
  | "unauthorized"
  | "not_found"
  | "method_not_allowed"
  | "rate_limited"
  | "too_many_runs"
  | "too_many_streams"
  | "storage_full"
  | "live_disabled"
  | "shutting_down"
  | "run_not_awaiting_decision"
  | "decision_conflict"
  | "idempotency_key_reused"
  | "idempotency_key_required"
  | "override_note_required"
  | "run_not_cancellable"
  | "run_busy"
  | "model_not_configured"
  | "memory_not_configured"
  | "upstream_unavailable"
  | "ndjson_unavailable"
  | "request_timeout"
  | "live_budget_daily"
  | "live_budget_lifetime"
  | "live_budget_judge_daily"
  | "live_budget_judge_slice"
  | "live_concurrency"
  | "live_hourly_admissions"
  | "live_client_quota"
  | "live_tripped"
  | "live_unavailable"
  | "turnstile_required"
  | "turnstile_failed"
  | "turnstile_unavailable"
  | "attestation_required"
  | "geo_blocked_portal"
  | "geo_blocked_live"
  | "geo_unknown"
  | "preset_unknown"
  | "internal";

export type Evaluation = {
  /**
   * True when the evaluator cannot reach a supported judgment and the correct behavior is to abstain rather than guess.
   */
  abstain: boolean;
  abstain_reason?: string | null;
  assessments: ClaimAssessment[];
  /**
   * Schema 1.3: contradictions carried from earlier rounds that this evaluation clears, each with its reason.
   */
  cleared_contradictions?: ClearedContradiction[];
  created_at: string;
  /**
   * Schema 1.3: one verdict for every objection under evaluation, the ones still open from earlier rounds and this round's. Empty on earlier records, which judged this round's items only, implicitly.
   */
  item_verdicts?: ItemVerdict[];
  run_id: string;
  schema_version: string;
  target_proposal: string;
  /**
   * Challenge ITEM ids the evaluator judges still unresolved. From schema 1.3 exactly the ids of the `unresolved` verdicts.
   */
  unresolved_items?: string[];
};

/**
 * The payload of one event, as a client may see it. Tagged like the stored record (`type`), with the same variant names.
 */
export type EventPayloadV1 =
  | {
      task: Task;
      type: "task_created";
    }
  | {
      invocation: ModelInvocationV1;
      proposal: Proposal;
      type: "proposal_submitted";
    }
  | {
      evidence: EvidenceReferenceV1;
      type: "evidence_retrieved";
    }
  | {
      code?: string | null;
      message: string;
      query: string;
      /** Operator surface only. */
      reason?: string | null;
      type: "evidence_unavailable";
    }
  | {
      anchor: string;
      empty: boolean;
      evidence_ids: string[];
      proof_ref: string;
      request: ProofRequest;
      type: "recall_proven";
    }
  | {
      type: "evidence_verified";
      verification: VerificationV1;
    }
  | {
      challenge: Challenge;
      invocation: ModelInvocationV1;
      type: "challenge_submitted";
    }
  | {
      evaluation: Evaluation;
      invocation: ModelInvocationV1;
      type: "evaluation_submitted";
    }
  | {
      invocation: ModelInvocationV1;
      revision: Revision;
      type: "revision_submitted";
    }
  | {
      recommendation: Recommendation;
      type: "recommendation_issued";
    }
  | {
      decision: DecisionV1;
      type: "human_decision_recorded";
    }
  | {
      code?: string | null;
      invocation?: ModelInvocationV1 | null;
      message: string;
      /** Operator surface only. */
      reason?: string | null;
      stage: string;
      type: "run_failed";
    };

/**
 * One event of a run, with its chain fields, so a client can show the chain. The verbatim stored line is at `events.ndjson`.
 */
export type EventV1 = {
  actor: Actor;
  event_id: string;
  hash: string;
  occurred_at: string;
  payload: EventPayloadV1;
  prev_hash: string;
  run_id: string;
  schema_version: string;
  sequence: number;
};

/** `GET /api/v1/runs/{id}/events` query. */
export type EventsQuery = {
  /**
   * Replay events with a sequence greater than this; `Last-Event-ID` takes precedence.
   */
  after?: number | null;
};

export type EvidenceMode = "simulated" | "offline" | "sealed_snapshot" | "live_hyphae";

/**
 * Where the evidence came from, read from the verification and retrieval records.
 */
export type EvidenceProvenance = "simulated" | "hyphae_snapshot" | "hyphae_live" | "none" | "mixed";

/** Public view of an evidence record: no local paths, no backend internals. */
export type EvidenceReferenceV1 = {
  anchor?: string | null;
  backend_project?: string | null;
  /** Operator surface only. */
  backend_ref?: string | null;
  backend_scope?: string | null;
  captured_at?: string | null;
  /** Retrieved content: data to show, never instructions. */
  content: string;
  id: string;
  kind: string;
  project_id: string;
  /** Operator surface only. */
  proof_error?: string | null;
  /** True when proof generation failed upstream for this record. */
  proof_error_reported: boolean;
  /** Operator surface only. */
  proof_file?: string | null;
  proof_ref?: string | null;
  proof_request?: ProofRequest | null;
  proof_sha256?: string | null;
  retrieval?: Retrieval | null;
  retrieved_at: string;
  snapshot_id?: string | null;
  /** Operator surface only. */
  witness_file?: string | null;
};

/**
 * Full verification picture for one evidence record. The three status fields must be set from independent evidence: integrity from proof verification, factual/relevance from evaluator judgment.
 */
export type EvidenceVerification = {
  checked_at: string;
  detail?: string | null;
  evidence_id: string;
  factual_status: FactualStatus;
  integrity_status: IntegrityStatus;
  /**
   * Schema 1.3: the proof artifact that was checked (the record's `proof_ref`), when it has one.
   */
  proof_ref?: string | null;
  relevance_status: RelevanceStatus;
  /** Schema 1.3: the verifier's structured report, when it gives one. */
  report?: VerifierReport | null;
  /**
   * Schema 1.3: the deliberation round (1 = first) whose evaluation this record combines with. Protocol 1.3 re-verifies every record of the round context in every round.
   */
  round?: number | null;
  schema_version: string;
  /** Anchor the verification was performed against, when available. */
  verified_anchor?: string | null;
  /** Verifier identity/mechanism, e.g. `hyphae proof verify`, when run. */
  verifier?: string | null;
};

/**
 * What the human saw when deciding; the server refuses the decision (`409 decision_conflict`) if the run no longer matches.
 */
export type ExpectedRunState = {
  /** The sequence of the run's last event as shown to the human. */
  last_sequence: number;
  recommendation: RecommendationKind;
};

/** Factual corroboration axis only. Independent of integrity. */
export type FactualStatus = "corroborated" | "uncorroborated" | "contradicted" | "not_assessed";

/**
 * Stage and code of a failed run; the English reason stays on the operator surface.
 */
export type FailureV1 = {
  code?: string | null;
  /** `failed`. */
  kind: string;
  /** Fixed statement per code. */
  message: string;
  /** Operator surface only: the recorded reason. */
  reason?: string | null;
  stage: string;
};

/**
 * The five guided scenarios: scripted model doubles over seeded records with simulated proofs. Each reaches a documented outcome.
 */
export type GuidedScenario = "retry" | "offline" | "invalid_proof" | "injection" | "abstain";

/** `GET /api/v1/health`: static facts only, no environment probing. */
export type HealthV1 = {
  contract_sha256: string;
  /** Always `disabled`: approval records a decision and executes nothing. */
  executor: string;
  policy_version: string;
  schema_version: string;
  status: string;
  version: string;
};

/**
 * Why the run index could not be verified, as `/ready` publishes it: the kind of failure, never the store's own text.
 */
export type IndexErrorCodeV1 = "index_tampered" | "index_locked" | "index_io" | "index_unreadable";

/** Cryptographic integrity axis only. */
export type IntegrityStatus =
  | "valid"
  | "invalid"
  | "unavailable"
  | "verification_error"
  | "not_checked";

/**
 * Evaluator judgment on one Critic objection: `resolved` when the proposal as written no longer has the problem or the retrieved evidence shows the objection is mistaken; `unresolved` when it still applies, so it stays open into the next round.
 */
export type ItemStatus = "resolved" | "unresolved";

/** One explicit verdict on one objection, by item id (schema 1.3). */
export type ItemVerdict = {
  item_id: string;
  /**
   * Why. Required (non-empty) for a resolved verdict: an objection is never closed without saying what settled it.
   */
  reason: string;
  status: ItemStatus;
};

/** Every limit the server enforces. */
export type LimitsV1 = {
  body_limit_bytes: number;
  guided_concurrency: number;
  /**
   * Visitor runs the store keeps at once; when full, the oldest resting visitor runs are purged to admit a new one.
   */
  guided_ring: number;
  list_page_max: number;
  live_concurrency: number;
  max_run_tokens: number;
  rate_runs_per_min: number;
  rate_session_per_min: number;
  rate_sse_per_min: number;
  /**
   * Seconds a request may take to be handled (body read included; a stream's body is not covered once its headers are sent).
   */
  request_timeout_secs: number;
  run_deadline_secs: number;
  session_max: number;
  session_ttl_secs: number;
  sse_heartbeat_secs: number;
  sse_max_global: number;
  sse_max_per_client: number;
  sse_max_per_run: number;
  visitor_access_hours: number;
  visitor_delete_days: number;
};

/** `GET /api/v1/runs` query: cursor pagination, newest first. */
export type ListRunsQuery = {
  /** The `next_cursor` of the previous page. */
  cursor?: string | null;
  /** 1 to 50; default 20. */
  limit?: number | null;
};

/** The live section of `/capabilities`, present when live mode is wired. */
export type LiveCapabilitiesV1 = {
  /**
   * The visitor-safe budget. Absent when the ledger could not be read: no run can be admitted until it can (`/ready` then reports `live.ledger_ok: false`), and no figure is invented in its place.
   */
  budget?: PublicBudgetV1 | null;
  presets: PresetV1[];
  /** Always true: `attest_adult` must be true on every live run. */
  requires_adult_attestation: boolean;
  /** Always true: a Turnstile token must accompany every live run. */
  requires_turnstile: boolean;
};

/**
 * The live mode as `/ready` reports it. `ready` is false while the ledger is tripped or unreadable, or the evidence source is not ok.
 */
export type LiveReadyV1 = {
  /**
   * The evidence source (the sealed snapshot in production) passed its check.
   */
  evidence_ok: boolean;
  /** The ledger could be read. */
  ledger_ok: boolean;
  trip_code?: string | null;
  tripped: boolean;
};

/**
 * Metadata for one model call, recorded so Meta model participation is demonstrable from receipts. `model` preserves the exact Model API identity string returned/configured for the call.
 */
export type ModelInvocation = {
  /** Schema 1.3: HTTP attempts made for this call (retries included). */
  attempts?: number | null;
  /** Schema 1.3: why the model stopped generating, as the API said it. */
  finish_reason?: string | null;
  input_tokens?: number | null;
  latency_ms?: number | null;
  /** Schema 1.3: the output bound sent with the request, if any. */
  max_completion_tokens?: number | null;
  model: string;
  output_tokens?: number | null;
  /** Schema 1.3: SHA-256 of the prompt template, lowercase hex. */
  prompt_sha256?: string | null;
  provider: string;
  /** Schema 1.3: reasoning effort requested, if any. */
  reasoning_effort?: string | null;
  /** Schema 1.3: reasoning tokens, when the API reports them apart. */
  reasoning_tokens?: number | null;
  request_id?: string | null;
  /** Schema 1.3: whether a pseudonymous safety identifier was sent. */
  safety_identifier_sent?: boolean | null;
  /** Schema 1.3: name of the structured-output schema requested. */
  schema_name?: string | null;
  /** Schema 1.3: SHA-256 of that schema as sent, lowercase hex. */
  schema_sha256?: string | null;
};

/** Public view of a model call. */
export type ModelInvocationV1 = {
  /** Operator surface only. */
  attempts?: number | null;
  finish_reason?: string | null;
  input_tokens?: number | null;
  latency_ms?: number | null;
  /** Operator surface only. */
  max_completion_tokens?: number | null;
  model: string;
  output_tokens?: number | null;
  /** Operator surface only. */
  prompt_sha256?: string | null;
  provider: string;
  reasoning_tokens?: number | null;
  request_id?: string | null;
  /** Operator surface only. */
  schema_name?: string | null;
  /** Operator surface only. */
  schema_sha256?: string | null;
  /** True for `provider=fake`: a scripted double, no model call. */
  scripted: boolean;
};

export type ModelMode = "scripted" | "meta";

/**
 * Which model answered the roles, read from the chained `ModelInvocation` records (provider and request id), never from the request or the declared mode.
 */
export type ModelProvenance = "scripted" | "live_meta" | "mixed" | "undetermined";

/**
 * A challenge item that is still open: listed as unresolved by the final evaluation ([`Deliberation::open_items`]), or raised after it ([`Deliberation::unevaluated_items`]). For a listed item, details are None when no recorded challenge contains an item with its id (pre-1.1 evaluations listed challenge ids, not item ids).
 */
export type OpenItem = {
  claim_id?: string | null;
  description?: string | null;
  /**
   * Item id; for an unevaluated item, empty on pre-1.1 records, which predate item ids.
   */
  id: string;
  kind?: ChallengeKind | null;
  /** Round whose challenge raised the item. */
  raised_in_round?: number | null;
};

/** Operator surface: which memory backend serves evidence. */
export type OperatorMemory = "fake" | "offline" | "hyphae";

/** Operator surface: which model answers the roles. */
export type OperatorModel = "fake" | "meta";

/**
 * The policy rule that decided a recommendation kind. Listed in the 1.2 priority order; 1.1 ranks `abstention` above `recorded_contradiction`.
 */
export type PolicyRule =
  | "invalid_proof"
  | "recorded_contradiction"
  | "abstention"
  | "unresolved_items"
  | "insufficient_evidence"
  | "all_checks_passed";

/**
 * Policy rule order a [`PolicyTrace`] was derived under, serialized as its version string.
 */
export type PolicyRules = "1.1" | "1.2";

/**
 * Why the policy derived the kind it did, from one evaluation and the verification records of its round. `rule` is the rule that decided `kind`; `reasons` lists every check that failed, in the priority order of `version`, so its first entry belongs to `rule`. `reasons` is empty exactly when the run is `supported`.
 */
export type PolicyTrace = {
  counts: TraceCounts;
  kind: RecommendationKind;
  reasons: TraceReason[];
  rule: PolicyRule;
  version: PolicyRules;
};

/**
 * One curated live preset, as `/capabilities` lists it: a canned task the recorded batteries ran, never visitor text.
 */
export type PresetV1 = {
  description: string;
  /**
   * Where the evidence comes from in this deployment (the sealed snapshot in production).
   */
  evidence: EvidenceProvenance;
  /** The battery tag, e.g. `R03-raise-to-10`: the `preset_id` to send. */
  id: string;
  /** Always `live_meta`: the owner's Meta key answers every role. */
  model: ModelProvenance;
  /** The project every preset runs against (`demo-repo`). */
  project: string;
  title: string;
};

/**
 * The request a memory proof seals (schema 1.3): the operation and arguments the proof's own result answers.
 */
export type ProofRequest = {
  limit?: number | null;
  /** Backend operation, e.g. `list` or `recall`. */
  operation: string;
  /** Backend project string the operation ran against. */
  project: string;
  query?: string | null;
};

/**
 * Proposer output: a plan plus the claims, assumptions, and uncertainties it rests on.
 */
export type Proposal = {
  assumptions: string[];
  author_role: AgentRole;
  claims: Claim[];
  created_at: string;
  id: string;
  plan_steps: string[];
  run_id: string;
  schema_version: string;
  summary: string;
  uncertainties: string[];
};

/** Something a client must show next to the provenance axes. */
export type ProvenanceFlag = "provenance_inconsistent" | "unchained" | "mode_undeclared";

/** The provenance axes of a run, every one derived from chained data. */
export type ProvenanceV1 = {
  /** The mode the run was declared with, as recorded in `TaskCreated`. */
  declared?: TaskMode | null;
  evidence: EvidenceProvenance;
  flags?: ProvenanceFlag[];
  /** The chained Meta request ids, when the model axis is `live_meta`. */
  meta_request_ids?: string[];
  model: ModelProvenance;
  source: SourceProvenance;
};

/**
 * What `/capabilities` publishes about the live budget: remaining amounts as the ledger renders them (USD, six decimals), whether live mode is tripped and the coarse reason code. Never an amount spent, a detail string, an upstream status or a path; the judge lane is not mentioned.
 */
export type PublicBudgetV1 = {
  /** What the public lane could reserve right now, pacing included. */
  daily_available_now: string;
  /** Today's remaining public budget (UTC day). */
  daily_remaining: string;
  lifetime_remaining: string;
  /**
   * `kill_switch`, `lifetime_cap`, `output_bound`, `charge_bound`, `provider_refused_key` or `inconsistent`, when tripped.
   */
  trip_code?: string | null;
  /**
   * True when the ledger refuses every admission until an operator resets it, or when the operator's kill-switch file exists (reported at once, before the next admission records the trip).
   */
  tripped: boolean;
};

/** `GET /api/v1/ready`. */
export type ReadyV1 = {
  active_runs: number;
  /** Present when live mode is wired on this server. */
  live?: LiveReadyV1 | null;
  queued_runs: number;
  ready: boolean;
  run_store: RunStoreStatusV1;
  showcase: ShowcaseStatusV1;
  shutting_down: boolean;
};

/** The Evaluator's judgment of one claim. */
export type ReceiptAssessment = {
  contradicting_evidence: string[];
  corroborated: boolean;
  note: string;
  relevant_evidence: string[];
};

/** The human decision, if one was recorded. */
export type ReceiptAuthorization = {
  /**
   * Authority the decision was recorded under: `unspecified` for every record before schema 1.3, which carried no authority and must never be read as operator authority. None while no decision is recorded.
   */
  authority?: string | null;
  decided_at?: string | null;
  note?: string | null;
  state: AuthorizationState;
};

/** The hash chain behind a receipt. The chain is unsigned. */
export type ReceiptChain = {
  /** Sequences of the run's first and last records in their log. */
  first_sequence: number;
  /**
   * Hash of the run's last event (not the head of a multi-run file). None when that record is unchained.
   */
  head_hash?: string | null;
  last_sequence: number;
  /** Plain-language statement of what the chain does and does not show. */
  note: string;
  scope: ChainScope;
};

/** One claim of the final proposal. */
export type ReceiptClaim = {
  /**
   * The final evaluation's assessment. None when it did not assess the claim.
   */
  assessment?: ReceiptAssessment | null;
  /** Evidence the proposer cited for the claim. */
  evidence_refs: string[];
  id: string;
  statement: string;
};

/** Where and why a run failed. */
export type ReceiptFailure = {
  /**
   * Stable failure code recorded with the failure (schema 1.3), for clients that localize from codes instead of the English reason.
   */
  code?: string | null;
  reason: string;
  stage: string;
};

/** One revision, in order. */
export type ReceiptRevision = {
  /** Item ids the revision says it addresses (a claim, not a verdict). */
  addressed_items: string[];
  /**
   * The evaluation round this revision answers. The revised proposal is judged in the next round.
   */
  after_round: number;
  proposal_id: string;
  summary: string;
  supersedes: string;
};

export type Recommendation = {
  issued_at: string;
  kind: RecommendationKind;
  /** Human-readable, concise justification citing claim/evidence ids. */
  rationale: string;
  schema_version: string;
  /** Assumptions that remain unresolved at decision time. */
  unresolved_assumptions: string[];
  /**
   * Schema 1.3: the final proposal's own uncertainties, carried to the human next to its assumptions.
   */
  unresolved_uncertainties?: string[];
};

/** Runtime-derived assessment of a run. Never written by a model. */
export type RecommendationKind = "supported" | "contested" | "insufficient_evidence" | "abstained" | "blocked";

/** The recommendation with its policy trace. */
export type RecommendationV1 = {
  issued_at: string;
  kind: RecommendationKind;
  /** `policy 1.2`, or `protocol 1.3 · policy rules 1.2`. */
  protocol: string;
  rationale: string;
  /** Stable code of the rule that fired, e.g. `contested.unresolved_items`. */
  rationale_code?: string | null;
  /**
   * `policy::explain` over the records the recommendation was issued on. None for a recommendation recorded before schema 1.1.
   */
  trace?: PolicyTrace | null;
  unresolved_assumptions: string[];
  unresolved_uncertainties?: string[];
};

/** Relevance axis only. Independent of integrity and factuality. */
export type RelevanceStatus = "relevant" | "irrelevant" | "not_assessed";

/** Why the server re-read the log. */
export type ResyncReason = "lagged" | "gap" | "closed";

/**
 * `event: resync`: the frames that follow were re-read from the durable log (delivery `replay`), starting after `from_sequence`.
 */
export type ResyncV1 = {
  from_sequence: number;
  reason: ResyncReason;
};

/** How an evidence record was obtained (schema 1.3). */
export type Retrieval = "live_recall" | "sealed_snapshot" | "simulated";

/** A revision is a new proposal document that supersedes an earlier one. */
export type Revision = {
  /** Which challenge ITEMS this revision claims to address, by item id. */
  addressed_items?: string[];
  proposal: Proposal;
  schema_version: string;
  supersedes: string;
};

/**
 * One deliberation round: the proposal judged, the critique, the evaluation and the verifications recorded for it.
 */
export type RoundV1 = {
  challenge?: Challenge | null;
  evaluation?: Evaluation | null;
  proposal_id?: string | null;
  /** 1-based. */
  round: number;
  verifications: VerificationV1[];
};

/**
 * A verification record and the evaluation round it belongs to. The record is nested, never flattened beside `round`: a field the record gains (schema 1.3 adds its own round) can then never collide with this receipt's tag.
 */
export type RoundVerification = {
  /**
   * 1-based evaluation round, from the record's position in the log: the records after the k-th evaluation are round k. 0 for a record made before any evaluation, which no policy derivation reads.
   */
  round: number;
  /** The verification record as stored. */
  verification: EvidenceVerification;
};

/**
 * `202 Accepted` of run creation: the run exists and its task is durable; the deliberation follows in the background.
 */
export type RunCreatedV1 = {
  links: RunLinksV1;
  run: RunSummaryV1;
};

/** The full view of one run. */
export type RunDetailV1 = {
  chain: ChainV1;
  challenge_items: ChallengeItemV1[];
  /** The proposal as it stands: the last proposal or revision. */
  current_proposal?: Proposal | null;
  decision?: DecisionV1 | null;
  events: EventV1[];
  model_invocations: ModelInvocationV1[];
  recommendation?: RecommendationV1 | null;
  rounds: RoundV1[];
  summary: RunSummaryV1;
  task?: Task | null;
};

/** Routes of a run, relative to the server origin. */
export type RunLinksV1 = {
  cancel: string;
  decision: string;
  events: string;
  events_ndjson: string;
  receipt: string;
  self: string;
};

/** `GET /api/v1/runs`: the caller's runs, newest first. */
export type RunListV1 = {
  next_cursor?: string | null;
  runs: RunSummaryV1[];
};

export type RunOrigin = "cli" | "operator" | "public";

/**
 * Receipt v2: everything a reader needs to see what a human is asked to authorize, and on what grounds, derived from one run's events.
 */
export type RunReceiptV2 = {
  authorization: ReceiptAuthorization;
  chain: ReceiptChain;
  deliberation: Deliberation;
  event_count: number;
  /** Stage and reason of a failed run. */
  failure?: ReceiptFailure | null;
  final_state: RunState;
  /** Always [`RunReceiptV2::FORMAT`]. */
  format: string;
  generated_at: string;
  /** Every model call recorded for the run, in order. */
  model_invocations: ModelInvocation[];
  /**
   * The policy re-derived from the evaluation and verification records the recommendation was issued on. None when no recommendation was issued, or for a pre-1.1 recommendation record, which predates re-derivation. A recommendation recorded under schema 1.1 or later always has one: no receipt is issued for it otherwise.
   */
  policy_trace?: PolicyTrace | null;
  /** Plain-language limits of what the proofs establish. Always present. */
  proof_limitations: string;
  /**
   * Protocol and policy rules of the run, e.g. `policy 1.2`, or `protocol 1.3 · policy rules 1.2` once carry-forward changed the policy's inputs.
   */
  protocol: string;
  /**
   * The recommendation as recorded. From schema 1.1 on, its kind, rationale and unresolved assumptions were re-derived from the run's records before this receipt was issued; a pre-1.1 one is shown as recorded, unverified.
   */
  recommendation?: Recommendation | null;
  run_id: string;
  /**
   * Schema version of the run's first record: the version it was deliberated under.
   */
  run_schema_version: string;
  /** Schema version of the software that produced this receipt. */
  schema_version: string;
  task?: Task | null;
  /**
   * Every verification record, tagged with its round. The policy used the final round's records only.
   */
  verifications: RoundVerification[];
};

/**
 * Deterministic lifecycle of one run. Transitions are owned and validated by the runtime state machine; see `docs/adr/0001-m1-architecture.md`.
 */
export type RunState = "created" | "proposed" | "evidence_review" | "evaluated" | "revised" | "abstained" | "awaiting_human_approval" | "approved" | "rejected" | "failed";

/** The run store as `/ready` reports it. */
export type RunStoreStatusV1 = {
  /**
   * Operator surface only: the store's own error text, which can name files by path. The server logs it once per change.
   */
  index_error?: string | null;
  /**
   * Why the run index could not be verified at the last check, on both surfaces; absent while it verifies.
   */
  index_error_code?: IndexErrorCodeV1 | null;
  index_head?: string | null;
  index_seq?: number | null;
  index_tail_repaired: boolean;
  index_tampered: boolean;
  ok: boolean;
};

/**
 * The one-line view of a run: list entries, the 202 of creation, every SSE frame.
 */
export type RunSummaryV1 = {
  /**
   * True while the deliberation is in progress on this server: never for a run at its decision point or terminal, whatever frame or view carries it (a replayed frame of a finished run says false).
   */
  active: boolean;
  authorization: AuthorizationState;
  created_at: string;
  event_count: number;
  failure?: FailureV1 | null;
  last_sequence: number;
  /** True when a human approved a `blocked` or `abstained` run. */
  override: boolean;
  provenance: ProvenanceV1;
  recommendation?: RecommendationKind | null;
  revision_round: number;
  run_id: string;
  state: RunState;
  title?: string | null;
  updated_at: string;
};

/** One guided scenario, as `GET /api/v1/scenarios` lists it. */
export type ScenarioV1 = {
  description: string;
  evidence: EvidenceProvenance;
  expected_outcome: RecommendationKind;
  id: GuidedScenario;
  model: ModelProvenance;
  /** Always true: scripted model, simulated proofs. */
  simulated: boolean;
  title: string;
};

export type ScenariosV1 = {
  scenarios: ScenarioV1[];
};

/** `POST /api/v1/session`, `GET /api/v1/session`. */
export type SessionV1 = {
  /** True when this response minted the session. */
  created: boolean;
  expires_at: string;
  /** The pseudonym recorded as `authorized_by` on this session's decisions. */
  pseudonym: string;
};

/** One archived run of the research showcase. */
export type ShowcaseEntryV1 = {
  authorization: string;
  battery: string;
  event_count: number;
  /** The event log inside `research/`, served at `/research/<file>`. */
  file: string;
  /** Hash of the run's last event; None for unchained (1.0) runs. */
  head_hash?: string | null;
  /** Archive label, e.g. `F03`. */
  label: string;
  /** The recommendation kind, or `failed:<stage>`. */
  outcome: string;
  provenance: ProvenanceV1;
  /**
   * The run record (task, detail, receipt as downloaded), when the archive has one.
   */
  record_file?: string | null;
  run_id: string;
  schema_version: string;
  /** SHA-256 of that file, as `research/MANIFEST.json` pins it. */
  sha256: string;
  title: string;
};

/** The showcase as `/ready` reports it. */
export type ShowcaseStatusV1 = {
  entries: number;
  errors?: string[];
  ok: boolean;
};

/** `GET /api/v1/showcase`. */
export type ShowcaseV1 = {
  entries: ShowcaseEntryV1[];
  /** `research/MANIFEST.json` as served at `/research/MANIFEST.json`. */
  manifest_file: string;
  manifest_sha256: string;
};

/** Where the records come from. */
export type SourceProvenance = "this_server" | "research_archive";

/**
 * One SSE frame, for the contract and tests. On the wire each variant is an SSE event of the same name whose `data` is the inner JSON.
 */
export type SseFrameV1 =
  | {
      delivery: Delivery;
      event: EventV1;
      frame: "consensus";
      summary: RunSummaryV1;
    }
  | {
      frame: "resync";
      from_sequence: number;
      reason: ResyncReason;
    }
  | {
      frame: "stream_end";
      last_sequence: number;
      reason: StreamEndReason;
      state: RunState;
    };

/** Why a stream closed. */
export type StreamEndReason = "awaiting_human_approval" | "terminal";

/** `event: stream_end`: the last frame of a stream. */
export type StreamEndV1 = {
  last_sequence: number;
  reason: StreamEndReason;
  state: RunState;
};

/** Which server answered. */
export type SurfaceV1 = "public" | "operator";

/** A user-defined problem submitted for multi-agent deliberation. */
export type Task = {
  /** RFC 3339 timestamp supplied by the runtime clock. */
  created_at: string;
  description: string;
  id: string;
  /**
   * Schema 1.3: the mode the run was declared with when it was created. A declaration, not proof: what actually happened is read from the recorded invocations and verifications.
   */
  mode?: TaskMode | null;
  project_id: string;
  run_id: string;
  schema_version: string;
  title: string;
};

/** How a run was declared at creation (schema 1.3). */
export type TaskMode = {
  evidence: EvidenceMode;
  model: ModelMode;
  origin: RunOrigin;
  /** Curated preset the task came from, when it did. */
  preset?: string | null;
};

/** The counts `rationale_for` reports, from the same inputs. */
export type TraceCounts = {
  /** Claim assessments in the evaluation. */
  claims: number;
  /** Of those, the ones judged corroborated. */
  corroborated: number;
  proofs_invalid: number;
  /** Integrity `unavailable` or `verification_error`. */
  proofs_unavailable_or_error: number;
  proofs_valid: number;
  /** Entries in the evaluation's `unresolved_items`. */
  unresolved_items: number;
};

/**
 * One policy check that keeps a run from `supported`, with the ids it concerns. Evidence and claim ids are listed once each, in the order the evaluation and verification records first name them; `OpenItems` is the exception and lists item ids exactly as recorded, duplicates included.
 */
export type TraceReason =
  | {
      evidence_ids: string[];
      reason: "invalid_proofs";
    }
  | {
      evidence_ids: string[];
      reason: "contradicted_evidence";
    }
  | {
      abstain_reason?: string | null;
      reason: "evaluator_abstained";
    }
  | {
      item_ids: string[];
      reason: "open_items";
    }
  | {
      reason: "no_assessments";
    }
  | {
      claim_ids: string[];
      reason: "uncorroborated_claims";
    }
  | {
      claim_ids: string[];
      reason: "claims_without_own_evidence";
    }
  | {
      evidence_ids: string[];
      reason: "cited_evidence_not_valid";
    }
  | {
      reason: "no_checked_evidence";
    };

/** A challenge item from an earlier round that no later round judged. */
export type UnrejudgedObjection = {
  claim_id?: string | null;
  description: string;
  /** Item id; empty on pre-1.1 records, which predate item ids. */
  id: string;
  kind: ChallengeKind;
  raised_in_round: number;
  /**
   * Whether the evaluation of the round that raised it listed it as unresolved (pre-1.1: listed its challenge's id). False means that evaluation left it unlisted, which before protocol 1.3 counted as resolved without any stated reason.
   */
  unresolved_in_its_round: boolean;
};

/**
 * Public view of a verification record: the three status axes, the verifier identity and anchor; the verifier's raw output stays on the operator surface.
 */
export type VerificationV1 = {
  checked_at: string;
  /** Operator surface only. */
  detail?: string | null;
  evidence_id: string;
  factual_status: FactualStatus;
  integrity_status: IntegrityStatus;
  proof_ref?: string | null;
  relevance_status: RelevanceStatus;
  report?: VerifierReport | null;
  round?: number | null;
  /**
   * True when the verifier is the scripted double: the proof outcome was scripted, not computed.
   */
  simulated: boolean;
  verified_anchor?: string | null;
  verifier?: string | null;
};

/**
 * Structured fields of a verifier's own report (schema 1.3), kept next to the free-text `detail` so a reader never has to parse prose.
 */
export type VerifierReport = {
  anchor_digest?: string | null;
  kind?: string | null;
  proof_digest?: string | null;
  result_digest?: string | null;
  scope?: string | null;
  status?: string | null;
};
