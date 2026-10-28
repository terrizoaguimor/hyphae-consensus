"use client";
// SPDX-License-Identifier: Apache-2.0
//
// Focus management on client-side navigation. Next.js announces the new
// page title (its route announcer) but leaves focus where it was, often on a
// link that no longer exists. After every navigation, focus moves to the new
// page's <h1> (tabindex="-1"), so keyboard and screen-reader users start at
// the top of the new content. The first page load keeps the browser's
// default (focus at the start of the document, where the skip link is).
//
// The [locale] root layout remounts when the language changes, so whether a
// load has already happened is kept at module level, which lives as long as
// the page's JavaScript does.

import { usePathname } from "next/navigation";
import { useEffect, useRef } from "react";

import { focusPageHeading } from "./Modal";

let loadedBefore = false;

export function RouteFocus() {
  const pathname = usePathname();
  const previous = useRef<string | null>(null);

  useEffect(() => {
    if (previous.current === pathname) {
      // Same page (an effect re-run in development's Strict Mode).
      return;
    }
    const isFirstRender = previous.current === null;
    previous.current = pathname;
    if (isFirstRender && !loadedBefore) {
      loadedBefore = true;
      return;
    }
    loadedBefore = true;
    // The new page is committed when this effect runs; wait one frame so
    // the router's own scroll handling has finished.
    const frame = requestAnimationFrame(focusPageHeading);
    return () => cancelAnimationFrame(frame);
  }, [pathname]);

  return null;
}
