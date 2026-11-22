//! Python source rewriter — rewrite/strip `--sql` strings.
//!
//! Handles:
//! - `rewrite_source()`: `"--sql\n..." → SQL[T]("--sql\n...")`
//! - `strip_source()`: `SQL[T]("--sql\n...") → "--sql\n..."`
//!
//! Both work on the tokens of the whole source (see `tokens`), so triple-quoted
//! strings spanning lines and implicitly concatenated strings are one query.
//! The rewriter never modifies string content — it only adds/removes
//! the `SQL[T]()` wrapper.  This guarantees `strip(rewrite(x)) == x`.

use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex, PoisonError};

use regex::Regex;
use std::sync::LazyLock;

use crate::edit::{apply_edits, Edit};
use crate::options::{ConfigError, Options};
use crate::rules::Rule;
use crate::scan::Scan;
use crate::tokens::{string_at, tokenize, Kind, Token};
use sqlty_sql::analyzer::{infer_type, returns_rows, route, SchemaData};

/// Standard-library modules inferred row types are spelled with.
pub(crate) const TYPE_MODULES: [&str; 5] = ["datetime", "uuid", "decimal", "ipaddress", "typing"];

// ---------------------------------------------------------------------------
// Regex patterns
// ---------------------------------------------------------------------------

static CODING: LazyLock<Regex> = LazyLock::new(|| sqlty_sql::regex(r"^#.*?coding[:=]"));

// ---------------------------------------------------------------------------
// Import style parsing
// ---------------------------------------------------------------------------

/// Parsed `import-style`: the import line to inject and the `[T](...)` wrapper prefix.
///
/// `from sqlty import SQL` -> `SQL`, `from sqlty import SQL as Q` -> `Q`,
/// `import sqlty` -> `sqlty.SQL`, `import sqlty as sy` -> `sy.SQL`.
pub struct ImportStyle {
    /// The import statement `rewrite` adds (and `strip` removes).
    pub import_line: String,
    /// The name the wrapper is called by in the source, such as `SQL` or `sqlty.SQL`.
    pub wrapper_name: String,
}

impl Default for ImportStyle {
    fn default() -> Self {
        ImportStyle {
            import_line: "from sqlty import SQL".to_string(),
            wrapper_name: "SQL".to_string(),
        }
    }
}

impl ImportStyle {
    /// Read the `import-style` setting; `None` or empty is `from sqlty import SQL`.
    pub fn parse(style: Option<&str>) -> Result<Self, ConfigError> {
        let Some(style) = style.map(str::trim).filter(|style| !style.is_empty()) else {
            return Ok(Self::default());
        };
        let wrapper_name = if style == "from sqlty import SQL" {
            "SQL".to_string()
        } else if let Some(alias) = style.strip_prefix("from sqlty import SQL as ") {
            alias.trim().to_string()
        } else if style == "import sqlty" {
            "sqlty.SQL".to_string()
        } else if let Some(alias) = style.strip_prefix("import sqlty as ") {
            format!("{}.SQL", alias.trim())
        } else {
            return Err(ConfigError::ImportStyle(style.to_string()));
        };
        Ok(ImportStyle {
            import_line: style.to_string(),
            wrapper_name,
        })
    }

    /// [`ImportStyle::parse`], with the default for an unsupported style.
    #[cfg(test)]
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub(crate) fn or_default(style: Option<&str>) -> Self {
        Self::parse(style).unwrap_or_default()
    }

    /// How `UNTYPED` is spelled: "UNTYPED" or "sqlty.UNTYPED".
    pub(crate) fn untyped_name(&self) -> String {
        match self.wrapper_name.rsplit_once('.') {
            Some((module, _)) => format!("{module}.UNTYPED"),
            None => "UNTYPED".to_string(),
        }
    }

    /// How `SQLTemplate` is spelled: "SQLTemplate" or "sqlty.SQLTemplate".
    pub(crate) fn template_name(&self) -> String {
        crate::tokens::template_wrapper(&self.wrapper_name)
    }

    /// Import line `SQLTemplate` needs, or `None` when the module is imported.
    pub(crate) fn template_import(&self) -> Option<&'static str> {
        (!self.wrapper_name.contains('.')).then_some("from sqlty import SQLTemplate")
    }

    /// Import line `UNTYPED` needs, or `None` when the module is imported.
    pub(crate) fn untyped_import(&self) -> Option<&'static str> {
        (!self.wrapper_name.contains('.')).then_some("from sqlty import UNTYPED")
    }

    /// Import line `NoParams` needs, or `None` when the module is imported.
    pub(crate) fn no_params_import(&self) -> Option<&'static str> {
        (!self.wrapper_name.contains('.')).then_some("from sqlty import NoParams")
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Rewrite Python source: `"--sql\n..."` → `SQL[T]("--sql\n...")`.
///
/// Adds necessary imports (SQL and the `TYPE_MODULES` row types use) at the top.
/// Never modifies string content — only adds the `SQL[T]()` wrapper.
/// With `options.untyped`, row-returning queries of unknown row type become
/// `SQL[UNTYPED](...)`. With `keep_lines`, imports go on existing lines (see
/// `insert_inline`).
pub fn rewrite_source_with(source: &str, options: &Options, keep_lines: bool) -> String {
    let scan = Scan::new(source, &options.style.wrapper_name);
    rewrite_scan(&scan, options, keep_lines, &|_| false).unwrap_or_else(|| source.to_string())
}

