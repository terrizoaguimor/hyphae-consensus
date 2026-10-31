# SPDX-License-Identifier: Apache-2.0
"""Build the web app's brand files (apps/web/public) from the Quorum logo set.

The source is the final logo folder (design/logo/quorum-final, v1.0,
2026-10-31; its README fixes the sizes and the rule "below 64 px, always
flat"). This script copies what the app links and derives what the set does
not hold at the size the app needs:

    favicon.svg               copy (the 16 px cut; its own <style> switches
                              the ink for a dark browser, so it is linked,
                              never inlined: the app CSP is hash-only)
    favicon-16.png, -32.png   copies (ink #141414, transparent)
    apple-touch-icon.png      copy of apple-touch-icon-180.png (dark tile)
    icon-1024.png, icon-512.png
                              app icon, dark tile (app-icon-1024-dark.png;
                              512 is a 2x2 box downscale of it)
    icon-1024-light.png, icon-512-light.png
                              the same for the light tile
    brand/quorum-glass-64-dark.svg, brand/quorum-glass-64-light.svg
                              copies of mark-glass-64.svg and
                              mark-glass-64-light.svg (the small glass, for
                              marks of 64 to 192 px)
    og-image.png              Open Graph image, 1200 x 630, greys only: the
                              dark stage of the recipe (radial #2B2B2B,
                              #121212 at .55, #060606, a soft white backlight)
                              with the stacked lockup at mark 256 px: the
                              1024 glass PNG (mark-glass-1024.png, which
                              carries the frost) downscaled 4x, and the
                              outlined wordmark of lockup-stacked-white.svg
                              rasterised in ink #F2F2F2.

Standard library only (zlib, struct). Every pixel it writes has R = G = B;
it stops if a source PNG holds a pixel with a hue.

Usage (from the repository root):
    python -I scripts/web_brand_assets.py <quorum-final directory>
"""

import math
import pathlib
import re
import shutil
import struct
import sys
import zlib

OUT = pathlib.Path(__file__).resolve().parent.parent / "apps" / "web" / "public"

COPIES = {
    "favicon.svg": "favicon.svg",
    "favicon-16.png": "favicon-16.png",
    "favicon-32.png": "favicon-32.png",
    "apple-touch-icon-180.png": "apple-touch-icon.png",
    "app-icon-1024-dark.png": "icon-1024.png",
    "app-icon-1024-light.png": "icon-1024-light.png",
    "mark-glass-64.svg": "brand/quorum-glass-64-dark.svg",
    "mark-glass-64-light.svg": "brand/quorum-glass-64-light.svg",
}

# --- PNG ---------------------------------------------------------------


def read_png(path):
    """(width, height, rows of RGBA bytes) of an 8-bit RGBA PNG."""
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise SystemExit(f"{path}: not a PNG")
    pos, idat, header = 8, bytearray(), None
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind = data[pos + 4 : pos + 8]
        body = data[pos + 8 : pos + 8 + length]
        pos += 12 + length
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat += body
    width, height, depth, colour, _, _, interlace = header
    if (depth, colour, interlace) != (8, 6, 0):
        raise SystemExit(f"{path}: only 8-bit RGBA, non-interlaced")
    raw = zlib.decompress(bytes(idat))
    stride, bpp = width * 4, 4
    rows, prev, i = [], bytearray(stride), 0
    for _ in range(height):
        kind = raw[i]
        line = bytearray(raw[i + 1 : i + 1 + stride])
        i += 1 + stride
        if kind == 1:
            for x in range(bpp, stride):
                line[x] = (line[x] + line[x - bpp]) & 255
        elif kind == 2:
            for x in range(stride):
                line[x] = (line[x] + prev[x]) & 255
        elif kind == 3:
            for x in range(stride):
                left = line[x - bpp] if x >= bpp else 0
                line[x] = (line[x] + ((left + prev[x]) >> 1)) & 255
        elif kind == 4:
            for x in range(stride):
                a = line[x - bpp] if x >= bpp else 0
                b = prev[x]
                c = prev[x - bpp] if x >= bpp else 0
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                pred = a if pa <= pb and pa <= pc else (b if pb <= pc else c)
                line[x] = (line[x] + pred) & 255
        rows.append(line)
        prev = line
    return width, height, rows


