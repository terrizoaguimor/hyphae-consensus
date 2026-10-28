// SPDX-License-Identifier: Apache-2.0
//
// SHA-256 in the browser, through WebCrypto (crypto.subtle, available in a
// secure context: HTTPS, or 127.0.0.1 / localhost), and in Node 24 alike, so
// the same code runs in the views and in the unit tests. A digest computed
// here is a digest of the bytes as received: nothing is decoded or
// re-encoded first. Comparing it with one published elsewhere (the archive's
// manifest, a receipt) is only as strong as that other copy.

export type Digest = (data: Uint8Array) => Promise<ArrayBuffer>;

/** SHA-256 through WebCrypto; throws when this context has none. */
export const sha256: Digest = (data) => {
  const subtle = globalThis.crypto?.subtle;
  if (subtle === undefined) {
    return Promise.reject(new Error("WebCrypto (crypto.subtle) is not available in this context"));
  }
  return subtle.digest("SHA-256", data as unknown as BufferSource);
};

/** Lowercase hex of a digest. */
export function hex(buffer: ArrayBuffer): string {
  return [...new Uint8Array(buffer)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

export async function sha256Hex(bytes: Uint8Array, digest: Digest = sha256): Promise<string> {
  return hex(await digest(bytes));
}

/** The bytes' SHA-256 and whether it is the expected one (lowercase hex). */
export async function verifyDigest(
  bytes: Uint8Array,
  expected: string,
  digest: Digest = sha256,
): Promise<{ sha256: string; matches: boolean }> {
  const computed = await sha256Hex(bytes, digest);
  return { sha256: computed, matches: computed === expected.trim().toLowerCase() };
}