/// [`rewrite_source_with`] on a scanned source, leaving alone the strings that start on a
/// line `skip` accepts: `None` when nothing is wrapped.
pub(crate) fn rewrite_scan(
    scan: &Scan,
    options: &Options,
    keep_lines: bool,
    skip: &dyn Fn(usize) -> bool,
) -> Option<String> {
    let (dialect, schema) = (options.dialect.name(), options.schema);
    let replacements = wraps(scan, options, skip, &|content| {
        infer_sql_content(content, dialect, schema)
    });
    if replacements.is_empty() {
        return None;
    }
    let (result, applied) = apply_edits(scan.source(), replacements);
    let wrapped: Vec<&str> = applied
        .iter()
        .map(|edit| edit.replacement.as_str())
        .collect();
    Some(with_wrapper_imports(
        result,
        scan,
        &wrapped,
        &options.style,
        &type_modules(options),
        keep_lines,
    ))
}

/// The modules a row type may name: [`TYPE_MODULES`], and those of the schema's
/// `[tool.sqlty.types]`.
pub(crate) fn type_modules(options: &Options) -> Vec<String> {
    TYPE_MODULES
        .iter()
        .map(ToString::to_string)
        .chain(
            options
                .schema
                .map(SchemaData::type_modules)
                .unwrap_or_default()
                .into_iter()
                .filter(|module| !TYPE_MODULES.contains(&module.as_str())),
        )
        .collect()
}

/// `result` with the imports the `wrapped` calls need: the wrapper, `UNTYPED` and the
/// `modules` their row types name. `scan` is of a source with the same imports.
pub(crate) fn with_wrapper_imports(
    result: String,
    scan: &Scan,
    wrapped: &[&str],
    istyle: &ImportStyle,
    modules: &[String],
    keep_lines: bool,
) -> String {
    if wrapped.is_empty() {
        return result;
    }
    let imports = wrapper_imports(scan, wrapped, istyle, modules);
    if imports.is_empty() {
        return result;
    }
    if keep_lines {
        return insert_inline(&result, &imports.join("; "));
    }
    insert_imports(&result, &imports)
}

/// The import lines the `wrapped` calls need that the source of `scan` lacks: see
/// [`with_wrapper_imports`].
pub(crate) fn wrapper_imports(
    scan: &Scan,
    wrapped: &[&str],
    istyle: &ImportStyle,
    modules: &[String],
) -> Vec<String> {
    let source = scan.source();
    let untyped = format!("[{}]", istyle.untyped_name());
    let needs_untyped_import = wrapped.iter().any(|r| {
        r.split_once('(')
            .is_some_and(|(head, _)| head.trim_end().ends_with(&untyped))
    });
    let needs = |marker: &str| wrapped.iter().any(|r| r.contains(marker));
    let mut imports = Vec::new();
    for module in modules {
        if needs(&format!("{module}.")) && !has_import(source, &format!("import {module}")) {
            imports.push(format!("import {module}"));
        }
    }
    let module_style = istyle.wrapper_name.contains('.');
    let bound = sqlty_imports(scan);
    let binds = |name: &str, binding: &str| {
        bound
            .iter()
            .any(|(n, b)| (n == name && b == binding) || (n == "*" && name == binding))
    };
    let wraps_with = |name: &str| wrapped.iter().any(|r| r.starts_with(&format!("{name}[")));
    let needs_wrapper =
        wraps_with(&istyle.wrapper_name) || (module_style && wraps_with(&istyle.template_name()));
    if needs_wrapper
        && ((module_style && !has_import(source, &istyle.import_line))
            || (!module_style && !binds("SQL", &istyle.wrapper_name)))
    {
        imports.push(istyle.import_line.clone());
    }
    if let Some(line) = istyle.template_import() {
        if wraps_with(&istyle.template_name()) && !binds("SQLTemplate", "SQLTemplate") {
            imports.push(line.to_string());
        }
    }
    if let Some(line) = istyle.untyped_import() {
        if needs_untyped_import && !binds("UNTYPED", "UNTYPED") {
            imports.push(line.to_string());
        }
    }
    imports.extend(Spelling::of(scan).imports_for(wrapped));
    imports
}

/// `source` with `imports` added after its header: the docstring, `from __future__`
/// imports and the comments before them. A blank line after a docstring stays there, above
/// the imports, and the code below them keeps one.
pub(crate) fn insert_imports(source: &str, imports: &[String]) -> String {
    let edit = import_edit(source, imports);
    format!(
        "{}{}{}",
        &source[..edit.range.start],
        edit.replacement,
        &source[edit.range.end..]
    )
}

