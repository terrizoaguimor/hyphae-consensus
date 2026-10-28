// SPDX-License-Identifier: Apache-2.0
//
// The honest placeholder for every view that needs the Hyphae Consensus API:
// a "Not connected" status, the fixed statement that the view is not
// connected in this build, and what the view will do once it is. No sample
// data, no spinner: nothing here pretends to load.

import { Badge } from "@astryxdesign/core/Badge";
import { getTranslations } from "next-intl/server";

import type { Locale } from "@/i18n/routing";

type NotConnectedProps = {
  locale: Locale;
  /** What the view will do once connected, one translated line each. */
  planned: string[];
};

export async function NotConnected({ locale, planned }: NotConnectedProps) {
  const t = await getTranslations({ locale, namespace: "NotConnected" });
  return (
    <div className="hc-panel hc-not-connected">
      <Badge variant="warning" label={t("badge")} />
      <p className="hc-not-connected-message">{t("message")}</p>
      <p>{t("plannedHeading")}</p>
      <ul className="hc-list">
        {planned.map((line) => (
          <li key={line}>{line}</li>
        ))}
      </ul>
    </div>
  );
}
