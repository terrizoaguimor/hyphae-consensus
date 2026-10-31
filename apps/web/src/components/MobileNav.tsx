"use client";
// SPDX-License-Identifier: Apache-2.0
//
// Navigation below the desktop breakpoint: a Menu button opens a modal
// drawer (src/components/Modal.tsx) with the primary links, the language
// links and the theme switch. Following a link closes the drawer; widening
// the window past the breakpoint closes it too, since the sidebar returns.

import { Button } from "@astryxdesign/core/Button";
import { useTranslations } from "next-intl";
import { useEffect, useState } from "react";

import type { Locale } from "@/i18n/routing";

import { LocaleSwitcher } from "./LocaleSwitcher";
import { Modal } from "./Modal";
import { PrimaryNav } from "./PrimaryNav";
import { ThemeControl } from "./ThemeControl";

/** Keep in sync with the desktop breakpoint in globals.css (64rem). */
const DESKTOP_QUERY = "(min-width: 64rem)";

type MobileNavProps = {
  locale: Locale;
  endonyms: Record<Locale, string>;
};

export function MobileNav({ locale, endonyms }: MobileNavProps) {
  const t = useTranslations("Shell");
  const [isOpen, setIsOpen] = useState(false);

  useEffect(() => {
    const query = window.matchMedia(DESKTOP_QUERY);
    const onChange = () => {
      if (query.matches) {
        setIsOpen(false);
      }
    };
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);

  const close = () => setIsOpen(false);

  return (
    <div className="hc-mobile-nav">
      <button
        type="button"
        className="hc-menu-button"
        aria-haspopup="dialog"
        aria-expanded={isOpen}
        // Points at the drawer only while it is rendered (open).
        aria-controls={isOpen ? "hc-mobile-drawer" : undefined}
        onClick={() => setIsOpen(true)}
      >
        <span className="hc-menu-icon" aria-hidden="true" />
        <span>{t("menuButton")}</span>
      </button>
      <Modal
        isOpen={isOpen}
        onClose={close}
        labelledBy="hc-mobile-drawer-title"
        className="hc-dialog hc-dialog--drawer"
      >
        <div id="hc-mobile-drawer" className="hc-drawer">
          <div className="hc-drawer-header">
            <h2 id="hc-mobile-drawer-title" className="hc-dialog-title">
              {t("menuTitle")}
            </h2>
            <Button variant="ghost" label={t("menuClose")} onClick={close} />
          </div>
          <nav aria-label={t("primaryNavLabel")} className="hc-drawer-nav">
            <PrimaryNav locale={locale} onNavigate={close} />
          </nav>
          <div className="hc-drawer-section">
            <LocaleSwitcher
              current={locale}
              endonyms={endonyms}
              onNavigate={close}
            />
          </div>
          <div className="hc-drawer-section">
            <ThemeControl />
          </div>
        </div>
      </Modal>
    </div>
  );
}