/// The insertion [`insert_imports`] makes.
pub(crate) fn import_edit(source: &str, imports: &[String]) -> Edit {
    let at = header_end(source);
    let eol = line_ending(source, at);
    let sep = if at > 0 && !source[..at].ends_with('\n') {
        eol
    } else {
        ""
    };
    let blank = source[at..]
        .split_inclusive('\n')
        .take_while(|line| line.trim().is_empty() && line.ends_with('\n'))
        .map(str::len)
        .sum::<usize>();
    if at == 0 || blank == 0 {
        return Edit::new(at..at, format!("{sep}{}{eol}", imports.join(eol)));
    }
    let next = source[at + blank..].trim_start();
    let gap = if next.is_empty() || next.starts_with("import ") || next.starts_with("from ") {
        ""
    } else {
        eol
    };
    Edit::new(
        at + blank..at + blank,
        format!("{}{eol}{gap}", imports.join(eol)),
    )
}

/// Line ending of the line at `at`, else of the line before it.
pub(crate) fn line_ending(source: &str, at: usize) -> &'static str {
    let end = source[at..]
        .find('\n')
        .map(|i| at + i)
        .or_else(|| source[..at].rfind('\n'));
    if end.is_some_and(|i| source[..i].ends_with('\r')) {
        "\r\n"
    } else {
        "\n"
    }
}

/// A group is a string or implicitly concatenated strings. Rewrite skips groups in
/// a wrapper call (`mod.SQL[...]` included), the argument of a psycopg `SQL(...)`,
/// groups with a prefixed string, and
/// every group on a line that holds an f- or t-string `--sql` group.
/// `infer` gives the row type of a group's content.
pub(crate) fn wraps(
    scan: &Scan,
    options: &Options,
    skip: &dyn Fn(usize) -> bool,
    infer: &dyn Fn(&str) -> Option<String>,
) -> Vec<Edit> {
    let source = scan.source();
    let dialect = options.dialect.name();
    let (istyle, untyped) = (&options.style, options.untyped);
    let interpolated: Vec<usize> = scan
        .groups()
        .iter()
        .filter(|group| {
            matches!(
                group.skipped,
                Some(Rule::FstringSkipped | Rule::TstringSkipped)
            )
        })
        .map(|group| group.line)
        .collect();

    let psycopg = source.contains("psycopg");
    let spelling = Spelling::of(scan);
    let mut replacements = Vec::new();
    for group in scan.groups() {
        let bare = scan.bare_call(group);
        if (psycopg && bare.is_none() && scan.in_composed_sql(group.tokens[0]))
            || scan.in_case_pattern(group.tokens[0])
        {
            continue;
        }
        let (start, end) = (group.start, group.end);
        let template = scan.is_template(&group.tokens);
        if !(scan.is_plain(group) || template)
            || (bare.is_some() && template)
            || group.concatenated
            || scan.calls.enclosing(start, end).is_some()
            || interpolated.binary_search(&group.line).is_ok()
            || skip(group.line)
        {
            continue;
        }
        let content = scan.content(group);
        if after_marker(content.trim()).is_none() {
            continue;
        }
        let type_str = match infer(&content) {
            Some(inferred) => spelling.respell(inferred),
            None if untyped && is_untyped(&content, dialect, options.schema) => {
                istyle.untyped_name()
            }
            // Don't wrap when inference fails — SQL[Any] is never useful.
            None => continue,
        };
        let wrapper = if template {
            istyle.template_name()
        } else {
            istyle.wrapper_name.clone()
        };
        if let Some((call, name_end)) = bare {
            let (name, rest) = (&source[call.start..name_end], &source[name_end..call.end]);
            replacements.push(Edit::new(call, format!("{name}[{type_str}]{rest}")));
            continue;
        }
        let wrapped = format!("{wrapper}[{type_str}]({})", &source[start..end]);
        replacements.push(Edit::new(start..end, wrapped));
    }
    replacements
}