def write_png(path, width, height, rows):
    """Writes 8-bit RGBA rows (filter 'sub' on every row)."""
    raw = bytearray()
    for line in rows:
        raw.append(1)
        out = bytearray(line)
        for x in range(len(line) - 1, 3, -1):
            out[x] = (line[x] - line[x - 4]) & 255
        raw += out

    def chunk(kind, body):
        return (
            struct.pack(">I", len(body))
            + kind
            + body
            + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)
        )

    ihdr = struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def assert_grey(name, rows):
    for y, line in enumerate(rows):
        for x in range(0, len(line), 4):
            if not (line[x] == line[x + 1] == line[x + 2]):
                raise SystemExit(f"{name}: pixel ({x // 4}, {y}) has a hue")


def downscale(width, height, rows, factor):
    """Box downscale by an integer factor, in premultiplied alpha."""
    w, h = width // factor, height // factor
    n = factor * factor
    out = []
    for y in range(h):
        line = bytearray(w * 4)
        for x in range(w):
            grey = alpha = 0
            for dy in range(factor):
                src = rows[y * factor + dy]
                for dx in range(factor):
                    i = (x * factor + dx) * 4
                    a = src[i + 3]
                    grey += src[i] * a
                    alpha += a
            if alpha:
                v = round(grey / alpha)
                line[x * 4 : x * 4 + 3] = bytes((v, v, v))
            line[x * 4 + 3] = round(alpha / n)
        out.append(line)
    return w, h, out


# --- wordmark path -----------------------------------------------------

TOKEN = re.compile(r"[MLHVCZ]|-?\d*\.?\d+(?:e-?\d+)?")


def flatten(d, transform):
    """Closed polygons of an absolute M/L/H/V/C/Z path, transformed."""
    tokens = TOKEN.findall(d)
    polygons, current, i = [], [], 0
    x = y = 0.0
    command = None

    def num():
        nonlocal i
        value = float(tokens[i])
        i += 1
        return value

    while i < len(tokens):
        if tokens[i].isalpha():
            command = tokens[i]
            i += 1
        if command == "M":
            if current:
                polygons.append(current)
            x, y = num(), num()
            current = [(x, y)]
            command = "L"
        elif command == "L":
            x, y = num(), num()
            current.append((x, y))
        elif command == "H":
            x = num()
            current.append((x, y))
        elif command == "V":
            y = num()
            current.append((x, y))
        elif command == "C":
            x1, y1, x2, y2, x3, y3 = (num() for _ in range(6))
            for step in range(1, 17):
                t = step / 16
                u = 1 - t
                current.append(
                    (
                        u * u * u * x + 3 * u * u * t * x1 + 3 * u * t * t * x2 + t * t * t * x3,
                        u * u * u * y + 3 * u * u * t * y1 + 3 * u * t * t * y2 + t * t * t * y3,
                    )
                )
            x, y = x3, y3
        elif command == "Z":
            if current:
                polygons.append(current)
                x, y = current[0]
            current = []
            command = None
        else:
            raise SystemExit(f"path command {command!r} not handled")
    if current:
        polygons.append(current)
    return [[transform(px, py) for px, py in polygon] for polygon in polygons]


def coverage(polygons, width, height, sub=8):
    """Anti-aliased nonzero coverage (0..1) per pixel, exact along x."""
    cover = [[0.0] * width for _ in range(height)]
    edges = []
    for polygon in polygons:
        for (x0, y0), (x1, y1) in zip(polygon, polygon[1:] + polygon[:1]):
            if y0 != y1:
                edges.append((x0, y0, x1, y1))
    for py in range(height):
        for s in range(sub):
            yy = py + (s + 0.5) / sub
            crossings = []
            for x0, y0, x1, y1 in edges:
                if (y0 <= yy < y1) or (y1 <= yy < y0):
                    xx = x0 + (yy - y0) * (x1 - x0) / (y1 - y0)
                    crossings.append((xx, 1 if y1 > y0 else -1))
            crossings.sort()
            winding = 0
            row = cover[py]
            for index, (xx, direction) in enumerate(crossings):
                before = winding
                winding += direction
                if before == 0 and winding != 0:
                    start = xx
                elif before != 0 and winding == 0:
                    a, b = max(start, 0.0), min(xx, float(width))
                    if b <= a:
                        continue
                    ia, ib = int(a), int(b)
                    if ia == ib:
                        row[ia] += (b - a) / sub
                    else:
                        row[ia] += (ia + 1 - a) / sub
                        for px in range(ia + 1, ib):
                            row[px] += 1 / sub
                        if ib < width:
                            row[ib] += (b - ib) / sub
    return cover


# --- Open Graph image --------------------------------------------------


