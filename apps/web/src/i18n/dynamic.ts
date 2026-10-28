// SPDX-License-Identifier: Apache-2.0
//
// Catalog-driven pages (Docs articles, legal pages, About) enumerate their
// sections from the catalog itself, so their message keys are built at run
// time and cannot be checked by next-intl's key types. This is the one
// untyped view of a translator; tests/locale-parity.test.ts checks instead
// that every locale has exactly the English keys, and the e2e suite renders
// every such page in every locale.

import type { ReactNode } from "react";

type RichValues = Record<string, (chunks: ReactNode) => ReactNode>;

export type DynamicTranslator = {
  (key: string, values?: Record<string, string | number>): string;
  rich(key: string, values: RichValues): ReactNode;
  raw(key: string): unknown;
  has(key: string): boolean;
};

export function dynamicTranslator(translator: unknown): DynamicTranslator {
  return translator as DynamicTranslator;
}

/** A catalog section: a heading with a paragraph, a list, or both. */
export type CatalogSection = {
  heading: string;
  body?: string;
  items?: Record<string, string>;
};

export function sectionsOf(
  t: DynamicTranslator,
  key: string,
): Array<[string, CatalogSection]> {
  const value = t.raw(key);
  if (value === null || typeof value !== "object") {
    return [];
  }
  return Object.entries(value as Record<string, CatalogSection>);
}

export function keysOf(t: DynamicTranslator, key: string): string[] {
  const value = t.raw(key);
  return value !== null && typeof value === "object"
    ? Object.keys(value as object)
    : [];
}
