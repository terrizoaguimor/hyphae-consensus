"use client";
// SPDX-License-Identifier: Apache-2.0
//
// A sentence whose truth depends on whether the portal reached its server:
// the variant is chosen from the runtime health probe (useApiHealth), never
// fixed at build time, so a connected page never says it is not connected,
// and an outage says that the server did not answer. Server pages translate
// the variants and pass them in; until the probe settles the `pending`
// variant is shown, which makes no claim either way (or nothing, when the
// sentence only exists in one state).

import type { ReactNode } from "react";

import { type ConnectionState, connectionState, useApiHealth } from "@/lib/api/useApiHealth";

type ConnectionTextProps = {
  /** Shown once GET /api/v1/health answered; null shows nothing. */
  connected: ReactNode;
  /** Shown once it did not answer; null shows nothing. */
  unreachable: ReactNode;
  /** Shown until the probe settles; null (the default) shows nothing. */
  pending?: ReactNode;
  /** Wraps the shown text (a paragraph by default). */
  as?: "p" | "span";
  className?: string;
};

export function useConnectionState(): ConnectionState {
  return connectionState(useApiHealth());
}

export function ConnectionText({
  connected,
  unreachable,
  pending = null,
  as = "p",
  className,
}: ConnectionTextProps) {
  const state = useConnectionState();
  const text = state === "connected" ? connected : state === "unreachable" ? unreachable : pending;
  if (text === null) {
    return null;
  }
  const Tag = as;
  return (
    <Tag className={className} data-connection-copy={state}>
      {text}
    </Tag>
  );
}