/// Strip Python source: `SQL[T]("--sql\n...")` → `"--sql\n..."`.
///
/// A wrapper whose argument is `--sql` string literals (implicitly concatenated,
/// with comments and a trailing comma) is replaced by them. When they can't stand
/// alone (a comment outside them, or line breaks only the call's parentheses
/// allow), only `SQL[T]` and the trailing comma go: `SQL[T]("a"  # c\n)` becomes
/// `("a"  # c\n)`. Other arguments are left alone. Removes unused imports added by
/// rewrite, and the classes `param-types` and `json-types` generated.
pub fn strip_source(source: &str, istyle: &ImportStyle) -> String {
    let source: &str = &crate::params::strip_generated(source);
    let scan = Scan::new(source, &istyle.wrapper_name);
    let mut replacements = Vec::new();
    for call in &scan.calls {
        if !call.exact || !call.content(source).is_some_and(|c| is_sqlty_managed(&c)) {
            continue;
        }
        if call.bare {
            replacements.push(Edit::new(call.span.clone(), call.argument(source)));
            continue;
        }
        replacements.push(Edit::new(call.span.start..call.open, ""));
        if let Some(comma) = &call.comma {
            replacements.push(Edit::new(comma.clone(), ""));
        }
    }

    if replacements.is_empty() {
        return source.to_string();
    }
    remove_unused_imports(&apply_edits(source, replacements).0, istyle)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Rules suppressed by `# sqlty: ignore` comments, by line: `None` suppresses every rule.
pub type Suppressed = HashMap<usize, Option<Vec<String>>>;

/// Whether a comment on `line` suppresses `rule`.
pub(crate) fn suppresses(suppressed: &Suppressed, line: usize, rule: Rule) -> bool {
    suppressed.get(&line).is_some_and(|rules| {
        rules
            .as_ref()
            .is_none_or(|rules| rules.iter().any(|r| r == rule.name()))
    })
}

/// Newline offsets of a source, so line lookups are a binary search.
pub(crate) struct LineIndex(Vec<usize>);

impl LineIndex {
    pub(crate) fn new(source: &str) -> Self {
        Self(source.match_indices('\n').map(|(i, _)| i).collect())
    }

    /// 1-indexed line number of a byte position.
    pub(crate) fn line_at(&self, pos: usize) -> usize {
        self.0.partition_point(|&newline| newline < pos) + 1
    }
}

/// Extract string content from a quoted string literal (removes quotes and a `u` prefix).
pub(crate) fn extract_string_content(literal: &str) -> &str {
    let literal = literal.trim_start_matches(['u', 'U']);
    let quote = if literal.starts_with("\"\"\"") || literal.starts_with("'''") {
        3
    } else {
        1
    };
    &literal[quote..literal.len() - quote]
}

/// Line breaks that can end the marker line: escaped in source text, or real.
const LINE_BREAKS: [&str; 4] = ["\\r\\n", "\\n", "\r\n", "\n"];

/// String content after the sqlty marker: `--sql`, or `-- sql` ending its line
/// (see [`ends_marker_line`]), a form MySQL also reads as a comment.
pub(crate) fn after_marker(content: &str) -> Option<&str> {
    let content = content.trim_start();
    if let Some(rest) = content.strip_prefix("--sql") {
        return Some(rest);
    }
    let rest = content.strip_prefix("-- sql")?;
    ends_marker_line(rest).then_some(rest)
}

/// Whether the rest of a marker line is blank, a `--` comment or a `sqlty:db=NAME`
/// database, up to a line break.
pub(crate) fn ends_marker_line(rest: &str) -> bool {
    let Some(end) = LINE_BREAKS.iter().filter_map(|lb| rest.find(lb)).min() else {
        return false;
    };
    let line = &rest[..end];
    let text = line.trim_start_matches([' ', '\t']);
    text.trim_end().is_empty()
        || (text.len() < line.len() && (text.starts_with("--") || text.starts_with("sqlty:")))
}

pub(crate) fn starts_with_line_break(text: &str) -> bool {
    LINE_BREAKS.iter().any(|lb| text.starts_with(lb))
}

/// `text` without one leading line break, escaped or real.
pub(crate) fn strip_line_break(text: &str) -> Option<&str> {
    LINE_BREAKS.iter().find_map(|lb| text.strip_prefix(lb))
}

/// Check if string content is sqlty-managed: the marker, then whitespace or a line break.
pub(crate) fn is_sqlty_managed(content: &str) -> bool {
    let Some(rest) = after_marker(content) else {
        return false;
    };
    rest.is_empty() || starts_with_line_break(rest) || rest.starts_with([' ', '\t'])
}

/// Whether `SQL[UNTYPED]` fits: rows are expected but their type is unknown.
///
/// Requires the marker to end its line; the space form is a SQL comment.
fn is_untyped(sql_content: &str, dialect: &str, schema: Option<&SchemaData>) -> bool {
    after_marker(sql_content)
        .and_then(strip_line_break)
        .is_some_and(|query| returns_rows(query, route(sql_content, dialect, schema).0))
}

/// Infer the row type of marked string content.
///
/// - `--sql\nSELECT ...` → an escaped line break (`\\n`, `\\r\\n`) becomes a real
///   one, and `analyzer::parser_input` drops the marker line before parsing.
/// - `--sql SELECT ...` → there is no line break, so nothing is left to parse → None.
///
/// Inference errors (an unknown `SUM` column) count as uninferable.
pub(crate) fn infer_sql_content(
    sql_content: &str,
    dialect: &str,
    schema: Option<&SchemaData>,
) -> Option<String> {
    infer_type(&marked_query(sql_content)?, dialect, schema)
}

/// Marked string content as the analyzer reads it: the string's value, escape sequences
/// decoded, from the `--sql` marker on. `None` when the content has no marker.
pub(crate) fn marked_query(sql_content: &str) -> Option<String> {
    let decoded = decode_escapes(sql_content);
    Some(format!("--sql{}", after_marker(&decoded)?))
}

/// The value of the source text of plain (not raw) string literals: `\n`, `\"`, `\\`,
/// `\x41`, `\u00e9`, `\N{BULLET}`, ... decoded, a backslash-newline (LF, CRLF or CR) dropped.
/// Unknown escapes, and a `\N{...}` that names no character, stay as written.
pub(crate) fn decode_escapes(text: &str) -> Cow<'_, str> {
    if !text.contains('\\') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    escapes(text, |piece, _| out.push_str(piece));
    Cow::Owned(out)
}