def og_image(source):
    width, height = 1200, 630
    scale = 4.0  # 64-grid units to pixels: the mark is 256 px tall
    lockup = (source / "lockup-stacked-white.svg").read_text(encoding="utf8")
    lockup_w, lockup_h = (float(v) for v in re.search(r'viewBox="0 0 ([\d.]+) ([\d.]+)"', lockup).groups())
    mark_x = float(re.search(r'<g transform="translate\(([\d.]+) 0\)">', lockup).group(1))
    word = re.search(
        r'<path fill="#FFFFFF" transform="translate\(([\d.]+) ([\d.]+)\) scale\(([\d.]+)\)" d="([^"]+)"',
        lockup,
    )
    wx, wy, ws, d = float(word.group(1)), float(word.group(2)), float(word.group(3)), word.group(4)
    left = (width - lockup_w * scale) / 2
    top = (height - lockup_h * scale) / 2

    # Stage: radial #2B2B2B, #121212 at .55, #060606, centred on the mark.
    cx, cy = width / 2, top + 32 * scale
    stops = [(0.0, 0x2B), (0.55, 0x12), (1.0, 0x06)]

    def stage(t):
        t = min(max(t, 0.0), 1.0)
        for (t0, v0), (t1, v1) in zip(stops, stops[1:]):
            if t <= t1:
                return v0 + (v1 - v0) * (t - t0) / (t1 - t0)
        return stops[-1][1]

    rx, ry = 760.0, 520.0
    grey = [[0.0] * width for _ in range(height)]
    for y in range(height):
        for x in range(width):
            dist = math.hypot((x + 0.5 - cx) / rx, (y + 0.5 - cy) / ry)
            value = stage(dist)
            # Backlight: white 10 % ellipse, soft (the recipe's blur 240).
            glow = 0.10 * math.exp(-(((x + 0.5 - cx) / 300) ** 2 + ((y + 0.5 - cy) / 200) ** 2) * 1.6)
            grey[y][x] = value + (255 - value) * glow

    # Glass mark: the 1024 master (with frost) at 256 px.
    mw, mh, mark = read_png(source / "mark-glass-1024.png")
    assert_grey("mark-glass-1024.png", mark)
    mw, mh, mark = downscale(mw, mh, mark, 4)
    ox, oy = round(left + mark_x * scale), round(top)
    for y in range(mh):
        line = mark[y]
        for x in range(mw):
            a = line[x * 4 + 3] / 255
            if a:
                gx, gy = ox + x, oy + y
                if 0 <= gx < width and 0 <= gy < height:
                    grey[gy][gx] = line[x * 4] * a + grey[gy][gx] * (1 - a)

    # Wordmark, flat ink #F2F2F2.
    def transform(px, py):
        return left + (wx + px * ws) * scale, top + (wy + py * ws) * scale

    polygons = flatten(d, transform)
    ys = [p[1] for polygon in polygons for p in polygon]
    y0, y1 = max(int(min(ys)) - 1, 0), min(int(max(ys)) + 2, height)
    shifted = [[(px, py - y0) for px, py in polygon] for polygon in polygons]
    cover = coverage(shifted, width, y1 - y0)
    for y in range(y1 - y0):
        for x in range(width):
            c = min(cover[y][x], 1.0)
            if c:
                grey[y0 + y][x] = 0xF2 * c + grey[y0 + y][x] * (1 - c)

    # Ordered dither against banding in the dark gradient; still R = G = B.
    bayer = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]
    rows = []
    for y in range(height):
        line = bytearray(width * 4)
        for x in range(width):
            v = int(grey[y][x] + (bayer[y % 4][x % 4] + 0.5) / 16)
            v = min(max(v, 0), 255)
            line[x * 4 : x * 4 + 4] = bytes((v, v, v, 255))
        rows.append(line)
    return width, height, rows


def main():
    if len(sys.argv) != 2:
        raise SystemExit(__doc__)
    source = pathlib.Path(sys.argv[1])
    for src, dst in COPIES.items():
        target = OUT / dst
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source / src, target)
        if src.endswith(".png"):
            assert_grey(src, read_png(target)[2])
        print(f"{dst} <- {src}")
    for tile, name in (("app-icon-1024-dark.png", "icon-512.png"), ("app-icon-1024-light.png", "icon-512-light.png")):
        w, h, rows = downscale(*read_png(source / tile), 2)
        assert_grey(name, rows)
        write_png(OUT / name, w, h, rows)
        print(f"{name} <- {tile} (2x2 box)")
    w, h, rows = og_image(source)
    assert_grey("og-image.png", rows)
    write_png(OUT / "og-image.png", w, h, rows)
    print("og-image.png <- mark-glass-1024.png + lockup-stacked-white.svg")


if __name__ == "__main__":
    main()
