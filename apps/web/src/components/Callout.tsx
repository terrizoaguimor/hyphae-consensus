// SPDX-License-Identifier: Apache-2.0
//
// A full-width callout: an icon, a short title in words and the body, in one
// of four roles (failed, contested, success, info). The words carry the
// meaning; the icon and the frame only repeat it (no meaning by a shade of
// grey alone). For a state of the page the reader must not miss (a chain broken
// in this browser, labels that disagree, a manifest that does not match),
// instead of one more equal tag in a row. Not a live region: whatever made
// it appear announces itself. The icons are inline SVG (src/components/icons:
// no style injection, hash-only CSP), hidden from assistive technology.
//
// A failed callout is the loudest element (palette-mono 2.3): a 2px ink frame
// and an inverted title band, with its icon inside the band (unlink for a
// broken chain, file-x for a manifest that differs, square-x otherwise).
// The other tones keep the icon beside the body.

import type { ReactNode } from "react";

import type { TagIconName } from "@/lib/provenance";

import { Icon } from "./icons";

export type CalloutTone = "failed" | "contested" | "success" | "info";

type DataAttributes = { [key: `data-${string}`]: string | number | boolean | undefined };

type CalloutProps = {
  tone: CalloutTone;
  title: ReactNode;
  /** The tone's icon family member that names this callout's subject (a failed one: unlink, file-x). */
  icon?: TagIconName;
  children?: ReactNode;
} & DataAttributes;

/** Each tone's icons, its default first; an icon outside the family falls back to the default. */
const TONE_ICONS: Record<CalloutTone, readonly TagIconName[]> = {
  failed: ["square-x", "unlink", "file-x", "shield-x"],
  contested: ["triangle-alert", "file-question-mark"],
  success: ["circle-dashed"],
  info: ["info"],
};

function toneIcon(tone: CalloutTone, icon: TagIconName | undefined): TagIconName {
  const family = TONE_ICONS[tone];
  return icon !== undefined && family.includes(icon) ? icon : (family[0] as TagIconName);
}

export function Callout({ tone, title, icon, children, ...data }: CalloutProps) {
  const name = toneIcon(tone, icon);
  return (
    <div className="hc-callout" data-callout-tone={tone} {...data}>
      {tone === "failed" ? null : <Icon name={name} size={20} className="hc-callout-icon" />}
      <div className="hc-callout-body">
        {tone === "failed" ? (
          // The icon sits inside the inverted band, in the band's own (inverted) ink.
          <p className="hc-callout-title">
            <Icon name={name} size={16} className="hc-callout-title-icon" />
            <span>{title}</span>
          </p>
        ) : (
          <p className="hc-callout-title">{title}</p>
        )}
        {children}
      </div>
    </div>
  );
}
