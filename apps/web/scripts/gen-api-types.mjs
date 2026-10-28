#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
//
// TypeScript types for the /api/v1 contract, generated from the JSON Schemas
// the Rust server exports (contracts/api-v1/*.schema.json, one file per
// request, response or SSE frame type):
//
//   node scripts/gen-api-types.mjs            write src/lib/api/types.ts
//   node scripts/gen-api-types.mjs --check    fail (exit 1) when the file drifts
//
// The schemas are schemars output (draft-07) and use a small, closed subset
// of JSON Schema: `definitions` with `$ref`, `properties` + `required`,
// `type` (a name or a name list, for nullable fields), `enum`, `oneOf` and
// `anyOf` unions, single-element `allOf` wrappers, `items`, `format`,
// `description`, `default`, `additionalProperties: false`. Anything outside
// that subset fails the generator loudly instead of producing a wrong type.
//
// The output is deterministic: definitions are emitted once each, sorted by
// name, object keys sorted, and the same name is checked to mean the same
// schema in every file (a conflicting definition gets a suffixed name and a
// comment). The header names the contract's SHA-256
// (contracts/api-v1/contract-sha256.txt), which GET /api/v1/health reports
// as `contract_sha256`, so a client can tell which contract it was built for.
//
// Where the contract is: `API_CONTRACT_DIR`, else `../../contracts/api-v1`
// (the repository layout), else `../contracts/api-v1` (the layout inside the
// Docker containers of in-docker.sh and playwright-docker.sh, which mount the
// repository's contracts/ directory read-only at /contracts).

import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

export const OUTPUT_RELATIVE = "src/lib/api/types.ts";

const appDir = fileURLToPath(new URL("..", import.meta.url));

/** The contract directory this checkout (or container) can see. */
export function contractDir(env = process.env) {
  const fromEnv = env.API_CONTRACT_DIR;
  if (fromEnv !== undefined && fromEnv !== "") {
    return path.resolve(fromEnv);
  }
  for (const candidate of ["../../contracts/api-v1", "../contracts/api-v1"]) {
    const resolved = path.resolve(appDir, candidate);
    if (existsSync(path.join(resolved, "contract-sha256.txt"))) {
      return resolved;
    }
  }
  throw new Error(
    "gen-api-types: contracts/api-v1 not found (set API_CONTRACT_DIR, or run through scripts/in-docker.sh, which mounts it)",
  );
}

/**
 * The contract as files: every `*.schema.json` sorted by name, parsed, plus
 * the pinned SHA-256.
 */
export function loadContract(dir) {
  const files = readdirSync(dir)
    .filter((name) => name.endsWith(".schema.json"))
    .sort();
  if (files.length === 0) {
    throw new Error(`gen-api-types: no *.schema.json in ${dir}`);
  }
  const schemas = files.map((file) => {
    const text = readFileSync(path.join(dir, file), "utf8");
    const schema = JSON.parse(text);
    const title = schema.title;
    if (typeof title !== "string" || title === "") {
      throw new Error(`gen-api-types: ${file} has no title`);
    }
    if (title !== file.slice(0, -".schema.json".length)) {
      throw new Error(`gen-api-types: ${file} is titled ${title}`);
    }
    return { file, title, schema, text };
  });
  const sha256 = readFileSync(path.join(dir, "contract-sha256.txt"), "utf8")
    .trim()
    .toLowerCase();
  if (!/^[0-9a-f]{64}$/.test(sha256)) {
    throw new Error("gen-api-types: contract-sha256.txt is not a SHA-256");
  }
  return { dir, schemas, sha256 };
}

/**
 * SHA-256 of the contract as the server computes it
 * (`contract_sha256_of` in crates/consensus-api/src/api/contract.rs): for
 * every schema in name order, the type name, a newline, then the schema file
 * as written (pretty JSON ending in a newline). The generator reports a
 * mismatch with the pinned value, so stale or edited schema files are
 * noticed.
 */
