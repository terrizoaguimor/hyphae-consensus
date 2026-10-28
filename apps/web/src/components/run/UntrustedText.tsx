"use client";
// SPDX-License-Identifier: Apache-2.0
//
// Text that comes from the run, not from this app: a retrieved record (data
// to read, never instructions; it may even try to give some, like the hostile
// note of the injection runs) or a model's output. It is rendered as plain
// text only (React escapes it; no markup, no links), marked lang="en" because
// every recorded text is English, and introduced by a note that says what it
// is; in another locale the note adds that the text is the English original.

import { useLocale, useTranslations } from "next-intl";

type UntrustedTextProps = {
  text: string;
  /** A retrieved record's content, or a model's output. */
  source: "record" | "model";
};

export function UntrustedText({ text, source }: UntrustedTextProps) {
  const t = useTranslations("Run.content");
  const locale = useLocale();
  const english = locale !== "en";
  return (
    <figure className="hc-untrusted" data-untrusted={source}>
      <figcaption className="hc-supporting">
        {source === "record" ? t("recordNote") : t("modelNote")}
        {english ? ` ${source === "record" ? t("originalRecord") : t("originalModel")}` : null}
      </figcaption>
      <blockquote lang="en" className="hc-untrusted-text">
        {text}
      </blockquote>
    </figure>
  );
}
