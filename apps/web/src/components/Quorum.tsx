// SPDX-License-Identifier: Apache-2.0
//
// The Quorum mark (design/logo/quorum-final, v1.0). Below 64 px the mark is
// always flat: QuorumMark is the hand-set 32 px pixel cut (mark-flat-32),
// one ink, whole pixels, drawn in currentColor so the theme picks the ink
// (the wordmark's text colour) without any style attribute. QuorumGlass is
// the brushed-glass mark for 64 px and more: the small glass files (64 to
// 192 px), one for light surfaces and one for dark, served as images (their
// gradient and filter ids never collide with another mark's) and switched by
// globals.css on <html data-theme> (or the OS setting before the head script
// has run). Both are decorative next to the wordmark text, which names them.

import { brandFiles } from "@/lib/brand";

/** The 32 px cut of mark-flat-32-black.svg, path verbatim. */
const FLAT_32 =
  "M14 3h4v1h-4z M14 4h4v1h-4z M13 5h6v1h-6z M13 6h6v1h-6z M12 7h3v1h-3z M12 8h3v1h-3z M11 9h3v1h-3z M11 10h3v1h-3z M18 10h3v1h-3z M10 11h3v1h-3z M19 11h3v1h-3z M10 12h3v1h-3z M19 12h3v1h-3z M9 13h3v1h-3z M20 13h3v1h-3z M9 14h3v1h-3z M15 14h2v1h-2z M20 14h3v1h-3z M8 15h3v1h-3z M15 15h2v1h-2z M21 15h3v1h-3z M8 16h3v1h-3z M14 16h4v1h-4z M21 16h3v1h-3z M7 17h3v1h-3z M14 17h4v1h-4z M22 17h3v1h-3z M7 18h3v1h-3z M13 18h6v1h-6z M22 18h3v1h-3z M6 19h3v1h-3z M13 19h6v1h-6z M23 19h3v1h-3z M6 20h3v1h-3z M12 20h8v1h-8z M23 20h3v1h-3z M5 21h3v1h-3z M12 21h8v1h-8z M24 21h3v1h-3z M5 22h3v1h-3z M11 22h10v1h-10z M24 22h3v1h-3z M25 23h3v1h-3z M25 24h3v1h-3z M26 25h3v1h-3z M3 26h20v1h-20z M26 26h3v1h-3z M2 27h22v1h-22z M27 27h3v1h-3z M2 28h22v1h-22z M27 28h3v1h-3z";

/** The flat mark at 32 px, for the top bar. */
export function QuorumMark() {
  return (
    <svg
      className="hc-quorum-mark"
      viewBox="0 0 32 32"
      width="32"
      height="32"
      aria-hidden="true"
      focusable="false"
      data-quorum="flat-32"
    >
      <path fill="currentColor" shapeRendering="crispEdges" d={FLAT_32} />
    </svg>
  );
}

/** The brushed-glass mark (64 px or more): both surfaces, CSS shows one. */
export function QuorumGlass() {
  return (
    <span className="hc-quorum-glass" data-quorum="glass">
      {/* Plain <img>: a fixed-size same-origin SVG in a static export, with
          no Next image optimisation to serve it. */}
      {/* eslint-disable-next-line @next/next/no-img-element */}
      <img
        className="hc-quorum-glass-img"
        data-surface="light"
        src={brandFiles.glassLight}
        alt=""
        width={96}
        height={96}
        decoding="async"
      />
      {/* eslint-disable-next-line @next/next/no-img-element */}
      <img
        className="hc-quorum-glass-img"
        data-surface="dark"
        src={brandFiles.glassDark}
        alt=""
        width={96}
        height={96}
        decoding="async"
      />
    </span>
  );
}
