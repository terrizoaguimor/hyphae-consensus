// SPDX-License-Identifier: Apache-2.0
//
// The onboarding tour's steps and the catalog key of each step's body
// (messages: `Tour.steps.<step>.*`), kept apart from the dialog so the
// choice per connection state is unit-tested (tests/tour.test.ts).

import type { ConnectionState } from "./api/useApiHealth";

export const tourSteps = [
  "welcome",
  "guided",
  "live",
  "runs",
  "labels",
  "decide",
] as const;

export type TourStep = (typeof tourSteps)[number];

/** The steps whose body speaks about the connection: one variant per state. */
export const CONNECTION_STEPS = ["welcome", "guided"] as const;

export type TourBodyKey =
  | `steps.${TourStep}.body`
  | `steps.${(typeof CONNECTION_STEPS)[number]}.bodyConnected`
  | `steps.${(typeof CONNECTION_STEPS)[number]}.bodyPending`;

/**
 * The catalog key of a step's body. The steps that speak about the
 * connection have one variant per state of the health probe (HE #8): the
 * catalog's `body` is the variant for a server that did not answer, a
 * connected portal reads `bodyConnected`, and before the probe settles they
 * read `bodyPending`, which makes no claim either way.
 */
export function tourBodyKey(step: TourStep, connection: ConnectionState): TourBodyKey {
  if (step === "welcome" || step === "guided") {
    if (connection === "connected") {
      return `steps.${step}.bodyConnected`;
    }
    return connection === "pending" ? `steps.${step}.bodyPending` : `steps.${step}.body`;
  }
  return `steps.${step}.body`;
}
