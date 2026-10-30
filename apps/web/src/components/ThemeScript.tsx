// SPDX-License-Identifier: Apache-2.0
//
// The only dangerouslySetInnerHTML in the app: eslint.config.mjs turns
// react/no-danger off for this file alone, and tests/theme-script.test.ts
// checks that no other file of src/ uses it and that this file uses it once.
// The body is THEME_SCRIPT, a fixed string constant (themeScriptSource.ts):
// no prop, input or data is interpolated into it. At run time it reads the
// stored theme and the OS preference and only ever writes "light" or "dark"
// to data-theme. A static export cannot carry a per-request nonce, so the
// CSP allows this script by its sha256 hash, which scripts/csp-manifest.mjs
// computes from each exported page.

import { THEME_SCRIPT } from "./themeScriptSource";

export function ThemeScript() {
  return <script dangerouslySetInnerHTML={{ __html: THEME_SCRIPT }} />;
}
