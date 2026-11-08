//! Parameter types: the `P` of `SQL[R, P]` that `param-types = true` writes.
//!
//! Each `SQL[R]("--sql\n...")` call gets the type of its parameters:
//!
//! - positional parameters (`%s`, `$1`, `?`) are a `tuple`, `NoParams` when there are none;
//! - named parameters (`%(name)s`, `:name`) are a `TypedDict` class, written after the imports
//!   below a `# sqlty: generated` line, as `json-types` writes its classes; `strip` removes it.
//!
//! A parameter is `T | None` unless it is stored in a `NOT NULL` column, and `typing.Any`
//! when its type is unknown or cast (`%s::int` takes anything).

use std::borrow::Cow;
use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;
use sqlty_sql::analyzer::{analyze_params, Nullability, Param};

use crate::edit::{apply_edits, Edit};
use crate::options::Options;
use crate::rewriter::{decode_escapes, insert_imports, sqlty_imports};
use crate::scan::Scan;
use crate::tokens::{Kind, Token};

const MARKER: &str = "# sqlty: generated";
const MODULES: [&str; 5] = ["datetime", "decimal", "ipaddress", "typing", "uuid"];
const KEYWORDS: [&str; 35] = [
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

/// The generated classes of a module, as `strip` removes them: one group between blank lines.
static GROUP: LazyLock<Regex> = LazyLock::new(|| {
    let block = format!(
        r"{MARKER}\r?\n(?:class [^\r\n]*:\r?\n(?:    [^\r\n]*\r?\n)+|[A-Za-z_]\w* = typing\.TypedDict\([^\r\n]*\)\r?\n)"
    );
    Regex::new(&format!(
        r"(?:\r?\n){{2}}{block}(?:(?:\r?\n){{2}}{block})*(?:\r?\n){{2}}"
    ))
    .unwrap()
});

/// `source` without the classes generated below `# sqlty: generated` lines.
pub(crate) fn strip_generated(source: &str) -> Cow<'_, str> {
    match source.contains(MARKER) {
        true => GROUP.replace_all(source, ""),
        false => Cow::Borrowed(source),
    }
}

/// `source` with the parameter type of each `SQL[R]("--sql\n...")` call written.
///
/// Example:
///
/// ```
/// use sqlty_python::options::Options;
/// use sqlty_python::params::add_param_types;
///
/// let options = Options::new("postgres", None, None, false).unwrap();
/// let source = "from sqlty import SQL\nq = SQL[tuple[int]](\"--sql\\nSELECT 1 LIMIT %s\")\n";
/// assert_eq!(
///     add_param_types(source, &options),
///     "from sqlty import SQL\nq = SQL[tuple[int], tuple[int | None]](\"--sql\\nSELECT 1 LIMIT %s\")\n",
/// );
/// ```
pub fn add_param_types(source: &str, options: &Options) -> String {
    let scan = Scan::new(source, &options.style.wrapper_name);
    let mut module = Module::of(&scan);
    let mut edits = Vec::new();
    let mut no_params = false;
    for call in &scan.calls {
        let row_end = source[..call.annotation.end].trim_end().len();
        if !call.exact || call.template || source[call.annotation.end..].starts_with(',') {
            continue;
        }
        let Some(content) = call.content(source) else {
            continue;
        };
        let Some(params) = analyze_params(
            &decode_escapes(&content),
            options.dialect.name(),
            options.schema,
        ) else {
            continue;
        };
        let types: Vec<(Option<String>, String)> = params
            .iter()
            .map(|param| (param.name.clone(), param_type(param)))
            .collect();
        let text = if types.is_empty() || types.iter().any(|(name, _)| name.is_none()) {
            let items: Vec<&str> = types.iter().map(|(_, text)| text.as_str()).collect();
            match items.is_empty() {
                true => {
                    no_params = true;
                    options.style.no_params_name()
                }
                false => format!("tuple[{}]", items.join(", ")),
            }
        } else {
            let name = target(&scan, &call.span).unwrap_or("query");
            let name = match is_upper(name) {
                true => pascal(&name.to_lowercase()),
                false => pascal(name),
            };
            let fields = types
                .into_iter()
                .map(|(name, text)| (name.unwrap_or_default(), text))
                .collect::<Vec<_>>();
            module.named_class(&fields, &format!("{name}Params"))
        };
        edits.push(Edit::new(row_end..row_end, format!(", {text}")));
    }
    if edits.is_empty() {
        return source.to_string();
    }
    let edited = with_imports(&module.with_classes(&apply_edits(source, edits).0));
    match options.style.no_params_import() {
        Some(line) if no_params && !binds_no_params(&edited) => with_sqlty_import(&edited, line),
        _ => edited,
    }
}

