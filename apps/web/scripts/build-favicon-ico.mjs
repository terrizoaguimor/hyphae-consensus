#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// Builds public/favicon.ico (16, 32 and 48 px) from the brand PNGs, so that
// GET /favicon.ico, which crawlers and older browsers ask for whatever the
// page links, answers with a real icon:
//
//   sh apps/web/scripts/in-docker.sh node scripts/build-favicon-ico.mjs
//
// The 16 and 32 px images are public/favicon-16.png and favicon-32.png as
// they are. The 48 px image is the 16 px cut (the pixel-art mark-flat-16 of
// favicon.svg, which its own comment calls crisp at 16, 32 and 48 device px)
// enlarged by whole pixels, 3 x 3 each, so no pixel is blended. Each image is
// stored as a PNG inside the ICO container (supported since Windows Vista and
// by every browser). The output is deterministic: the same inputs give the
// same bytes. tests/brand-assets.test.ts checks the file.

import { readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { crc32, deflateSync, inflateSync } from "node:zlib";

const publicDir = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "..",
  "public",
);

/** Decodes an 8-bit RGBA, non-interlaced PNG: { width, height, pixels }. */
function readPng(data) {
  if (data.subarray(0, 8).toString("hex") !== "89504e470d0a1a0a") {
    throw new Error("not a PNG");
  }
  let pos = 8;
  let width = 0;
  let height = 0;
  const idat = [];
  while (pos < data.length) {
    const length = data.readUInt32BE(pos);
    const kind = data.toString("latin1", pos + 4, pos + 8);
    const body = data.subarray(pos + 8, pos + 8 + length);
    if (kind === "IHDR") {
      width = body.readUInt32BE(0);
      height = body.readUInt32BE(4);
      if (body[8] !== 8 || body[9] !== 6 || body[12] !== 0) {
        throw new Error("need an 8-bit RGBA, non-interlaced PNG");
      }
    } else if (kind === "IDAT") {
      idat.push(body);
    }
    pos += 12 + length;
  }
  const raw = inflateSync(Buffer.concat(idat));
  const stride = width * 4;
  const pixels = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y += 1) {
    const filter = raw[y * (stride + 1)];
    const line = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    const out = pixels.subarray(y * stride, (y + 1) * stride);
    const prev =
      y > 0 ? pixels.subarray((y - 1) * stride, y * stride) : Buffer.alloc(stride);
    for (let x = 0; x < stride; x += 1) {
      const a = x >= 4 ? out[x - 4] : 0;
      const b = prev[x];
      const c = x >= 4 ? prev[x - 4] : 0;
      let predictor = 0;
      if (filter === 1) predictor = a;
      else if (filter === 2) predictor = b;
      else if (filter === 3) predictor = (a + b) >> 1;
      else if (filter === 4) {
        const p = a + b - c;
        const pa = Math.abs(p - a);
        const pb = Math.abs(p - b);
        const pc = Math.abs(p - c);
        predictor = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      } else if (filter !== 0) {
        throw new Error(`PNG filter ${filter}`);
      }
      out[x] = (line[x] + predictor) & 255;
    }
  }
  return { width, height, pixels };
}

function chunk(kind, body) {
  const head = Buffer.alloc(8);
  head.writeUInt32BE(body.length, 0);
  head.write(kind, 4, "latin1");
  const tail = Buffer.alloc(4);
  tail.writeUInt32BE(crc32(Buffer.concat([head.subarray(4), body])), 0);
  return Buffer.concat([head, body, tail]);
}

/** Encodes RGBA pixels as an 8-bit, non-interlaced PNG (filter 0). */
function writePng({ width, height, pixels }) {
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0);
  ihdr.writeUInt32BE(height, 4);
  ihdr.set([8, 6, 0, 0, 0], 8);
  const stride = width * 4;
  const raw = Buffer.alloc((stride + 1) * height);
  for (let y = 0; y < height; y += 1) {
    pixels.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride);
  }
  return Buffer.concat([
    Buffer.from("89504e470d0a1a0a", "hex"),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(raw, { level: 9 })),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

/** Every source pixel becomes a factor x factor block. */
function enlarge({ width, height, pixels }, factor) {
  const out = Buffer.alloc(width * factor * height * factor * 4);
  for (let y = 0; y < height * factor; y += 1) {
    for (let x = 0; x < width * factor; x += 1) {
      pixels.copy(
        out,
        (y * width * factor + x) * 4,
        (Math.floor(y / factor) * width + Math.floor(x / factor)) * 4,
        (Math.floor(y / factor) * width + Math.floor(x / factor)) * 4 + 4,
      );
    }
  }
  return { width: width * factor, height: height * factor, pixels: out };
}

/** An ICO file holding these PNG images (each { size, png }). */
function writeIco(images) {
  const header = Buffer.alloc(6);
  header.writeUInt16LE(1, 2); // type: icon
  header.writeUInt16LE(images.length, 4);
  let offset = 6 + 16 * images.length;
  const entries = images.map(({ size, png }) => {
    const entry = Buffer.alloc(16);
    entry[0] = size; // width (0 would mean 256)
    entry[1] = size;
    entry.writeUInt16LE(1, 4); // colour planes
    entry.writeUInt16LE(32, 6); // bits per pixel
    entry.writeUInt32LE(png.length, 8);
    entry.writeUInt32LE(offset, 12);
    offset += png.length;
    return entry;
  });
  return Buffer.concat([header, ...entries, ...images.map(({ png }) => png)]);
}

const png16 = await readFile(path.join(publicDir, "favicon-16.png"));
const png32 = await readFile(path.join(publicDir, "favicon-32.png"));
const source = readPng(png16);
if (source.width !== 16 || source.height !== 16) {
  throw new Error("favicon-16.png must be 16 x 16");
}
const ico = writeIco([
  { size: 16, png: png16 },
  { size: 32, png: png32 },
  { size: 48, png: writePng(enlarge(source, 3)) },
]);
await writeFile(path.join(publicDir, "favicon.ico"), ico);
console.log(`favicon.ico: ${ico.length} bytes (16, 32, 48 px)`);
