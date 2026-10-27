// SPDX-License-Identifier: Apache-2.0

/**
 * Picks the first supported locale for the visitor's preference list (for
 * example `navigator.languages`). An exact tag wins; otherwise the primary
 * language decides, so `pt-PT` gets `pt-BR` and `es-MX` gets `es`. Falls back
 * to `fallback` when nothing matches.
 */
export function negotiateLocale<L extends string>(
  preferred: readonly string[],
  supported: readonly L[],
  fallback: L,
): L {
  const byTag = new Map<string, L>();
  const byLanguage = new Map<string, L>();
  for (const locale of supported) {
    byTag.set(locale.toLowerCase(), locale);
    const language = primaryLanguage(locale);
    if (!byLanguage.has(language)) {
      byLanguage.set(language, locale);
    }
  }
  for (const tag of preferred) {
    const match =
      byTag.get(tag.toLowerCase()) ?? byLanguage.get(primaryLanguage(tag));
    if (match !== undefined) {
      return match;
    }
  }
  return fallback;
}

function primaryLanguage(tag: string): string {
  return (tag.split("-")[0] ?? "").toLowerCase();
}
