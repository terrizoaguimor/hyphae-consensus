#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// Minimal static server for the end-to-end tests, on Node's http module
// alone (no dependency):
//
//   node scripts/serve-static.mjs [--dir out] [--host 127.0.0.1] [--port 4173] [--csp]
//
// It serves the export the way serve-public is planned to: a directory path
// serves its index.html, a page path without its trailing slash redirects
// (308) to the slashed form, a missing file answers 404 with out/404.html, and
// with --csp every HTML response carries that page's Content-Security-Policy,
// built from out/csp-manifest.json (scripts/csp-manifest.mjs). Nothing outside
// the served directory is reachable: the path is decoded, normalized and
// checked to stay inside it.

import { createReadStream } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import http from "node:http";
import path from "node:path";
import { pathToFileURL } from "node:url";

import { MANIFEST_NAME, pagePath } from "./csp-manifest.mjs";

const CONTENT_TYPES = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".txt": "text/plain; charset=utf-8",
  ".xml": "application/xml",
  ".map": "application/json; charset=utf-8",
  ".woff2": "font/woff2",
  ".woff": "font/woff",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".webmanifest": "application/manifest+json",
};

/**
 * Maps a request path to a file under the served root (as a POSIX path
 * relative to it), a redirect, or null when it can never be served.
 * `isDirectory(rel)` and `isFile(rel)` answer for paths relative to the root.
 */
export async function resolveRequest(urlPath, { isDirectory, isFile }) {
  let decoded;
  try {
    decoded = decodeURIComponent(urlPath);
  } catch {
    return null;
  }
  if (decoded.includes("\0") || decoded.includes("\\")) {
    return null;
  }
  const normalized = path.posix.normalize(decoded);
  if (!normalized.startsWith("/") || normalized.split("/").includes("..")) {
    return null;
  }
  const rel = normalized.replace(/^\/+/, "");
  if (rel === "" || normalized.endsWith("/")) {
    const index = path.posix.join(rel, "index.html");
    return (await isFile(index)) ? { file: index } : null;
  }
  if (await isFile(rel)) {
    return { file: rel };
  }
  if (await isDirectory(rel)) {
    if (await isFile(path.posix.join(rel, "index.html"))) {
      return { redirect: `/${rel}/` };
    }
  }
  return null;
}

/** The Content-Security-Policy for one page, from its manifest entry. */
export function contentSecurityPolicy(page) {
  const scripts = (page?.scriptHashes ?? []).map((hash) => `'${hash}'`);
  const styles = (page?.styleHashes ?? []).map((hash) => `'${hash}'`);
  return [
    "default-src 'none'",
    ["script-src 'self'", ...scripts].join(" "),
    ["style-src 'self'", ...styles].join(" "),
    // Astryx sets style attributes (README, "CSP manifest").
    "style-src-attr 'unsafe-inline'",
    "img-src 'self' data:",
    "font-src 'self'",
    "connect-src 'self'",
    "manifest-src 'self'",
    "base-uri 'none'",
    "form-action 'self'",
    "frame-ancestors 'none'",
    "object-src 'none'",
  ].join("; ");
}

function parseArgs(argv) {
  const options = { dir: "out", host: "127.0.0.1", port: 4173, csp: false };
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--csp") {
      options.csp = true;
    } else if (arg === "--dir" || arg === "--host" || arg === "--port") {
      const value = argv[index + 1];
      if (value === undefined) {
        throw new Error(`${arg} needs a value`);
      }
      options[arg.slice(2)] = arg === "--port" ? Number(value) : value;
      index += 1;
    } else {
      throw new Error(`unknown argument ${arg}`);
    }
  }
  return options;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const root = path.resolve(options.dir);
  const manifest = options.csp
    ? JSON.parse(await readFile(path.join(root, MANIFEST_NAME), "utf8"))
    : null;

  const statOf = async (rel) => {
    const absolute = path.join(root, ...rel.split("/"));
    if (absolute !== root && !absolute.startsWith(root + path.sep)) {
      return null;
    }
    try {
      return await stat(absolute);
    } catch {
      return null;
    }
  };
  const checks = {
    isFile: async (rel) => (await statOf(rel))?.isFile() ?? false,
    isDirectory: async (rel) => (await statOf(rel))?.isDirectory() ?? false,
  };

  const send = (request, response, status, rel) => {
    const extension = path.posix.extname(rel);
    const headers = {
      "Content-Type": CONTENT_TYPES[extension] ?? "application/octet-stream",
      "Cache-Control": "no-store",
      "X-Content-Type-Options": "nosniff",
      "Referrer-Policy": "no-referrer",
    };
    if (manifest !== null && extension === ".html") {
      headers["Content-Security-Policy"] = contentSecurityPolicy(
        manifest.pages[pagePath(rel)],
      );
    }
    response.writeHead(status, headers);
    if (request.method === "HEAD") {
      response.end();
      return;
    }
    createReadStream(path.join(root, ...rel.split("/"))).pipe(response);
  };

  const server = http.createServer(async (request, response) => {
    if (request.method !== "GET" && request.method !== "HEAD") {
      response.writeHead(405, { Allow: "GET, HEAD" }).end();
      return;
    }
    const url = new URL(request.url ?? "/", "http://localhost");
    const resolved = await resolveRequest(url.pathname, checks);
    if (resolved?.redirect !== undefined) {
      response
        .writeHead(308, { Location: `${resolved.redirect}${url.search}` })
        .end();
    } else if (resolved?.file !== undefined) {
      send(request, response, 200, resolved.file);
    } else if (await checks.isFile("404.html")) {
      send(request, response, 404, "404.html");
    } else {
      response.writeHead(404, { "Content-Type": "text/plain" }).end("Not found");
    }
  });

  server.listen(options.port, options.host, () => {
    console.log(
      `serve-static: ${root} on http://${options.host}:${options.port}/${options.csp ? " (with per-page CSP)" : ""}`,
    );
  });
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  await main();
}