export function schemasSha256(schemas) {
  const hash = createHash("sha256");
  for (const { title, text } of [...schemas].sort((a, b) =>
    a.title < b.title ? -1 : a.title > b.title ? 1 : 0,
  )) {
    hash.update(title, "utf8");
    hash.update("\n", "utf8");
    hash.update(text, "utf8");
  }
  return hash.digest("hex");
}

// ---------------------------------------------------------------------------
// Schema -> TypeScript

const KNOWN_KEYWORDS = new Set([
  "$schema",
  "$ref",
  "title",
  "description",
  "type",
  "properties",
  "required",
  "additionalProperties",
  "items",
  "enum",
  "const",
  "oneOf",
  "anyOf",
  "allOf",
  "definitions",
  "format",
  "minimum",
  "maximum",
  "default",
]);

function fail(where, message) {
  throw new Error(`gen-api-types: ${where}: ${message}`);
}

function checkKeywords(schema, where) {
  for (const key of Object.keys(schema)) {
    if (!KNOWN_KEYWORDS.has(key)) {
      fail(where, `unsupported JSON Schema keyword "${key}"`);
    }
  }
}

/** Stable JSON text (sorted keys), to compare two definitions. */
export function canonical(value) {
  if (Array.isArray(value)) {
    return `[${value.map(canonical).join(",")}]`;
  }
  if (value !== null && typeof value === "object") {
    return `{${Object.keys(value)
      .sort()
      .map((key) => `${JSON.stringify(key)}:${canonical(value[key])}`)
      .join(",")}}`;
  }
  return JSON.stringify(value);
}

