// SPDX-License-Identifier: Apache-2.0
//
// A locale's catalog is three files merged: messages/<locale>.json (the UI),
// messages/docs/<locale>.json (Docs) and messages/legal/<locale>.json (Legal
// and About). Each file owns its own top-level namespaces, and
// tests/locale-parity.test.ts keeps every file's keys identical across the
// four locales.

import type { AbstractIntlMessages } from "next-intl";

import type { Locale } from "./routing";

export async function loadMessages(
  locale: Locale,
): Promise<AbstractIntlMessages> {
  const [ui, docs, legal] = await Promise.all([
    import(`../../messages/${locale}.json`),
    import(`../../messages/docs/${locale}.json`),
    import(`../../messages/legal/${locale}.json`),
  ]);
  return { ...ui.default, ...docs.default, ...legal.default };
}

/**
 * The namespaces client components read. Only these are serialized into each
 * page for the browser; Docs and Legal texts render on the server and reach
 * the client as HTML of the page that shows them.
 */
export const clientNamespaces = [
  "Nav",
  "Shell",
  "Theme",
  "LocaleSwitcher",
  "Settings",
  "Tour",
  // Views that talk to the API at runtime (ApiGate, New deliberation, My
  // runs, the run workspace) and the shared pieces they render.
  "NotConnected",
  "ApiGate",
  "Api",
  "Overview",
  "NewDeliberation",
  "Runs",
  "Run",
  "Provenance",
] as const;

export function pickClientMessages(
  messages: AbstractIntlMessages,
): AbstractIntlMessages {
  return Object.fromEntries(
    clientNamespaces
      .filter((namespace) => namespace in messages)
      .map((namespace) => [namespace, messages[namespace]]),
  ) as AbstractIntlMessages;
}
