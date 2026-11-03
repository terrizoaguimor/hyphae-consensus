// SPDX-License-Identifier: Apache-2.0
//! The static web export served by `serve-public`, with a per-page
//! Content-Security-Policy (ABUSE-5).
//!
//! A static export cannot carry nonces, so every inline script and style
//! is allowed by its SHA-256, page by page, from `csp-manifest.json`
//! (written by `apps/web/scripts/csp-manifest.mjs` after the build). A
//! page the manifest does not know gets the base policy with no hashes:
//! its inline scripts are blocked, never allowed with `unsafe-inline`.
//!
//! Paths are decoded and checked segment by segment before any file is
//! touched: no `..`, no empty segment, no backslash, no NUL, no drive
//! letter; the resolved file must stay under the canonical root.

use axum::body::Body;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Origins the Turnstile widget needs once live mode is wired.
pub const TURNSTILE_ORIGIN: &str = "https://challenges.cloudflare.com";

#[derive(Debug, Deserialize)]
struct CspManifest {
    #[serde(default)]
    pages: HashMap<String, CspPage>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CspPage {
    #[serde(default)]
    script_hashes: Vec<String>,
    #[serde(default)]
    style_hashes: Vec<String>,
}

/// The export directory and its CSP manifest.
pub struct StaticSite {
    root: PathBuf,
    pages: HashMap<String, CspPage>,
}

/// The CSP header for a page with the given inline hashes.
pub fn csp_header(script_hashes: &[String], style_hashes: &[String]) -> String {
    let mut script_src = String::from("'self'");
    for hash in script_hashes {
        script_src.push_str(&format!(" '{hash}'"));
    }
    script_src.push(' ');
    script_src.push_str(TURNSTILE_ORIGIN);
    let mut style_src = String::from("'self'");
    for hash in style_hashes {
        style_src.push_str(&format!(" '{hash}'"));
    }
    format!(
        "default-src 'none'; script-src {script_src}; frame-src {TURNSTILE_ORIGIN}; connect-src 'self'; manifest-src 'self'; style-src {style_src}; style-src-attr 'unsafe-inline'; img-src 'self' data:; font-src 'self'; base-uri 'none'; form-action 'self'; object-src 'none'; frame-ancestors 'none'"
    )
}

/// `sha256-<base64>` of an inline body, as a CSP source expression.
pub fn csp_hash(body: &str) -> String {
    let digest = Sha256::digest(body.as_bytes());
    format!("sha256-{}", base64(&digest))
}

fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Bodies of the inline `<script>` and `<style>` elements of an HTML
/// document, for hashing. Good enough for the operator console, which
/// has one of each; the web export uses the build's manifest instead.
pub fn inline_bodies(html: &str, tag: &str) -> Vec<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut bodies = Vec::new();
    let mut rest = html;
    while let Some(start) = rest.find(&open) {
        let after_open = &rest[start..];
        let Some(gt) = after_open.find('>') else {
            break;
        };
        let body_start = gt + 1;
        let Some(end) = after_open[body_start..].find(&close) else {
            break;
        };
        // An external script (src=) has an empty body: nothing to hash.
        let attrs = &after_open[..gt];
        let body = &after_open[body_start..body_start + end];
        if !attrs.contains(" src=") && !body.is_empty() {
            bodies.push(body.to_string());
        }
        rest = &after_open[body_start + end + close.len()..];
    }
    bodies
}

/// Percent-decodes a path, refusing anything that is not valid UTF-8.
fn percent_decode(path: &str) -> Option<String> {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let value = u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
            out.push(value);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Splits a request path into safe segments, or None when any segment
/// could escape the root.
pub fn safe_segments(path: &str) -> Option<Vec<String>> {
    let decoded = percent_decode(path)?;
    if decoded.contains('\0') || decoded.contains('\\') || decoded.contains(':') {
        return None;
    }
    let mut segments = Vec::new();
    for seg in decoded.split('/') {
        if seg.is_empty() {
            continue;
        }
        if seg == "." || seg == ".." {
            return None;
        }
        segments.push(seg.to_string());
    }
    Some(segments)
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "woff" => "font/woff",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    }
}

/// The manifest key of an exported file: `index.html` -> `/`,
/// `en/index.html` -> `/en/`, `404.html` -> `/404.html`.
fn page_key(relative: &Path) -> String {
    let posix: Vec<String> = relative
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    let joined = posix.join("/");
    if joined == "index.html" {
        "/".into()
    } else if let Some(dir) = joined.strip_suffix("/index.html") {
        format!("/{dir}/")
    } else {
        format!("/{joined}")
    }
}

