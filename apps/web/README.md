# apps/web: Mission Control (Day-0 scaffold)

The public portal's frontend: a Next.js App Router app exported as static
files (`output: "export"`), in English, Spanish (`es`), Brazilian Portuguese
(`pt-BR`) and French (`fr`), styled with Astryx's prebuilt CSS and a custom
`hyphae-mission` theme. This is plan item E1 at Day 0: one page per locale
(header, short description, sample provenance badges, non-affiliation
footer), the i18n and theme plumbing, and the CSP manifest. Nothing here talks
to the Rust server yet.

## Build only through Docker

Never run `node`, `npm`, `pnpm`, `npx` or any script of this app on the host.

Why: the owner's Windows user environment holds the Meta API key, and every
process started on the host inherits it. Dependency code does not only run at
install time (where build scripts are blocked here); it also runs during
`next build`, linting and tests (Next config and plugins, the CSS pipeline,
ESLint plugins, Vitest), with the caller's full environment and file access.
One compromised package would then reach the key, `~/.ssh`, `~/.cloudflared`
and the git credentials. So the whole Node toolchain runs in a throwaway
container that sees `apps/web` and nothing else (red-team finding ABUSE-3).

From the repository root, in Git Bash (or any POSIX shell on Linux or macOS):

```sh
sh apps/web/scripts/in-docker.sh pnpm install --frozen-lockfile
sh apps/web/scripts/in-docker.sh pnpm check
```

`scripts/in-docker.sh` runs `docker run --rm` on `node:24-bookworm-slim`
pinned by digest (Node 24.22.1), mounts only `apps/web` at `/app`, passes no
`-e` variables, no home directory and no TTY, enables corepack, switches off
Next.js telemetry inside the container, and runs the given command. The
`pnpm` script results below were produced through it; the audit, the policy
probes and the browser checks ran in separate throwaway containers with
`apps/web` mounted read-only.

Notes:

- `node_modules` is created by Linux inside the container. It holds Linux
  binaries and Linux symlinks, so Windows tools cannot use it (by design).
  pnpm 12 keeps its package store in `node_modules/.pnpm-store` when only the
  project is mounted, so later installs reuse downloaded packages.
- The bind mount from Windows is slow: a full `pnpm check` took between 4
  and 10 minutes on 2026-10-27, most of it `tsc` and `next build` reading
  files.
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
| `pnpm test` | Vitest: locale parity and ICU parsing, theme script, CSP manifest, i18n helpers |
| `pnpm build` | Static export to `out/`, then `node scripts/csp-manifest.mjs` |
| `pnpm theme:build` | Compile `src/theme/hyphaeMissionTheme.ts` with `astryx theme build` |
| `pnpm theme:check` | Fail if the committed theme outputs drift from the source |
| `pnpm check` | `theme:check`, `typecheck`, `lint`, `test` and `build`, in that order |

Lint rules beyond the Next.js defaults: `react/no-danger` (the theme script
file is the single exception) and `react/jsx-no-literals` (UI text comes from
`messages/*.json`).

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
  reason in `pnpm-workspace.yaml`; the build works without them.
- **`pnpm audit --prod`**: "No known vulnerabilities found" (2026-10-27).

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
| `@types/node` | 24.19.2 | 2026-10-09 | Matches the Node 24 runtime; 26.x is newer |
| `@types/react`, `@types/react-dom` | 19.3.0 | 2026-09-09 | |
| `pnpm` | 12.15.0 | 2026-10-21 | |

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
  (white on `#4BA9FE` measures 2.50:1 and fails WCAG AA); Hyphae cyan
  `#45E4D0` as local tokens reserved for proof and evidence; radii 8 px
  (element) and 16 px (container). Fonts are set through the typography
  roles, not plain tokens: neutral's touch-and-narrow adaptation re-derives
  the font tokens from those roles.
- **Fonts**: Inter (variable, with optical size) and JetBrains Mono through
  `next/font/google`, downloaded at build time and served from
  `/_next/static/media`. theme-neutral's `fonts.css` is never imported (it
  loads Figtree from Google at runtime). `.hc-tabular-nums` and `.hc-code`
  are the utilities for metrics and for hashes and ids.
