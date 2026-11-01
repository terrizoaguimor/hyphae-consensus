"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The Cloudflare Turnstile widget for the live form. Its script
// (challenges.cloudflare.com) is loaded ONLY when NEXT_PUBLIC_TURNSTILE_SITE_KEY
// was set at build time, and then at runtime by this component, never from
// the exported HTML (scripts/csp-manifest.mjs rejects cross-origin script
// tags; the server's CSP must allow the origin in script-src and frame-src).
// Without a site key the live form is closed with the fixed statement that
// live mode needs the challenge (src/lib/live/form.ts), and this component
// is not rendered. The widget itself (the live-run action, the single-use
// token) is src/lib/live/turnstile.ts; the form spends the token through
// `controlRef` on every submit, which also asks for a fresh challenge.

import { Button } from "@astryxdesign/core/Button";
import { useTranslations } from "next-intl";
import { type RefObject, useEffect, useRef, useState } from "react";

import {
  type TurnstileApi,
  type TurnstileStatus,
  TurnstileWidget,
  turnstileStatusMessage,
} from "@/lib/live/turnstile";

export const TURNSTILE_SITE_KEY: string | undefined =
  process.env.NEXT_PUBLIC_TURNSTILE_SITE_KEY === undefined ||
  process.env.NEXT_PUBLIC_TURNSTILE_SITE_KEY === ""
    ? undefined
    : process.env.NEXT_PUBLIC_TURNSTILE_SITE_KEY;

const SCRIPT_URL = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";

declare global {
  interface Window {
    turnstile?: TurnstileApi;
  }
}

let loader: Promise<TurnstileApi> | null = null;

function loadTurnstile(): Promise<TurnstileApi> {
  if (loader === null) {
    loader = new Promise<TurnstileApi>((resolve, reject) => {
      if (window.turnstile !== undefined) {
        resolve(window.turnstile);
        return;
      }
      const script = document.createElement("script");
      script.src = SCRIPT_URL;
      script.async = true;
      script.addEventListener("load", () => {
        if (window.turnstile === undefined) {
          reject(new Error("turnstile did not initialise"));
        } else {
          resolve(window.turnstile);
        }
      });
      script.addEventListener("error", () => {
        // A later mount may try again.
        loader = null;
        reject(new Error("turnstile script failed"));
      });
      document.head.append(script);
    });
  }
  return loader;
}

/** What the form may do with the widget. */
export type TurnstileControl = {
  /** Hands the current token to one request and asks for a new challenge. */
  spend: () => string | null;
};

type TurnstileProps = {
  siteKey: string;
  /** The current token, or null when there is none (not solved, expired, failed, spent). */
  onToken: (token: string | null) => void;
  controlRef: RefObject<TurnstileControl | null>;
};

export function Turnstile({ siteKey, onToken, controlRef }: TurnstileProps) {
  const t = useTranslations("NewDeliberation.live");
  const container = useRef<HTMLDivElement>(null);
  const widgetRef = useRef<TurnstileWidget | null>(null);
  const [status, setStatus] = useState<TurnstileStatus>("loading");
  // Whether a widget exists to reset (the script may fail to load at all).
  const [rendered, setRendered] = useState(false);

  useEffect(() => {
    const element = container.current;
    if (element === null) {
      return;
    }
    let disposed = false;
    loadTurnstile()
      .then((api) => {
        if (disposed) {
          return;
        }
        const widget = new TurnstileWidget(api, {
          onToken: (token) => {
            if (!disposed) {
              onToken(token);
            }
          },
          onStatus: (next) => {
            if (!disposed) {
              setStatus(next);
            }
          },
        });
        widget.render(element, siteKey);
        widgetRef.current = widget;
        controlRef.current = { spend: () => widget.spend() };
        setRendered(true);
      })
      .catch(() => {
        if (!disposed) {
          setStatus("failed");
        }
      });
    return () => {
      disposed = true;
      widgetRef.current?.remove();
      widgetRef.current = null;
      controlRef.current = null;
    };
  }, [siteKey, onToken, controlRef]);

  return (
    <div data-turnstile={status}>
      <div ref={container} className="hc-turnstile" />
      <p role="status" className="hc-supporting">
        {t(turnstileStatusMessage(status))}
      </p>
      {status === "failed" && rendered ? (
        <div className="hc-actions">
          <Button
            variant="secondary"
            size="sm"
            label={t("turnstileRetry")}
            onClick={() => widgetRef.current?.reset()}
          />
        </div>
      ) : null}
    </div>
  );
}
