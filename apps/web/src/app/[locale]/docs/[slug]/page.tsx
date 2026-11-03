// SPDX-License-Identifier: Apache-2.0
//
// One docs article, pre-rendered per locale and slug: a plain-language
// summary, the catalog's sections, technical details (expanded when the
// explanation level is "technical") and links to the neighboring articles.

import type { Metadata } from "next";
import Link from "next/link";
import { notFound } from "next/navigation";
import { getTranslations } from "next-intl/server";

import { ContentSections } from "@/components/ContentSections";
import { PageHeader } from "@/components/PageHeader";
import { richTags } from "@/components/rich";
import { TechnicalDetails } from "@/components/TechnicalDetails";
import { dynamicTranslator, keysOf } from "@/i18n/dynamic";
import { localizedPageMetadata, pageLocale } from "@/lib/pageLocale";
import { type DocSlug, docPath, docSlugs, localePath } from "@/lib/site";

type DocParams = { params: Promise<{ locale: string; slug: string }> };

export const dynamicParams = false;

// The [locale] layout supplies the locales; this adds the slugs under each.
export function generateStaticParams() {
  return docSlugs.map((slug) => ({ slug }));
}

function docSlug(slug: string): DocSlug {
  const found = docSlugs.find((candidate) => candidate === slug);
  if (found === undefined) {
    notFound();
  }
  return found;
}

export async function generateMetadata({
  params,
}: DocParams): Promise<Metadata> {
  const locale = await pageLocale(params);
  const slug = docSlug((await params).slug);
  const t = await getTranslations({ locale, namespace: "Docs" });
  return localizedPageMetadata(params, {
    path: `docs/${slug}`,
    descriptionKey: `docDescriptions.${slug}`,
    title: t(`articles.${slug}.title`),
  });
}

export default async function DocArticlePage({ params }: DocParams) {
  const locale = await pageLocale(params);
  const slug = docSlug((await params).slug);
  const t = dynamicTranslator(
    await getTranslations({ locale, namespace: "Docs" }),
  );
  const tags = richTags(locale);
  const base = `articles.${slug}`;
  const position = docSlugs.indexOf(slug);
  const previous = docSlugs[position - 1];
  const next = docSlugs[position + 1];

  return (
    <article className="hc-article">
      <PageHeader
        title={t(`${base}.title`)}
        eyebrow={
          <p className="hc-eyebrow">
            <Link href={localePath(locale, "docs")} className="hc-link">
              {t("allDocs")}
            </Link>
          </p>
        }
      />

      <section className="hc-summary">
        <h2 id={`${slug}-summary`}>{t("inShort")}</h2>
        <p>{t.rich(`${base}.summary`, tags)}</p>
      </section>

      <ContentSections
        locale={locale}
        namespace={`Docs.${base}`}
        idPrefix={slug}
        // The policy's rules apply in order: an ordered list says so.
        ordered={["rules"]}
        // "In this build" of Modes says what the portal's server answered (HE #8).
        connectionBodies={
          slug === "modes"
            ? {
                build: {
                  connected: t("modesBuild.connected"),
                  pending: t("modesBuild.pending"),
                },
              }
            : {}
        }
      />

      <TechnicalDetails summary={t("technicalSummary")}>
        <ul className="hc-list">
          {keysOf(t, `${base}.technical`).map((key) => (
            <li key={key}>{t.rich(`${base}.technical.${key}`, tags)}</li>
          ))}
        </ul>
      </TechnicalDetails>

      <nav aria-label={t("pagerLabel")} className="hc-pager">
        {previous === undefined ? null : (
          <Link
            href={docPath(locale, previous)}
            className="hc-pager-link"
            rel="prev"
          >
            {t("previous", { title: t(`articles.${previous}.title`) })}
          </Link>
        )}
        {next === undefined ? null : (
          <Link
            href={docPath(locale, next)}
            className="hc-pager-link hc-pager-next"
            rel="next"
          >
            {t("next", { title: t(`articles.${next}.title`) })}
          </Link>
        )}
      </nav>
    </article>
  );
}
