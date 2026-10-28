"use client";
// SPDX-License-Identifier: Apache-2.0
//
// The Cloudflare Turnstile widget for the live form. Its script
// (challenges.cloudflare.com) is loaded ONLY when NEXT_PUBLIC_TURNSTILE_SITE_KEY
// was set at build time, and then at runtime by this component, never from
// the exported HTML (scripts/csp-manifest.mjs rejects cross-origin script
// tags; the server's CSP must allow the origin in script-src and frame-src).
// Without a site key the component shows the fixed statement that live mode
// needs the challenge, and the server will refuse the request: nothing here
// pretends to load a challenge it cannot load.

import { useTranslations } from "next-intl";
import { useEffect, useRef, useState } from "react";

export const TURNSTILE_SITE_KEY: string | undefined =
  process.env.NEXT_PUBLIC_TURNSTILE_SITE_KEY === undefined ||
  process.env.NEXT_PUBLIC_TURNSTILE_SITE_KEY === ""
    ? undefined
    : process.env.NEXT_PUBLIC_TURNSTILE_SITE_KEY;

const SCRIPT_URL = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";

type TurnstileApi = {
  render: (
    container: HTMLElement,
    options: {
      sitekey: string;
      callback: (token: string) => void;
      "expired-callback"?: () => void;
      "error-callback"?: () => void;
    },
  ) => string;
  remove: (widgetId: string) => void;
};

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
      script.addEventListener("error", () => reject(new Error("turnstile script failed")));
      document.head.append(script);
    });
  }
  return loader;
}

type TurnstileProps = {
  /** The response token, or null when it expired or failed. */
  onToken: (token: string | null) => void;
};

export function Turnstile({ onToken }: TurnstileProps) {
  const t = useTranslations("NewDeliberation.live");
  const container = useRef<HTMLDivElement>(null);
  const [status, setStatus] = useState<"loading" | "passed" | "failed">("loading");

  useEffect(() => {
    if (TURNSTILE_SITE_KEY === undefined || container.current === null) {
      return;
    }
    const element = container.current;
    let widgetId: string | null = null;
    let disposed = false;
    loadTurnstile()
      .then((turnstile) => {
        if (disposed) {
          return;
        }
        widgetId = turnstile.render(element, {
          sitekey: TURNSTILE_SITE_KEY,
          callback: (token) => {
            setStatus("passed");
            onToken(token);
          },
          "expired-callback": () => {
            setStatus("loading");
            onToken(null);
          },
          "error-callback": () => {
            setStatus("failed");
            onToken(null);
          },
        });
      })
      .catch(() => {
        if (!disposed) {
          setStatus("failed");
        }
      });
    return () => {
      disposed = true;
      if (widgetId !== null) {
        window.turnstile?.remove(widgetId);
      }
    };
  }, [onToken]);

  if (TURNSTILE_SITE_KEY === undefined) {
    return (
      <p className="hc-supporting" data-turnstile="missing">
        {t("turnstileMissing")}
      </p>
    );
  }
  return (
    <div data-turnstile={status}>
      <div ref={container} className="hc-turnstile" />
      <p role="status" className="hc-supporting">
        {status === "loading"
          ? t("turnstileLoading")
          : status === "passed"
            ? t("turnstilePassed")
            : t("turnstileFailed")}
      </p>
    </div>
  );
}
