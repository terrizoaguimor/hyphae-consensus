"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The Overview's invitation to the tour. Shown only to visitors who never
// closed the tour (read from localStorage after hydration, so a returning
// visitor never sees it flash), never forced: "No, thanks" hides it for good.

import { Button } from "@astryxdesign/core/Button";
import { useTranslations } from "next-intl";

import { focusPageHeading } from "./Modal";
import { usePreferences } from "./preferences";

export function TourInvite() {
  const t = useTranslations("Tour");
  const { tour, openTour, closeTour } = usePreferences();
  if (tour !== "new") {
    return null;
  }
  return (
    <section className="hc-panel hc-tour-invite">
      <h2 id="hc-tour-invite-title" className="hc-panel-title">
        {t("inviteTitle")}
      </h2>
      <p>{t("inviteText")}</p>
      <div className="hc-actions">
        <Button variant="primary" label={t("start")} onClick={openTour} />
        <Button
          variant="ghost"
          label={t("dismiss")}
          onClick={() => {
            closeTour("skipped");
            // The invitation disappears with the button that had focus.
            requestAnimationFrame(focusPageHeading);
          }}
        />
      </div>
    </section>
  );
}

/** "Take the tour", for the Docs page: opens the tour whatever its state. */
export function TourLauncher() {
  const t = useTranslations("Tour");
  const { openTour } = usePreferences();
  return <Button variant="secondary" label={t("launch")} onClick={openTour} />;
}
