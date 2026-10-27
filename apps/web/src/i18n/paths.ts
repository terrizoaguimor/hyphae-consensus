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
