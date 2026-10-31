// SPDX-License-Identifier: Apache-2.0
//
// The honest placeholder for every view that needs the Hyphae Consensus API:
// a "Not connected" status, the fixed statement that the view is not
// connected, what the view does once it is, and, when the API was probed and
// did not answer, the failed request's code. No sample data, no spinner:
// nothing here pretends to load. Rendered by server pages (the Overview's
// metrics) and by the client-side ApiGate alike, so it reads its messages
// with the isomorphic useTranslations.

import { Button } from "@astryxdesign/core/Button";
import { useTranslations } from "next-intl";

import { Tag } from "./Tag";

type NotConnectedProps = {
  /** What the view will do once connected, one translated line each. */
  planned: string[];
  /** The error code of the failed health probe, when the API was probed. */
  unreachableCode?: string;
  /** Probes again; shown as a button when given. */
  onRetry?: () => void;
};

export function NotConnected({ planned, unreachableCode, onRetry }: NotConnectedProps) {
  const t = useTranslations("NotConnected");
  return (
    <div className="hc-panel hc-not-connected">
      <Tag variant="warning" icon="plug-zap" label={t("badge")} />
      <p className="hc-not-connected-message">{t("message")}</p>
      {unreachableCode === undefined ? null : (
        <p className="hc-supporting">{t("unreachable", { code: unreachableCode })}</p>
      )}
      <p>{t("plannedHeading")}</p>
      <ul className="hc-list">
        {planned.map((line) => (
          <li key={line}>{line}</li>
        ))}
      </ul>
      {onRetry === undefined ? null : (
        <div className="hc-actions">
          <Button variant="secondary" label={t("retry")} onClick={onRetry} />
        </div>
      )}
    </div>
  );
}
