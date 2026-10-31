// SPDX-License-Identifier: Apache-2.0
//
// Status tags without hue (design/palette-mono.md 2.1 and 2.3): the variant
// to role mapping of src/lib/provenance.ts, the icon families, the Tag and
// Callout components rendered to HTML, and the role hooks of globals.css
// (RECORDED's own mid tint, the simulated hatch, the pending bar, the LIVE
// pulse, the 2px radius). The honesty lines are the ones of
// tests/provenance.test.ts, held on the role a tag is drawn as: nothing
// but a real verifier's accepted proof is drawn verified, nothing simulated
// gets the verified shield or loses its hatch, LIVE is the LIVE labels'
// alone, and RECORDED no longer looks like THIS SERVER.

import { readFileSync } from "node:fs";

import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { Callout } from "../src/components/Callout";
import { ICON_SHAPES } from "../src/components/icons";
import { Tag } from "../src/components/Tag";
import {
  assessmentVariant,
  BLOCKING_FLAGS,
  DECISION_ICONS,
  deliveryLabel,
  evidenceLabel,
  flagLabel,
  LIVE_VARIANT,
  MODEL_PROVENANCES,
  modelLabel,
  OUTCOME_ICONS,
  proofToken,
  type ProvenanceLabel,
  RECOMMENDATION_VARIANTS,
  ROLE_ICONS,
  sourceLabel,
  TAG_ROLES,
  type TagIconName,
  tagLook,
  type TagRole,
  type TagVariant,
  VARIANT_ALLOWED_ROLES,
  VARIANT_ROLES,
} from "../src/lib/provenance";

import { declarations } from "./colour-scan";

const VARIANTS = Object.keys(VARIANT_ROLES) as TagVariant[];
const STATUSES = ["valid", "invalid", "unavailable", "verification_error", "not_checked"] as const;
const EVIDENCES = ["simulated", "hyphae_snapshot", "hyphae_live", "none", "mixed"] as const;

/** Every label provenance.ts hands a view, with the key it is translated from. */
function everyLabel(): ProvenanceLabel[] {
  const flagSets = [[], ...BLOCKING_FLAGS.map((flag) => [flag])];
  return [
    ...MODEL_PROVENANCES.flatMap((model) =>
      flagSets.flatMap((flags) =>
        (["live", "replay", undefined] as const).flatMap((delivery) => [
          modelLabel({ model, flags }, delivery),
          ...(delivery === undefined ? [] : [deliveryLabel(delivery, { model, flags })]),
        ]),
      ),
    ),
    ...EVIDENCES.map(evidenceLabel),
    sourceLabel("this_server"),
    sourceLabel("research_archive"),
    ...BLOCKING_FLAGS.map(flagLabel),
    ...STATUSES.flatMap((status) => [proofToken(status, true), proofToken(status, false)]),
  ];
}