/// Where the byte at `decoded` of [`decode_escapes`]`(text)` is written in `text`: in an
/// escape, at its start, or its end with `end`.
pub(crate) fn escaped_offset(text: &str, decoded: usize, end: bool) -> usize {
    let (mut at, mut found) = (0, None);
    escapes(text, |piece, written: Range<usize>| {
        let next = at + piece.len();
        let inside = if end {
            at < decoded && decoded <= next
        } else {
            at <= decoded && decoded < next
        };
        if found.is_none() && inside {
            found = Some(match (written.len() == piece.len(), end) {
                (true, _) => written.start + decoded - at,
                (false, false) => written.start,
                (false, true) => written.end,
            });
        }
        at = next;
    });
    found.unwrap_or(text.len())
}

/// Each piece of [`decode_escapes`]`(text)`, with the bytes of `text` it is decoded from.
pub(crate) fn escapes(text: &str, mut piece: impl FnMut(&str, Range<usize>)) {
    let mut rest = text;
    let position = |rest: &str| text.len() - rest.len();
    while let Some(at) = rest.find('\\') {
        let start = position(rest);
        piece(&rest[..at], start..start + at);
        let from = start + at;
        let escape = &rest[at + 1..];
        let Some(next) = escape.chars().next() else {
            piece("\\", from..from + 1);
            return;
        };
        if next == '\r' {
            rest = escape[1..].strip_prefix('\n').unwrap_or(&escape[1..]);
            piece("", from..position(rest));
            continue;
        }
        let simple = match next {
            '\n' => Some(""),
            '\\' => Some("\\"),
            '\'' => Some("'"),
            '"' => Some("\""),
            'a' => Some("\u{7}"),
            'b' => Some("\u{8}"),
            'f' => Some("\u{c}"),
            'n' => Some("\n"),
            'r' => Some("\r"),
            't' => Some("\t"),
            'v' => Some("\u{b}"),
            _ => None,
        };
        if let Some(value) = simple {
            rest = &escape[next.len_utf8()..];
            piece(value, from..position(rest));
            continue;
        }
        let (digits, radix) = match next {
            'x' => (2, 16),
            'u' => (4, 16),
            'U' => (8, 16),
            '0'..='7' => (
                escape
                    .bytes()
                    .take(3)
                    .take_while(|b| (b'0'..=b'7').contains(b))
                    .count(),
                8,
            ),
            _ => (0, 0),
        };
        let skip = usize::from(radix == 16);
        let (code, used) = match named(escape) {
            Some((ch, used)) => (Some(ch), used),
            _ => (
                escape
                    .get(skip..skip + digits)
                    .filter(|_| digits > 0)
                    .and_then(|code| u32::from_str_radix(code, radix).ok())
                    .and_then(char::from_u32),
                skip + digits,
            ),
        };
        match code {
            Some(ch) => {
                rest = &escape[used..];
                piece(ch.encode_utf8(&mut [0; 4]), from..position(rest));
            }
            None => {
                piece("\\", from..from + 1);
                rest = escape;
            }
        }
    }
    piece(rest, position(rest)..text.len());
}

/// The character and the bytes of `N{NAME}` at the start of `escape` (what follows a
/// backslash), by Unicode name as Python reads it; the name table is the one the parser
/// already links.
fn named(escape: &str) -> Option<(char, usize)> {
    let name = escape.strip_prefix("N{")?;
    let close = name.find('}')?;
    Some((unicode_names2::character(&name[..close])?, close + 3))
}

/// `\bname\b`, compiled once per name: the few import styles in use repeat call after call.
pub(crate) fn word_regex(name: &str) -> Arc<Regex> {
    static CACHE: LazyLock<Mutex<HashMap<String, Arc<Regex>>>> = LazyLock::new(Mutex::default);
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(found) = cache.get(name) {
        return Arc::clone(found);
    }
    let compiled = Arc::new(sqlty_sql::regex(&format!(r"\b{}\b", regex::escape(name))));
    cache.insert(name.to_string(), Arc::clone(&compiled));
    compiled
}

static NO_PARAMS: LazyLock<Regex> = LazyLock::new(|| sqlty_sql::regex(r"\bNoParams\b"));

