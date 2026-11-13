import type { Table } from "./web/config";

export const PLAYGROUND = "https://sqlty.kuncar.dev/playground/";

/** As `docs/assets/playground/share.js`: longer links break in browsers and chat apps. */
export const MAX_LINK = 8000;

/** The keys the playground's config takes; `dialect` has its own control. */
const KEYS = [
  "driver",
  "import-style",
  "param-types",
  "preview",
  "max-query-variants",
  "rules",
  "types",
];

export type Session = { source: string; schema: string; dialect: string; config: string };

export function session(source: string, project: { settings: Table; schema: string }): Session {
  const { settings, schema } = project;
  const kept = Object.fromEntries(Object.entries(settings).filter(([key]) => KEYS.includes(key)));
  const dialect = typeof settings.dialect === "string" ? settings.dialect : "postgres";
  return { source, schema, dialect, config: toml(kept) };
}

/** `session` as `share.js` encodes it: its JSON, deflated, as base64url. */
export async function encode(session: Session): Promise<string> {
  const bytes = new TextEncoder().encode(JSON.stringify(session));
  const stream = new Blob([bytes]).stream().pipeThrough(new CompressionStream("deflate"));
  const deflated = new Uint8Array(await new Response(stream).arrayBuffer());
  const binary = Array.from(deflated, (byte) => String.fromCharCode(byte)).join("");
  return btoa(binary).replaceAll("+", "-").replaceAll("/", "_").replace(/=+$/, "");
}

export function toml(table: Table, path: string[] = []): string {
  const scalars: string[] = [];
  const tables: string[] = [];
  for (const [key, value] of Object.entries(table)) {
    if (isTable(value)) {
      const nested = [...path, key];
      tables.push(`[${nested.map(tomlKey).join(".")}]\n${toml(value, nested)}`);
    } else {
      scalars.push(`${tomlKey(key)} = ${tomlValue(value)}\n`);
    }
  }
  return [scalars.join(""), ...tables].filter(Boolean).join("\n");
}

function tomlKey(key: string): string {
  return /^[A-Za-z0-9_-]+$/.test(key) ? key : JSON.stringify(key);
}

function tomlValue(value: unknown): string {
  if (Array.isArray(value)) {
    return `[${value.map(tomlValue).join(", ")}]`;
  }
  return typeof value === "string" ? JSON.stringify(value) : String(value);
}

function isTable(value: unknown): value is Table {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