describe("the variant to role mapping", () => {
  it("every variant a view uses is drawn as one role, and every role is reachable", () => {
    expect(VARIANT_ROLES).toEqual({
      success: "verified",
      error: "failed",
      purple: "simulated",
      pink: "live",
      warning: "contested",
      neutral: "info",
      info: "info",
      blue: "info",
    });
    const reachable = new Set(VARIANTS.flatMap((variant) => VARIANT_ALLOWED_ROLES[variant]));
    expect([...reachable].sort()).toEqual([...TAG_ROLES].sort());
    for (const variant of VARIANTS) {
      // The default role comes first in the allowed list.
      expect(VARIANT_ALLOWED_ROLES[variant][0], variant).toBe(VARIANT_ROLES[variant]);
    }
  });

  it("only the success variant can be drawn verified, only LIVE's live, only the simulated one simulated", () => {
    for (const variant of VARIANTS) {
      const allowed = VARIANT_ALLOWED_ROLES[variant];
      expect(allowed.includes("verified"), variant).toBe(variant === "success");
      expect(allowed.includes("live"), variant).toBe(variant === LIVE_VARIANT);
      expect(allowed.includes("simulated"), variant).toBe(variant === "purple");
      expect(allowed.includes("failed"), variant).toBe(variant === "error");
    }
    // The success variant may only get quieter: checked, never anything louder.
    expect(VARIANT_ALLOWED_ROLES.success).toEqual(["verified", "checked"]);
  });

  it("a role a variant does not allow is ignored, whatever the view asks for", () => {
    for (const variant of VARIANTS) {
      for (const role of TAG_ROLES) {
        const look = tagLook(variant, { role });
        expect(VARIANT_ALLOWED_ROLES[variant], `${variant} as ${role}`).toContain(look.role);
        if (!VARIANT_ALLOWED_ROLES[variant].includes(role)) {
          expect(look.role, `${variant} as ${role}`).toBe(VARIANT_ROLES[variant]);
        }
      }
    }
    expect(tagLook("purple", { role: "verified" }).role).toBe("simulated");
    expect(tagLook("neutral", { role: "verified" }).role).toBe("info");
    expect(tagLook("info", { role: "live" }).role).toBe("info");
    expect(tagLook("success", { role: "checked" }).role).toBe("checked");
    expect(tagLook("blue", { role: "pending" }).role).toBe("pending");
  });

  it("RECORDED has its own role; THIS SERVER and the evidence labels stay informational", () => {
    const recorded = sourceLabel("research_archive");
    expect(tagLook(recorded.variant, { key: recorded.key })).toEqual({ role: "recorded", icon: "archive" });
    const server = sourceLabel("this_server");
    expect(tagLook(server.variant, { key: server.key })).toEqual({ role: "info", icon: "server" });
    for (const evidence of EVIDENCES) {
      const label = evidenceLabel(evidence);
      expect(tagLook(label.variant, { key: label.key }).role, evidence).not.toBe("recorded");
    }
    // The RECORDED variant is shared, its role is not: the same variant without the key is info.
    expect(tagLook(recorded.variant).role).toBe("info");
  });

  it("the LIVE role belongs to the LIVE labels alone, and they carry no icon (the square is the mark)", () => {
    const labels = everyLabel();
    const live = labels.filter((label) => tagLook(label.variant, { key: label.key }).role === "live");
    expect(new Set(live.map((label) => label.key))).toEqual(new Set(["live.label", "liveMeta.label"]));
    for (const label of live) {
      expect(tagLook(label.variant, { key: label.key }).icon).toBeNull();
    }
    expect(ROLE_ICONS.live).toEqual([]);
  });

  it("only a real verifier's accepted proof is drawn verified; a simulated one is simulated with the flask", () => {
    const verified = STATUSES.flatMap((status) =>
      [true, false]
        .map((simulated) => ({ status, simulated, token: proofToken(status, simulated) }))
        .filter(({ token }) => tagLook(token.variant, { key: token.key }).role === "verified"),
    );
    expect(verified.map(({ status, simulated }) => ({ status, simulated }))).toEqual([
      { status: "valid", simulated: false },
    ]);
    for (const status of STATUSES) {
      const token = proofToken(status, true);
      // The key names "proof.valid", but the shield is not in the simulated family.
      expect(tagLook(token.variant, { key: token.key }), status).toEqual({ role: "simulated", icon: "flask-conical" });
    }
    expect(tagLook("success", { key: "proof.valid" }).icon).toBe("shield-check");
    expect(tagLook("error", { key: "proof.invalid" })).toEqual({ role: "failed", icon: "shield-x" });
  });

  it("no outcome or assessment is drawn verified, and an outcome never carries a check", () => {
    for (const [kind, variant] of Object.entries(RECOMMENDATION_VARIANTS)) {
      const look = tagLook(variant, { icon: OUTCOME_ICONS[kind as keyof typeof OUTCOME_ICONS] });
      expect(look.role, kind).not.toBe("verified");
      expect(look.role, kind).not.toBe("checked");
      expect(look.icon, kind).not.toBe("shield-check");
      // Each outcome's own icon is in its role's family, so it is the one drawn.
      expect(look.icon, kind).toBe(OUTCOME_ICONS[kind as keyof typeof OUTCOME_ICONS]);
    }
    expect(OUTCOME_ICONS.supported).toBe("circle-dot");
    for (const corroborated of [true, false]) {
      expect(tagLook(assessmentVariant(corroborated)).role).toBe("info");
    }
  });
});

