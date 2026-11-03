// SPDX-License-Identifier: Apache-2.0

/**
 * The same page in another locale: replaces a leading locale segment of
 * `pathname` with `target`, or prefixes `target` when there is none. Always
 * ends in a slash, matching the export's `trailingSlash: true`.
 */
export function switchLocalePath(
  pathname: string | null,
  target: string,
  locales: readonly string[],
): string {
  const segments = (pathname ?? "/").split("/").filter(Boolean);
  const [first] = segments;
  if (first !== undefined && locales.includes(first)) {
    segments.shift();
  }
  return `/${[target, ...segments].join("/")}/`;
}

/**
 * Appends the current page's query string and hash to a locale path, so a
 * language switch keeps the page's state (the run page reads ?id=...&source=).
 * Only what is already in the URL is carried over; an empty or bare "?" / "#"
 * adds nothing, so pages without a query keep their plain path.
 */
export function withLocation(
  path: string,
  search: string,
  hash: string,
): string {
  const query = search.length > 1 && search.startsWith("?") ? search : "";
  const fragment = hash.length > 1 && hash.startsWith("#") ? hash : "";
  return `${path}${query}${fragment}`;
}