/// Remove sqlty and `TYPE_MODULES` imports that are no longer referenced.
fn remove_unused_imports(source: &str, istyle: &ImportStyle) -> String {
    let wrapper_marker = format!("{}[", istyle.wrapper_name);
    let template_marker = format!("{}[", istyle.template_name());
    let module_style = istyle.wrapper_name.contains('.');
    let template_import = istyle.template_import();
    let untyped_import = istyle.untyped_import();
    let untyped_used = source
        .lines()
        .any(|line| line.contains("UNTYPED") && Some(line.trim()) != untyped_import);
    let no_params_import = istyle.no_params_import();
    let no_params_used = source.contains("NoParams")
        && source
            .lines()
            .any(|line| NO_PARAMS.is_match(line) && Some(line.trim()) != no_params_import);
    let unused = |line: &str| {
        let line = line.trim_end();
        (Some(line) == untyped_import && !untyped_used)
            || (Some(line) == no_params_import && !no_params_used)
            || (Some(line) == template_import && !source.contains(&template_marker))
            || (line == istyle.import_line
                && !source.contains(&wrapper_marker)
                && !(module_style && source.contains(&template_marker)))
            || TYPE_MODULES.iter().any(|module| {
                line == format!("import {module}") && !source.contains(&format!("{module}."))
            })
            || unused_datetime_import(source, line)
    };
    let lines: Vec<(&str, bool)> = lines_in_strings(source).collect();
    let mut kept = String::with_capacity(source.len());
    let mut i = 0;
    while i < lines.len() {
        let (line, quoted) = lines[i];
        if quoted || !unused(line) {
            kept.push_str(line);
            i += 1;
            continue;
        }
        let end = (i..lines.len())
            .find(|&j| lines[j].1 || !unused(lines[j].0))
            .unwrap_or(lines.len());
        // A block `insert_imports` set between blank lines leaves one of them.
        let blank = |at: usize| {
            lines
                .get(at)
                .is_some_and(|(line, quoted)| !quoted && line.trim().is_empty())
        };
        let after_blank = i > 0 && blank(i - 1);
        i = if after_blank && blank(end) {
            end + 1
        } else {
            end
        };
    }
    kept
}

/// Whether `line` is a `from datetime import X` that [`Spelling`] adds, and nothing else
/// in `source` names what it binds.
fn unused_datetime_import(source: &str, line: &str) -> bool {
    let Some(rest) = line.strip_prefix("from datetime import ") else {
        return false;
    };
    let (class, binding) = rest.split_once(" as ").unwrap_or((rest, rest));
    if !DATETIME_CLASSES.contains(&class)
        || !(binding == class || binding == format!("datetime_{class}"))
    {
        return false;
    }
    word_regex(binding).find_iter(source).count() == 1
}

/// Each line of `source` with its line ending, and whether it starts inside a string
/// literal (the lines of a docstring after its first): text, not code.
fn lines_in_strings(source: &str) -> impl Iterator<Item = (&str, bool)> {
    let strings: Vec<Range<usize>> = tokenize(source)
        .into_iter()
        .filter(|token| token.kind == Kind::Str)
        .map(|token| token.span)
        .collect();
    let mut at = 0;
    source.split_inclusive('\n').map(move |line| {
        let start = at;
        at += line.len();
        let before = strings.partition_point(|span| span.start < start);
        (line, before > 0 && strings[before - 1].end > start)
    })
}

/// The names sqlty's `SQL` is bound to: `SQL` and its aliases, `sqlty.SQL` and the like.
pub(crate) fn sql_names(imports: &[Import]) -> Vec<String> {
    imports
        .iter()
        .filter_map(|(from, name, binding)| match from.as_deref() {
            Some("sqlty") if name == "SQL" => Some(binding.clone()),
            Some("sqlty") if name == "*" => Some("SQL".to_string()),
            None if name == "sqlty" => Some(format!("{binding}.SQL")),
            _ => None,
        })
        .collect()
}

/// `(name, binding)` pairs of top-level `from sqlty import ...` statements, read from
/// tokens so strings and comments don't count; `*` imports every name.
pub(crate) fn sqlty_imports(scan: &Scan) -> Vec<(String, String)> {
    from_imports(scan, "sqlty")
}

/// `(name, binding)` pairs of the top-level `from module import ...` statements.
fn from_imports(scan: &Scan, module: &str) -> Vec<(String, String)> {
    scan.imports()
        .iter()
        .filter(|(from, _, _)| from.as_deref() == Some(module))
        .map(|(_, name, binding)| (name.clone(), binding.clone()))
        .collect()
}

/// `(binding, qualified name)` of the top-level imports that bind a name other than a
/// module's own: `from decimal import Decimal` gives `("Decimal", "decimal.Decimal")`.
pub(crate) fn imported_names(scan: &Scan) -> Vec<(String, String)> {
    scan.imports()
        .iter()
        .filter_map(|(from, name, binding)| match from {
            Some(module) if name != "*" => Some((binding.clone(), format!("{module}.{name}"))),
            None if name != binding => Some((binding.clone(), name.clone())),
            _ => None,
        })
        .collect()
}

/// `(from, name, binding)` of a top-level import, `from` being the module of a
/// `from ... import`: `import os.path` binds `os`, `from a import b as c` binds `c`.
pub(crate) type Import = (Option<String>, String, String);

