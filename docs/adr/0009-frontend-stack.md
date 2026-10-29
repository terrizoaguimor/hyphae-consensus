# ADR-0009: Mission Control frontend stack

Date: 2026-10-27 · Status: accepted; scaffold implemented (`2249ae7`), shell and legal
pages in review · Scope: `apps/web`

## Context

The operator console (`console.html`, ADR-0004) is a dark, dense developer tool bound to
the loopback API. The public portal needs a first-class light and dark interface in four
languages that nontechnical judges can follow, built on Meta's open-source design system
where honest, while staying clearly independent from Meta. The facts below were verified
on 2026-10-27 against the packages, their repositories and Meta's own pages.

## Decisions

1. **Next.js 16.4 App Router as a static export**, React 19.3, TypeScript strict with
   `moduleResolution: "bundler"`. Locales `en`, `es`, `pt-BR`, `fr` are pre-rendered under
   `/[locale]`; run pages read the run id client-side. A static export builds on Windows,
   needs no frontend server runtime, keeps cookies, CSRF and sessions in the Rust API
   (same origin), and avoids vinext (beta), OpenNext (Windows caveat) and Vercel Hobby
   (non-commercial terms).

2. **Astryx through its prebuilt path only.** `@astryxdesign/core`, `theme-neutral` and
   `cli` pinned exactly at 0.7.4 (MIT, "Currently in Beta", minors carry breaking
   changes), `@stylexjs/stylex` 0.19.1 listed explicitly. CSS imported in the documented
   layer order `reset.css` → `astryx.css` → our compiled theme; no swizzle, no StyleX
   compiler, no Tailwind. Interactive Astryx components are client modules, so pages stay
   Server Components with client islands behind one Providers file (Theme, LinkProvider,
   Astryx `InternationalizationProvider` with the `es-ES`, `pt-BR` and `fr-FR` catalogs
   registered under our locale codes, `NextIntlClientProvider`, `MotionConfig
   reducedMotion="user"`). `astryx init` is never run: it writes agent-instruction files.

3. **Theme `hyphae-mission`**, compiled with `astryx theme build` and committed
   (drift-checked). Light: white and ice-blue surfaces, soft radial gradients, accent
   `#0064E0` (5.39:1 with white). Dark: deep midnight with soft blue ambient light, not an
   inversion; accent text `#4BA9FE`, primary buttons `#111112` on accent or `#0064E0`
   fills, because white on `#4BA9FE` or `#2694FE` fails WCAG AA (2.5–3.1:1). Hyphae cyan
   `#45E4D0` is reserved for proof and evidence. Status tokens always carry a text label.
   Radii 8 px element / 16 px container; frosted glass only on navigation and overlays,
   solid under `prefers-reduced-transparency`. One hashed inline head script sets the
   theme before first paint; it is the only `dangerouslySetInnerHTML` in the app
   (enforced by lint and a test).

4. **Type.** Inter (variable, optical sizes, tabular numerals for metrics) and JetBrains
   Mono (hashes and ids, ligatures off, slashed zero) self-hosted through `next/font`; no
   runtime font requests. Meta's Optimistic typeface is proprietary and is not used; no
   Meta logo and no Meta AI gradient appear anywhere; a visible "independent project; not
   affiliated with, sponsored by, or endorsed by Meta" line is shown.

5. **Libraries.** `next-intl` 4.14 (ICU messages with explicit `=0` and `other`
   branches: Spanish maps 0 to `other`, Portuguese and French to `one`), Motion 14
   (opacity and transform only), `@xyflow/react` 12 with dagre for a read-only,
   keyboard-accessible claim/evidence graph (attribution badge kept; elkjs rejected for
   its EPL/GPL license).
   **Note 2026-10-29:** the graph library was not adopted. Neither `@xyflow/react` nor
   dagre is in `apps/web/package.json`, `apps/web/pnpm-lock.yaml` or the web table of
   `THIRD_PARTY_NOTICES.md`, and no source file imports them; no view draws a node graph
   or an SVG. The relations are shown as semantic HTML instead: the Evidence tab is an
   ordered list of retrieved records (`hc-evidence-list`), each with what its proof
   covers and its verifications grouped by round (`hc-rounds`), plus the verifications
   that name no retrieved record and the failed retrievals; the Explorer and Audit tabs
   and the Meta API invocations are tables (`hc-timeline`); the Proof inspector is an
   ordered list of proofs (`hc-proof-list`); the Decision tab lists the open objections
   (`src/components/run/`). Lists and tables are keyboard and screen-reader accessible
   without a graph widget and need no attribution badge.

6. **Honesty is data, not styling.** Provenance axes come only from server fields derived
   from chained records (`model`, `evidence`, `source`, per-frame `delivery`); LIVE is
   shown only for live frames of a live model run and always next to the evidence axis;
   simulated proofs never render as a green "valid"; recorded runs are played back, never
   presented as running; guided pacing is client-side playback labelled "simulated · paced
   for readability", with real server timestamps underneath. Simple and technical
   explanations render the same `PolicyTrace` through deterministic ICU templates.

7. **Supply chain and isolation.** The Node toolchain runs only inside Docker containers
   (`scripts/in-docker.sh`, `scripts/playwright-docker.sh`: digest-pinned images, only
   `apps/web` mounted, no environment, no home directory), because the owner's machine
   holds the Meta key. pnpm pinned through `packageManager`, exact versions, frozen
   lockfile, `minimumReleaseAge` of three days, every dependency build script reviewed and
   denied, `pnpm audit` in the gates.

8. **Content Security Policy.** A post-build step hashes every inline script and style of
   every exported page into `csp-manifest.json`; the Rust public server sends each page
   its own policy (`script-src 'self' <hashes> https://challenges.cloudflare.com`, no
   `unsafe-inline` or `unsafe-eval`, `frame-ancestors 'none'`). Verified in headless
   Chromium: zero violations, and removing the hashes blocks hydration.

## Consequences

- Astryx's own accessibility CI tolerates 138 contrast violations, so axe runs on every
  page in both themes and all four locales are part of our gates, not Astryx's.
- Overlays need CSS anchor positioning (Chrome/Edge 125+, Safari 26+, Firefox 147+);
  older Safari and Firefox are tested, and no status is shown only inside a tooltip.
- Legal and trust texts are drafts in four languages until the owner signs them off.
- Typed API contracts are generated from the Rust side (`contracts/`, ADR-0006); the
  frontend never hand-declares a server type.
