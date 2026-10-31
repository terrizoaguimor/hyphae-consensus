// SPDX-License-Identifier: Apache-2.0
//
// Renders the `sections` of a catalog-driven page (legal pages, About, Docs
// articles) in catalog order: each section is an <h2> with a paragraph, a
// list, or both. Text may carry the tags of src/i18n/richTags.ts only, plus
// the named `values` the page supplies (fixed constants such as the contact
// address, never visitor input).

import { getTranslations } from "next-intl/server";
import type { ReactNode } from "react";

import { dynamicTranslator, keysOf, sectionsOf } from "@/i18n/dynamic";
import type { Locale } from "@/i18n/routing";

import { ConnectionText } from "./ConnectionText";
import { richTags } from "./rich";

type ContentSectionsProps = {
  locale: Locale;
  /** Catalog path of the page, e.g. "Legal.terms" or "About". */
  namespace: string;
  /** Prefix for the heading ids, unique on the page. */
  idPrefix: string;
  /** Sections whose items are a sequence (rendered as an ordered list). */
  ordered?: readonly string[];
  /** ICU arguments the texts may reference, e.g. {contact}. */
  values?: Record<string, string | number | Date>;
  /**
   * Sections whose body states whether the portal reached its server: the
   * catalog's body is the variant for a server that did not answer, and
   * these are the variants for a connected portal and for the moment before
   * the health probe settles (src/components/ConnectionText.tsx).
   */
  connectionBodies?: Readonly<Record<string, { connected: string; pending: string }>>;
};

export async function ContentSections({
  locale,
  namespace,
  idPrefix,
  ordered = [],
  values = {},
  connectionBodies = {},
}: ContentSectionsProps) {
  const t = dynamicTranslator(
    await getTranslations({ locale, namespace: namespace as never }),
  );
  const rich = { ...richTags(locale), ...values };
  return (
    <>
      {sectionsOf(t, "sections").map(([key, section]) => {
        const headingId = `${idPrefix}-${key}`;
        const base = `sections.${key}`;
        const variants = connectionBodies[key];
        return (
          <section key={key} className="hc-section">
            <h2 id={headingId}>{t(`${base}.heading`)}</h2>
            {section.body === undefined ? null : variants === undefined ? (
              <p>{t.rich(`${base}.body`, rich)}</p>
            ) : (
              <ConnectionText
                connected={variants.connected}
                unreachable={t.rich(`${base}.body`, rich)}
                pending={variants.pending}
              />
            )}
            {section.items === undefined
              ? null
              : renderList(
                  ordered.includes(key),
                  keysOf(t, `${base}.items`).map((item) => (
                    <li key={item}>{t.rich(`${base}.items.${item}`, rich)}</li>
                  )),
                )}
          </section>
        );
      })}
    </>
  );
}

function renderList(ordered: boolean, items: ReactNode[]): ReactNode {
  return ordered ? (
    <ol className="hc-list">{items}</ol>
  ) : (
    <ul className="hc-list">{items}</ul>
  );
}
