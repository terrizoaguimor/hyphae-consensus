"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The research archive as the views read it: GET /showcase (the entries,
// each derived by the server from the archived chains, and the manifest's
// path and SHA-256), and, when asked, the manifest itself
// (research/MANIFEST.json, served at /research/<file>). The manifest's bytes
// are hashed in this browser and compared with the digest GET /showcase
// published; its notes are shown only when they match (a manifest that does
// not is said to differ, and nothing of it is used). Every failure is kept
// as an error to show with a retry, never as a loading line: the listing's
// retry reads both again, the manifest's own retry reads the manifest alone.

import { useCallback, useEffect, useMemo, useState } from "react";

import { verifyDigest } from "@/lib/chain/digest";
import { api, ApiError, isApiError } from "@/lib/api/client";
import { archiveManifestSchema } from "@/lib/api/schemas";
import type { ShowcaseV1 } from "@/lib/api/types";
import { type ArchiveManifest, type LogPin, logPin } from "@/lib/showcase";

export type ManifestState =
  | { status: "loading" }
  | { status: "error"; error: ApiError }
  /** `manifest` is null when the bytes did not match, or did not parse as the archive's format. */
  | { status: "ready"; sha256: string; matches: boolean; manifest: ArchiveManifest | null };

export type ArchiveState =
  | { status: "loading" }
  | { status: "error"; error: ApiError }
  | { status: "ready"; showcase: ShowcaseV1; manifest: ManifestState };

/** The manifest this browser checked: read, its bytes matching the published digest, and parsed; else null. */
export function checkedManifest(state: ArchiveState): ArchiveManifest | null {
  return state.status === "ready" && state.manifest.status === "ready" && state.manifest.matches
    ? state.manifest.manifest
    : null;
}

/**
 * What the manifest check found, once it has: "matches" only when the bytes
 * match the published digest AND read as the archive's format (the manifest
 * is confirmed); "unparsed" when they match but do not read as that format;
 * "differs" when they do not match. Null while it is checked or after an error.
 */
export function manifestOutcome(state: ManifestState): "matches" | "unparsed" | "differs" | null {
  if (state.status !== "ready") {
    return null;
  }
  return !state.matches ? "differs" : state.manifest === null ? "unparsed" : "matches";
}

/**
 * The archive could not be confirmed in this browser: its manifest could not
 * be read, does not match the published digest, or does not read as the
 * archive's format. False while it is still being checked, and once it is
 * confirmed (checkedManifest is not null).
 */
export function manifestUnconfirmed(state: ArchiveState): boolean {
  if (state.status !== "ready" || state.manifest.status === "loading") {
    return false;
  }
  return checkedManifest(state) === null;
}

/**
 * The pin of one archive file as the views show it: "checking" while the
 * manifest is still being read and checked, else the listing's digest
 * cross-checked against the checked manifest (src/lib/showcase.ts logPin).
 */
export function archivePin(state: ArchiveState, path: string, listing: string): LogPin {
  if (state.status === "ready" && state.manifest.status === "loading") {
    return { status: "checking", sha256: listing };
  }
  return logPin(checkedManifest(state), path, listing);
}

function toApiError(error: unknown, path: string): ApiError {
  return isApiError(error)
    ? error
    : new ApiError({
        code: "unreachable",
        message: error instanceof Error ? error.message : "request failed",
        status: 0,
        path,
      });
}

/** Reads the manifest's bytes and checks them against the digest the server published. */
export async function readManifest(showcase: ShowcaseV1): Promise<ManifestState> {
  try {
    const bytes = await api.bytes(api.researchUrl(showcase.manifest_file));
    const { sha256, matches } = await verifyDigest(bytes, showcase.manifest_sha256);
    let manifest: ArchiveManifest | null = null;
    if (matches) {
      try {
        const parsed = archiveManifestSchema.safeParse(JSON.parse(new TextDecoder().decode(bytes)));
        if (parsed.success) {
          manifest = {
            created: parsed.data.created ?? null,
            files: parsed.data.files.map((file) => ({
              path: file.path,
              sha256: file.sha256,
              kind: file.kind,
              description: file.description ?? null,
            })),
          };
        }
      } catch {
        manifest = null;
      }
    }
    return { status: "ready", sha256, matches, manifest };
  } catch (error) {
    return { status: "error", error: toApiError(error, showcase.manifest_file) };
  }
}

type ListingState =
  | { status: "loading" }
  | { status: "error"; error: ApiError }
  | { status: "ready"; showcase: ShowcaseV1 };

export type Archive = {
  state: ArchiveState;
  /** Reads GET /showcase again (and then the manifest): the listing's retry. */
  reload: () => void;
  /** Reads the manifest again, the listing kept: the manifest's own retry. */
  retryManifest: () => void;
};

export function useArchive(withManifest: boolean): Archive {
  const [listing, setListing] = useState<ListingState>({ status: "loading" });
  const [manifest, setManifest] = useState<ManifestState>({ status: "loading" });
  const [attempt, setAttempt] = useState(0);
  const [manifestAttempt, setManifestAttempt] = useState(0);

  useEffect(() => {
    let cancelled = false;
    api
      .showcase()
      .then((showcase) => {
        if (!cancelled) {
          setListing({ status: "ready", showcase });
        }
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setListing({ status: "error", error: toApiError(error, "/showcase") });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [attempt]);

  // The manifest is read once the listing named it, and again on its own retry.
  const showcase = listing.status === "ready" ? listing.showcase : null;
  useEffect(() => {
    if (showcase === null || !withManifest) {
      return;
    }
    let cancelled = false;
    void readManifest(showcase).then((read) => {
      if (!cancelled) {
        setManifest(read);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [showcase, manifestAttempt, withManifest]);

  const reload = useCallback(() => {
    setListing({ status: "loading" });
    setManifest({ status: "loading" });
    setAttempt((n) => n + 1);
  }, []);

  const retryManifest = useCallback(() => {
    setManifest({ status: "loading" });
    setManifestAttempt((n) => n + 1);
  }, []);

  const state = useMemo<ArchiveState>(
    () => (listing.status === "ready" ? { status: "ready", showcase: listing.showcase, manifest } : listing),
    [listing, manifest],
  );

  return { state, reload, retryManifest };
}
