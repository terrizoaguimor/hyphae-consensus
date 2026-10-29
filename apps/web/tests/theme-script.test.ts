// SPDX-License-Identifier: Apache-2.0
//
// The head script is the app's only dangerouslySetInnerHTML, and its body is
// pinned by hash in the CSP. These tests keep both facts true and check what
// the script does.

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { describe, expect, it } from "vitest";

import {
  THEME_SCRIPT,
  THEME_STORAGE_KEY,
} from "../src/components/themeScriptSource";

import { IO_BOUND_TEST_TIMEOUT_MS } from "./io-timeout";

const srcDir = fileURLToPath(new URL("../src", import.meta.url));

function sourceFiles(dir: string): string[] {
  return readdirSync(dir, { recursive: true, encoding: "utf8" })
    .filter((file) => /\.(ts|tsx|js|jsx|mjs)$/.test(file))
    .map((file) => path.join(dir, file));
}

type Environment = {
  stored: string | null;
  prefersDark: boolean;
  storageThrows?: boolean;
};

/** Runs the script against stand-ins for the browser globals it reads. */
function run({ stored, prefersDark, storageThrows = false }: Environment) {
  const attributes = new Map<string, string>();
  const document = {
    documentElement: {
      setAttribute: (name: string, value: string) => {
        attributes.set(name, value);
      },
    },
  };
  const localStorage = {
    getItem: (key: string) => {
      if (storageThrows) {
        throw new Error("storage disabled");
      }
      return key === THEME_STORAGE_KEY ? stored : null;
    },
  };
  const window = {
    matchMedia: (query: string) => ({
      matches: prefersDark && query === "(prefers-color-scheme: dark)",
    }),
  };
  new Function("document", "localStorage", "window", THEME_SCRIPT)(
    document,
    localStorage,
    window,
  );
  return attributes.get("data-theme");
}

describe("theme script", () => {
  it("is the only dangerouslySetInnerHTML in src/", () => {
    const users = sourceFiles(srcDir)
      .filter((file) =>
        readFileSync(file, "utf8").includes("dangerouslySetInnerHTML"),
      )
      .map((file) => path.relative(srcDir, file).split(path.sep).join("/"));
    expect(users).toEqual(["components/ThemeScript.tsx"]);
    const themeScript = readFileSync(
      path.join(srcDir, "components", "ThemeScript.tsx"),
      "utf8",
    );
    expect(themeScript.match(/dangerouslySetInnerHTML=/g)).toHaveLength(1);
    // Reads every file of src/ through the bind mount: see tests/io-timeout.ts.
  }, IO_BOUND_TEST_TIMEOUT_MS);

  it("is one line and reads the storage key the toggle writes", () => {
    expect(THEME_SCRIPT).not.toMatch(/[\r\n]/);
    expect(THEME_SCRIPT).toContain(`"${THEME_STORAGE_KEY}"`);
  });

  it("applies a stored choice over the OS preference", () => {
    expect(run({ stored: "dark", prefersDark: false })).toBe("dark");
    expect(run({ stored: "light", prefersDark: true })).toBe("light");
  });

  it("follows the OS preference without a valid stored choice", () => {
    expect(run({ stored: null, prefersDark: true })).toBe("dark");
    expect(run({ stored: null, prefersDark: false })).toBe("light");
    expect(run({ stored: "sepia", prefersDark: true })).toBe("dark");
  });

  it("follows the OS preference when storage throws", () => {
    expect(run({ stored: "light", prefersDark: true, storageThrows: true })).toBe(
      "dark",
    );
  });
});
