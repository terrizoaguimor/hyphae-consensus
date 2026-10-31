// SPDX-License-Identifier: Apache-2.0
//
// A status tag: Astryx's Badge with its role drawn without hue (palette-mono
// 2.1, globals.css "Status without hue") and its icon. The variant still
// names what a view means (src/lib/provenance.ts); `tagLook` turns it into a
// role (`data-tag-role`, which the role hooks of globals.css read: RECORDED's
// own mid tint, the "checked" hollow, the pending bar) and an icon from that
// role's family. A view may name a role or an icon, but only one its
// variant allows: no tag can be drawn verified, LIVE or simulated by an
// override. LIVE has no icon: its 8px square is the mark. Tags are
// rectangles (radius 2), never pills.

import { Badge } from "@astryxdesign/core/Badge";
import type { ReactNode } from "react";

import { type TagIconName, tagLook, type TagRole, type TagVariant } from "@/lib/provenance";

import { Icon } from "./icons";

type DataAttributes = { [key: `data-${string}`]: string | number | boolean | undefined };

type TagProps = {
  variant: TagVariant;
  label: ReactNode;
  /** A role other than the variant's default, when the variant allows it (`VARIANT_ALLOWED_ROLES`). */
  role?: TagRole;
  /** An icon of the role's family other than its default. */
  icon?: TagIconName;
  /** The `Provenance.<key>` the label was translated from: it picks the label's own role and icon. */
  labelKey?: string;
  className?: string;
} & DataAttributes;

export function Tag({ variant, label, role, icon, labelKey, className, ...data }: TagProps) {
  const look = tagLook(variant, { role, icon, key: labelKey });
  return (
    <Badge
      variant={variant}
      label={label}
      className={className}
      icon={look.icon === null ? undefined : <Icon name={look.icon} size={12} className="hc-tag-icon" />}
      data-tag-role={look.role}
      {...data}
    />
  );
}