/// Every top-level [`Import`] of `source`.
pub(crate) fn imports_of(source: &str, tokens: &[Token]) -> Vec<Import> {
    let text = |i: usize| {
        tokens
            .get(i)
            .map_or("", |t: &Token| &source[t.span.clone()])
    };
    let mut found = Vec::new();
    let mut logical = 0;
    for i in 0..tokens.len() {
        if i > 0 && tokens[i - 1].kind == Kind::Newline {
            logical = i;
        }
        let (from, names_at) = match text(i) {
            "from" if text(i + 2) == "import" => (Some(text(i + 1).to_string()), i + 3),
            "import" => (None, i + 1),
            _ => continue,
        };
        // The logical line up to the token: a compound header may span physical lines.
        let first = (logical..i)
            .find(|&j| tokens[j].kind != Kind::Comment)
            .unwrap_or(i);
        let line_start = source[..tokens[first].span.start]
            .rfind('\n')
            .map_or(0, |n| n + 1);
        let line = &source[line_start..tokens[i].span.start];
        let statement = match text(i.wrapping_sub(1)) {
            "" | "\n" | "\r\n" => true,
            ";" => !is_compound(line),
            _ => false,
        };
        if !statement || line.starts_with([' ', '\t']) {
            continue;
        }
        let mut pairs = Vec::new();
        let mut names = Vec::new();
        for token in &tokens[names_at..] {
            match (token.kind, &source[token.span.clone()]) {
                (Kind::Name, name) => names.push(name),
                (Kind::Op, "*") => names.push("*"),
                (Kind::Op, "," | "(" | ")") | (Kind::Comment, _) => flush(&mut names, &mut pairs),
                _ => break,
            }
        }
        flush(&mut names, &mut pairs);
        for (name, binding) in pairs {
            let binding = match (&from, name == binding) {
                // `import os.path` binds `os`.
                (None, true) => binding.split('.').next().unwrap_or_default().to_string(),
                _ => binding,
            };
            found.push((from.clone(), name, binding));
        }
    }
    found
}

/// How row types spell the `datetime` classes in a module that binds the name `datetime`
/// to something else, as `from datetime import datetime` does: by the names it imports
/// them under, importing the missing ones (under an alias when their name is taken).
#[derive(Default)]
pub(crate) struct Spelling {
    /// `(type, spelling)`, such as `("datetime.date", "date")`.
    names: Vec<(String, String)>,
    /// `(spelling, import line)` for the classes the module does not import yet.
    imports: Vec<(String, String)>,
}

/// The `datetime` classes row types name.
const DATETIME_CLASSES: [&str; 5] = ["datetime", "date", "time", "timedelta", "timezone"];

impl Spelling {
    pub(crate) fn of(scan: &Scan) -> Self {
        if !scan.source().contains("datetime") {
            return Self::default();
        }
        let imports = scan.imports();
        if !imports
            .iter()
            .any(|(from, _, binding)| from.is_some() && binding == "datetime")
        {
            return Self::default();
        }
        let bound = |name: &str| imports.iter().any(|(_, _, binding)| binding == name);
        let mut spelling = Self::default();
        for class in DATETIME_CLASSES {
            let imported = imports
                .iter()
                .find(|(from, name, _)| from.as_deref() == Some("datetime") && name == class);
            let spelled = match imported {
                Some((_, _, binding)) => binding.clone(),
                None => {
                    let spelled = if bound(class) {
                        format!("datetime_{class}")
                    } else {
                        class.to_string()
                    };
                    let line = if spelled == class {
                        format!("from datetime import {class}")
                    } else {
                        format!("from datetime import {class} as {spelled}")
                    };
                    spelling.imports.push((spelled.clone(), line));
                    spelled
                }
            };
            spelling.names.push((format!("datetime.{class}"), spelled));
        }
        spelling
    }

    /// `row_type` spelled as the module can evaluate it.
    pub(crate) fn respell(&self, row_type: String) -> String {
        if self.names.is_empty() || !row_type.contains("datetime.") {
            return row_type;
        }
        DATETIME_TYPE
            .replace_all(&row_type, |caps: &regex::Captures| {
                let canonical = &caps[0];
                self.names
                    .iter()
                    .find(|(name, _)| name == canonical)
                    .map_or_else(|| canonical.to_string(), |(_, spelled)| spelled.clone())
            })
            .into_owned()
    }

    /// The import lines the respelled `wrapped` calls need.
    pub(crate) fn imports_for(&self, wrapped: &[&str]) -> Vec<String> {
        self.imports
            .iter()
            .filter(|(spelled, _)| {
                let word = word_regex(spelled);
                wrapped.iter().any(|text| {
                    text.split_once('(')
                        .is_some_and(|(head, _)| word.is_match(head))
                })
            })
            .map(|(_, line)| line.clone())
            .collect()
    }
}

static DATETIME_TYPE: LazyLock<Regex> =
    LazyLock::new(|| sqlty_sql::regex(r"\bdatetime\.(?:datetime|date|timedelta|timezone|time)\b"));

fn flush(names: &mut Vec<&str>, pairs: &mut Vec<(String, String)>) {
    match names[..] {
        [name] => pairs.push((name.to_string(), name.to_string())),
        [name, "as", alias] => pairs.push((name.to_string(), alias.to_string())),
        _ => {}
    }
    names.clear();
}

/// Check for a top-level import; indented lines (blocks) and lines in strings don't count.
pub(crate) fn has_import(source: &str, import_str: &str) -> bool {
    lines_in_strings(source).any(|(line, quoted)| !quoted && line.trim_end() == import_str)
}