impl StaticSite {
    /// Opens the export at `root`; a missing manifest leaves every page
    /// without hashes (and is reported by the caller).
    pub fn load(root: &Path, manifest: &Path) -> Result<(Self, Option<String>), String> {
        let root = root
            .canonicalize()
            .map_err(|e| format!("web root {}: {e}", root.display()))?;
        let (pages, warning) = match std::fs::read(manifest) {
            Ok(bytes) => {
                let parsed: CspManifest = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("csp manifest {}: {e}", manifest.display()))?;
                (parsed.pages, None)
            }
            Err(e) => (
                HashMap::new(),
                Some(format!(
                    "csp manifest {} unreadable ({e}): pages get the base policy without inline hashes",
                    manifest.display()
                )),
            ),
        };
        Ok((Self { root, pages }, warning))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The file a request path maps to, and its manifest key.
    fn resolve(&self, path: &str) -> Option<(PathBuf, String)> {
        let segments = safe_segments(path)?;
        let mut candidate = self.root.clone();
        for seg in &segments {
            candidate.push(seg);
        }
        let trailing_slash = path.ends_with('/') || segments.is_empty();
        let file = if candidate.is_dir() {
            candidate.join("index.html")
        } else if candidate.is_file() {
            if trailing_slash {
                return None;
            }
            candidate
        } else if !trailing_slash && candidate.extension().is_none() {
            let html = candidate.with_extension("html");
            if html.is_file() {
                html
            } else {
                let index = candidate.join("index.html");
                if index.is_file() {
                    index
                } else {
                    return None;
                }
            }
        } else {
            return None;
        };
        let canonical = file.canonicalize().ok()?;
        if !canonical.starts_with(&self.root) {
            return None;
        }
        let relative = canonical.strip_prefix(&self.root).ok()?.to_path_buf();
        Some((canonical, page_key(&relative)))
    }

    fn csp_for(&self, key: &str) -> String {
        let page = self.pages.get(key).cloned().unwrap_or_default();
        csp_header(&page.script_hashes, &page.style_hashes)
    }

    fn file_response(
        &self,
        file: &Path,
        key: &str,
        status: StatusCode,
        request_headers: &HeaderMap,
    ) -> Response {
        let bytes = match std::fs::read(file) {
            Ok(bytes) => bytes,
            Err(_) => return (StatusCode::NOT_FOUND, "not found").into_response(),
        };
        let etag = format!("\"{}\"", hex(&Sha256::digest(&bytes)));
        let kind = content_type(file);
        let is_html = kind.starts_with("text/html");
        let cache = if key.starts_with("/_next/static/") {
            "public, max-age=31536000, immutable"
        } else if is_html {
            "no-cache"
        } else {
            "public, max-age=3600"
        };
        if status == StatusCode::OK
            && request_headers
                .get(header::IF_NONE_MATCH)
                .and_then(|v| v.to_str().ok())
                .is_some_and(|v| v.split(',').any(|t| t.trim() == etag))
        {
            let mut response = StatusCode::NOT_MODIFIED.into_response();
            set(response.headers_mut(), header::ETAG, &etag);
            set(response.headers_mut(), header::CACHE_CONTROL, cache);
            return response;
        }
        let mut response = Response::builder()
            .status(status)
            .body(Body::from(bytes))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        let headers = response.headers_mut();
        set(headers, header::CONTENT_TYPE, kind);
        set(headers, header::ETAG, &etag);
        set(headers, header::CACHE_CONTROL, cache);
        if is_html {
            set(headers, header::CONTENT_SECURITY_POLICY, &self.csp_for(key));
        }
        response
    }

    /// Serves `path`, or the export's `404.html` (status 404) when there
    /// is none.
    pub fn serve(&self, path: &str, request_headers: &HeaderMap) -> Response {
        if let Some((file, key)) = self.resolve(path) {
            return self.file_response(&file, &key, StatusCode::OK, request_headers);
        }
        let not_found = self.root.join("404.html");
        if not_found.is_file() {
            return self.file_response(
                &not_found,
                "/404.html",
                StatusCode::NOT_FOUND,
                request_headers,
            );
        }
        let mut response = (StatusCode::NOT_FOUND, "not found").into_response();
        set(response.headers_mut(), header::CACHE_CONTROL, "no-store");
        response
    }
}

