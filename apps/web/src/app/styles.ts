// SPDX-License-Identifier: Apache-2.0
//
// Global stylesheets, in the cascade-layer order Astryx requires:
// reset (@layer reset) -> components (@layer astryx-base) -> compiled theme
// (@layer astryx-theme) -> app styles (@layer app): the monochrome token
// layer (hc-mono-tokens.css, palette-mono.md), the brushed glass material
// (hc-mono-glass.css), then the app's own rules, whose component layout
// (position, radius, borders of emphasis) wins over the material's defaults.
// Prebuilt CSS only: no StyleX compiler, no swizzled components, no
// Tailwind. Both root layouts import this module so the order is defined
// once.

import "@astryxdesign/core/reset.css";
import "@astryxdesign/core/astryx.css";
import "@/theme/hyphae-mission.css";
import "./hc-mono-tokens.css";
import "./hc-mono-glass.css";
import "./globals.css";
