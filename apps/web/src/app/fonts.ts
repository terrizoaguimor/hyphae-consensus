// SPDX-License-Identifier: Apache-2.0
//
// Inter (variable, optical size) and JetBrains Mono through next/font: the
// files are downloaded at build time and served from /_next/static/media, so
// browsers never contact Google. Only the latin subset is preloaded; it
// covers English, Spanish, Portuguese and French text, and the other subsets
// (latin-ext and more) are still self-hosted and load by unicode-range when a
// page needs them. The theme maps these
// variables to Astryx's font-family tokens (src/theme/hyphaeMissionTheme.ts).
// theme-neutral's fonts.css is never imported: it loads Figtree from Google
// at runtime.

import { Inter, JetBrains_Mono } from "next/font/google";

const inter = Inter({
  subsets: ["latin"],
  axes: ["opsz"],
  variable: "--font-inter",
  display: "swap",
});

const jetbrainsMono = JetBrains_Mono({
  subsets: ["latin"],
  variable: "--font-jetbrains-mono",
  display: "swap",
});

/** Class names that define both font variables; set them on <html>. */
export const fontVariables = `${inter.variable} ${jetbrainsMono.variable}`;
