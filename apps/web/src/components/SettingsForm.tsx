"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The Settings page's controls. Every change applies at once, is saved in
// localStorage only, and is confirmed in a polite live region (visible as
// well). A language change opens Settings in the new language, which then
// confirms the change in that language (a one-shot "notice" key).

import { Button } from "@astryxdesign/core/Button";
import { RadioList, RadioListItem } from "@astryxdesign/core/RadioList";
import { useTranslations } from "next-intl";
import { useEffect, useRef, useState } from "react";

import type { Locale } from "@/i18n/routing";
import {
  explanationLevels,
  motionPreferences,
  parseExplanation,
  parseMotion,
  parseTheme,
  readStorage,
  STORAGE_KEYS,
  themePreferences,
  writeStorage,
} from "@/lib/preferences";

import { useConnectionState } from "./ConnectionText";
import { LocaleSwitcher } from "./LocaleSwitcher";
import { usePreferences } from "./preferences";

type SettingsFormProps = {
  locale: Locale;
  endonyms: Record<Locale, string>;
};

export function SettingsForm({ locale, endonyms }: SettingsFormProps) {
  const t = useTranslations("Settings");
  const {
    theme,
    motion,
    explanation,
    setTheme,
    setMotion,
    setExplanation,
    resetAll,
  } = usePreferences();
  const connection = useConnectionState();
  const [status, setStatus] = useState("");
  // null until the one-shot notice key was read (once per mount).
  const languageNotice = useRef<boolean | null>(null);

  // Confirm a language change made on the previous page (another locale).
  useEffect(() => {
    if (languageNotice.current === null) {
      languageNotice.current = readStorage(STORAGE_KEYS.notice) === "locale";
      if (languageNotice.current) {
        writeStorage(STORAGE_KEYS.notice, null);
      }
    }
    if (!languageNotice.current) {
      return;
    }
    // After the route announcement, into a region that is already mounted.
    // A re-run (new translator) reschedules; the notice is consumed once.
    const timer = window.setTimeout(() => {
      languageNotice.current = false;
      setStatus(t("feedback.language", { value: endonyms[locale] }));
    }, 300);
    return () => window.clearTimeout(timer);
  }, [endonyms, locale, t]);

  return (
    <div className="hc-settings">
      <section className="hc-panel hc-glass hc-glass--static">
        <h2 id="settings-theme" className="hc-panel-title">
          {t("theme.label")}
        </h2>
        <RadioList
          label={t("theme.label")}
          isLabelHidden
          value={theme}
          onChange={(value) => {
            const next = parseTheme(value);
            setTheme(next);
            setStatus(
              t("feedback.theme", { value: t(`theme.options.${next}`) }),
            );
          }}
        >
          {themePreferences.map((option) => (
            <RadioListItem
              key={option}
              value={option}
              label={t(`theme.options.${option}`)}
            />
          ))}
        </RadioList>
      </section>

      <section className="hc-panel hc-glass hc-glass--static">
        <h2 id="settings-language" className="hc-panel-title">
          {t("language.label")}
        </h2>
        <p className="hc-panel-description">{t("language.description")}</p>
        <LocaleSwitcher
          current={locale}
          endonyms={endonyms}
          landmark={false}
          className="hc-language-list"
          onNavigate={(target) => {
            if (target !== locale) {
              writeStorage(STORAGE_KEYS.notice, "locale");
            }
          }}
        />
      </section>

      <section className="hc-panel hc-glass hc-glass--static">
        <h2 id="settings-motion" className="hc-panel-title">
          {t("motion.label")}
        </h2>
        <RadioList
          label={t("motion.label")}
          isLabelHidden
          value={motion}
          onChange={(value) => {
            const next = parseMotion(value);
            setMotion(next);
            setStatus(
              t("feedback.motion", { value: t(`motion.options.${next}`) }),
            );
          }}
        >
          {motionPreferences.map((option) => (
            <RadioListItem
              key={option}
              value={option}
              label={t(`motion.options.${option}`)}
              description={t(`motion.descriptions.${option}`)}
            />
          ))}
        </RadioList>
      </section>

      <section className="hc-panel hc-glass hc-glass--static">
        <h2 id="settings-explanation" className="hc-panel-title">
          {t("explanation.label")}
        </h2>
        <RadioList
          label={t("explanation.label")}
          isLabelHidden
          value={explanation}
          onChange={(value) => {
            const next = parseExplanation(value);
            setExplanation(next);
            setStatus(
              t("feedback.explanation", {
                value: t(`explanation.options.${next}`),
              }),
            );
          }}
        >
          {explanationLevels.map((option) => (
            <RadioListItem
              key={option}
              value={option}
              label={t(`explanation.options.${option}`)}
              description={
                option === "technical" ? (
                  // What the level reaches depends on the connection: no
                  // run is explained when the server did not answer (HE #8).
                  <span data-connection-copy={connection}>
                    {connection === "unreachable"
                      ? t("explanation.descriptions.technical")
                      : t("explanation.technicalConnected")}
                  </span>
                ) : (
                  t(`explanation.descriptions.${option}`)
                )
              }
            />
          ))}
        </RadioList>
      </section>

      <section className="hc-panel hc-glass hc-glass--static">
        <h2 id="settings-reset" className="hc-panel-title">
          {t("reset.heading")}
        </h2>
        <p className="hc-panel-description">{t("reset.description")}</p>
        <div className="hc-actions">
          <Button
            variant="secondary"
            label={t("reset.button")}
            onClick={() => {
              resetAll();
              setStatus(t("feedback.reset"));
            }}
          />
        </div>
      </section>

      <p role="status" className="hc-status">
        {status}
      </p>
    </div>
  );
}