describe("the icon families", () => {
  it("no icon outline is shared by two roles, and the check glyph is the verified role's alone", () => {
    const seen = new Map<TagIconName, TagRole>();
    for (const role of TAG_ROLES) {
      for (const icon of ROLE_ICONS[role]) {
        expect(seen.get(icon), `${icon} in ${role} and ${String(seen.get(icon))}`).toBeUndefined();
        seen.set(icon, role);
      }
    }
    expect(seen.get("shield-check")).toBe("verified");
    expect(ROLE_ICONS.verified).toEqual(["shield-check"]);
    // Every icon a family names has a drawn shape, and nothing is drawn that no family names.
    expect(Object.keys(ICON_SHAPES).sort()).toEqual([...seen.keys()].sort());
  });

  it("every label resolves to an icon of its own role's family (none for LIVE)", () => {
    for (const label of everyLabel()) {
      const look = tagLook(label.variant, { key: label.key });
      if (look.role === "live") {
        expect(look.icon, label.key).toBeNull();
      } else {
        expect(ROLE_ICONS[look.role], label.key).toContain(look.icon);
      }
    }
  });

  it("a recorded decision is an informational tag told apart by its own icon, never a louder fill", () => {
    for (const [decision, icon] of Object.entries(DECISION_ICONS)) {
      expect(tagLook("info", { icon }), decision).toEqual({ role: "info", icon });
      expect(icon, decision).not.toBe(tagLook("neutral", { key: "thisServer.label" }).icon);
    }
    expect(DECISION_ICONS.rejected).not.toBe(DECISION_ICONS.approved);
  });

  it("an icon outside the role's family falls back to the role's own", () => {
    expect(tagLook("purple", { icon: "shield-check" }).icon).toBe("flask-conical");
    expect(tagLook("success", { role: "checked", icon: "shield-check" }).icon).toBe("circle-dashed");
    expect(tagLook("pink", { icon: "info" }).icon).toBeNull();
    expect(tagLook("warning", { icon: "link-2-off" }).icon).toBe("link-2-off");
    expect(tagLook("error", { icon: "unlink" }).icon).toBe("unlink");
  });
});