- **Client boundaries**: of the Astryx modules used here, `Theme`,
  `LinkProvider`, `InternationalizationProvider`, `Heading`, `Text` and
  `Switch` ship as `'use client'` modules; `Badge` is server-safe (so are the
  stacks and `VisuallyHidden`). `Heading`, `Text` and `Badge` render directly
  from server components. `Theme` and `LinkProvider` need a client file of
  ours, because their props cannot cross from a server component (a theme
  object holding icon components, a link component): they live in
  `src/components/Providers.tsx`, the single provider file, with Astryx i18n,
  `NextIntlClientProvider` and `MotionConfig reducedMotion="user"`. `Switch`
  takes an event handler, so it sits in the client `ThemeToggle.tsx`.
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

## Color mode

`src/components/themeScriptSource.ts` holds a one-line inline script, the
app's only `dangerouslySetInnerHTML` (`ThemeScript.tsx`). In `<head>`, before
the body is parsed, it sets `data-theme` on `<html>` from localStorage
`hc-theme`, or from `prefers-color-scheme` when nothing valid is stored.

The static HTML renders Astryx's root `Theme` in "system" mode, and hydration
must match it. `Providers` reads the head script's choice during the first
client render and applies it in a layout effect, before the first paint after
hydration. Until then, `globals.css` makes the root Theme wrapper inherit the
color scheme of `<html>`. That includes Lightning CSS's
`--lightningcss-light` / `--lightningcss-dark` switches: for Next's default
browser targets, the build compiles every `light-dark()` into them. The theme
switch shows "off" in the static HTML and takes its real state on hydration.

## i18n

next-intl 4 with static rendering and no middleware or proxy:
`generateStaticParams` and `dynamicParams = false` in
`src/app/[locale]/layout.tsx`, `setRequestLocale` in the localized layout
and page, messages from `messages/<locale>.json`, and a fixed `UTC` time
zone. Catalog keys are typed from `messages/en.json`, and the tests require
identical key sets and valid ICU in all four. Counts use an explicit `=0`
branch (CLDR maps 0 to "other" in Spanish but to "one" in Portuguese and
French).

The root page `/` (its own root layout) lists the four languages, sends
JavaScript visitors to their preferred locale (`navigator.languages`), and
sends visitors without JavaScript to `/en/` through a `<noscript>` refresh.
The redirect was checked in headless Chromium (`es-MX` → `/es/`, `en-US` →
`/en/`, `fr-CA` → `/fr/`); the no-JavaScript refresh was not, because
headless Chromium does not load a page with scripting disabled.

## CSP manifest

`scripts/csp-manifest.mjs` runs after the export. It parses every
`out/**/*.html` with parse5, hashes each inline `<script>` and `<style>` body
(sha256, base64), and writes `out/csp-manifest.json`: page path → hashes. It
fails on inline event handler attributes, `javascript:` URLs, and
subresources loaded from another origin (including absolute URLs in the
exported CSS). serve-public is meant to send each page
`script-src 'self' <that page's hashes>`; when Turnstile is added, its origin
has to be allowed in the script and in `script-src` / `frame-src`.

On 2026-10-27 each locale page had 4 inline scripts: the theme script (the
same hash on every page), two Next.js flight-data scripts (one of them
page-specific) and Turbopack's chunk-loader bootstrap; no inline styles; and
one `style` attribute, from Astryx's `Switch`, so the policy needs
`style-src-attr 'unsafe-inline'`. Next's default 404 page has 3 inline
scripts and 1 inline style.

A headless Chromium 156 run (in a separate throwaway container, each page
served with a policy built from the manifest) logged no console messages on
`/`, `/en/`, `/es/`, `/pt-BR/`, `/fr/` or the 404 page, and the locale pages
hydrated. As a control, `/en/` served with the hashes removed logged 4
inline-script violations and did not hydrate.

## Known gaps

- `out/404.html` is Next's default page: English only, without the theme or
  fonts.
- No `hreflang` alternates yet: they need the public hostname (Day 2).
- No generated contract types yet (FEAS-4; the contract freezes on Day 1).
- Script weight: `/en/` loads 11 async scripts, 774,536 bytes raw (220,845
  gzip at level 9), plus a 112,594-byte `noModule` polyfill that modern
  browsers skip. The whole export is 65 files, 1,847,953 bytes. The largest app chunk includes theme-neutral's source theme
  object, which the generated `hyphae-mission.js` imports for its icons. Not
  optimized yet.
- Browser checks so far: the headless Chromium run above. No axe or
  Playwright runs yet, and no Safari or Firefox.