function jsdoc(description, indent) {
  if (typeof description !== "string" || description.trim() === "") {
    return "";
  }
  const text = description.replace(/\*\//g, "*\\/").trim();
  const lines = text.split(/\r?\n/);
  if (lines.length === 1 && text.length + indent.length <= 72) {
    return `${indent}/** ${text} */\n`;
  }
  return `${indent}/**\n${lines
    .map((line) => `${indent} * ${line}`.trimEnd())
    .join("\n")}\n${indent} */\n`;
}

function literal(value) {
  return JSON.stringify(value);
}

const IDENTIFIER = /^[A-Za-z_$][A-Za-z0-9_$]*$/;

function propertyKey(name) {
  return IDENTIFIER.test(name) ? name : JSON.stringify(name);
}

function isSimple(text) {
  return /^[A-Za-z0-9_$.<>\[\]"' |-]+$/.test(text) && !text.includes("\n");
}

function unionOf(members) {
  const unique = [...new Set(members)];
  if (unique.length === 1) {
    return unique[0];
  }
  return unique.join(" | ");
}

/**
 * Emits the TypeScript type for `schema`. `names` maps a definition name of
 * the current file to its global type name; `indent` is the current
 * indentation of multi-line output.
 */
function typeOf(schema, names, indent, where) {
  if (schema === true) {
    return "unknown";
  }
  if (schema === false) {
    return "never";
  }
  if (schema === null || typeof schema !== "object" || Array.isArray(schema)) {
    fail(where, "a schema must be an object or a boolean");
  }
  checkKeywords(schema, where);

  if (typeof schema.$ref === "string") {
    const prefix = "#/definitions/";
    if (!schema.$ref.startsWith(prefix)) {
      fail(where, `unsupported $ref ${schema.$ref}`);
    }
    const name = schema.$ref.slice(prefix.length);
    const global = names.get(name);
    if (global === undefined) {
      fail(where, `$ref to unknown definition ${name}`);
    }
    return global;
  }

  if (Array.isArray(schema.allOf)) {
    const parts = schema.allOf.map((part, index) =>
      typeOf(part, names, indent, `${where}/allOf/${index}`),
    );
    return parts.length === 1 ? parts[0] : parts.map((p) => `(${p})`).join(" & ");
  }

  const variants = schema.oneOf ?? schema.anyOf;
  if (Array.isArray(variants)) {
    const keyword = schema.oneOf ? "oneOf" : "anyOf";
    const inline = variants.map((variant, index) =>
      typeOf(variant, names, indent, `${where}/${keyword}/${index}`),
    );
    if (inline.every(isSimple) && unionOf(inline).length + indent.length <= 72) {
      return unionOf(inline);
    }
    // One member per line; object members are indented under their `|`.
    const members = variants.map((variant, index) =>
      typeOf(variant, names, `${indent}    `, `${where}/${keyword}/${index}`),
    );
    return `\n${[...new Set(members)]
      .map((member) => `${indent}  | ${member}`)
      .join("\n")}`;
  }

  if (Array.isArray(schema.enum)) {
    return unionOf(schema.enum.map(literal));
  }
  if ("const" in schema) {
    return literal(schema.const);
  }

  const types = Array.isArray(schema.type) ? schema.type : [schema.type];
  if (types.length === 1 && types[0] === undefined) {
    // No type at all (schemars for a `serde_json::Value`): anything.
    if (schema.properties !== undefined) {
      return objectType(schema, names, indent, where);
    }
    return "unknown";
  }
  const members = types.map((type) => {
    switch (type) {
      case "string":
        return "string";
      case "integer":
      case "number":
        return "number";
      case "boolean":
        return "boolean";
      case "null":
        return "null";
      case "array": {
        if (schema.items === undefined) {
          return "unknown[]";
        }
        const inner = typeOf(schema.items, names, indent, `${where}/items`);
        return isSimple(inner) && !inner.includes(" ") ? `${inner}[]` : `Array<${inner}>`;
      }
      case "object":
        return objectType(schema, names, indent, where);
      default:
        return fail(where, `unsupported type ${String(type)}`);
    }
  });
  return unionOf(members);
}

function objectType(schema, names, indent, where) {
  const properties = schema.properties ?? {};
  const keys = Object.keys(properties).sort();
  const required = new Set(schema.required ?? []);
  for (const name of required) {
    if (!(name in properties)) {
      fail(where, `required property ${name} is not declared`);
    }
  }
  const extra = schema.additionalProperties;
  if (keys.length === 0) {
    if (extra === undefined || extra === true) {
      return "Record<string, unknown>";
    }
    if (extra === false) {
      return "Record<string, never>";
    }
    return `Record<string, ${typeOf(extra, names, indent, `${where}/additionalProperties`)}>`;
  }
  const inner = `${indent}  `;
  const lines = keys.map((key) => {
    const property = properties[key];
    const doc =
      property !== null && typeof property === "object" && !Array.isArray(property)
        ? jsdoc(property.description, inner)
        : "";
    const optional = required.has(key) ? "" : "?";
    const type = typeOf(property, names, inner, `${where}/properties/${key}`);
    return `${doc}${inner}${propertyKey(key)}${optional}: ${type};`;
  });
  if (extra !== undefined && extra !== false) {
    const type =
      extra === true
        ? "unknown"
        : typeOf(extra, names, inner, `${where}/additionalProperties`);
    lines.push(`${inner}[key: string]: ${type};`);
  }
  return `{\n${lines.join("\n")}\n${indent}}`;
}

// ---------------------------------------------------------------------------
// The whole file

/**
 * Collects every definition and every root type across the schema files into
 * one namespace. A name that means the same schema everywhere is emitted
 * once; a name used for two different schemas keeps its first meaning and
 * gets `<Name>__<Title>` aliases for the others.
 */
function collect(schemas) {
  /** @type {Map<string, {schema: unknown, source: string, canonical: string}>} */
  const globals = new Map();
  /** @type {Map<string, Map<string, string>>} per file: local name -> global name */
  const perFile = new Map();
  const conflicts = [];

  const register = (name, schema, source, local) => {
    const text = canonical(schema);
    const existing = globals.get(name);
    if (existing === undefined) {
      globals.set(name, { schema, source, canonical: text });
      return name;
    }
    if (existing.canonical === text) {
      return name;
    }
    const alias = `${name}__${source}`;
    if (!globals.has(alias)) {
      globals.set(alias, { schema, source, canonical: text });
      conflicts.push(`${local} in ${source} differs from ${name} in ${existing.source}: emitted as ${alias}`);
    }
    return alias;
  };

  for (const { title, schema } of schemas) {
    const names = new Map();
    const definitions = schema.definitions ?? {};
    for (const name of Object.keys(definitions).sort()) {
      if (!IDENTIFIER.test(name)) {
        fail(title, `definition name ${name} is not an identifier`);
      }
      names.set(name, register(name, definitions[name], title, name));
    }
    perFile.set(title, names);
  }
  for (const { title, schema } of schemas) {
    const root = { ...schema };
    delete root.$schema;
    delete root.definitions;
    delete root.title;
    const names = perFile.get(title);
    if (names.has(title)) {
      // A root type also defined by other files (e.g. RunSummaryV1): they
      // must agree, and the root is emitted once.
      const global = globals.get(names.get(title));
      if (global.canonical !== canonical(root)) {
        fail(title, `root schema differs from the definition of the same name`);
      }
      continue;
    }
    register(title, root, title, title);
    names.set(title, title);
  }
  return { globals, perFile, conflicts };
}

export function generate(contract) {
  const { schemas, sha256 } = contract;
  const { globals, perFile, conflicts } = collect(schemas);
  const titles = schemas.map((s) => s.title);

  const out = [];
  out.push("// SPDX-License-Identifier: Apache-2.0");
  out.push("//");
  out.push("// GENERATED FILE: do not edit. Written by scripts/gen-api-types.mjs from the");
  out.push("// JSON Schemas of contracts/api-v1 (`pnpm api:types`); tests/api-types.test.ts");
  out.push("// regenerates it in memory and fails on drift (`pnpm api:check`).");
  out.push("//");
  out.push(`// contract-sha256: ${sha256}`);
  out.push(`// schemas (${titles.length}): ${titles.join(", ")}`);
  for (const conflict of conflicts) {
    out.push(`// conflict: ${conflict}`);
  }
  out.push("");
  out.push("/**");
  out.push(" * SHA-256 of the contract these types were generated from");
  out.push(" * (contracts/api-v1/contract-sha256.txt); GET /api/v1/health reports the");
  out.push(" * server's as `contract_sha256`.");
  out.push(" */");
  out.push(`export const CONTRACT_SHA256 = ${literal(sha256)};`);
  out.push("");
  out.push("/** The schema files of the contract, one per request, response or frame type. */");
  out.push("export const CONTRACT_SCHEMAS = [");
  for (const title of titles) {
    out.push(`  ${literal(title)},`);
  }
  out.push("] as const;");
  out.push("");

  const names = [...globals.keys()].sort();
  for (const name of names) {
    const { schema, source } = globals.get(name);
    const fileNames = perFile.get(source);
    const where = `${source}#${name}`;
    const doc = jsdoc(
      schema !== null && typeof schema === "object" ? schema.description : undefined,
      "",
    );
    const body = typeOf(schema, fileNames, "", where);
    const separator = body.startsWith("\n") ? "" : " ";
    out.push(`${doc}export type ${name} =${separator}${body};`);
    out.push("");
  }
  return `${out.join("\n").trimEnd()}\n`;
}

async function main() {
  const check = process.argv.includes("--check");
  const contract = loadContract(contractDir());
  const computed = schemasSha256(contract.schemas);
  if (computed !== contract.sha256) {
    console.error(
      `gen-api-types: the schema files hash to ${computed}, contract-sha256.txt says ${contract.sha256}; the contract directory is inconsistent (regenerate it with the Rust exporter)`,
    );
    process.exit(2);
  }
  const text = generate(contract);
  const target = path.join(appDir, OUTPUT_RELATIVE);
  if (check) {
    const current = existsSync(target) ? readFileSync(target, "utf8") : "";
    if (current !== text) {
      console.error(
        `gen-api-types: ${OUTPUT_RELATIVE} differs from the contract; run \`pnpm api:types\``,
      );
      process.exit(1);
    }
    console.log(`gen-api-types: ${OUTPUT_RELATIVE} matches contract ${contract.sha256.slice(0, 12)}`);
    return;
  }
  writeFileSync(target, text);
  console.log(
    `gen-api-types: wrote ${OUTPUT_RELATIVE} (${contract.schemas.length} schemas, contract ${contract.sha256.slice(0, 12)})`,
  );
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  await main();
}