describe("Tag and Callout, rendered", () => {
  const tag = (props: Parameters<typeof Tag>[0]) => renderToStaticMarkup(createElement(Tag, props));

  it("a tag names its role and carries its icon, aria-hidden, at 12px", () => {
    const html = tag({ variant: "success", label: "Verified at run time" });
    expect(html).toContain('data-variant="success"');
    expect(html).toContain('data-tag-role="verified"');
    expect(html).toMatch(/<svg[^>]*aria-hidden="true"[^>]*data-icon="shield-check"/);
    expect(html).toMatch(/<svg[^>]*width="12"[^>]*height="12"/);
    expect(html).not.toContain("style=");
  });

  it("RECORDED and THIS SERVER are told apart by role and icon", () => {
    const recorded = sourceLabel("research_archive");
    const server = sourceLabel("this_server");
    const a = tag({ variant: recorded.variant, labelKey: recorded.key, label: "RECORDED" });
    const b = tag({ variant: server.variant, labelKey: server.key, label: "THIS SERVER" });
    expect(a).toContain('data-tag-role="recorded"');
    expect(a).toContain('data-icon="archive"');
    expect(b).toContain('data-tag-role="info"');
    expect(b).toContain('data-icon="server"');
  });

  it("LIVE has no icon; a simulated proof asking for the shield gets the flask", () => {
    expect(tag({ variant: LIVE_VARIANT, label: "LIVE" })).not.toContain("<svg");
    const simulated = tag({ variant: "purple", icon: "shield-check", labelKey: "proof.valid", label: "SIMULATED" });
    expect(simulated).toContain('data-tag-role="simulated"');
    expect(simulated).toContain('data-icon="flask-conical"');
    expect(simulated).not.toContain("shield-check");
  });

  it("a failed callout carries its icon inside its inverted title band; the others beside the body", () => {
    const failed = renderToStaticMarkup(createElement(Callout, { tone: "failed", icon: "unlink", title: "Chain broken" }));
    expect(failed).toMatch(
      /<p class="hc-callout-title"><svg[^>]*class="hc-callout-title-icon"[^>]*aria-hidden="true"[^>]*data-icon="unlink"[^>]*>.*<\/svg><span>Chain broken<\/span><\/p>/,
    );
    // Exactly one icon, the band's.
    expect(failed.match(/<svg/g)).toHaveLength(1);
    const manifest = renderToStaticMarkup(createElement(Callout, { tone: "failed", icon: "file-x", title: "Differs" }));
    expect(manifest).toContain('data-icon="file-x"');
    const plain = renderToStaticMarkup(createElement(Callout, { tone: "failed", title: "Failed" }));
    expect(plain).toContain('data-icon="square-x"');
    const contested = renderToStaticMarkup(
      createElement(Callout, { tone: "contested", icon: "file-x", title: "Not confirmed" }),
    );
    // Outside its family the icon falls back to the tone's own, and sits before the body, not in the title.
    expect(contested).toMatch(/^<div class="hc-callout" data-callout-tone="contested"><svg[^>]*data-icon="triangle-alert"/);
    expect(contested).toContain('<p class="hc-callout-title">Not confirmed</p>');
  });
});

