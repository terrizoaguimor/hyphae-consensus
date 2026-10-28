"use client";
// SPDX-License-Identifier: Apache-2.0
//
// Renders its children only once GET /api/v1/health answered; while the probe
// is in flight it says so in one line (a real request, not a fake spinner),
// and when the API is unreachable it shows the NotConnected placeholder with
// the failed request's code and a retry button.

import { useTranslations } from "next-intl";
import type { ReactNode } from "react";

import { useApiHealth } from "@/lib/api/useApiHealth";

import { NotConnected } from "./NotConnected";

type ApiGateProps = {
  /** What the gated view does once connected (translated by the page). */
  planned: string[];
  children: ReactNode;
};

export function ApiGate({ planned, children }: ApiGateProps) {
  const t = useTranslations("ApiGate");
  const health = useApiHealth();
  if (health.status === "pending") {
    return (
      <p role="status" className="hc-supporting" data-api-gate="pending">
        {t("checking")}
      </p>
    );
  }
  if (health.status === "unreachable") {
    return (
      <div data-api-gate="unreachable">
        <NotConnected
          planned={planned}
          unreachableCode={health.error.code}
          onRetry={health.retry}
        />
      </div>
    );
  }
  return <div data-api-gate="ok">{children}</div>;
}
