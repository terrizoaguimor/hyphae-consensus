// SPDX-License-Identifier: Apache-2.0
//
// The only dangerouslySetInnerHTML in the app (ESLint allows react/no-danger
// in this file alone, and tests/no-danger.test.ts keeps it that way). The body
// is a fixed constant, never derived from input. A static export cannot carry
// a per-request nonce, so the CSP allows this script by its sha256 hash.

import { THEME_SCRIPT } from "./themeScriptSource";

export function ThemeScript() {
  return <script dangerouslySetInnerHTML={{ __html: THEME_SCRIPT }} />;
}
