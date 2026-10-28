"use client";
// SPDX-License-Identifier: Apache-2.0
//
// Light/dark switch in the header (Settings also offers "match system").
// The page colors come from the head script before first paint; this control
// shows "off" in the static HTML and takes the real state when the page
// hydrates.

import { Switch } from "@astryxdesign/core/Switch";
import { useTranslations } from "next-intl";

import { usePreferences } from "./preferences";

export function ThemeToggle() {
  const t = useTranslations("Theme");
  const { colorMode, setTheme } = usePreferences();
  return (
    <Switch
      label={t("darkTheme")}
      value={colorMode === "dark"}
      onChange={(checked) => setTheme(checked ? "dark" : "light")}
    />
  );
}
