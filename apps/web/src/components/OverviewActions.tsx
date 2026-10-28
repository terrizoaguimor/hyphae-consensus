"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The Overview's main call to action, decided at runtime from the health
// probe. The main button promises only what the server can do: until the
// probe answers, and when the API is unreachable, it reads "How guided
// examples will work" and opens the Modes article (with the build note and
// the caveat under it); once the API answered it reads "Run a guided example"
// and opens New deliberation, and a status line names the server's schema,
// policy and contract versions from the health answer itself.

import { Button } from "@astryxdesign/core/Button";
import { useTranslations } from "next-intl";

import type { Locale } from "@/i18n/routing";
import { useApiHealth } from "@/lib/api/useApiHealth";
import { docPath, localePath } from "@/lib/site";

type OverviewActionsProps = {
  locale: Locale;
  /** `App.buildNote`, translated by the page (the App namespace stays on the server). */
  buildNote: string;
};

export function OverviewActions({ locale, buildNote }: OverviewActionsProps) {
  const t = useTranslations("Overview");
  const gate = useTranslations("ApiGate");
  const health = useApiHealth();
  const connected = health.status === "ok";
  return (
    <>
      {health.status === "unreachable" ? (
        <p className="hc-build-note">{buildNote}</p>
      ) : null}
      <div className="hc-actions">
        {connected ? (
          <Button
            href={localePath(locale, "new")}
            variant="primary"
            label={t("primaryAction")}
          />
        ) : (
          <Button
            href={docPath(locale, "modes")}
            variant="primary"
            label={t("primaryActionPlanned")}
          />
        )}
        <Button
          href={localePath(locale, "docs")}
          variant="secondary"
          label={t("secondaryAction")}
        />
      </div>
      {health.status === "unreachable" ? (
        <p className="hc-supporting">{t("primaryActionNote")}</p>
      ) : null}
      {connected ? (
        <p className="hc-supporting" data-api-status="connected">
          {gate("connected", {
            schema: health.health.schema_version,
            policy: health.health.policy_version,
            contract: health.health.contract_sha256.slice(0, 12),
          })}
        </p>
      ) : null}
    </>
  );
}
