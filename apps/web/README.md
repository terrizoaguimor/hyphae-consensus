# apps/web: Mission Control

The public portal's frontend: a Next.js App Router app exported as static
files (`output: "export"`), in English, Spanish (`es`), Brazilian Portuguese
(`pt-BR`) and French (`fr`), styled with Astryx's prebuilt CSS and a custom
`hyphae-mission` theme. Plan items E1 (scaffold, theme, i18n plumbing, CSP
manifest, Node container) and E2 (shell, navigation, onboarding, settings,
legal and trust pages, in-app docs), with a Playwright and axe harness.
Nothing here talks to the Rust server yet: every view that needs the API says
so ("This view connects to the Hyphae Consensus API; it is not connected in
this build"), with no sample data and no spinner.

## Pages

17 pages per locale, all pre-rendered (`/<locale>/…/`, trailing slash), plus
the language-neutral root page `/` and the 404 page:

| Page | Path | Content |
|---|---|---|
| Overview | `/` | What the system does, the main call to action (while `API_CONNECTED` is false: "How guided examples will work", linking to the Modes article; once connected: "Run a guided example"), the tour invitation, how a deliberation works, the provenance legend; impact metrics are a not-connected placeholder |
| New deliberation | `new/` | Not-connected placeholder: what the view will offer (guided example, live presets, recorded fallback, coming back through My runs) |
| My runs | `runs/` | Not-connected placeholder (session-scoped runs, 24 h access, deletion after 7 days) |
| Research showcase | `showcase/` | Not-connected placeholder (recorded runs, RECORDED label, dated corrections, MANIFEST) |
| Docs | `docs/` | Index of 7 articles, tour launcher, explanation level |
| Docs articles | `docs/<slug>/` | `what-is`, `three-roles`, `not-voting`, `verified-evidence`, `abstain-contested`, `human-authorization`, `modes`: summary, sections, technical details, previous/next |
| Settings | `settings/` | Theme, language, motion, explanation level, remove saved settings |
| Terms of use, Privacy notice, Limitations | `terms/`, `privacy/`, `limitations/` | Drafts for the planned public deployment, marked "Draft" while `LEGAL_TEXT_STATUS` is `"draft"`, dated from `LEGAL_TEXT_UPDATED` |
| About | `about/` | Independence from Meta, Hyphae as the owner's earlier project (copyright Celiums Solutions LLC, Apache-2.0, docs CC-BY-SA-4.0), license, AI assistance, source repository (text until `SOURCE_REPOSITORY_URL` is set); marked "Draft" like the other legal pages |
| Not found | `/404.html` | `src/app/global-not-found.tsx` (Next's experimental `globalNotFound`): `lang="en"`, the four language links, the non-affiliation line, the theme script and the app styles; served for every unknown path |

Every internal link is built from the site map in `src/lib/site.ts`;
`tests/site-map.test.ts` requires a route file for each entry, and the e2e
crawler requires every exported link to answer 200 without a redirect.

Docs and legal statements come from the root README, `docs/threat-model.md`,
the multi-agent (AI) audit of 2026-10-27 and the approved plan, with the owner's decisions of
2026-10-27 for the legal texts: the whole portal is geo-blocked at country
level for Meta's Restricted Territories (Hong Kong and Macau named as part of
China; all of Ukraine, because the listed regions cannot be isolated at
country level) and South Korea for live mode only; the data controller is
Mario Gutiérrez with a public contact address (`CONTACT_EMAIL` in
`src/lib/site.ts`, a marked placeholder until the owner provides it); hosting
is a Linux server administered by the owner at "the owner's hosting provider"
behind Cloudflare (`HOSTING_COUNTRY`, a marked placeholder until the owner
confirms it); retention is run files deleted whole after 7 days except a
one-line run-index entry and the budget-ledger cost entry, hashed-address
tables in memory and bounded, server/cloudflared/host logs under the host's
rotation, and an unpublished operator detail log keyed by request id.
**The legal texts (Terms, Privacy, Limitations, About) are drafts for the
owner's review** in all four languages; the ES/PT/FR texts are drafts too and
need the Day-5 language sign-off.

Honesty rules the texts follow: a valid proof shows that the Hyphae recall it
seals reproduces against its pinned anchor, not yet that the displayed text is
the sealed text (audit finding HY-1), and never that a record is true,
relevant or complete or that a model used it; unbuilt features are described
in the future tense; LIVE META is never shown alone but next to its evidence
label (SEALED SNAPSHOT, or SIMULATED for the archived hybrid runs of a live
model with simulated proofs); runs recorded under protocol 1.2 read only the
final round and the 2026-10-27 multi-agent (AI) audit found two fail-open
paths (MA-1, RT-1), while protocol 1.3 rejects inexact ids and carries every
open objection and contradiction forward and has not yet been re-validated
live.

## Build only through Docker

Never run `node`, `npm`, `pnpm`, `npx` or any script of this app on the host.

Why: the owner's Windows user environment holds the Meta API key, and every
process started on the host inherits it. Dependency code does not only run at
install time (where build scripts are blocked here); it also runs during
`next build`, linting and tests (Next config and plugins, the CSS pipeline,
ESLint plugins, Vitest, Playwright), with the caller's full environment and
file access. One compromised package would then reach the key, `~/.ssh`,
`~/.cloudflared` and the git credentials. So the whole Node toolchain runs in
throwaway containers that see `apps/web` and nothing else (red-team finding
ABUSE-3).

From the repository root, in Git Bash (or any POSIX shell on Linux or macOS):

```sh
sh apps/web/scripts/in-docker.sh pnpm install --frozen-lockfile
sh apps/web/scripts/in-docker.sh pnpm check          # theme, types, lint, Vitest, export, CSP manifest
sh apps/web/scripts/playwright-docker.sh             # e2e + axe against out/
```

`scripts/in-docker.sh` runs `docker run --rm` on `node:24-bookworm-slim`
pinned by digest (Node 24.22.1), mounts only `apps/web` at `/app`, passes no
`-e` variables, no home directory and no TTY, enables corepack, switches off
Next.js telemetry inside the container, and runs the given command. Two parts
of the mount are read-only, so code running in the container can never
rewrite what the host runs or trusts: `scripts/` (the two `*.sh` files run on
the host, where the Meta key is set) and `pnpm-workspace.yaml` (the
supply-chain policy). The rest stays writable because the toolchain writes
there: `node_modules/` (pnpm install, with its store in
`node_modules/.pnpm-store`), `.next/` and `next-env.d.ts` (`next typegen`,
`next build`), `tsconfig.tsbuildinfo` (`tsc`), `out/` (the export and
`csp-manifest.json`). Nothing else in `apps/web` is written by a command of
this project.

`scripts/playwright-docker.sh` runs `playwright test` in
`mcr.microsoft.com/playwright:v1.64.1-noble`, pinned by digest
(`sha256:47b510dd…b8ea`, Node 24.22.1, the browsers of Playwright 1.64.1, the
same release as the pinned `@playwright/test`). Same isolation as
`in-docker.sh`, plus `--network none`: the tests serve `out/` on the
container's own loopback, so nothing a page loads can come from anywhere
else. The container is further confined: `/app` is mounted read-only with
`test-results/` as the only writable directory (Playwright's transform cache
goes to the container's `/tmp`), it runs as the image's unprivileged `pwuser`
(so `test-results/` is not root-owned on Linux and macOS), every capability
is dropped and no process can gain privileges (`--cap-drop ALL
--security-opt no-new-privileges`; Playwright launches Chromium with its
sandbox off by default, so none is needed). Extra arguments go to
`playwright test` (for example `--project=chromium e2e/a11y.spec.ts`). It
needs a built export.

Notes:

- `node_modules` is created by Linux inside the container. It holds Linux
  binaries and Linux symlinks, so Windows tools cannot use it (by design).
  pnpm 12 keeps its package store in `node_modules/.pnpm-store` when only the
  project is mounted, so later installs reuse downloaded packages.
- The bind mount from Windows is slow: `pnpm build` takes about 50 s, a full
  `pnpm check` several minutes.
- Never run `astryx init`: it writes the Astryx component index into
  `AGENTS.md`/`CLAUDE.md`, which would change the instructions of coding
  agents working on this repository. Run the Astryx CLI only through the
  `theme:*` scripts or `pnpm exec astryx …` inside the container, and check
  `git status` afterwards for new `AGENTS.md`, `CLAUDE.md`, `.cursor*` or
  `.github/copilot-instructions.md` files.

## Scripts

| Script | What it does |
|---|---|
| `pnpm install --frozen-lockfile` | Install exactly what `pnpm-lock.yaml` records |
| `pnpm typecheck` | `next typegen`, then `tsc --noEmit` (strict) |
| `pnpm lint` | ESLint (`eslint-config-next` core-web-vitals + TypeScript), zero warnings allowed |
| `pnpm test` | Vitest: locale parity, ICU and rich-text tags of every catalog, site map, preferences, theme contrast, theme script, CSP manifest, static server, i18n helpers |
| `pnpm build` | Static export to `out/`, then `node scripts/csp-manifest.mjs` |
| `pnpm theme:build` | Compile `src/theme/hyphaeMissionTheme.ts` with `astryx theme build` |
| `pnpm theme:check` | Fail if the committed theme outputs drift from the source |
| `pnpm check` | `theme:check`, `typecheck`, `lint`, `test` and `build`, in that order |
| `pnpm serve` | `scripts/serve-static.mjs`: serve `out/` on 127.0.0.1:4173 with per-page CSP (inside a container) |
| `pnpm e2e` | Fails fast (exit 64) with a message pointing at `scripts/playwright-docker.sh`: the Node container has no browsers, and the suite runs only there |

Lint rules beyond the Next.js defaults: `react/no-danger` (the theme script
file is the single exception) and `react/jsx-no-literals` (UI text comes from
the message catalogs).

## Supply chain

- **pnpm 12.15.0**, pinned with its sha512 through `packageManager`, so
  corepack verifies the package it downloads (the pin matches the registry's
  integrity value). pnpm 12 is a native binary; according to its wrapper's
  source, the wrapper downloads that binary from the npm registry and checks
  npm's registry signature.
- **Exact versions only**, and the lockfile is committed. Installs use
  `--frozen-lockfile`.
- **Release cooldown**: `minimumReleaseAge: 4320` (3 days) in
  `pnpm-workspace.yaml`. Because the value is explicit, pnpm enforces it
  strictly: adding `motion@14.7.1` (published the same day) fails with
  `ERR_PNPM_NO_MATURE_MATCHING_VERSION`. pnpm 12 also fails on any setting it
  does not recognize in that file (`minimumReleaseAg` gives
  `ERR_PNPM_UNRECOGNIZED_WORKSPACE_SETTINGS`), so a misspelled policy cannot
  be dropped silently.
- **Dependency build scripts**: pnpm 12 runs none unless a package is set to
  `true` under `allowBuilds`, and fails the install while any is undecided.
  The five packages that have scripts are denied, each with the reviewed
  reason in `pnpm-workspace.yaml`; the build works without them. The test
  packages added on 2026-10-27 (`@playwright/test`, `playwright`,
  `playwright-core`, `@axe-core/playwright`, `axe-core`) have no install
  scripts.
- **Browsers** come from the digest-pinned Playwright image, never from
  `playwright install`.
- **`pnpm audit --prod`** and **`pnpm audit`**: "No known vulnerabilities
  found" (2026-10-27, with the test packages).

## Versions

Pinned 2026-10-27, each the newest release at least 3 days old unless an
exception is noted.

| Package | Version | Published | Note |
|---|---|---|---|
| `next`, `eslint-config-next` | 16.4.3 | 2026-10-23 | |
| `react`, `react-dom` | 19.3.1 | 2026-10-14 | |
| `@astryxdesign/core`, `theme-neutral`, `cli` | 0.7.4 | 2026-10-23 | Beta; minor releases have carried breaking changes |
| `@stylexjs/stylex` | 0.19.1 | 2026-09-15 | Peer dependency that Astryx imports at runtime, listed explicitly (Astryx issue #4276) |
| `next-intl` | 4.14.9 | 2026-10-02 | |
| `motion` | 14.6.0 | 2026-10-22 | Exception: 14.7.0 and 14.7.1 were published 2026-10-27, inside the cooldown |
| `typescript` | 6.0.3 | 2026-04-16 | Exception: 7.0.2 is newer, but typescript-eslint 8.72.0 (used by `eslint-config-next`) requires `<6.1.0` |
| `eslint` | 9.39.5 | 2026-07-10 | Exception: 10.13.0 is newer, but `eslint-plugin-react` 7.37.5, `eslint-plugin-import` 2.32.0 and `eslint-plugin-jsx-a11y` 6.10.2 (dependencies of `eslint-config-next` 16.4.3) declare peers only up to ESLint 9, and `create-next-app` 16.4.3 pins `^9`. npm marks 9.x "no longer supported" |
| `vitest` | 5.0.5 | 2026-10-15 | |
| `vite` | 8.3.5 | 2026-10-21 | Required peer of Vitest 5 |
| `intl-messageformat` | 11.2.15 | 2026-09-12 | 11.x on purpose: next-intl and Astryx use 11.x; 12.1.4 is newer |
| `parse5` | 8.0.1 | 2026-04-19 | HTML parser for the CSP manifest |
| `@playwright/test` | 1.64.1 | 2026-10-14 | Newest stable (1.65 is in alpha); brings `playwright` and `playwright-core` 1.64.1 |
| `@axe-core/playwright` | 4.14.0 | 2026-10-15 | Brings `axe-core` 4.14.0 (2026-10-05), already in the lockfile through `eslint-plugin-jsx-a11y` |
| `@types/node` | 24.19.2 | 2026-10-09 | Matches the Node 24 runtime; 26.x is newer |
| `@types/react`, `@types/react-dom` | 19.3.0 | 2026-09-09 | |
| `pnpm` | 12.15.0 | 2026-10-21 | |
| Image `node:24-bookworm-slim` | `sha256:55fb5f5a…d205` | pulled 2026-10-27 | Node 24.22.1 |
| Image `mcr.microsoft.com/playwright:v1.64.1-noble` | `sha256:47b510dd…b8ea` | built 2026-10-23, pulled 2026-10-27 | Ubuntu 24.04, Node 24.22.1, Chromium/Firefox/WebKit of Playwright 1.64.1 |

## Astryx

Prebuilt path only: no swizzled components, no StyleX compiler, no Tailwind.

- **Stylesheets** (`src/app/styles.ts`, imported by both root layouts), in
  layer order: `@astryxdesign/core/reset.css` (`reset`) →
  `@astryxdesign/core/astryx.css` (`astryx-base`) →
  `src/theme/hyphae-mission.css` (`astryx-theme`) → `src/app/globals.css`
  (`app`). App rules stay inside `@layer app`, because unlayered rules would
  beat every Astryx layer.
- **Theme**: `src/theme/hyphaeMissionTheme.ts` extends the neutral theme with
  `defineTheme`. `pnpm theme:build` writes the committed
  `hyphae-mission.{css,js,d.ts,css.d.ts,variants.d.ts}`; rebuild after any
  Astryx upgrade. Tokens: accent `#0064E0` (light) and `#4BA9FE` (dark, text
  and icons); text on accent white in light mode and `#111112` in dark mode
  (white on `#4BA9FE` measures 2.50:1 and fails WCAG AA); surfaces white and
  ice blue in light mode (body `#F3F7FC`, muted `#EDF3FB`) and deep midnight
  in dark mode (body `#0A0F1E`, surface `#121A2D`, card `#0F1628`, popover
  `#141D33`), with a soft blue ambient glow behind the content sheet; Hyphae
  cyan `#45E4D0` as local tokens reserved for proof and evidence (nothing in
  E2 uses it); radii 8 px (element) and 16 px (container).
  `tests/contrast.test.ts` reads the compiled theme and requires 4.5:1 for
  every text token on every surface token, in both modes. Fonts are set
  through the typography roles, not plain tokens: neutral's
  touch-and-narrow adaptation re-derives the font tokens from those roles.
- **Fonts**: Inter (variable, with optical size) and JetBrains Mono through
  `next/font/google`, downloaded at build time and served from
  `/_next/static/media`. theme-neutral's `fonts.css` is never imported (it
  loads Figtree from Google at runtime). `.hc-tabular-nums` and `.hc-code`
  are the utilities for metrics and for hashes and ids.
- **Components used**: `Button`, `RadioList`, `Switch`, `Badge`, `Heading`
  and `Text`. Long-form content (docs, legal pages) is plain HTML styled by
  the theme's reset layer, so it needs no hydration. Dialogs (tour, mobile
  drawer) are our own `src/components/Modal.tsx` on the native `<dialog>`,
  with a strict focus trap.
- **Client boundaries**: `Theme`, `LinkProvider`, Astryx i18n,
  `NextIntlClientProvider`, `MotionConfig` and the tour live in
  `src/components/Providers.tsx`; the preference state is
  `src/components/preferences.tsx`.
- **Astryx strings**: Astryx resolves a locale only from specific to general,
  so its `es-ES`, `pt-BR` and `fr-FR` catalogs (default exports of
  `@astryxdesign/core/locales/<tag>.generated.js`) are registered under `es`,
  `pt-BR` and `fr`. Astryx's own Spanish strings are therefore Spain Spanish,
  while the app's catalog is neutral Latin American Spanish.
- **License**: `@astryxdesign/core@0.7.4` ships **no LICENSE file** (its
  `files` list omits it); its `package.json` says MIT.
  `@astryxdesign/theme-neutral` and `@astryxdesign/cli` 0.7.4 ship the
  standard MIT text, "Copyright (c) 2026 Meta Platforms, Inc.". Notices for
  core have to carry that text.

## Preferences (localStorage only, never cookies)

| Key | Values | Default when absent |
|---|---|---|
| `hc-theme` | `light`, `dark` | Match the system (read by the head script) |
| `hc-motion` | `reduce` | Match the system's `prefers-reduced-motion` |
| `hc-explanation` | `technical` | `simple` |
| `hc-locale` | `en`, `es`, `pt-BR`, `fr` | The root page negotiates `navigator.languages` |
| `hc-tour` | `skipped`, `completed` | The Overview invites to the tour |
| `hc-notice` | `locale` | One-shot: Settings confirms a language change on the next page |

Settings writes them (a default removes the key), confirms each change in a
polite live region, and can remove them all. Other tabs follow through the
`storage` event.

## Color mode and motion

`src/components/themeScriptSource.ts` holds a one-line inline script, the
app's only `dangerouslySetInnerHTML` (`ThemeScript.tsx`). In `<head>`, before
the body is parsed, it sets `data-theme` on `<html>` from localStorage
`hc-theme`, or from `prefers-color-scheme` when nothing valid is stored.
"Match system" in Settings removes the key; the page then follows the OS,
live.

The static HTML renders Astryx's root `Theme` in "system" mode, and hydration
must match it. The preference state reads the head script's choice during the
first client render and applies it in a layout effect, before the first
paint after hydration. Until then, `globals.css` makes the root Theme wrapper
inherit the color scheme of `<html>`, including Lightning CSS's
`--lightningcss-light` / `--lightningcss-dark` switches (the build compiles
every `light-dark()` into them). The header switch shows "off" in the static
HTML and takes its real state on hydration.

Reduced motion comes from the OS (`prefers-reduced-motion`) or from Settings
(`data-motion="reduce"` on `<html>`), and is tiered: the drawer's slide and
the dialog's small shift go, short opacity fades stay, everything else is cut
to 0.01 ms (so `transitionend` still fires), and Motion gets
`reducedMotion="always"`. Frosted glass is limited to the top bar, the
sidebar and the two dialogs, with a solid background under
`prefers-reduced-transparency: reduce`, without `backdrop-filter` support,
and in forced-colors mode.

## i18n

next-intl 4 with static rendering and no middleware or proxy:
`generateStaticParams` and `dynamicParams = false` in the `[locale]` layout
and the docs route, `setRequestLocale` in every localized layout and page,
and a fixed `UTC` time zone.

Each locale has three catalog files, merged by `src/i18n/messages.ts`:
`messages/<locale>.json` (UI), `messages/docs/<locale>.json` (Docs) and
`messages/legal/<locale>.json` (Legal and About). Keys are typed from the
English files. Only the UI namespaces the client components read (`Nav`,
`Shell`, `Theme`, `LocaleSwitcher`, `Settings`, `Tour`) are serialized into
every page; docs and legal texts reach the browser only as the HTML of the
page that shows them. Rich text uses a fixed set of tags (`src/i18n/richTags.ts`:
`strong`, `em`, `code`, `en` for an English term kept in a translation, which
renders `lang="en"`, and links to fixed destinations), so a translation
cannot add a URL or markup. The tests require, for every file: identical key
sets in all four locales, valid ICU, the same rich-text tags as English
(`<en>` excepted, since English has nothing to mark), no URLs or HTML in the
text, no shared top-level namespace, and the required facts in the legal
drafts. The legal pages' date is an ICU argument (`{date, date, long}`),
formatted per locale from `LEGAL_TEXT_UPDATED`; the contact address and the
hosting country are ICU arguments too (`{contact}`, `{hostingCountry}`),
filled from the constants in `src/lib/site.ts`. Counts use an explicit `=0`
branch (CLDR maps 0 to "other" in Spanish but to "one" in Portuguese and
French). French text uses no-break spaces before `: ; ! ?` and inside « ».

The root page `/` (its own root layout) lists the four languages, sends
JavaScript visitors to the language they chose before (`hc-locale`) or else
to their browser's preference, and sends visitors without JavaScript to
`/en/` through a `<noscript>` refresh.

## Accessibility

- Skip link (first focusable element) to `<main id="main" tabindex="-1">`;
  landmarks: banner, "Primary" and "Language" navigation, main, contentinfo
  with its own navigation. Section headings are plain headings, not region
  landmarks.
- Focus management on route change (`src/components/RouteFocus.tsx`): after
  every client-side navigation, focus moves to the new page's `<h1>`; Next's
  route announcer announces the title.
- Current page marked with `aria-current` and by weight and an indicator
  bar, not color alone. Touch targets at least 40 px high in the navigation.
- Lists styled without markers carry `role="list"` (Safari drops the list
  semantics otherwise). The Settings status box is sticky, and while it has
  text the document gets `scroll-padding-block-end`, so focus scrolling keeps
  the focused control above it (WCAG 2.2, 2.4.11). The docs cards hand their
  focus ring to the card only inside `@supports selector(:has(*))`; the Menu
  icon's bars use `CanvasText` in forced-colors mode. English terms kept in
  translations (outcome names such as "(blocked)") carry `lang="en"`.
- The root page has one `<h1>` (the wordmark) and the non-affiliation line;
  the 404 page has `lang`, one `<h1>`, the four language links and the
  non-affiliation line.
- Dialogs: native modal `<dialog>` (the page behind is inert), named by their
  title, Escape and backdrop close them, Tab and Shift+Tab wrap inside, and
  focus returns to the opener (or to the `<h1>` when the opener is gone). The
  tour keeps focus on its primary button and announces each step in a polite
  live region.
- axe (2026-10-28, Chromium 1.64.1 build): 158 scans (17 pages × 4 locales ×
  2 themes, the tour and the drawer in each locale and theme, the 404 page in
  both themes, and 4 phone layouts): **0 violations of any impact**, 6,400
  rule passes, 43,509 element checks passed. `color-contrast` stays
  "incomplete" on every scan:
  axe cannot compute contrast over the body gradient and the translucent
  glass (the wordmark, the Menu button, the footer line); the solid
  surfaces are covered by `tests/contrast.test.ts`.

## CSP manifest

`scripts/csp-manifest.mjs` runs after the export. It parses every
`out/**/*.html` with parse5, hashes each inline `<script>` and `<style>` body
(sha256, base64), and writes `out/csp-manifest.json`: page path → hashes. It
fails on inline event handler attributes, `javascript:` URLs, and
subresources loaded from another origin (including absolute URLs in the
exported CSS). serve-public is meant to send each page
`script-src 'self' <that page's hashes>`; when Turnstile is added, its origin
has to be allowed in the script and in `script-src` / `frame-src`.

On 2026-10-27 the export had 72 HTML files. Each locale page has 4 inline
scripts (the theme script with the same hash everywhere, two Next.js
flight-data scripts and Turbopack's chunk-loader bootstrap), no inline
style, and 2 `style` attributes (the header and drawer theme switches; 9 on
Settings, from the radio lists), so the policy needs
`style-src-attr 'unsafe-inline'`. The 404 page (`/404.html`, also exported as
`/404/` and `/_not-found/`) is rendered from `src/app/global-not-found.tsx`
and has the same 4 inline scripts, no inline style and no style attribute;
the e2e server sends it with its own hashes for every unknown path.

The e2e server (`scripts/serve-static.mjs`) sends every page the policy built
from the manifest (`default-src 'none'`, `script-src 'self'` plus the page's
hashes, `style-src-attr 'unsafe-inline'`, `frame-ancestors 'none'`, and so
on), and the suite fails on any CSP violation, console error, failed
same-origin request or request to another origin.

## End-to-end tests

`e2e/` (Playwright 1.64.1, run through `scripts/playwright-docker.sh`):

| Spec | Covers |
|---|---|
| `navigation.spec.ts` | Every primary nav item in every locale (URL, `<h1>`, focus on the `<h1>`, `aria-current`, `lang`, title); a crawl of all 69 pages that requests every internal link without following redirects and allows only the four known external links; unknown paths answer 404 with the global not-found page (lang, language links, non-affiliation line, CSP); skip link; landmarks; docs pager and section marking; the mobile drawer (modal, focus trap, Escape, focus return, navigation, language and theme) |
| `preferences.spec.ts` | Theme switch and persistence; OS following; Settings (each choice, its announcement, persistence, Docs details, reset); "Match system"; a language chosen in Settings confirmed in the new language; the locale switcher and the root page's redirect; no cookies at all |
| `onboarding.spec.ts` | Offered but never opened by itself; Skip, Escape and backdrop close it and are remembered; all six steps with Back and Finish; strict focus trap and inert page; reopening from Docs; translated in every locale |
| `motion.spec.ts` | OS reduced motion and the in-app setting remove movement and keep a short fade; "Match system" follows the OS live; the drawer; reduced transparency makes the glass solid (Chromium, through the DevTools protocol) |
| `a11y.spec.ts` | axe on every page in both themes and all locales, the overlays, the 404 page in both themes, and phone layouts |

Projects: `chromium` (everything except `@mobile`), `chromium-mobile`
(Pixel 7, `@mobile`), and `firefox` and `webkit` (the `@smoke` tests). Results
go to `test-results/` (git-ignored); `test-results/axe-summary.json` adds up
the axe scans.

## Known gaps

- The 404 page is English only (with the four language links); a 404 cannot
  know the visitor's locale before any script runs.
- `CONTACT_EMAIL` and `HOSTING_COUNTRY` in `src/lib/site.ts` are marked
  placeholders until the owner provides them; the texts render them as such.
- Outside this directory, and not changed from here: the root `README.md`
  still describes the protocol 1.2 stop rule and "fixes in progress", and the
  territory list of the Terms (owner decisions of 2026-10-27) has to go into
  release gate G10 of the plan.
- No `hreflang` alternates yet: they need the public hostname (Day 2).
- No generated contract types yet (FEAS-4; the contract freezes on Day 1).
- Script weight: `/en/` loads 13 async scripts, 797,215 bytes raw (230,921
  gzip at level 9), plus a 112,594-byte `noModule` polyfill that modern
  browsers skip. The whole export is 460 files, 7,132,989 bytes (each page
  also exports its RSC payloads). Not optimized yet.
- Browser coverage: Chromium (full suite), Firefox and WebKit builds of
  Playwright 1.64.1 (smoke tests). No real Safari or older Firefox, and no
  screen-reader pass with NVDA, JAWS or VoiceOver yet.
- The legal and trust texts are drafts awaiting the owner's review, and the
  About page shows no repository link until `SOURCE_REPOSITORY_URL` is set
  in `src/lib/site.ts`.
