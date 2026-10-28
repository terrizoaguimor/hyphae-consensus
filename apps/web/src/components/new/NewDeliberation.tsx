"use client";
// SPDX-License-Identifier: Apache-2.0
//
// New deliberation, connected: the guided scenarios from GET /scenarios, each
// with its expected outcome as a neutral chip and its SIMULATED model and
// evidence badges, and a Run button (ensureSession, POST /runs, then the run
// page); the live presets from capabilities.live when the server lists them,
// with the adult attestation, the optional invite code under a disclosure,
// the Turnstile widget and a Start button (POST /live-runs); or, when live
// mode is disabled, the fixed honest state linking to the showcase. Every
// API error is shown with its code's sentence and the Retry-After wait.

import { Badge } from "@astryxdesign/core/Badge";
import { Button } from "@astryxdesign/core/Button";
import { CheckboxInput } from "@astryxdesign/core/CheckboxInput";
import { RadioList, RadioListItem } from "@astryxdesign/core/RadioList";
import { TextInput } from "@astryxdesign/core/TextInput";
import Link from "next/link";
import { useRouter } from "next/navigation";
import { useFormatter, useTranslations } from "next-intl";
import { useCallback, useEffect, useState } from "react";

import type { Locale } from "@/i18n/routing";
import { api, ApiError, isApiError, newIdempotencyKey } from "@/lib/api/client";
import type { Capabilities } from "@/lib/api/schemas";
import type { GuidedScenario, ScenarioV1 } from "@/lib/api/types";
import { localePath, runPath } from "@/lib/site";

import { ApiErrorNotice } from "../ApiErrorNotice";
import { Turnstile } from "./Turnstile";

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
  const [starting, setStarting] = useState<GuidedScenario | null>(null);
  const [startError, setStartError] = useState<ApiError | null>(null);

  useEffect(() => {
    let cancelled = false;
    Promise.all([api.capabilities(), api.scenarios()])
      .then(([caps, list]) => {
        if (!cancelled) {
          setCapabilities(caps);
          setScenarios(list.scenarios);
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
  }, []);

  const startGuided = async (scenario: GuidedScenario) => {
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
    return <ApiErrorNotice error={loadError} />;
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
        ) : scenarios.length === 0 ? (
          <p className="hc-supporting">{t("guided.empty")}</p>
        ) : (
          <ul className="hc-scenario-list" role="list">
            {scenarios.map((scenario) => (
              <li key={scenario.id} className="hc-scenario-card" data-scenario={scenario.id}>
                <div className="hc-scenario-head">
                  <h3 className="hc-card-title">{scenario.title}</h3>
                  <ul className="hc-badges" role="list">
                    <li>
                      <Badge variant="purple" label={t("guided.modelBadge")} />
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
                  <Button
                    variant="primary"
                    label={starting === scenario.id ? t("guided.starting") : t("guided.run")}
                    aria-label={t("guided.runScenario", { title: scenario.title })}
                    isDisabled={starting !== null}
                    isLoading={starting === scenario.id}
                    onClick={() => void startGuided(scenario.id)}
                  />
                </div>
              </li>
            ))}
          </ul>
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
  const router = useRouter();
  const format = useFormatter();
  const [presetId, setPresetId] = useState<string>("");
  const [attest, setAttest] = useState(false);
  const [judgeCode, setJudgeCode] = useState("");
  const [token, setToken] = useState<string | null>(null);
  const [starting, setStarting] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);
  const onToken = useCallback((value: string | null) => setToken(value), []);

  const presets = capabilities.live?.presets ?? [];
  const budget = capabilities.live?.budget ?? null;

  const start = async () => {
    if (presetId === "" || !attest) {
      return;
    }
    setStarting(true);
    setError(null);
    try {
      await api.ensureSession();
      const created = await api.createLiveRun(
        presetId,
        { turnstileToken: token, attestAdult: attest, judgeCode },
        newIdempotencyKey(),
      );
      router.push(runPath(locale, created.run.run_id));
    } catch (cause) {
      setError(toApiError(cause, "/live-runs"));
      setStarting(false);
    }
  };

  return (
    <section className="hc-section" aria-labelledby="live-heading" data-live-enabled={capabilities.live_enabled}>
      <h2 id="live-heading">{t("heading")}</h2>
      <ul className="hc-badges" role="list">
        <li>
          <Badge variant="info" label={t("modelBadge")} />
        </li>
        <li>
          <Badge variant="neutral" label={t("evidenceBadge")} />
        </li>
      </ul>
      <p>{t("intro")}</p>
      {!capabilities.live_enabled ? (
        <div className="hc-panel hc-not-connected" data-live="disabled">
          <p className="hc-not-connected-message">{t("disabled")}</p>
          <p>{t("disabledHint")}</p>
          <Link href={localePath(locale, "showcase")} className="hc-link">
            {t("showcaseLink")}
          </Link>
        </div>
      ) : presets.length === 0 ? (
        <p className="hc-supporting" data-live="no-presets">
          {t("noPresets")}
        </p>
      ) : (
        <form
          className="hc-form"
          data-live="form"
          onSubmit={(event) => {
            event.preventDefault();
            void start();
          }}
        >
          <RadioList label={t("presetLegend")} value={presetId} onChange={setPresetId}>
            {presets.map((preset) => (
              <RadioListItem
                key={preset.id}
                value={preset.id}
                label={preset.title ?? t("untitledPreset", { id: preset.id })}
                description={preset.description ?? undefined}
              />
            ))}
          </RadioList>
          {budget === null || Object.keys(budget).length === 0 ? null : (
            <div className="hc-field">
              <h3 className="hc-panel-title">{t("budgetHeading")}</h3>
              <dl className="hc-kv">
                {Object.entries(budget).map(([key, value]) => (
                  <div key={key}>
                    <dt>
                      <code className="hc-code">{key}</code>
                    </dt>
                    <dd>
                      {typeof value === "number"
                        ? format.number(value)
                        : typeof value === "string" || typeof value === "boolean"
                          ? String(value)
                          : JSON.stringify(value)}
                    </dd>
                  </div>
                ))}
              </dl>
            </div>
          )}
          <CheckboxInput
            label={t("attest")}
            description={t("attestDescription")}
            value={attest}
            onChange={(checked) => setAttest(checked)}
          />
          <details className="hc-details">
            <summary>{t("inviteSummary")}</summary>
            <div className="hc-details-body">
              <TextInput
                label={t("inviteLabel")}
                description={t("inviteDescription")}
                value={judgeCode}
                onChange={(value) => setJudgeCode(value)}
                isOptional
              />
            </div>
          </details>
          <div className="hc-field">
            <h3 className="hc-panel-title">{t("turnstileHeading")}</h3>
            <Turnstile onToken={onToken} />
          </div>
          {presetId === "" ? (
            <p className="hc-supporting">{t("needsPreset")}</p>
          ) : !attest ? (
            <p className="hc-supporting">{t("needsAttestation")}</p>
          ) : null}
          <div className="hc-actions">
            <Button
              type="submit"
              variant="primary"
              label={starting ? t("starting") : t("start")}
              isDisabled={presetId === "" || !attest || starting}
              isLoading={starting}
            />
          </div>
          {error === null ? null : <ApiErrorNotice error={error} />}
        </form>
      )}
    </section>
  );
}
