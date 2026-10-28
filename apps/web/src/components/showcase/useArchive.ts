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
// as an error to show with a retry, never as a loading line.

import { useCallback, useEffect, useState } from "react";

import { verifyDigest } from "@/lib/chain/digest";
import { api, ApiError, isApiError } from "@/lib/api/client";
import { archiveManifestSchema } from "@/lib/api/schemas";
import type { ShowcaseV1 } from "@/lib/api/types";
import type { ArchiveManifest } from "@/lib/showcase";

export type ManifestState =
  | { status: "loading" }
  | { status: "error"; error: ApiError }
  /** `manifest` is null when the bytes did not match, or did not parse as the archive's format. */
  | { status: "ready"; sha256: string; matches: boolean; manifest: ArchiveManifest | null };

export type ArchiveState =
  | { status: "loading" }
  | { status: "error"; error: ApiError }
  | { status: "ready"; showcase: ShowcaseV1; manifest: ManifestState };

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

export function useArchive(withManifest: boolean): { state: ArchiveState; reload: () => void } {
  const [state, setState] = useState<ArchiveState>({ status: "loading" });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    let cancelled = false;
    api
      .showcase()
      .then((showcase) => {
        if (cancelled) {
          return;
        }
        setState({ status: "ready", showcase, manifest: { status: "loading" } });
        if (!withManifest) {
          return;
        }
        void readManifest(showcase).then((manifest) => {
          if (!cancelled) {
            setState({ status: "ready", showcase, manifest });
          }
        });
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          setState({ status: "error", error: toApiError(error, "/showcase") });
        }
      });
    return () => {
      cancelled = true;
    };
  }, [attempt, withManifest]);

  const reload = useCallback(() => {
    setState({ status: "loading" });
    setAttempt((n) => n + 1);
  }, []);

  return { state, reload };
}