/// `source` with `line` below its first `from sqlty import ...`, or with the other imports.
fn with_sqlty_import(source: &str, line: &str) -> String {
    let scan = Scan::new(source, "");
    let text = |i: usize| &source[scan.tokens[i].span.clone()];
    let Some(at) = statements(&scan)
        .find(|statement| {
            is_import(source, &scan.tokens[statement.clone()], false)
                && text(statement.start) == "from"
                && statement.len() > 1
                && text(statement.start + 1) == "sqlty"
        })
        .map(|statement| end_line(&scan, statement))
    else {
        return insert_imports(source, &[line.to_string()]);
    };
    let eol = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines: Vec<String> = source.split_inclusive('\n').map(String::from).collect();
    lines.insert(at.min(lines.len()), format!("{line}{eol}"));
    lines.concat()
}

fn binds_no_params(source: &str) -> bool {
    sqlty_imports(&Scan::new(source, ""))
        .iter()
        .any(|(name, binding)| (name == "NoParams" && binding == "NoParams") || name == "*")
}

fn param_type(param: &Param) -> String {
    match &param.python_type {
        Some(text) if !param.cast && param.nullability == Nullability::NonNull => text.clone(),
        Some(text) if !param.cast => format!("{text} | None"),
        _ => "typing.Any".to_string(),
    }
}

/// The name a call is assigned to, `x` of `x = SQL[R](...)` or `x: T = SQL[R](...)`.
fn target<'s>(scan: &'s Scan, call: &Range<usize>) -> Option<&'s str> {
    let (source, tokens) = (scan.source(), &scan.tokens);
    let text = |i: usize| &source[tokens[i].span.clone()];
    let mut first = tokens.partition_point(|token| token.span.start < call.start);
    let mut last = tokens.partition_point(|token| token.span.end <= call.end);
    while first > 0
        && last < tokens.len()
        && text(first - 1) == "("
        && text(last) == ")"
        && tokens[first - 1].depth == tokens[last].depth
    {
        (first, last) = (first - 1, last + 1);
    }
    let ends = tokens.get(last).is_none_or(|token| {
        matches!(token.kind, Kind::Newline | Kind::Comment) || text(last) == ";"
    });
    let assigned = first >= 2
        && text(first - 1) == "="
        && tokens[first - 1].depth == tokens[first].depth
        && !matches!(text(first - 2), "=" | "!" | "<" | ">" | ":");
    if !ends || !assigned {
        return None;
    }
    let start = (0..first)
        .rev()
        .find(|&i| tokens[i].kind == Kind::Newline || (text(i) == ";" && tokens[i].depth == 0))
        .map_or(0, |i| i + 1);
    let plain = |i: usize| tokens[i].kind == Kind::Name && !text(i).contains('.');
    let before = first - 2;
    if plain(before) && (before == start || text(before - 1) == "=") {
        return Some(text(before));
    }
    (plain(start) && start + 1 < first && text(start + 1) == ":").then(|| text(start))
}

/// The top-level names of a module, its generated classes and the ones added so far.
struct Module {
    taken: Vec<String>,
    existing: Vec<(String, String)>,
    new: Vec<(String, String)>,
}