/// Offset where imports go: after shebang, coding cookie, docstring and `__future__`.
pub(crate) fn header_end(source: &str) -> usize {
    header(source).0
}

/// [`header_end`], and where the code of the header's last statement ends, if it has one.
fn header(source: &str) -> (usize, Option<usize>) {
    let line_end_at = |from: usize| {
        source[from..]
            .find('\n')
            .map_or(source.len(), |i| from + i + 1)
    };
    let (mut pos, mut end, mut lineno) = (0, 0, 0);
    let mut stmt = None;
    let mut docstring = true;
    while pos < source.len() {
        let mut line_end = line_end_at(pos);
        let line = source[pos..line_end].trim();
        lineno += 1;
        if line.is_empty() || line.starts_with('#') {
            if (pos == 0 && line.starts_with("#!")) || (lineno <= 2 && CODING.is_match(line)) {
                end = line_end;
            }
            pos = line_end;
            continue;
        }
        if docstring {
            if let Some((close, close_line_end)) = docstring_end(source, pos) {
                docstring = false;
                stmt = Some(close);
                pos = close_line_end;
                end = close_line_end;
                continue;
            }
        }
        if !line.starts_with("from __future__ import") {
            break;
        }
        docstring = false;
        if line.contains('(') && !line.contains(')') {
            line_end = source[pos..]
                .find(')')
                .map_or(source.len(), |i| line_end_at(pos + i));
        }
        let last = source[..line_end].trim_end_matches(['\n', '\r']);
        let last_start = last.rfind('\n').map_or(0, |i| i + 1);
        let code = last[last_start..].split('#').next().unwrap_or_default();
        stmt = Some(last_start + code.trim_end().len());
        pos = line_end;
        end = line_end;
    }
    (end, stmt)
}

/// Add `imports` (`; `-joined) to `source` without adding a line, so line numbers stay the same.
///
/// CPython decodes a script from the end of its coding line and drops the first decoded line,
/// so imports never go on a line of their own above the first statement. They are appended to
/// the docstring or last `__future__` import, else put in front of the first statement, else
/// (a compound statement) on a blank or comment line above it after line 1. Only a compound
/// statement right after the shebang and coding lines gets a new line.
fn insert_inline(source: &str, imports: &str) -> String {
    let (mut pos, stmt) = header(source);
    if let Some(end) = stmt {
        return format!("{}; {}{}", &source[..end], imports, &source[end..]);
    }
    let first_line_end = source.find('\n').map_or(source.len(), |i| i + 1);
    let mut spare = None;
    while pos < source.len() {
        let line_end = source[pos..]
            .find('\n')
            .map_or(source.len(), |i| pos + i + 1);
        let line = source[pos..line_end].trim();
        if !line.is_empty() && !line.starts_with('#') {
            break;
        }
        if pos >= first_line_end && spare.is_none() {
            spare = Some((pos, if line.is_empty() { "" } else { "  " }));
        }
        pos = line_end;
    }
    let line = source[pos..].lines().next().unwrap_or_default();
    if let Some(close) =
        string_end(source, pos).filter(|&c| source[c..].trim_start().starts_with(';'))
    {
        return format!("{}; {}{}", &source[..close], imports, &source[close..]);
    }
    let (at, text) = match spare {
        _ if !is_compound(line) => (pos, format!("{imports}; ")),
        Some((at, sep)) => (at, format!("{imports}{sep}")),
        None => (pos, format!("{imports}{}", line_ending(source, pos))),
    };
    format!("{}{}{}", &source[..at], text, &source[at..])
}

/// Whether a statement starting with `line` can't follow `; `.
fn is_compound(line: &str) -> bool {
    let word = line
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .next()
        .unwrap_or_default();
    line.starts_with('@')
        || matches!(
            word,
            "def"
                | "class"
                | "if"
                | "elif"
                | "else"
                | "for"
                | "while"
                | "with"
                | "try"
                | "except"
                | "finally"
                | "async"
        )
        || (word == "match"
            && line
                .split('#')
                .next()
                .unwrap_or_default()
                .trim_end()
                .ends_with(':'))
}

/// End of a string literal starting at `pos` (a docstring: no prefix but `r` or `u`).
fn string_end(source: &str, pos: usize) -> Option<usize> {
    let prefix = source[pos..].trim_start_matches(|c: char| c.is_ascii_alphabetic());
    let prefix = &source[pos..source.len() - prefix.len()];
    string_at(source, pos).filter(|_| matches!(prefix, "" | "r" | "R" | "u" | "U"))
}

/// End of a string-literal statement starting at `pos`, and of the line closing it.
fn docstring_end(source: &str, pos: usize) -> Option<(usize, usize)> {
    let close = string_end(source, pos)?;
    let rest_end = source[close..]
        .find('\n')
        .map_or(source.len(), |n| close + n);
    let rest = source[close..rest_end].trim();
    if !rest.is_empty() && !rest.starts_with('#') {
        return None;
    }
    Some((close, (rest_end + 1).min(source.len())))
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn a_word_regex_compiles_once() {
        assert!(Arc::ptr_eq(
            &word_regex("datetime"),
            &word_regex("datetime")
        ));
    }
}
