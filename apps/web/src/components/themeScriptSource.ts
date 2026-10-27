// SPDX-License-Identifier: Apache-2.0

/** localStorage key holding the visitor's explicit theme choice. */
export const THEME_STORAGE_KEY = "hc-theme";

/**
 * Body of the inline <head> script that sets data-theme on <html> before
 * first paint: the stored choice when it is "light" or "dark", otherwise the
 * OS preference. One line on purpose: its sha256 is pinned in each page's CSP
 * (scripts/csp-manifest.mjs), and a line-ending change would alter the hash.
 * Keep the key literal in sync with THEME_STORAGE_KEY (a test checks it).
 */
export const THEME_SCRIPT =
  '(function(){var d=document.documentElement,m=null;try{m=localStorage.getItem("hc-theme")}catch(e){}if(m!=="light"&&m!=="dark"){m=window.matchMedia&&window.matchMedia("(prefers-color-scheme: dark)").matches?"dark":"light"}d.setAttribute("data-theme",m)})();';
