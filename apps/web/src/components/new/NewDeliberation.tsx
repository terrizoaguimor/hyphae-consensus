"use client";
// SPDX-License-Identifier: Apache-2.0
//
// New deliberation, connected: the guided scenarios from GET /scenarios, each
// with its expected outcome as a neutral chip, its SCRIPTED model badge (the
// run pages' label) and SIMULATED evidence badge, and a Run button
// (ensureSession, POST /runs, then the run page); the live presets from capabilities.live (LiveCapabilitiesV1), each
// with the model and evidence badges of its own recorded axes, the public
// budget in US dollars, the 18+ and territory attestation, the optional
// invite code under a disclosure, the Turnstile widget and a Start button
// (POST /live-runs); or, when live mode is disabled, the fixed honest state
// linking to the showcase. The form is closed with one sentence when the
// ledger is tripped or unreadable, when no preset is listed, or when this
// build cannot solve the Turnstile challenge the server requires
// (src/lib/live/form.ts); Start waits for a preset, the attestation and a
// token, and every submit spends the token and asks for a fresh challenge.
// Every API error is shown with its code's sentence and the Retry-After wait.

import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { RadioList, RadioListItem } from "@astryxdesign/core/RadioList";
import { TextInput } from "@astryxdesign/core/TextInput";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useFormatter, useTranslations } from "next-intl";
import { useCallback, useEffect, useRef, useState } from "react";

import { dynamicTranslator } from "@/i18n/dynamic";
import type { Locale } from "@/i18n/routing";
import { api, ApiError, isApiError, JUDGE_CODE_MAX_CHARS, newIdempotencyKey } from "@/lib/api/client";
import type { Capabilities } from "@/lib/api/schemas";
import type {
  GuidedScenario,
  LiveCapabilitiesV1,
  PresetV1,
  PublicBudgetV1,
  ScenarioV1,
} from "@/lib/api/types";
import {
  focusAfterRefusal,
  isTripCode,
  type LiveClosedReason,
  liveClosedReason,
  parseUsd,
  type StartBlocker,
  startBlocker,
  startNativelyDisabled,
} from "@/lib/live/form";
import { evidenceLabel, modelLabel } from "@/lib/provenance";
import { localePath, runPath } from "@/lib/site";

import { ApiErrorNotice } from "../ApiErrorNotice";
import { richTags } from "../rich";
import { Turnstile, type TurnstileControl, TURNSTILE_SITE_KEY } from "./Turnstile";

type NewDeliberationProps = {
  locale: Locale;
};

function toApiError(error: unknown, path: string): ApiError {
  return isApiError(error)
    ? error
    : new ApiError({
        code: "unreachable",
        message: error instanceof Error ? error.message : "request failed",
        status: 0,
        path,
      });
}

