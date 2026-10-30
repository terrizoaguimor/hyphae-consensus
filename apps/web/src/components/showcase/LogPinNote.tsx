"use client";
// SPDX-License-Identifier: Apache-2.0
//
// A log file's SHA-256 as GET /showcase lists it, cross-checked here against
// the manifest this browser checked (src/lib/showcase.ts logPin): the same
// pin, another pin (said as a mismatch, with both digests), a file the
// checked manifest does not name, a manifest still being checked (a loading
// line), or no checked manifest to compare with.

import { Badge } from "@astryxdesign/core/Badge";
import { useTranslations } from "next-intl";

import { digestPrefix } from "@/lib/run/evidence";
import type { LogPin } from "@/lib/showcase";

export function LogPinNote({ pin }: { pin: LogPin }) {
  const t = useTranslations("Showcase.pin");
  return (
    <p
      className={pin.status === "differs" ? undefined : "hc-supporting"}
      data-log-pin={pin.status}
      role={pin.status === "checking" ? "status" : undefined}
    >
      {pin.status === "differs" ? (
        <>
          <Badge variant="error" label={t("differsBadge")} />{" "}
          {t("differs", {
            sha256: digestPrefix(pin.sha256) ?? pin.sha256,
            listing: digestPrefix(pin.listing) ?? pin.listing,
          })}
        </>
      ) : (
        t(pin.status)
      )}
    </p>
  );
}
