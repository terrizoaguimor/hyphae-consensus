// SPDX-License-Identifier: Apache-2.0
//
// The only tags catalog messages may use with t.rich(). Each one renders a
// fixed element or a link to a fixed destination (src/components/rich.tsx), so
// a translation can never introduce a URL or arbitrary markup.
// tests/locale-parity.test.ts rejects any other tag, and requires every locale
// to use the same tags in the same message, except <en>: a translation wraps
// an English term it keeps (a label, an outcome name) in <en>, which renders
// lang="en", and the English message has no such term to mark.

export const RICH_TAGS = [
  "strong",
  "em",
  "code",
  "en",
  "aup",
  "geo",
  "hyphae",
  "cloudflare",
  "privacy",
  "terms",
  "limitations",
  "settings",
] as const;

export type RichTag = (typeof RICH_TAGS)[number];

/** Tags a translation may use even though the English message does not. */
export const LANGUAGE_TAGS: readonly RichTag[] = ["en"];
