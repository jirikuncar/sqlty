//! `TypedDict`s for JSON columns: what `json-types = "typing.TypedDict"` writes into a
//! module (`sqlty._json_types`).
//!
//! `rewrite` types a JSON column `typing.Any`. Where the engine knows the shape of the
//! JSON a query builds, the `typing.Any` items of a `SQL[tuple[...]]` annotation become
//! nested types, with a `TypedDict` per object. Each class is written after the imports,
//! below a `# sqlty: generated` line, and `strip` removes them again.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use ruff_python_ast::comparable::ComparableStmt;
use ruff_python_ast::visitor::{walk_expr, Visitor};
use ruff_python_ast::{Expr, ExprCall, Stmt};
use ruff_text_size::Ranged;
use sqlty_sql::analyzer::{self, Nullability};
use sqlty_sql::json::{JsonField, JsonShape, JsonValue};

use crate::options::Options;
use crate::types::extract::class_start;
use crate::types::stdlib::Version;
use crate::types::symbols::Source;
use crate::unparse::{repr_str, unparse};

/// The line above a generated class.
pub const MARKER: &str = "# sqlty: generated";
const ANY: &str = "typing.Any";

struct Calls<'a>(Vec<&'a ExprCall>);

impl<'a> Visitor<'a> for Calls<'a> {
    #[recursive::recursive]
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(call) = expr {
            self.0.push(call);
        }
        walk_expr(self, expr);
    }
}

fn is_any(item: &Expr, text: &str) -> bool {
    matches!(unparse(item, text).as_str(), ANY | "Any")
}

/// `source` with the `typing.Any` row items of JSON columns of known shape typed.
pub fn add_json_types(text: &str, options: &Options<'_>, version: Version) -> String {
    let Some(source) = Source::new(text, version) else {
        return text.to_string();
    };
    let wrapper = options.style.wrapper_name.as_str();
    let mut calls = Calls(Vec::new());
    calls.visit_body(source.body());
    let mut module = Module::new(&source);
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for call in calls.0 {
        let (Expr::Subscript(func), Some(Expr::StringLiteral(query))) =
            (call.func.as_ref(), call.arguments.args.first())
        else {
            continue;
        };
        if unparse(&func.value, text) != wrapper {
            continue;
        }
        let Expr::Subscript(row) = func.slice.as_ref() else {
            continue;
        };
        if unparse(&row.value, text) != "tuple" {
            continue;
        }
        let items: Vec<&Expr> = match row.slice.as_ref() {
            Expr::Tuple(tuple) => tuple.elts.iter().collect(),
            item => vec![item],
        };
        if !items.iter().any(|item| is_any(item, text)) {
            continue;
        }
        let query = crate::parse::text(query, text);
        let columns = analyzer::analyze_select(&query, options.dialect.name(), options.schema);
        let Ok(Some(columns)) = columns else {
            continue;
        };
        if columns.len() != items.len() {
            continue;
        }
        for (item, column) in items.into_iter().zip(columns) {
            let Some(shape) = column.json.filter(|shape| *shape != JsonShape::Any) else {
                continue;
            };
            if !is_any(item, text) {
                continue;
            }
            let value = JsonValue::new(shape, column.nullability == Nullability::Nullable);
            let written = module.type_of(&value, &pascal(&column.name));
            edits.push((item.start().to_usize(), item.end().to_usize(), written));
        }
    }
    if edits.is_empty() {
        return text.to_string();
    }
    edits.sort();
    let mut edited = text.to_string();
    for (start, end, written) in edits.into_iter().rev() {
        edited.replace_range(start..end, &written);
    }
    module.with_classes(edited)
}

/// The top-level names of a module and the classes generated for it so far.
struct Module<'a> {
    source: &'a Source,
    taken: HashSet<String>,
    existing: HashMap<String, &'a Stmt>,
    new: Vec<(String, String)>,
}

impl<'a> Module<'a> {
    fn new(source: &'a Source) -> Self {
        let text = &*source.text;
        let lines = &source.lines;
        let generated = |line: usize| {
            line > 1 && {
                let above = &text[lines.start(line - 1, text.len())..lines.start(line, text.len())];
                above.trim_end_matches(['\r', '\n']) == MARKER
            }
        };
        let (mut taken, mut existing) = (HashSet::new(), HashMap::new());
        for stmt in source.body() {
            let targets: &[Expr] = match stmt {
                Stmt::Assign(assign) => &assign.targets,
                Stmt::Delete(delete) => &delete.targets,
                _ => &[],
            };
            let mut names: Vec<String> = targets
                .iter()
                .filter_map(|target| Some(target.as_name_expr()?.id.to_string()))
                .collect();
            let mut start = stmt.start();
            match stmt {
                Stmt::ClassDef(class) => {
                    names.push(class.name.to_string());
                    start = class_start(class, text);
                }
                Stmt::FunctionDef(function) => names.push(function.name.to_string()),
                Stmt::Import(import) => names.extend(import.names.iter().map(local)),
                Stmt::ImportFrom(import) => names.extend(import.names.iter().map(local)),
                _ => {}
            }
            if let Some(first) = names.first().filter(|_| generated(source.line(start))) {
                existing.insert(first.clone(), stmt);
            }
            taken.extend(names);
        }
        Module {
            source,
            taken,
            existing,
            new: Vec::new(),
        }
    }

