// What the ESLint rule and the docs playground share: a tagged template as offsets into its
// source, checked by the engine's `query` op. No ESLint, no Node.js.
import { drivers, tsType } from "./types.js";

export { drivers, tsType };

export const fields = {
  camel: (name) => name.replace(/_+([a-z0-9])/g, (_, c) => c.toUpperCase()),
  pascal: (name) => fields.camel(name).replace(/^./, (c) => c.toUpperCase()),
  snake: (name) => name.replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase(),
  "screaming snake": (name) => fields.snake(name).toUpperCase(),
};

const key = (name) => (/^[A-Za-z_$][\w$]*$/.test(name) ? name : JSON.stringify(name));

/** The row type of `columns` as `target` (`transform`, `fieldTransform`) writes it. */
export function rowType(columns, target, driver, overrides = {}) {
  const rename = fields[target.fieldTransform] ?? ((name) => name);
  const members = columns.map(({ name, sql_type, nullable }) => {
    const type = tsType(sql_type, driver, overrides);
    return `${key(rename(name))}: ${nullable ? `${type} | null` : type}`;
  });
  const type = members.length ? `{ ${members.join("; ")} }` : "Record<string, never>";
  return target.transform ? target.transform.replaceAll("{type}", type) : type;
}

const ESCAPE = /\\(u\{([\da-fA-F]+)\}|u([\da-fA-F]{4})|x([\da-fA-F]{2})|\r\n|[^])/y;
const SIMPLE = { n: "\n", t: "\t", r: "\r", b: "\b", f: "\f", v: "\v", 0: "\0" };

/** The value of a template's `raw` text, and the offset into `raw` of each of its UTF-16 units and of its end. */
export function cook(raw) {
  let text = "";
  const units = [];
  for (let i = 0; i < raw.length; ) {
    if (raw[i] !== "\\") {
      text += raw[i];
      units.push(i++);
      continue;
    }
    ESCAPE.lastIndex = i;
    const [escape, body, braced, four, two] = ESCAPE.exec(raw);
    const hex = braced ?? four ?? two;
    const value = hex ? String.fromCodePoint(parseInt(hex, 16)) : /^(\r\n|[\n\r\u2028\u2029])$/.test(body) ? "" : (SIMPLE[body] ?? body);
    text += value;
    for (let n = 0; n < value.length; n++) units.push(i);
    i += escape.length;
  }
  units.push(raw.length);
  return { text, units };
}

/**
 * The SQL of a template, its interpolations as `$1`, `$2`, ..., and `range`, the source range
 * of a range of it. `quasis` are its texts as written, each with the source offset of its
 * start, and `expressions` the source ranges of its interpolations. A range in an
 * interpolation's placeholder is all of the interpolation.
 */
export function compose(quasis, expressions) {
  let sql = "";
  const segments = [];
  quasis.forEach(({ raw, start }, i) => {
    const { text, units } = cook(raw);
    segments.push({ at: sql.length, length: text.length, start, units });
    sql += text;
    const expression = expressions[i];
    if (expression) {
      const placeholder = `$${i + 1}`;
      segments.push({ at: sql.length, length: placeholder.length, start: expression.start, end: expression.end });
      sql += placeholder;
    }
  });
  const offset = (at, isEnd) => {
    const inside = (s) => (isEnd ? s.at < at && at <= s.at + s.length : s.at <= at && at < s.at + s.length);
    const found = segments.find(inside) ?? segments.at(isEnd ? 0 : -1);
    if (found.end !== undefined) return isEnd ? found.end : found.start;
    return found.start + found.units[Math.min(Math.max(at - found.at, 0), found.length)];
  };
  const range = (start, end) => {
    const from = offset(start, false);
    return [from, Math.max(offset(end, true), from + 1)];
  };
  return { sql, range };
}

const normalized = (text) => text.replace(/[\s;,]+/g, "");

/**
 * Check one template: `quasis` and `expressions` as `compose` takes them, `typeArguments` the
 * source range and `inner` text of its `<...>` if written, `tag` the source range of its tag.
 * `settings` has the engine, `schema` (`key` and `text()`), `dialect`, `config`, `driver`,
 * `overrides` and `target`.
 *
 * The answer has `problems`: offsets into the source, each with its `fix` edits; the row type
 * one (`rule: "row-type"`) also says whether it is `missing`. `columns`, `params` and `type`
 * are what the engine read; `error` says why it could not.
 */
export function check({ quasis, expressions, typeArguments, tag }, settings) {
  const { engine, schema, target = {} } = settings;
  const { sql, range } = compose(quasis, expressions);
  const request = {
    op: "query",
    sql,
    schema_key: schema?.key,
    dialect: settings.dialect ?? "postgres",
    config: settings.config ?? "",
  };
  let answer = engine(request);
  if (answer.need === "schema") answer = engine({ ...request, schema: schema.text() });
  if (answer.error) return { sql, error: answer.error, problems: [] };
  const problems = answer.diagnostics.map((found) => {
    const [start, end] = range(found.start, found.end);
    const fix = found.fix.map((edit) => {
      const [from, to] = range(edit.start, edit.end);
      return { start: from, end: to, replacement: edit.replacement };
    });
    return { rule: found.rule, message: found.message, severity: found.severity, start, end, fix };
  });
  const result = { sql, columns: answer.columns, params: answer.params, problems };
  if (!answer.returns_rows || !answer.columns || problems.length) return result;
  const type = rowType(answer.columns, target, drivers[settings.driver ?? "postgres"], settings.overrides);
  result.type = type;
  if (target.skipTypeAnnotations || (typeArguments && normalized(typeArguments.inner) === normalized(type))) return result;
  const [start, end] = typeArguments ? [typeArguments.start, typeArguments.end] : [tag.end, tag.end];
  problems.push({
    rule: "row-type",
    message: typeArguments ? `Row type does not match the query: expected ${type}` : `Query is missing its row type: ${type}`,
    severity: "error",
    missing: !typeArguments,
    start: typeArguments ? start : tag.start,
    end: typeArguments ? end : tag.end,
    fix: [{ start, end, replacement: `<${type}>` }],
  });
  return result;
}
