// TypeScript in the playground: the checks of @sqlty/eslint-plugin (core.js, copied from
// npm/eslint-plugin by `make playground`) on the `sql` tagged templates of the source. ESLint
// reads templates from a TypeScript syntax tree; this reads them with a scanner that skips
// comments and strings, enough for the examples a playground holds.
import { check, drivers, tsType } from "./core.js";

export { drivers, tsType };

const IDENTIFIER = /[\w$.]/;

/** The tagged templates of `source` whose tag is `tag`, as `check` takes them. */
export function templates(source, tag = "sql") {
  const found = [];
  let i = 0;
  while (i < source.length) {
    const c = source[i];
    if (c === "/" && (source[i + 1] === "/" || source[i + 1] === "*")) {
      const close = source[i + 1] === "/" ? "\n" : "*/";
      const at = source.indexOf(close, i + 2);
      i = at < 0 ? source.length : at + close.length;
    }
    else if (c === '"' || c === "'") i = string(source, i);
    else if (c === "`") {
      const template = read(source, i);
      const tagged = tagOf(source, i);
      if (tagged?.name === tag) found.push({ ...template, ...tagged });
      i = template.end;
    } else i++;
  }
  return found;
}

function string(source, i) {
  const quote = source[i++];
  while (i < source.length && source[i] !== quote && source[i] !== "\n") i += source[i] === "\\" ? 2 : 1;
  return i + 1;
}

/** The template starting at the backtick at `i`: its texts, interpolations and end. */
function read(source, i) {
  const quasis = [];
  const expressions = [];
  let start = i + 1;
  for (i = start; i < source.length; i++) {
    if (source[i] === "\\") i++;
    else if (source[i] === "`") break;
    else if (source[i] === "$" && source[i + 1] === "{") {
      quasis.push({ raw: source.slice(start, i), start });
      const from = i + 2;
      i = closing(source, from);
      expressions.push({ start: from, end: i });
      start = i + 1;
    }
  }
  quasis.push({ raw: source.slice(start, i), start });
  return { quasis, expressions, end: i + 1 };
}

/** The offset of the `}` that closes the interpolation from `i`. */
function closing(source, i) {
  for (let depth = 0; i < source.length; i++) {
    const c = source[i];
    if (c === '"' || c === "'") i = string(source, i) - 1;
    else if (c === "`") i = read(source, i).end - 1;
    else if (c === "{") depth++;
    else if (c === "}" && depth-- === 0) return i;
  }
  return i;
}

/** The tag of the template at `i` and its `<...>`, when it has one. */
function tagOf(source, i) {
  let end = i;
  let typeArguments;
  if (source[end - 1] === ">") {
    let depth = 0;
    let at = end - 1;
    for (; at >= 0; at--) {
      if (source[at] === ">") depth++;
      else if (source[at] === "<" && --depth === 0) break;
    }
    if (at < 0) return undefined;
    typeArguments = { start: at, end, inner: source.slice(at + 1, end - 1) };
    end = at;
  }
  let start = end;
  while (start > 0 && IDENTIFIER.test(source[start - 1])) start--;
  if (start === end) return undefined;
  return { name: source.slice(start, end), tag: { start, end }, typeArguments };
}

/**
 * What the plugin reports on `source`, as the playground lists diagnostics, and each query
 * it checked: `settings` as `check` takes them.
 */
export function lint(source, settings) {
  const diagnostics = [];
  const queries = [];
  const starts = [0];
  for (let at = source.indexOf("\n"); at >= 0; at = source.indexOf("\n", at + 1)) starts.push(at + 1);
  const line = (offset) => {
    let [low, high] = [0, starts.length - 1];
    while (low < high) {
      const middle = (low + high + 1) >> 1;
      if (starts[middle] <= offset) low = middle;
      else high = middle - 1;
    }
    return low + 1;
  };
  for (const template of templates(source)) {
    if (template.expressions.some(({ start, end }) => /\bsql\s*[`(<]/.test(source.slice(start, end)))) continue;
    const found = check(template, settings);
    if (found.error) return { error: found.error, diagnostics: [], queries: [] };
    diagnostics.push(...found.problems.map((problem) => ({ ...problem, line: line(problem.start) })));
    queries.push({ line: line(template.tag.start), sql: found.sql, type: found.type, columns: found.columns ?? [], params: found.params ?? [] });
  }
  return { diagnostics, queries };
}