export function NewDeliberation({ locale }: NewDeliberationProps) {
  const t = useTranslations("NewDeliberation");
  const tRun = useTranslations("Run");
  const router = useRouter();
  const [capabilities, setCapabilities] = useState<Capabilities | null>(null);
  const [scenarios, setScenarios] = useState<ScenarioV1[] | null>(null);
  const [loadError, setLoadError] = useState<ApiError | null>(null);
  const [attempt, setAttempt] = useState(0);
  const [starting, setStarting] = useState<GuidedScenario | null>(null);
  const [startError, setStartError] = useState<ApiError | null>(null);

  useEffect(() => {
    let cancelled = false;
    Promise.all([api.capabilities(), api.scenarios()])
      .then(([caps, list]) => {
        if (!cancelled) {
          setCapabilities(caps);
          setScenarios(list.scenarios);
          setLoadError(null);
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setLoadError(toApiError(error, "/scenarios"));
        }
      });
    return () => {
      cancelled = true;
    };
  }, [attempt]);

  const startGuided = async (scenario: GuidedScenario) => {
    // The pressed button is busy (aria-disabled, still focusable) rather than
    // disabled, so a second press reaches here and is dropped.
    if (starting !== null) {
      return;
    }
    setStarting(scenario);
    setStartError(null);
    try {
      await api.ensureSession();
      const created = await api.createGuidedRun(scenario, newIdempotencyKey());
      router.push(runPath(locale, created.run.run_id));
    } catch (error) {
      setStartError(toApiError(error, "/runs"));
      setStarting(null);
    }
  };

  if (loadError !== null) {
    return <ApiErrorNotice error={loadError} onRetry={() => setAttempt((n) => n + 1)} />;
  }
  if (capabilities === null || scenarios === null) {
    return (
      <p role="status" className="hc-supporting">
        {tRun("loading")}
      </p>
    );
  }

  return (
    <div className="hc-new">
      <section className="hc-section" aria-labelledby="guided-heading">
        <h2 id="guided-heading">{t("guided.heading")}</h2>
        <p>{t("guided.intro")}</p>
        {!capabilities.guided_enabled ? (
          <p className="hc-supporting">{t("guided.disabled")}</p>
        ) : (
          <>
            <p className="hc-supporting hc-tabular-nums" data-scenario-count={scenarios.length}>
              {t("guided.count", { count: scenarios.length })}
            </p>
            {scenarios.length === 0 ? null : (
              <ul className="hc-scenario-list" role="list">
                {scenarios.map((scenario) => (
                  <li key={scenario.id} className="hc-scenario-card" data-scenario={scenario.id}>
                    <div className="hc-scenario-head">
                      <h3 className="hc-card-title">{scenario.title}</h3>
                      <ul className="hc-badges" role="list">
                        <li>
                          {/* The model axis as every run page labels it
                              (src/lib/provenance.ts modelLabel): SCRIPTED for
                              a guided scenario, never a card-only wording. */}
                          <ScenarioModelBadge model={scenario.model} />
                        </li>
                        <li>
                          <Badge variant="purple" label={t("guided.evidenceBadge")} />
                        </li>
                      </ul>
                    </div>
                    <p>{scenario.description}</p>
                    <p className="hc-scenario-expected">
                      <span className="hc-supporting">{t("guided.expected")}</span>
                      <Badge
                        variant="neutral"
                        label={tRun(`kinds.${scenario.expected_outcome}`)}
                      />
                    </p>
                    <div className="hc-actions">
                      {/* The pressed button is busy, never disabled: a disabled button would drop the keyboard focus. */}
                      <Button
                        variant="primary"
                        label={starting === scenario.id ? t("guided.starting") : t("guided.run")}
                        aria-label={t("guided.runScenario", { title: scenario.title })}
                        isDisabled={starting !== null && starting !== scenario.id}
                        isLoading={starting === scenario.id}
                        onClick={() => void startGuided(scenario.id)}
                      />
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </>
        )}
        {startError === null ? null : <ApiErrorNotice error={startError} />}
      </section>

      <LiveSection locale={locale} capabilities={capabilities} />
    </div>
  );
}

type LiveSectionProps = {
  locale: Locale;
  capabilities: Capabilities;
};

function LiveSection({ locale, capabilities }: LiveSectionProps) {
  const t = useTranslations("NewDeliberation.live");
  const live = capabilities.live ?? null;
  return (
    <section
      className="hc-section"
      aria-labelledby="live-heading"
      data-live-enabled={capabilities.live_enabled}
    >
      <h2 id="live-heading">{t("heading")}</h2>
      <p>{t("intro")}</p>
      {!capabilities.live_enabled || live === null ? (
        <div className="hc-panel hc-not-connected" data-live="disabled">
          <p className="hc-not-connected-message">{t("disabled")}</p>
          <p>{t("disabledHint")}</p>
          <Link href={localePath(locale, "showcase")} className="hc-link">
            {t("showcaseLink")}
          </Link>
        </div>
      ) : (
        <LiveForm locale={locale} live={live} />
      )}
    </section>
  );
}

function LiveForm({ locale, live }: { locale: Locale; live: LiveCapabilitiesV1 }) {
  const t = useTranslations("NewDeliberation.live");
  const router = useRouter();
  const [presetId, setPresetId] = useState<string>("");
  const [attested, setAttested] = useState(false);
  const [judgeCode, setJudgeCode] = useState("");
  const [token, setToken] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);
  const turnstile = useRef<TurnstileControl | null>(null);
  const submitRef = useRef<HTMLButtonElement>(null);
  const errorRef = useRef<HTMLDivElement>(null);
  // Set when a request has just been refused, until the form that comes back
  // has been rendered: where the keyboard focus goes then.
  const refused = useRef(false);
  const onToken = useCallback((value: string | null) => setToken(value), []);

  const closed = liveClosedReason(live, TURNSTILE_SITE_KEY);
  const blocker = startBlocker({ live, presetId, attested, judgeCode, token, starting });
  const budget = live.budget ?? null;

  // The refused request spent the Turnstile token, so the form usually comes
  // back waiting for a fresh one, with Start natively disabled again: the
  // focus it held would fall to <body>. The error notice takes it then
  // (src/lib/live/form.ts focusAfterRefusal); a Start that can be pressed
  // again keeps it. Only a focus still on Start (or already dropped) moves.
  useEffect(() => {
    if (!refused.current || starting) {
      return;
    }
    refused.current = false;
    const button = submitRef.current;
    const active = document.activeElement;
    if (active !== null && active !== document.body && active !== button) {
      return;
    }
    if (focusAfterRefusal(closed, blocker) === "notice") {
      errorRef.current?.focus();
    } else {
      button?.focus();
    }
  });

  const start = async () => {
    if (closed !== null || blocker !== null) {
      return;
    }
    // A Turnstile token is single-use (the server spends it at Siteverify
    // before it reserves the budget, whatever it answers): it leaves with
    // this request only, and the widget is asked for a fresh one at once.
    const spent = live.requires_turnstile ? (turnstile.current?.spend() ?? null) : null;
    // The fields are disabled while the request is in flight: a submit made
    // with Enter from one of them would drop the keyboard focus with it, so
    // the focus goes to the Start button, which stays focusable (busy).
    submitRef.current?.focus();
    setToken(null);
    setStarting(true);
    setError(null);
    try {
      await api.ensureSession();
      const created = await api.createLiveRun(
        presetId,
        {
          turnstileToken: spent,
          attestAdult: live.requires_adult_attestation ? attested : false,
          judgeCode,
        },
        newIdempotencyKey(),
      );
      router.push(runPath(locale, created.run.run_id));
    } catch (cause) {
      refused.current = true;
      setError(toApiError(cause, "/live-runs"));
      setStarting(false);
    }
  };

  // A closed form lists the presets for reading and offers no control that
  // could not be used; an open one is the request form.
  return (
    <form
      className="hc-form"
      data-live="form"
      data-live-state={closed === null ? "open" : "closed"}
      onSubmit={(event) => {
        event.preventDefault();
        void start();
      }}
    >
      {closed === null ? null : <ClosedNotice locale={locale} reason={closed} budget={budget} />}
      {closed === null ? (
        <RadioList
          label={t("presetLegend")}
          value={presetId}
          onChange={setPresetId}
          isDisabled={starting}
        >
          {live.presets.map((preset) => (
            <RadioListItem
              key={preset.id}
              value={preset.id}
              label={preset.title}
              description={<PresetDescription preset={preset} />}
            />
          ))}
        </RadioList>
      ) : live.presets.length === 0 ? null : (
        <ul className="hc-preset-list" role="list" aria-label={t("presetLegend")}>
          {live.presets.map((preset) => (
            <li key={preset.id} className="hc-preset-item">
              <p className="hc-card-title">{preset.title}</p>
              <PresetDescription preset={preset} />
            </li>
          ))}
        </ul>
      )}
      {budget === null ? null : <BudgetPanel budget={budget} />}
      {closed === null && live.requires_adult_attestation ? (
        <div className="hc-field" data-live-attestation>
          <CheckboxInput
            label={t("attest")}
            description={t("attestDescription")}
            value={attested}
            onChange={(checked) => setAttested(checked)}
            isDisabled={starting}
          />
          <p className="hc-supporting">{t.rich("attestTerms", richTags(locale))}</p>
        </div>
      ) : null}
      {closed === null ? (
        <details className="hc-details">
          <summary>{t("inviteSummary")}</summary>
          <div className="hc-details-body">
            <TextInput
              label={t("inviteLabel")}
              description={t("inviteDescription", { max: JUDGE_CODE_MAX_CHARS })}
              value={judgeCode}
              onChange={(value) => setJudgeCode(value)}
              isOptional
              isDisabled={starting}
              autoComplete="off"
            />
          </div>
        </details>
      ) : null}
      {/* Without a site key the closed notice above says why; nothing is loaded. */}
      {live.requires_turnstile && TURNSTILE_SITE_KEY !== undefined ? (
        <div className="hc-field">
          <h3 className="hc-panel-title">{t("turnstileHeading")}</h3>
          {closed === null ? (
            <Turnstile siteKey={TURNSTILE_SITE_KEY} onToken={onToken} controlRef={turnstile} />
          ) : (
            <p className="hc-supporting" data-turnstile="closed">
              {t("turnstileClosed")}
            </p>
          )}
        </div>
      ) : null}
      {closed === null && blocker !== null && blocker !== "starting" ? (
        <p className="hc-supporting" data-start-blocker={blocker}>
          {blocker === "judge_code"
            ? t("needsShorterCode", { max: JUDGE_CODE_MAX_CHARS })
            : t(blockerKey(blocker))}
        </p>
      ) : null}
      <div className="hc-actions">
        {/* Busy while the request is in flight, never disabled by it: a disabled button would drop the keyboard focus. */}
        <Button
          ref={submitRef}
          type="submit"
          variant="primary"
          label={starting ? t("starting") : t("start")}
          isDisabled={startNativelyDisabled(closed, blocker)}
          isLoading={starting}
        />
      </div>
      {error === null ? null : (
        // Focusable from script only: it takes the focus after a refusal that leaves Start disabled.
        <div ref={errorRef} tabIndex={-1} data-live-error>
          <ApiErrorNotice error={error} />
        </div>
      )}
    </form>
  );
}

function blockerKey(
  blocker: Exclude<StartBlocker, "starting" | "judge_code">,
): "needsPreset" | "needsAttestation" | "needsTurnstile" {
  switch (blocker) {
    case "preset":
      return "needsPreset";
    case "attestation":
      return "needsAttestation";
    case "turnstile":
      return "needsTurnstile";
  }
}

/** A guided scenario's model axis, labelled by provenance.ts like a run's (no delivery: never LIVE). */
function ScenarioModelBadge({ model }: { model: ScenarioV1["model"] }) {
  const tProvenance = dynamicTranslator(useTranslations("Provenance"));
  const label = modelLabel({ model });
  return (
    <span data-scenario-model={model}>
      <Badge variant={label.variant} label={tProvenance(label.key)} />
    </span>
  );
}

/** A preset's text and the badges of its own recorded axes (never assumed for the section). */
function PresetDescription({ preset }: { preset: PresetV1 }) {
  const tProvenance = dynamicTranslator(useTranslations("Provenance"));
  // A preset is not a run being watched: no delivery, so the model badge is
  // never the LIVE-worded one (src/lib/provenance.ts modelLabel).
  const model = modelLabel({ model: preset.model });
  const evidence = evidenceLabel(preset.evidence);
  return (
    <span className="hc-preset" data-preset={preset.id}>
      <span className="hc-preset-text">{preset.description}</span>
      <span className="hc-badges" role="list">
        <span role="listitem">
          <Badge variant={model.variant} label={tProvenance(model.key)} />
        </span>
        <span role="listitem">
          <Badge variant={evidence.variant} label={tProvenance(evidence.key)} />
        </span>
      </span>
    </span>
  );
}

/** The public budget as the ledger reports it, in US dollars (cut to cents, never rounded up). */
function BudgetPanel({ budget }: { budget: PublicBudgetV1 }) {
  const t = useTranslations("NewDeliberation.live.budget");
  const format = useFormatter();
  const usd = (value: string) => {
    const amount = parseUsd(value);
    return amount === null ? (
      <code className="hc-code">{value}</code>
    ) : (
      format.number(amount, {
        style: "currency",
        currency: "USD",
        minimumFractionDigits: 2,
        maximumFractionDigits: 2,
      })
    );
  };
  return (
    <div className="hc-field" data-live-budget>
      <h3 className="hc-panel-title">{t("heading")}</h3>
      <dl className="hc-kv">
        <div>
          <dt>{t("availableNow")}</dt>
          <dd className="hc-tabular-nums" data-budget="daily_available_now">
            {usd(budget.daily_available_now)}
          </dd>
        </div>
        <div>
          <dt>{t("dailyRemaining")}</dt>
          <dd className="hc-tabular-nums" data-budget="daily_remaining">
            {usd(budget.daily_remaining)}
          </dd>
        </div>
        <div>
          <dt>{t("lifetimeRemaining")}</dt>
          <dd className="hc-tabular-nums" data-budget="lifetime_remaining">
            {usd(budget.lifetime_remaining)}
          </dd>
        </div>
      </dl>
      <p className="hc-supporting">{t("note")}</p>
    </div>
  );
}

/** Why no live run can start right now, and where to look instead. */
function ClosedNotice({
  locale,
  reason,
  budget,
}: {
  locale: Locale;
  reason: LiveClosedReason;
  budget: PublicBudgetV1 | null;
}) {
  const t = useTranslations("NewDeliberation.live");
  if (reason === "turnstile_unconfigured") {
    // The server requires a Turnstile token and this build cannot show the
    // challenge: every request would be refused (403 turnstile_required).
    return (
      <p
        className="hc-not-connected-message"
        data-live-closed="turnstile_unconfigured"
        data-turnstile="missing"
      >
        {t("turnstileMissing")}
      </p>
    );
  }
  if (reason === "no_presets") {
    return (
      <p className="hc-not-connected-message" data-live-closed="no_presets" data-live="no-presets">
        {t("noPresets")}
      </p>
    );
  }
  const tripCode = budget?.trip_code ?? null;
  return (
    <div className="hc-panel hc-not-connected" data-live-closed={reason}>
      <p className="hc-not-connected-message">
        {reason === "tripped" ? t("closed.tripped") : t("closed.budgetUnknown")}
      </p>
      {reason === "tripped" && tripCode !== null ? (
        <p data-trip-code={tripCode}>
          {isTripCode(tripCode) ? t(`trip.${tripCode}`) : t("trip.other", { code: tripCode })}
        </p>
      ) : null}
      <p>{t("disabledHint")}</p>
      <Link href={localePath(locale, "showcase")} className="hc-link">
        {t("showcaseLink")}
      </Link>
    </div>
  );
}