    /// The Python type of a JSON value; objects get a class `name`.
    fn type_of(&mut self, value: &JsonValue, name: &str) -> String {
        let written = match &value.shape {
            JsonShape::Integer => "int".to_string(),
            JsonShape::Number => "float".to_string(),
            JsonShape::String => "str".to_string(),
            JsonShape::Boolean => "bool".to_string(),
            JsonShape::Null => "None".to_string(),
            JsonShape::Any | JsonShape::Never => ANY.to_string(),
            JsonShape::Array(item) => {
                format!("list[{}]", self.type_of(item, &format!("{name}Item")))
            }
            JsonShape::Map(item) => {
                format!("dict[str, {}]", self.type_of(item, &format!("{name}Value")))
            }
            JsonShape::Object(fields) => self.typed_dict(fields, name),
        };
        if value.nullable && written != "None" {
            format!("{written} | None")
        } else {
            written
        }
    }

    /// The name of a `TypedDict` class with `fields`, written once per module.
    fn typed_dict(&mut self, fields: &[JsonField], name: &str) -> String {
        let types: Vec<(&str, String)> = fields
            .iter()
            .map(|field| {
                let written = self.type_of(&field.value, &format!("{name}{}", pascal(&field.name)));
                let written = if field.optional {
                    format!("typing.NotRequired[{written}]")
                } else {
                    written
                };
                (field.name.as_str(), written)
            })
            .collect();
        let candidates =
            std::iter::once(name.to_string()).chain((2..100).map(|n| format!("{name}{n}")));
        for candidate in candidates {
            let code = class(&candidate, &types);
            let written = self.new.iter().find(|(name, _)| *name == candidate);
            if written.is_some_and(|(_, known)| *known == code)
                || self.is_existing(&candidate, &code)
            {
                return candidate;
            }
            if written.is_none() && !self.taken.contains(&candidate) {
                self.new.push((candidate.clone(), code));
                return candidate;
            }
        }
        ANY.to_string()
    }

    /// Whether the module holds the generated statement `code` under `name`, however spelled.
    fn is_existing(&self, name: &str, code: &str) -> bool {
        let Some(existing) = self.existing.get(name) else {
            return false;
        };
        crate::parse::module(code).is_some_and(|parsed| {
            parsed
                .syntax()
                .body
                .first()
                .is_some_and(|stmt| ComparableStmt::from(stmt) == ComparableStmt::from(*existing))
        })
    }

    /// `edited` with the new classes after the imports, or after the generated ones.
    fn with_classes(self, mut edited: String) -> String {
        static GROUP: LazyLock<Regex> = LazyLock::new(|| {
            let block = format!(
                r"{}\r?\n(?:class [^\r\n]*:\r?\n(?:    [^\r\n]*\r?\n)+|[A-Za-z_]\w* = typing\.TypedDict\([^\r\n]*\)\r?\n)",
                regex::escape(MARKER)
            );
            Regex::new(&format!(
                r"(?:\r?\n){{2}}{block}(?:(?:\r?\n){{2}}{block})*(?:\r?\n){{2}}"
            ))
            .expect("a valid pattern")
        });
        if self.new.is_empty() {
            return edited;
        }
        let eol = if edited.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let blocks: Vec<String> = self
            .new
            .iter()
            .map(|(_, code)| format!("{MARKER}\n{code}"))
            .collect();
        let blocks = blocks.join("\n\n").replace('\n', eol);
        if let Some(group) = GROUP.find_iter(&edited).last() {
            let at = group.end();
            edited.insert_str(at, &format!("{blocks}{eol}{eol}"));
            return edited;
        }
        let text = &*self.source.text;
        let header = self
            .source
            .body()
            .iter()
            .rfind(|stmt| matches!(stmt, Stmt::Import(_) | Stmt::ImportFrom(_)));
        // Where the line after the imports started before the edits, counted in characters.
        let before = header.map_or(0, |stmt| {
            let line = self
                .source
                .line(stmt.end() - ruff_text_size::TextSize::new(1));
            text[..self.source.lines.start(line + 1, text.len())]
                .chars()
                .count()
        });
        let mut at = edited
            .char_indices()
            .nth(before)
            .map_or(edited.len(), |(at, _)| at);
        if at > 0 && !edited[..at].ends_with('\n') {
            edited.insert_str(at, eol);
            at += eol.len();
        }
        edited.insert_str(at, &format!("{eol}{eol}{blocks}{eol}{eol}"));
        edited
    }
}

fn local(alias: &ruff_python_ast::Alias) -> String {
    let name = alias.asname.as_ref().unwrap_or(&alias.name).as_str();
    name.split('.').next().unwrap_or(name).to_string()
}

/// Whether `key` can name a class field: an identifier that is not a keyword.
fn is_field_name(key: &str) -> bool {
    crate::parse::expression(key).is_some_and(|parsed| {
        let body = &parsed.syntax().body;
        body.is_name_expr() && body.range().len().to_usize() == key.len()
    })
}

fn class(name: &str, fields: &[(&str, String)]) -> String {
    if !fields.is_empty() && fields.iter().all(|(key, _)| is_field_name(key)) {
        let body: String = fields
            .iter()
            .map(|(key, written)| format!("    {key}: {written}\n"))
            .collect();
        return format!("class {name}(typing.TypedDict):\n{body}");
    }
    let items: Vec<String> = fields
        .iter()
        .map(|(key, written)| format!("{}: {written}", repr_str(key)))
        .collect();
    format!(
        "{name} = typing.TypedDict({}, {{{}}})\n",
        repr_str(name),
        items.join(", ")
    )
}

/// `order_lines` as `OrderLines`; a name with no letters is `Json`.
fn pascal(name: &str) -> String {
    let mut written = String::new();
    for word in name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
    {
        let mut chars = word.chars();
        if let Some(first) = chars.next() {
            written.push(first.to_ascii_uppercase());
            written.push_str(chars.as_str());
        }
    }
    if written.is_empty() || written.starts_with(|c: char| c.is_ascii_digit()) {
        written.insert_str(0, "Json");
    }
    written
}
