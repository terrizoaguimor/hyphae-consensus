// SPDX-License-Identifier: Apache-2.0
//
// Terms of use, Privacy notice, Limitations and About share one layout:
// title, lead, the Draft badge and notice only while LEGAL_TEXT_STATUS is "draft",
// the day the page's own text last changed (LEGAL_TEXT_CHANGED), and the
// catalog's sections (messages/legal/<locale>.json). About appends its
// source-repository section through `children`.

import { getTranslations } from "next-intl/server";
import type { ReactNode } from "react";

import type { Locale } from "@/i18n/routing";
import {
  CONTACT_EMAIL,
  HOSTING_COUNTRY,
  LEGAL_TEXT_STATUS,
  LEGAL_TEXT_CHANGED,
  type LegalPage,
} from "@/lib/site";

import { ContentSections } from "./ContentSections";
import { PageHeader } from "./PageHeader";
import { Tag } from "./Tag";

type LegalPageContentProps = {
  locale: Locale;
  page: LegalPage | "about";
  children?: ReactNode;
};

export async function LegalPageContent({
  locale,
  page,
  children,
}: LegalPageContentProps) {
  const t = await getTranslations({ locale });
  const isDraft = LEGAL_TEXT_STATUS === "draft";
  const namespace =
    page === "about" ? ("About" as const) : (`Legal.${page}` as const);
  return (
    <>
      <PageHeader
        title={t(`Nav.${page}`)}
        lead={t(`${namespace}.lead` as const)}
        eyebrow={
          isDraft ? (
            <Tag variant="warning" label={t("Legal.draftBadge")} />
          ) : undefined
        }
      >
        {isDraft ? (
          <p className="hc-draft-notice">{t("Legal.draftNotice")}</p>
        ) : null}
        <p className="hc-supporting">
          {t("Legal.updated", { date: LEGAL_TEXT_CHANGED[page] })}
        </p>
      </PageHeader>
      <ContentSections
        locale={locale}
        namespace={namespace}
        idPrefix={page}
        // Fixed constants the privacy notice names; harmless elsewhere.
        values={{ contact: CONTACT_EMAIL, hostingCountry: HOSTING_COUNTRY }}
      />
      {children}
    </>
  );
}