impl Module {
    fn of(scan: &Scan) -> Self {
        let source = scan.source();
        let mut module = Module {
            taken: Vec::new(),
            existing: Vec::new(),
            new: Vec::new(),
        };
        for statement in statements(scan) {
            let names = names(source, &scan.tokens[statement.clone()]);
            let start = scan.tokens[statement.start].span.start;
            let above = source[..start].trim_end_matches(['\n', '\r']);
            let above = &above[above.rfind('\n').map_or(0, |i| i + 1)..];
            if above == MARKER && !names.is_empty() {
                module
                    .existing
                    .push((names[0].clone(), block(&source[start..])));
            }
            module.taken.extend(names);
        }
        module
    }

    /// The name of a `TypedDict` class with `fields`, written once per module.
    fn named_class(&mut self, fields: &[(String, String)], name: &str) -> String {
        for n in 1..100 {
            let candidate = match n {
                1 => name.to_string(),
                n => format!("{name}{n}"),
            };
            let code = class(&candidate, fields);
            let same = |found: &[(String, String)], text: &str| {
                found
                    .iter()
                    .any(|(name, code)| *name == candidate && code == text)
            };
            if same(&self.existing, code.trim_end()) || same(&self.new, &code) {
                return candidate;
            }
            let free =
                |found: &[(String, String)]| found.iter().all(|(name, _)| *name != candidate);
            if !self.taken.contains(&candidate) && free(&self.new) {
                self.new.push((candidate.clone(), code));
                return candidate;
            }
        }
        "typing.Any".to_string()
    }

    /// `source` with the new classes after the imports, or after the generated ones.
    fn with_classes(&self, source: &str) -> String {
        if self.new.is_empty() {
            return source.to_string();
        }
        let eol = if source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let blocks = self
            .new
            .iter()
            .map(|(_, code)| format!("{MARKER}\n{code}"))
            .collect::<Vec<_>>()
            .join("\n\n")
            .replace('\n', eol);
        if let Some(group) = GROUP.find_iter(source).last() {
            let at = group.end();
            return format!("{}{blocks}{eol}{eol}{}", &source[..at], &source[at..]);
        }
        let scan = Scan::new(source, "");
        let line = head(&scan)
            .filter(|statement| is_import(source, &scan.tokens[statement.clone()], true))
            .last()
            .map_or(0, |statement| end_line(&scan, statement));
        let mut at = source
            .split_inclusive('\n')
            .take(line)
            .map(str::len)
            .sum::<usize>();
        let mut source = source.to_string();
        if at > 0 && !source[..at].ends_with('\n') {
            source.insert_str(at, eol);
            at += eol.len();
        }
        format!(
            "{}{eol}{eol}{blocks}{eol}{eol}{}",
            &source[..at],
            &source[at..]
        )
    }
}

/// The statements before the first line of code: the docstring, comments and imports.
fn head<'a>(scan: &'a Scan) -> impl Iterator<Item = Range<usize>> + 'a {
    let source = scan.source();
    statements(scan).take_while(move |statement| {
        let tokens = &scan.tokens[statement.clone()];
        is_import(source, tokens, true)
            || (statement.start == 0 && tokens[0].kind == Kind::Str)
            || tokens.iter().all(|token| token.kind == Kind::Comment)
    })
}

