// SPDX-License-Identifier: Apache-2.0
//
// Global stylesheets, in the cascade-layer order Astryx requires:
// reset (@layer reset) -> components (@layer astryx-base) -> compiled theme
// (@layer astryx-theme) -> app styles (@layer app). Prebuilt CSS only: no
// StyleX compiler, no swizzled components, no Tailwind. Both root layouts
// import this module so the order is defined once.

import "@astryxdesign/core/reset.css";
import "@astryxdesign/core/astryx.css";
import "@/theme/hyphae-mission.css";
import "./globals.css";
