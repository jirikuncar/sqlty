/** A sqlty table, as `sqlty.toml` or `[tool.sqlty]` holds it. */
export type Settings = Record<string, unknown>;

type Source = {
  /** The Python module, read as Python 3.14. */
  source: string;
  /** `CREATE TABLE` statements the queries are typed against. */
  schema?: string;
  /** `postgres` (default), `mysql`, `sqlite`, or another sqlparser-rs dialect. */
  dialect?: string;
  /** The text of a `sqlty.toml`: `dialect`, `driver`, `import-style`, `param-types`, `preview`, `max-query-variants`, `rules` and `types` apply. */
  config?: string;
  /** A table `config` or `project` answered, in place of `config`. */
  settings?: Settings;
  /** With `file`, the directory `settings` came from: routes `file` to its database and overrides. */
  base?: string;
  file?: string;
};

export type AnalyzeRequest = Source & { op: "analyze" };
export type FormatRequest = Source & { op: "format" };
/** `offset` counts UTF-16 code units, as JavaScript strings do. */
export type CompleteRequest = Source & { op: "complete"; offset: number };
export type HoverRequest = Source & { op: "hover"; offset: number };
export type SignatureRequest = Source & { op: "signature"; offset: number };
/** The diagnostics `source` gets against `schema` but not against `before`, as `sqlty breaking` reports. */
export type BreakingRequest = Source & { op: "breaking"; before: string };
/** Validates the sqlty table of the file `path` holding `source`. */
export type ConfigRequest = { op: "config"; source: string; path: string; required?: boolean };
/** Finds the configuration of the directory `path` as `sqlty check` does, reading `files` (`null`: no such file). */
export type ProjectRequest = {
  op: "project";
  path: string;
  files: Record<string, string | null>;
};
export type Request =
  | AnalyzeRequest
  | FormatRequest
  | CompleteRequest
  | HoverRequest
  | SignatureRequest
  | BreakingRequest
  | ConfigRequest
  | ProjectRequest;

export type Failure = { error: string };

/** A replacement of `start` to `end`; offsets count UTF-16 code units. */
export type Edit = { start: number; end: number; replacement: string };

export type Diagnostic = {
  rule: string;
  message: string;
  severity: "error" | "warn";
  line: number;
  start: number;
  end: number;
  /** The edits of its quick fix, if any. */
  fix: Edit[];
};

export type Rule = { id: string; reference: string; reasoning: string };

export type Query = {
  line: number;
  sql: string;
  /** Such as `tuple[int, str]`; `null` when unknown. */
  row_type: string | null;
  columns: {
    name: string;
    type: string | null;
    sql_type: string | null;
    type_rule: Rule | null;
    nullability_rule: Rule | null;
  }[];
  params: {
    index: number;
    name: string | null;
    type: string;
    sql_type: string | null;
    nullable: boolean | null;
  }[];
};

export type Analysis = {
  diagnostics: Diagnostic[];
  queries: Query[];
  /** SQL tokens to color: start, end and `keyword`, `string`, `number`, `comment`, `operator` or `parameter`. */
  highlights: [number, number, string][];
  /** What `sqlty rewrite`, `sqlty strip` and `sqlty check --fix` make of the source. */
  rewritten: string;
  stripped: string;
  fixed: string;
};

export type Completions = {
  start: number;
  end: number;
  items: {
    label: string;
    kind: "table" | "view" | "cte" | "column" | "join";
    detail: string;
    sql_type: string | null;
    nullable: boolean | null;
  }[];
};

/** Markdown about the word at `offset`. */
export type Hover = { start: number; end: number; contents: string };

/** The overloads of the built-in function whose arguments hold `offset`. */
export type Signatures = {
  /** Each `parameters` entry is the `[start, end]` of a parameter in `label`. */
  signatures: { label: string; url: string; parameters: [number, number][] }[];
  active_signature: number;
  /** Past the last parameter, the count of parameters: none is highlighted. */
  active_parameter: number;
};

export type ProjectAnswer =
  | { config: Settings; file: string }
  | { need: string }
  | { warning: string }
  | null;

export type Answer =
  | Analysis
  | { formatted: string }
  | Completions
  | Hover
  | Signatures
  | { diagnostics: Diagnostic[] }
  | { config: Settings }
  | ProjectAnswer
  | null;

/** Answers a request synchronously. */
export interface Engine {
  (request: AnalyzeRequest): Analysis | Failure;
  (request: FormatRequest): { formatted: string } | Failure;
  (request: CompleteRequest): Completions | null | Failure;
  (request: HoverRequest): Hover | null | Failure;
  (request: SignatureRequest): Signatures | null | Failure;
  (request: BreakingRequest): { diagnostics: Diagnostic[] } | Failure;
  (request: ConfigRequest): { config: Settings } | null | Failure;
  (request: ProjectRequest): ProjectAnswer | Failure;
  (request: Request): Answer | Failure;
}

/**
 * Starts the engine from `sqlty_wasm.wasm`: its bytes, a `Response`, or a URL to fetch (or
 * read, for a `file:` URL in Node.js; a relative one resolves against the page). By default,
 * the module next to this file.
 */
export function load(
  wasm?: BufferSource | Response | URL | string | Promise<BufferSource | Response | URL | string>,
): Promise<Engine>;