/// `source` with `import M` for each module of [`MODULES`] it names but lacks, in order.
fn with_imports(source: &str) -> String {
    let scan = Scan::new(source, "");
    let imports: Vec<(bool, String, Range<usize>)> = head(&scan)
        .filter(|statement| is_import(source, &scan.tokens[statement.clone()], false))
        .map(|statement| {
            let tokens = &scan.tokens[statement.clone()];
            let from = &source[tokens[0].span.clone()] == "from";
            let first = tokens
                .get(1)
                .map_or("", |token| &source[token.span.clone()]);
            let start = scan.lines.line_at(tokens[0].span.start);
            (from, first.to_string(), start..end_line(&scan, statement))
        })
        .collect();
    let imported: Vec<String> = statements(&scan)
        .map(|statement| &scan.tokens[statement])
        .filter(|tokens| &source[tokens[0].span.clone()] == "import")
        .flat_map(|tokens| imported_names(source, &tokens[1..], false))
        .collect();
    let missing: Vec<&str> = MODULES
        .into_iter()
        .filter(|module| !imported.iter().any(|name| name == module))
        .filter(|module| {
            Regex::new(&format!(r"\b{module}\."))
                .unwrap()
                .is_match(source)
        })
        .collect();
    let Some(last) = imports.last() else {
        return source.to_string();
    };
    if missing.is_empty() {
        return source.to_string();
    }
    let eol = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut lines: Vec<String> = source.split_inclusive('\n').map(String::from).collect();
    for module in missing.into_iter().rev() {
        let line = imports
            .iter()
            .find(|(from, first, _)| *from || first.as_str() > module)
            .map_or(last.2.end, |(_, _, lines)| lines.start - 1);
        lines.insert(line.min(lines.len()), format!("import {module}{eol}"));
    }
    lines.concat()
}

/// Token ranges of the top-level statements, split at line breaks and `;`.
fn statements<'a>(scan: &'a Scan) -> impl Iterator<Item = Range<usize>> + 'a {
    let (source, tokens) = (scan.source(), &scan.tokens);
    let at_line_start = |token: &Token| {
        let start = token.span.start;
        start == 0 || source.as_bytes()[start - 1] == b'\n'
    };
    let mut i = 0;
    std::iter::from_fn(move || {
        while i < tokens.len() {
            let start = i;
            let mut end = i;
            while end < tokens.len()
                && tokens[end].kind != Kind::Newline
                && !(tokens[end].depth == 0 && &source[tokens[end].span.clone()] == ";")
            {
                end += 1;
            }
            let after_semicolon = start > 0 && &source[tokens[start - 1].span.clone()] == ";";
            i = end + 1;
            if end > start
                && tokens[start].kind == Kind::Name
                && (at_line_start(&tokens[start]) || after_semicolon)
            {
                return Some(start..end);
            }
        }
        None
    })
}

/// The last line of a statement, 1-based.
fn end_line(scan: &Scan, statement: Range<usize>) -> usize {
    let span = &scan.tokens[statement.end - 1].span;
    scan.lines.line_at(span.end - 1)
}

/// Whether a statement is an import; `future` counts `from __future__ import ...`.
fn is_import(source: &str, tokens: &[Token], future: bool) -> bool {
    match &source[tokens[0].span.clone()] {
        "import" => true,
        "from" => {
            future
                || tokens
                    .get(1)
                    .is_some_and(|t| &source[t.span.clone()] != "__future__")
        }
        _ => false,
    }
}

/// The names a top-level statement binds, as the generated classes must not take them.
fn names(source: &str, tokens: &[Token]) -> Vec<String> {
    let text = |i: usize| {
        tokens
            .get(i)
            .map_or("", |token| &source[token.span.clone()])
    };
    match text(0) {
        "class" | "def" => vec![text(1).to_string()],
        "async" if text(1) == "def" => vec![text(2).to_string()],
        "import" => imported_names(source, &tokens[1..], true),
        "from" => match (0..tokens.len()).find(|&i| text(i) == "import") {
            Some(i) => imported_names(source, &tokens[i + 1..], false),
            None => Vec::new(),
        },
        _ => (0..tokens.len())
            .filter(|&i| {
                tokens[i].kind == Kind::Name
                    && !text(i).contains('.')
                    && (i == 0 || text(i - 1) == "=")
                    && text(i + 1) == "="
                    && text(i + 2) != "="
            })
            .map(|i| text(i).to_string())
            .collect(),
    }
}

