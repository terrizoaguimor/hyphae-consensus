// SPDX-License-Identifier: Apache-2.0
//
// A full-width callout: an icon, a short title in words and the body, in one
// of four roles (failed, contested, success, info). The words carry the
// meaning; the icon and the bar's color only repeat it (no meaning by color
// alone). For a state of the page the reader must not miss (a chain broken
// in this browser, labels that disagree, a manifest that does not match),
// instead of one more equal tag in a row. Not a live region: whatever made
// it appear announces itself. The icons are inline SVG (no style injection,
// hash-only CSP), hidden from assistive technology.

import type { ReactNode } from "react";

export type CalloutTone = "failed" | "contested" | "success" | "info";

type DataAttributes = { [key: `data-${string}`]: string | number | boolean | undefined };

type CalloutProps = {
  tone: CalloutTone;
  title: ReactNode;
  children?: ReactNode;
} & DataAttributes;

/** The icon of each role, drawn on a 20-unit grid with square joins. */
function CalloutIcon({ tone }: { tone: CalloutTone }) {
  return (
    <svg
      className="hc-callout-icon"
      viewBox="0 0 20 20"
      width="20"
      height="20"
      aria-hidden="true"
      focusable="false"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="square"
      strokeLinejoin="miter"
    >
      {tone === "failed" ? (
        // An octagon with a cross: stop.
        <>
          <path d="M6.5 1.5h7l5 5v7l-5 5h-7l-5-5v-7z" />
          <path d="M7 7l6 6M13 7l-6 6" />
        </>
      ) : tone === "contested" ? (
        // A triangle with an exclamation mark: look closer.
        <>
          <path d="M10 1.5l8.5 16h-17z" />
          <path d="M10 7.5v4.5M10 15v0.5" />
        </>
      ) : tone === "success" ? (
        // A square with a check mark.
        <>
          <path d="M2 2h16v16H2z" />
          <path d="M6 10.5l3 3 5-7" />
        </>
      ) : (
        // A square with an "i".
        <>
          <path d="M2 2h16v16H2z" />
          <path d="M10 9v5.5M10 5.5v0.5" />
        </>
      )}
    </svg>
  );
}

export function Callout({ tone, title, children, ...data }: CalloutProps) {
  return (
    <div className="hc-callout" data-callout-tone={tone} {...data}>
      <CalloutIcon tone={tone} />
      <div className="hc-callout-body">
        <p className="hc-callout-title">{title}</p>
        {children}
      </div>
    </div>
  );
}