fn set(headers: &mut HeaderMap, name: header::HeaderName, value: &str) {
    if let Ok(v) = HeaderValue::from_str(value) {
        headers.insert(name, v);
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn site() -> (StaticSite, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "hyphae-static-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(dir.join("en")).unwrap();
        std::fs::create_dir_all(dir.join("_next/static")).unwrap();
        std::fs::write(dir.join("index.html"), "<html><script>a()</script></html>").unwrap();
        std::fs::write(dir.join("en/index.html"), "<html>en</html>").unwrap();
        std::fs::write(dir.join("about.html"), "<html>about</html>").unwrap();
        std::fs::write(dir.join("404.html"), "<html>missing</html>").unwrap();
        std::fs::write(dir.join("_next/static/app.js"), "console.log(1)").unwrap();
        std::fs::write(dir.join("secret.txt"), "x").unwrap();
        let manifest = dir.join("csp-manifest.json");
        std::fs::write(
            &manifest,
            serde_json::json!({
                "version": 1,
                "pages": {
                    "/": {"scriptHashes": [csp_hash("a()")], "styleHashes": []},
                    "/en/": {"scriptHashes": [], "styleHashes": ["sha256-x"]}
                }
            })
            .to_string(),
        )
        .unwrap();
        let (site, warning) = StaticSite::load(&dir, &manifest).unwrap();
        assert!(warning.is_none());
        (site, dir)
    }

    #[test]
    fn csp_hash_matches_the_specification_example() {
        assert_eq!(
            csp_hash("alert('Hello, world.');"),
            "sha256-qznLcsROx4GACP2dm0UCKCzCG+HiZ1guq6ZZDob/Tng="
        );
        assert_eq!(
            csp_hash(""),
            "sha256-47DEQpj8HBSa+/TImW+5JCeuQeRkm5NMpJWZG3hSuFU="
        );
    }

    #[test]
    fn pages_get_their_own_hashes_and_unknown_pages_get_none() {
        let (site, dir) = site();
        let headers = HeaderMap::new();
        let root = site.serve("/", &headers);
        assert_eq!(root.status(), StatusCode::OK);
        let csp = root.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .to_string();
        assert!(csp.contains(&format!("'{}'", csp_hash("a()"))), "{csp}");
        assert!(csp.contains("frame-ancestors 'none'") && !csp.contains("unsafe-eval"));
        assert!(csp.contains("style-src-attr 'unsafe-inline'"));
        // The web app manifest is a subresource of every page.
        assert!(csp.contains("manifest-src 'self'"), "{csp}");
        let en = site.serve("/en/", &headers);
        let csp = en.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .to_string();
        assert!(csp.contains("'sha256-x'") && !csp.contains(&csp_hash("a()")));
        let about = site.serve("/about", &headers);
        assert_eq!(about.status(), StatusCode::OK);
        let csp = about.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .to_string();
        assert!(
            csp.starts_with(
                "default-src 'none'; script-src 'self' https://challenges.cloudflare.com;"
            ),
            "{csp}"
        );
        let js = site.serve("/_next/static/app.js", &headers);
        assert_eq!(
            js.headers()[header::CACHE_CONTROL],
            "public, max-age=31536000, immutable"
        );
        assert!(js.headers().get(header::CONTENT_SECURITY_POLICY).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn search_and_icon_files_are_served_with_their_own_types() {
        let (site, dir) = site();
        std::fs::write(dir.join("robots.txt"), "User-Agent: *\nAllow: /\n").unwrap();
        std::fs::write(dir.join("sitemap.xml"), "<urlset/>").unwrap();
        std::fs::write(dir.join("manifest.webmanifest"), "{}").unwrap();
        std::fs::write(dir.join("favicon.ico"), [0u8, 0, 1, 0]).unwrap();
        let headers = HeaderMap::new();
        for (path, kind) in [
            ("/robots.txt", "text/plain; charset=utf-8"),
            ("/sitemap.xml", "application/xml"),
            ("/manifest.webmanifest", "application/manifest+json"),
            ("/favicon.ico", "image/x-icon"),
        ] {
            let response = site.serve(path, &headers);
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            assert_eq!(response.headers()[header::CONTENT_TYPE], kind, "{path}");
            // Not content-addressed: revalidated within the hour, never immutable.
            assert_eq!(
                response.headers()[header::CACHE_CONTROL],
                "public, max-age=3600",
                "{path}"
            );
            // The CSP belongs to HTML responses only.
            assert!(
                response
                    .headers()
                    .get(header::CONTENT_SECURITY_POLICY)
                    .is_none(),
                "{path}"
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn traversal_is_refused_and_missing_pages_get_the_404_page() {
        let (site, dir) = site();
        let headers = HeaderMap::new();
        for bad in [
            "/../Cargo.toml",
            "/%2e%2e/Cargo.toml",
            "/en/..%2F..%2Fsecret.txt",
            "/C:/Windows/win.ini",
            "/en%00/",
            "/\\secret.txt",
        ] {
            let response = site.serve(bad, &headers);
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{bad}");
        }
        let missing = site.serve("/nope/", &headers);
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert!(missing
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn etags_answer_304() {
        let (site, dir) = site();
        let first = site.serve("/en/", &HeaderMap::new());
        let etag = first.headers()[header::ETAG].clone();
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_NONE_MATCH, etag);
        assert_eq!(
            site.serve("/en/", &headers).status(),
            StatusCode::NOT_MODIFIED
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn inline_bodies_find_the_console_script_and_style() {
        let html = "<html><head><style>p{}</style></head><body><script src=\"/x.js\"></script><script>\n'use strict';\n</script></body></html>";
        assert_eq!(inline_bodies(html, "style"), ["p{}"]);
        assert_eq!(inline_bodies(html, "script"), ["\n'use strict';\n"]);
    }
}
