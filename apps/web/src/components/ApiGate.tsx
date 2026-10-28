"use client";
// SPDX-License-Identifier: Apache-2.0
//
// Renders its children only once GET /api/v1/health answered; while the probe
// is in flight it says so in one line (a real request, not a fake spinner),
// and when the API is unreachable it shows the NotConnected placeholder with
// the failed request's code and a retry button. When the server reports a
// contract other than the one this build's types were generated from
// (health.contract_sha256 against CONTRACT_SHA256), a note says so above the
// view: the views validate every response, but may not know what is new.

import { useTranslations } from "next-intl";
import type { ReactNode } from "react";

import { CONTRACT_SHA256 } from "@/lib/api/types";
import { useApiHealth } from "@/lib/api/useApiHealth";

import { NotConnected } from "./NotConnected";

type ApiGateProps = {
  /** What the gated view does once connected (translated by the page). */
  planned: string[];
  children: ReactNode;
};

/** The note shown when the server's API contract differs from this build's. */
export function ContractMismatch({ server }: { server: string }) {
  const t = useTranslations("ApiGate");
  return (
    <p className="hc-build-note" data-contract-mismatch>
      {t("contractMismatch", {
        build: CONTRACT_SHA256.slice(0, 12),
        server: server.slice(0, 12),
      })}
    </p>
  );
}

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
  return (
    <div data-api-gate="ok">
      {health.health.contract_sha256 === CONTRACT_SHA256 ? null : (
        <ContractMismatch server={health.health.contract_sha256} />
      )}
      {children}
    </div>
  );
}