/// The names `a, b.c as d` binds: the alias, else the name, the first part with `first`.
fn imported_names(source: &str, tokens: &[Token], first: bool) -> Vec<String> {
    let mut found = Vec::new();
    let words: Vec<&str> = tokens
        .iter()
        .filter(|token| token.kind == Kind::Name || &source[token.span.clone()] == ",")
        .map(|token| &source[token.span.clone()])
        .collect();
    for item in words.split(|word| *word == ",") {
        let name = match item {
            [.., "as", alias] => alias.to_string(),
            [name, ..] if first => name.split('.').next().unwrap_or(name).to_string(),
            [name, ..] => name.to_string(),
            [] => continue,
        };
        found.push(name);
    }
    found
}

/// The statement at the start of `text` with the indented lines of its body, without the
/// line break that ends it.
fn block(text: &str) -> String {
    let mut lines = text.split_inclusive('\n');
    let mut found = lines.next().unwrap_or_default().trim_end().to_string();
    for line in lines.take_while(|line| line.starts_with(' ') && !line.trim().is_empty()) {
        found.push('\n');
        found.push_str(line.trim_end());
    }
    found
}

fn class(name: &str, fields: &[(String, String)]) -> String {
    let identifiers = fields
        .iter()
        .all(|(key, _)| is_identifier(key) && !KEYWORDS.contains(&key.as_str()));
    if !fields.is_empty() && identifiers {
        let body: String = fields
            .iter()
            .map(|(key, text)| format!("    {key}: {text}\n"))
            .collect();
        return format!("class {name}(typing.TypedDict):\n{body}");
    }
    let items: Vec<String> = fields
        .iter()
        .map(|(key, text)| format!("{}: {text}", repr(key)))
        .collect();
    format!(
        "{name} = typing.TypedDict({}, {{{}}})\n",
        repr(name),
        items.join(", ")
    )
}

fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// A Python `repr` of a string without control characters.
fn repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let escaped = text.replace('\\', "\\\\");
    let escaped = match quote {
        '\'' => escaped.replace('\'', "\\'"),
        _ => escaped,
    };
    format!("{quote}{escaped}{quote}")
}

/// Whether `text` has a cased character and only upper-case ones, as `str.isupper`.
fn is_upper(text: &str) -> bool {
    text.chars().any(char::is_uppercase) && !text.chars().any(char::is_lowercase)
}

/// `order_lines` as `OrderLines`; a name with no letters is `Json`.
fn pascal(name: &str) -> String {
    let text: String = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| word[..1].to_uppercase() + &word[1..])
        .collect();
    match text.starts_with(|c: char| c.is_ascii_digit()) || text.is_empty() {
        true => format!("Json{text}"),
        false => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_params_import_is_added_once() {
        let options = Options::new("postgres", None, None, false).unwrap();
        let rewrite = |source: &str| add_param_types(source, &options);
        let wrapped = "q = SQL[tuple[int]](\"--sql\\nSELECT 1\")\n";
        assert_eq!(
            rewrite(&format!("import datetime\nfrom sqlty import SQL\n{wrapped}")),
            "import datetime\nfrom sqlty import SQL\nfrom sqlty import NoParams\nq = SQL[tuple[int], NoParams](\"--sql\\nSELECT 1\")\n",
        );
        assert_eq!(
            rewrite(&format!("from sqlty import SQL\n{wrapped}from sqlty import Bound\n")),
            "from sqlty import SQL\nfrom sqlty import NoParams\nq = SQL[tuple[int], NoParams](\"--sql\\nSELECT 1\")\nfrom sqlty import Bound\n",
        );
        assert_eq!(
            rewrite(&format!("from sqlty import SQL, NoParams as NP\n{wrapped}")),
            "from sqlty import SQL, NoParams as NP\nfrom sqlty import NoParams\nq = SQL[tuple[int], NoParams](\"--sql\\nSELECT 1\")\n",
        );
        for header in ["from sqlty import SQL, NoParams\n", "from sqlty import *\n"] {
            assert_eq!(
                rewrite(&format!("{header}{wrapped}")),
                format!("{header}q = SQL[tuple[int], NoParams](\"--sql\\nSELECT 1\")\n"),
            );
        }
    }
}
