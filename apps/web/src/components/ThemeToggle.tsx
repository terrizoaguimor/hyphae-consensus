"use client";
// SPDX-License-Identifier: Apache-2.0
//
// Light/dark switch. The page colors come from the head script before first
// paint; this control shows "off" in the static HTML and takes the real state
// when the page hydrates.

import { Switch } from "@astryxdesign/core/Switch";
import { useTranslations } from "next-intl";

import { useColorMode } from "./Providers";

export function ThemeToggle() {
  const t = useTranslations("Theme");
  const { mode, setMode } = useColorMode();
  return (
    <Switch
      label={t("darkTheme")}
      value={mode === "dark"}
      onChange={(checked) => setMode(checked ? "dark" : "light")}
    />
  );
}
