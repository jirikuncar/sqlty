import fs from "node:fs";
import path from "node:path";
import { load } from "@sqlty/wasm";
import { check, drivers, fields } from "./core.js";
import metadata from "./package.json" with { type: "json" };

const engine = await load();
const texts = new Map();

function read(file, mtimeMs) {
  const cached = texts.get(file);
  if (cached?.mtimeMs === mtimeMs) return cached.text;
  const text = fs.readFileSync(file, "utf8");
  texts.set(file, { mtimeMs, text });
  return text;
}

/** The schema files of `options`: `key` names their version, `text()` reads them. */
function schemaOf(options, cwd) {
  const paths = [options.schema ?? []].flat().map((file) => path.resolve(cwd, file));
  if (options.migrationsDir) {
    const dir = path.resolve(cwd, options.migrationsDir);
    const found = fs.readdirSync(dir, { recursive: true }).filter((name) => name.endsWith(".sql"));
    paths.push(...found.sort().map((name) => path.join(dir, name)));
  }
  const files = paths.map((file) => [file, fs.statSync(file).mtimeMs]);
  return {
    key: files.length ? JSON.stringify(files) : undefined,
    text: () => files.map(([file, mtimeMs]) => read(file, mtimeMs)).join(";\n"),
  };
}

function matcher(tag) {
  const regex = /^\/(.*)\/$/.exec(tag)?.[1];
  if (!regex) return (name) => name === tag;
  const compiled = new RegExp(`^(?:${regex})$`);
  return (name) => compiled.test(name);
}

function tagName(node) {
  if (node.type === "Identifier") return node.name;
  if (node.type === "MemberExpression" && !node.computed) {
    const object = tagName(node.object);
    return object && `${object}.${node.property.name}`;
  }
  return node.type === "ThisExpression" ? "this" : undefined;
}

const checkSql = {
  meta: {
    type: "problem",
    docs: {
      description: "Type-check SQL in tagged templates against a schema, with no database",
      url: "https://sqlty.kuncar.dev/",
    },
    fixable: "code",
    hasSuggestions: true,
    schema: [
      {
        type: "object",
        properties: {
          schema: { anyOf: [{ type: "string" }, { type: "array", items: { type: "string" } }] },
          migrationsDir: { type: "string" },
          dialect: { type: "string" },
          driver: { enum: Object.keys(drivers) },
          rules: { type: "object", additionalProperties: { enum: ["error", "warn", "ignore"] } },
          enforceType: { enum: ["fix", "suggest"] },
          overrides: {
            type: "object",
            properties: { types: { type: "object", additionalProperties: { type: "string" } } },
            additionalProperties: false,
          },
          targets: {
            type: "array",
            items: {
              type: "object",
              properties: {
                tag: { type: "string" },
                transform: { type: "string" },
                fieldTransform: { enum: Object.keys(fields) },
                skipTypeAnnotations: { type: "boolean" },
              },
              required: ["tag"],
              additionalProperties: false,
            },
          },
        },
        additionalProperties: false,
      },
    ],
    messages: {
      sql: "{{message}} [{{rule}}]",
      missingTypeAnnotation: "Query is missing its row type: {{type}}",
      incorrectTypeAnnotation: "Row type does not match the query: expected {{type}}",
      apply: "Replace with `{{replacement}}`",
      annotate: "Annotate the row type {{type}}",
    },
  },
  create(context) {
    const options = context.options[0] ?? {};
    const targets = (options.targets ?? [{ tag: "sql" }]).map((target) => ({ ...target, matches: matcher(target.tag) }));
    const config = Object.entries(options.rules ?? {})
      .map(([rule, severity]) => `${JSON.stringify(rule)} = ${JSON.stringify(severity)}`)
      .join("\n");
    const { sourceCode } = context;
    // JavaScript has no type arguments to write.
    const typed = !/\.[cm]?jsx?$/.test(context.filename);
    const targeted = (node) => {
      const name = tagName(node);
      return name && targets.find((target) => target.matches(name));
    };
    const fragment = (node) =>
      (node.type === "TaggedTemplateExpression" && targeted(node.tag)) || (node.type === "CallExpression" && targeted(node.callee));
    let schema;

    return {
      TaggedTemplateExpression(node) {
        const target = targeted(node.tag);
        if (!target || node.quasi.expressions.some(fragment)) return;
        schema ??= schemaOf(options, context.cwd);
        const written = node.typeArguments ?? node.typeParameters;
        const template = {
          quasis: node.quasi.quasis.map((element) => ({ raw: element.value.raw, start: element.range[0] + 1 })),
          expressions: node.quasi.expressions.map(({ range: [start, end] }) => ({ start, end })),
          typeArguments: written && { start: written.range[0], end: written.range[1], inner: sourceCode.getText(written.params[0]) },
          tag: { start: node.tag.range[0], end: node.tag.range[1] },
        };
        const found = check(template, {
          engine,
          schema,
          dialect: options.dialect,
          config: config && `[rules]\n${config}\n`,
          driver: options.driver,
          overrides: options.overrides?.types,
          target: typed ? target : { ...target, skipTypeAnnotations: true },
        });
        if (found.error) throw new Error(`sqlty: ${found.error}`);
        for (const problem of found.problems) {
          const loc = { start: sourceCode.getLocFromIndex(problem.start), end: sourceCode.getLocFromIndex(problem.end) };
          const fixes = problem.fix.map((edit) => (fixer) => fixer.replaceTextRange([edit.start, edit.end], edit.replacement));
          if (problem.rule !== "row-type") {
            const suggest = problem.fix.map((edit, i) => ({ messageId: "apply", data: { replacement: edit.replacement }, fix: fixes[i] }));
            context.report({ loc, messageId: "sql", data: { message: problem.message, rule: problem.rule }, suggest });
            continue;
          }
          const data = { type: found.type };
          context.report({
            loc,
            messageId: problem.missing ? "missingTypeAnnotation" : "incorrectTypeAnnotation",
            data,
            ...(options.enforceType === "suggest" ? { suggest: [{ messageId: "annotate", data, fix: fixes[0] }] } : { fix: fixes[0] }),
          });
        }
      },
    };
  },
};

const plugin = {
  meta: { name: metadata.name, version: metadata.version },
  rules: { "check-sql": checkSql },
};

export default plugin;
