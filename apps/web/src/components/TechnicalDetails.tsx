"use client";
// SPDX-License-Identifier: Apache-2.0
//
// A docs article's technical details in a <details> element. It starts
// collapsed for the "simple" explanation level and expanded for
// "technical" (Settings). The visitor can still open or close it; a change of
// level re-creates the element in the new starting state.

import type { ReactNode } from "react";

import { usePreferences } from "./preferences";

type TechnicalDetailsProps = {
  summary: string;
  children: ReactNode;
};

export function TechnicalDetails({ summary, children }: TechnicalDetailsProps) {
  const { hydrated, explanation } = usePreferences();
  // The static HTML is rendered for the default level.
  const level = hydrated ? explanation : "simple";
  return (
    <details
      key={level}
      className="hc-details"
      open={level === "technical"}
      data-level={level}
    >
      <summary>{summary}</summary>
      <div className="hc-details-body">{children}</div>
    </details>
  );
}