describe("the role hooks of globals.css", () => {
  const globals = declarations(readFileSync(new URL("../src/app/globals.css", import.meta.url), "utf8"));

  /** The declarations of the rules whose selector list includes `own`, in or out of @media (by `media`). */
  function rulesOf(own: string, media?: string): Record<string, string> {
    const result: Record<string, string> = {};
    for (const { selector, property, value, context } of globals) {
      const inMedia = context.find((entry) => entry.startsWith("@media"));
      if ((media === undefined && inMedia !== undefined) || (media !== undefined && inMedia !== media)) {
        continue;
      }
      if (selector.split(",").map((part) => part.trim().replace(/\s+/g, " ")).includes(own)) {
        result[property] = value;
      }
    }
    return result;
  }

  const variant = (name: string) => rulesOf(`.astryx-badge[data-variant="${name}"]`);

  it("RECORDED has its own mid tint; the neutral labels take the informational tint", () => {
    const recorded = rulesOf('.astryx-badge[data-tag-role="recorded"]');
    expect(recorded).toEqual({
      color: "var(--hc-role-recorded-fg)",
      "background-color": "var(--hc-role-recorded-bg)",
      outline: "1px solid var(--hc-role-recorded-border)",
      "outline-offset": "-1px",
    });
    expect(variant("neutral")["background-color"]).toBe("var(--hc-role-info-bg)");
    expect(variant("neutral")).toEqual(variant("info"));
    expect(variant("neutral")["background-color"]).not.toBe(recorded["background-color"]);
  });

  it("simulated is the only tag with the pixel hatch strip, inside its dashed line", () => {
    const purple = variant("purple");
    expect(purple["background-image"]).toBe("var(--hc-hatch)");
    expect(purple["background-size"]).toBe("4px 4px");
    expect(purple["background-repeat"]).toBe("repeat-y");
    expect(purple["image-rendering"]).toBe("pixelated");
    expect(purple.outline).toBe("1px dashed var(--hc-role-simulated-border)");
    for (const name of ["success", "error", "pink", "warning", "neutral", "info", "blue"]) {
      expect(variant(name)["background-image"], name).toBeUndefined();
    }
    for (const role of ["recorded", "checked", "pending"]) {
      expect(rulesOf(`.astryx-badge[data-tag-role="${role}"]`)["background-image"], role).toBeUndefined();
    }
    // Forced colours drop the hatch and keep the dashed line.
    expect(rulesOf('.astryx-badge[data-variant="purple"]', "@media (forced-colors: active)")["background-image"]).toBe(
      "none",
    );
  });

  it("pending is a 2px frame on the strong tint with the 3px ink bar, not the LIVE ink frame", () => {
    const pending = rulesOf('.astryx-badge[data-tag-role="pending"]');
    expect(pending["background-color"]).toBe("var(--hc-role-pending-bg)");
    expect(pending.outline).toBe("2px solid var(--hc-role-pending-border)");
    expect(pending["border-inline-start"]).toBe("3px solid var(--hc-accent-fg)");
    expect(pending.outline).not.toBe(variant(LIVE_VARIANT).outline);
    expect(rulesOf('.astryx-badge[data-tag-role="pending"]::before')).toEqual({});
  });

  it("the checked hook is the hollow checked look, never the verified solid", () => {
    const checked = rulesOf('.astryx-badge[data-tag-role="checked"]');
    expect(checked).toEqual(rulesOf('[data-chain-check] .astryx-badge[data-variant="success"]'));
    expect(checked["background-color"]).toBe("var(--hc-role-checked-bg)");
    expect(checked["background-color"]).not.toBe(variant("success")["background-color"]);
    // In forced colours it is not an inverted block either.
    expect(rulesOf('.astryx-badge[data-tag-role="checked"]', "@media (forced-colors: active)")["background-color"]).toBe(
      "Canvas",
    );
  });

  it("the LIVE square pulses 3 times and stops, and never under reduced motion", () => {
    const pulse = rulesOf('.astryx-badge[data-variant="pink"]::before', "@media (prefers-reduced-motion: no-preference)");
    expect(pulse["animation-name"]).toBe("hc-live-pulse");
    expect(pulse["animation-iteration-count"]).toBe("3");
    expect(pulse["animation-duration"]).toBe("1.6s");
    // Outside the no-preference query the square does not move.
    expect(rulesOf('.astryx-badge[data-variant="pink"]::before')["animation-name"]).toBeUndefined();
    expect(rulesOf('html[data-motion="reduce"] .astryx-badge[data-variant="pink"]::before')["animation-name"]).toBe(
      "none",
    );
    const frames = globals.filter(({ context }) => context.some((entry) => entry === "@keyframes hc-live-pulse"));
    expect(frames.map(({ selector, property, value }) => `${selector} ${property}: ${value}`)).toEqual([
      "50% opacity: 0.6",
    ]);
  });

  it("tags are rectangles of radius 2, never pills", () => {
    const theme = readFileSync(new URL("../src/theme/hyphae-mission.css", import.meta.url), "utf8");
    expect(theme).toMatch(/--radius-inner: 2px;/);
    expect(rulesOf(".astryx-badge")["border-radius"]).toBe("var(--radius-inner)");
    expect(rulesOf("[data-recommendation-kind] .astryx-badge")["border-radius"]).toBe("var(--radius-inner)");
  });
});

describe("the hatch tiles", () => {
  it.each(["hatch-light.png", "hatch-dark.png"])("%s is a 4 x 4 grey PNG (no hue possible)", (name) => {
    const bytes = readFileSync(new URL(`../public/brand/${name}`, import.meta.url));
    expect(bytes.subarray(1, 4).toString("latin1")).toBe("PNG");
    expect(bytes.subarray(12, 16).toString("latin1")).toBe("IHDR");
    expect(bytes.readUInt32BE(16)).toBe(4);
    expect(bytes.readUInt32BE(20)).toBe(4);
    // Colour type 0 (grey) or 4 (grey and alpha): the format cannot carry a hue.
    expect([0, 4]).toContain(bytes[25]);
  });

  it("the tokens point at those tiles", () => {
    const tokens = readFileSync(new URL("../src/app/hc-mono-tokens.css", import.meta.url), "utf8");
    expect(tokens).toContain('--hc-hatch: url("/brand/hatch-light.png");');
    expect(tokens).toContain('--hc-hatch: url("/brand/hatch-dark.png");');
  });
});
