"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The optional one-minute onboarding tour: six short steps in a modal dialog.
// It never opens by itself; the Overview invitation and the Docs page open
// it. Skipping (button, Escape or backdrop) and finishing are both remembered
// in localStorage, which hides the invitation; Docs can always reopen it.
// Focus stays on the primary button from step to step, and a polite live
// region announces each new step.

import { Button } from "@astryxdesign/core/Button";
import { useTranslations } from "next-intl";
import { type RefObject, useRef, useState } from "react";

import { tourBodyKey, tourSteps } from "@/lib/tour";

import { useConnectionState } from "./ConnectionText";
import { Modal } from "./Modal";
import { usePreferences } from "./preferences";

export { type TourStep, tourSteps } from "@/lib/tour";

const TITLE_ID = "hc-tour-title";
const BODY_ID = "hc-tour-body";

export function Tour() {
  const { tourOpen, closeTour } = usePreferences();
  const primaryRef = useRef<HTMLButtonElement>(null);
  return (
    <Modal
      isOpen={tourOpen}
      onClose={() => closeTour("skipped")}
      labelledBy={TITLE_ID}
      describedBy={BODY_ID}
      className="hc-dialog hc-dialog--tour"
      initialFocus={primaryRef}
    >
      {/* Mounted per opening, so every opening starts at the first step. */}
      {tourOpen ? (
        <TourSteps primaryRef={primaryRef} onClose={closeTour} />
      ) : null}
    </Modal>
  );
}

type TourStepsProps = {
  primaryRef: RefObject<HTMLButtonElement | null>;
  onClose: (outcome: "skipped" | "completed") => void;
};

// Each step's body has one variant per connection state (src/lib/tour.ts).
function TourSteps({ primaryRef, onClose }: TourStepsProps) {
  const t = useTranslations("Tour");
  const connection = useConnectionState();
  const [index, setIndex] = useState(0);
  const [announcement, setAnnouncement] = useState("");

  const step = tourSteps[index] ?? tourSteps[0];
  const total = tourSteps.length;
  const isLast = index === total - 1;

  const goTo = (next: number) => {
    const target = tourSteps[next];
    if (target === undefined) {
      return;
    }
    setIndex(next);
    setAnnouncement(
      t("announcement", {
        current: next + 1,
        total,
        title: t(`steps.${target}.title`),
        body: t(tourBodyKey(target, connection)),
      }),
    );
    // Back disappears on the first step: keep focus inside the dialog.
    if (next === 0) {
      primaryRef.current?.focus();
    }
  };

  return (
    <>
      <p className="hc-tour-progress">
        {t("progress", { current: index + 1, total })}
      </p>
      <h2 id={TITLE_ID} className="hc-dialog-title">
        {t(`steps.${step}.title`)}
      </h2>
      <p id={BODY_ID} className="hc-tour-body" data-connection-copy={connection}>
        {t(tourBodyKey(step, connection))}
      </p>
      <ol className="hc-tour-bar" aria-hidden="true">
        {tourSteps.map((name, position) => (
          <li
            key={name}
            data-state={
              position === index ? "current" : position < index ? "done" : "todo"
            }
          />
        ))}
      </ol>
      <p className="hc-visually-hidden hc-tour-live" aria-live="polite">
        {announcement}
      </p>
      <div className="hc-dialog-actions">
        <Button
          variant="ghost"
          label={t("skip")}
          onClick={() => onClose("skipped")}
        />
        <div className="hc-dialog-actions-end">
          {index > 0 ? (
            <Button
              variant="secondary"
              label={t("back")}
              onClick={() => goTo(index - 1)}
            />
          ) : null}
          <Button
            ref={primaryRef}
            variant="primary"
            label={isLast ? t("finish") : t("next")}
            onClick={() => (isLast ? onClose("completed") : goTo(index + 1))}
          />
        </div>
      </div>
    </>
  );
}
