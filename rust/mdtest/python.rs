//! The Python assertions of the mdtest suite (`# error:`, `# rewritten:`, `# fixed:`, ...) and
//! the SQL ones of `driver` sections, through sqlty-python's public API. sqlty-format runs the
//! ones that need sqruff (`# formatted:`, `sqruff` diagnostics); pytest runs `# decoded:`,
//! which needs the Python codec.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::parser::{Section, Unit};
use super::{harness, sql};
use ruff_python_ast::token::TokenKind;
use ruff_python_ast::visitor::{walk_expr, Visitor};
use ruff_python_ast::Expr;
use ruff_text_size::Ranged;
use sqlty_python::checker::Diagnostic;
use sqlty_python::index::{defines_helpers, helpers_of, Builder, HelperIndex, Module, Roots};
use sqlty_python::options::Options;
use sqlty_python::rewriter::ImportStyle;
use sqlty_python::rules::RuleConfig;
use sqlty_python::suppress;
use sqlty_python::types::symbols::Source;
use sqlty_python::{
    breaking, check, config, fixer, json_types, locate, params, queries, rewriter, rows,
};
use sqlty_sql::analyzer::{self, CandidateKind, Schema};
use sqlty_sql::highlight::Highlight;

const VERSION: (u8, u8) = (3, 14);
const PYTHON: [&str; 9] = [
    "error",
    "rewritten",
    "rewritten-file",
    "fixed",
    "stripped",
    "completions",
    "hover",
    "signature",
    "highlighted",
];
/// `# completions:` lists the first ones, then `...`: the system catalogs make lists long.
const COMPLETIONS_SHOWN: usize = 10;
const MAX_QUERY_VARIANTS: i64 = 64;

/// sqruff's findings in a module, as `(message, span, query start)`.
pub type Lint = fn(&str, &str, &ImportStyle) -> Vec<check::Lint>;
/// sqruff's layout of a module's queries.
pub type Format = fn(&str, &str, &ImportStyle) -> String;

/// The Python assertions; with `sqruff`, only those that need its linter or formatter.
pub struct Python {
    pub sqruff: Option<(Lint, Format)>,
}

impl harness::Runner for Python {
    fn owns(&self, section: &Section, unit: &Unit, kind: &str) -> bool {
        if self.sqruff.is_some() {
            return unit.lang == "python"
                && (kind == "formatted" || kind == "error" && lints(section));
        }
        if unit.lang == "sql" && kind != "error" {
            return sql::KINDS.contains(&kind) && section.setting("driver").is_some();
        }
        PYTHON.contains(&kind) && !(kind == "error" && lints(section))
    }

    fn compute(&self, section: &Section, unit: &Unit, kind: &str) -> Result<String, String> {
        let run = Run::new(section, self.sqruff)?;
        if unit.lang == "sql" {
            return sql::compute(section, unit, kind, run.schema.as_ref());
        }
        run.compute(unit, kind)
    }

    fn diagnostics(&self, section: &Section, unit: &Unit) -> Result<Vec<harness::Found>, String> {
        let run = Run::new(section, self.sqruff)?;
        let text = unit.text();
        let own = run.schema.as_ref();
        let found = match (&run.against, unit.lang) {
            _ if unit.migration => {
                let baseline = section.files.get("against.sql").map_or("", String::as_str);
                let style = ImportStyle::default();
                let writers: Vec<(String, String)> = section
                    .files
                    .iter()
                    .filter(|(path, _)| path.ends_with(".py"))
                    .flat_map(|(path, text)| {
                        sqlty_python::migrations::queries_of(path, text, &style, None)
                    })
                    .collect();
                let files = [(text.as_str(), &run.rules)];
                sqlty_python::migrations::check_migrations_with(baseline, &files, &writers)
                    .into_iter()
                    .flatten()
                    .collect()
            }
            _ if unit.schema => {
                let types = sql::types(section, Vec::new());
                let overrides = analyzer::TypeOverrides::new(&types, run.dialect)?;
                let (schema, _) =
                    analyzer::load_schema_with_overrides(&text, run.dialect, overrides);
                let files = [(text.as_str(), &run.rules)];
                sqlty_python::migrations::check_schema(&schema, &files)
                    .into_iter()
                    .flatten()
                    .collect()
            }
            (None, "sql") => run.queries(&text, own)?,
            (None, _) => run.check(&text, own)?,
            // `sqlty breaking`: what the other schema adds.
            (Some(against), "sql") => {
                let own = run.queries(&text, own)?;
                let found = run.queries(&text, Some(against))?;
                breaking::breaking(&own, found, key, |_| "")
                    .into_iter()
                    .filter(|d| d.rule.name() != "unused-ignore")
                    .collect()
            }
            (Some(against), _) => {
                let own = run.check(&text, own)?;
                let found = run.check(&text, Some(against))?;
                breaking::breaking(&own, found, key, |d| d.rule.name())
            }
        };
        let md_lines = unit.md_lines();
        let column = |offset: usize, line: usize| {
            let (at, col) = position(&text, offset);
            if at == line {
                col.to_string()
            } else {
                format!("{:+}.{col}", at as isize - line as isize)
            }
        };
        Ok(found
            .into_iter()
            .map(|d| {
                (
                    md_lines[d.line - 1],
                    d.rule.name().to_string(),
                    d.message,
                    format!(
                        "[{}:{}]",
                        column(d.span.start, d.line),
                        column(d.span.end, d.line)
                    ),
                )
            })
            .collect())
    }
}

fn key(found: &Diagnostic) -> (usize, &'static str, String) {
    (found.line, found.rule.name(), found.message.clone())
}

/// Whether the section turns `sqruff` on: its diagnostics need sqlty-format's linter.
fn lints(section: &Section) -> bool {
    section
        .setting("rules")
        .and_then(|rules| rules.get("sqruff"))
        .and_then(toml::Value::as_str)
        .is_some_and(|severity| severity != "ignore")
}

/// The settings of a section, read as `tests/_mdtest/runner.py` reads them.
struct Run<'s> {
    section: &'s Section,
    dialect: &'static str,
    schema: Option<Schema>,
    import_style: Option<&'s str>,
    untyped: bool,
    typed_json: bool,
    param_types: bool,
    closed_params: bool,
    rules: RuleConfig,
    variants: Option<usize>,
    coerce: HashSet<String>,
    helpers: Option<Arc<HelperIndex>>,
    /// The schema of `against.sql`, which `# error:` compares with as `sqlty breaking` does.
    against: Option<Schema>,
    sqruff: Option<(Lint, Format)>,
}

impl<'s> Run<'s> {
    fn new(section: &'s Section, sqruff: Option<(Lint, Format)>) -> Result<Self, String> {
        let dialect = sql::dialect(section.str_setting("dialect").unwrap_or("postgres"));
        let defaults = section
            .str_setting("driver")
            .map(|driver| config::driver_types(driver, dialect))
            .unwrap_or_default();
        let schema = sql::schema(section, &sql::types(section, defaults))?;
        let flag = |key: &str, default: bool| {
            section
                .setting(key)
                .and_then(toml::Value::as_bool)
                .unwrap_or(default)
        };
        let rules = section.setting("rules").map(|rules| {
            config::rule_entries(Some(rules))
                .into_iter()
                .collect::<HashMap<_, _>>()
        });
        let requires = section
            .config
            .get("project")
            .and_then(|project| project.get("requires-python"))
            .and_then(toml::Value::as_str);
        Ok(Run {
            section,
            dialect,
            schema,
            import_style: section.str_setting("import-style"),
            untyped: section.str_setting("untyped") == Some("sentinel"),
            typed_json: section.str_setting("json-types") == Some("typing.TypedDict"),
            param_types: flag("param-types", true),
            closed_params: requires.is_some_and(requires_closed),
            rules: RuleConfig::from_overrides(rules).map_err(|e| e.to_string())?,
            variants: flag("preview", true).then(|| {
                let max = section.setting("max-query-variants");
                let max = max.and_then(toml::Value::as_integer);
                usize::try_from(max.unwrap_or(MAX_QUERY_VARIANTS)).unwrap()
            }),
            coerce: section
                .setting("coerce-row-types")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|name| name.as_str().map(str::to_string))
                .collect(),
            helpers: helpers(section),
            against: section
                .files
                .get("against.sql")
                .map(|sql| analyzer::load_schema_with_skipped(sql, dialect).0),
            sqruff,
        })
    }

    fn options(&self, untyped: bool) -> Result<Options<'_>, String> {
        Options::new(
            self.dialect,
            self.schema.as_deref(),
            self.import_style,
            untyped,
        )
        .map_err(|e| e.to_string())
    }

    fn style(&self) -> Result<ImportStyle, String> {
        ImportStyle::parse(self.import_style).map_err(|e| e.to_string())
    }

    fn compute(&self, unit: &Unit, kind: &str) -> Result<String, String> {
        let text = unit.text();
        let text = text.as_str();
        Ok(match kind {
            "rewritten" => {
                let rewritten = parses(text, self.rewrite(text)?, true)?;
                without_imports(&rewritten, &self.imports())
                    .trim()
                    .to_string()
            }
            "rewritten-file" => parses(text, self.rewrite(text)?, true)?.trim().to_string(),
            "fixed" => {
                let fixed = parses(text, self.fix(text)?, false)?;
                let again = self.fix(&fixed)?;
                if again != fixed {
                    return Err(format!(
                        "check --fix is not idempotent; a second run gives:\n{again}"
                    ));
                }
                fixed.trim().to_string()
            }
            "formatted" => {
                let (_, format) = self.sqruff.ok_or("`# formatted:` runs in sqlty-format")?;
                let style = self.style()?;
                let formatted = parses(text, format(text, self.dialect, &style), false)?;
                let again = format(&formatted, self.dialect, &style);
                if again != formatted {
                    return Err(format!(
                        "format is not idempotent; a second run gives:\n{again}"
                    ));
                }
                formatted.trim().to_string()
            }
            "stripped" => {
                let stripped = rewriter::strip_source(text, &self.style()?);
                parses(text, stripped, true)?.trim().to_string()
            }
            "completions" => {
                let offset = self.cursor(unit, kind)?;
                let queries = self.queries_of(text)?;
                let found =
                    locate::complete(&queries, offset, self.dialect, self.schema.as_deref(), 200);
                match found {
                    Some(found) if !found.candidates.is_empty() => {
                        let more = found.incomplete || found.candidates.len() > COMPLETIONS_SHOWN;
                        let mut lines: Vec<String> = found
                            .candidates
                            .into_iter()
                            .take(COMPLETIONS_SHOWN)
                            .map(|c| {
                                let detail = c.detail.filter(|d| !d.is_empty());
                                let detail = detail.as_deref().unwrap_or("-");
                                format!("{} {} {detail}", c.label, candidate_kind(c.kind))
                            })
                            .collect();
                        if more {
                            lines.push("...".to_string());
                        }
                        lines.join("\n")
                    }
                    _ => "None".to_string(),
                }
            }
            "hover" => {
                let offset = self.cursor(unit, kind)?;
                let queries = self.queries_of(text)?;
                locate::hover(&queries, offset, self.dialect, self.schema.as_deref())
                    .map_or_else(|| "None".to_string(), |found| found.contents)
            }
            "signature" => {
                let offset = self.cursor(unit, kind)?;
                let queries = self.queries_of(text)?;
                locate::signature_help(&queries, offset, self.dialect, self.schema.as_deref())
                    .map_or_else(|| "None".to_string(), |help| signature_lines(&help))
            }
            "highlighted" => {
                let queries = self.queries_of(text)?;
                let lines: Vec<String> = queries
                    .highlights(self.dialect)
                    .into_iter()
                    .flat_map(|(range, kind)| {
                        text[range]
                            .lines()
                            .filter(|line| !line.trim().is_empty())
                            .map(|line| format!("{} {line}", highlight_kind(kind)))
                            .collect::<Vec<_>>()
                    })
                    .collect();
                if lines.is_empty() {
                    "None".to_string()
                } else {
                    lines.join("\n")
                }
            }
            _ => return Err(format!("not a Python assertion: {kind}")),
        })
    }

    /// `core.rewrite_source`: the wrappers, then the JSON and parameter types.
    fn rewrite(&self, text: &str) -> Result<String, String> {
        let rewritten = rewriter::rewrite_source_with(text, &self.options(self.untyped)?, false);
        self.typed(text, rewritten)
    }

    /// `core.fix_source`: the fixes, then the JSON and parameter types.
    fn fix(&self, text: &str) -> Result<String, String> {
        let parsed = Source::new(text, VERSION);
        let fixed = fixer::fix_module(
            text,
            parsed.as_ref(),
            &self.options(self.untyped)?,
            &self.rules,
            self.helpers.as_deref(),
            None,
        );
        self.typed(text, fixed)
    }

    fn typed(&self, text: &str, mut written: String) -> Result<String, String> {
        let options = self.options(false)?;
        if self.typed_json && written != text {
            written = json_types::add_json_types(&written, &options, VERSION);
        }
        if self.param_types {
            written = params::add_param_types(&written, &options, self.closed_params);
        }
        Ok(written)
    }

    /// `core.check_source`: the module's diagnostics, its `# sqlty: ignore` comments applied,
    /// without the `redundant-optional` another of its queries refutes.
    fn check(&self, text: &str, schema: Option<&Schema>) -> Result<Vec<Diagnostic>, String> {
        let options = Options::new(
            self.dialect,
            schema.map(|s| &**s),
            self.import_style,
            self.untyped,
        )
        .map_err(|e| e.to_string())?;
        let types = self.helpers.as_ref().map(|index| index.types());
        let settings = rows::Settings {
            options: &options,
            rules: &self.rules,
            index: types.as_ref(),
            module: None,
            coerce: &self.coerce,
        };
        let has_schema =
            schema.is_some_and(|schema| !schema.databases().is_some_and(|all| all.absent));
        let run = check::Run {
            settings: &settings,
            has_schema,
            lint: &|text: &str| match (self.sqruff, self.style()) {
                (Some((lint, _)), Ok(style)) => lint(text, self.dialect, &style),
                _ => Vec::new(),
            },
            helpers: self.helpers.as_deref(),
            variants: self.variants,
        };
        let parsed = Source::new(text, VERSION).map(Arc::new);
        let found = check::module(text, parsed.as_ref(), &run);
        let ignores = match &parsed {
            Some(parsed) => suppress::ignores(parsed),
            None => suppress::unparsed_ignores(text),
        };
        let needed = found.needed;
        Ok(suppress::apply(&ignores, found.diagnostics, &self.rules)
            .into_iter()
            .filter(|reported| {
                !reported
                    .redundant
                    .as_ref()
                    .is_some_and(|key| needed.contains(key))
            })
            .map(|reported| reported.diagnostic)
            .collect())
    }

    /// `check` of a `.sql` file of named queries.
    fn queries(&self, text: &str, schema: Option<&Schema>) -> Result<Vec<Diagnostic>, String> {
        let format = self.section.str_setting("query-format").unwrap_or("auto");
        let format = format
            .parse::<queries::Format>()
            .map_err(|()| format!("unknown query-format {format}"))?;
        let options = Options::new(self.dialect, schema.map(|s| &**s), None, false)
            .map_err(|e| e.to_string())?;
        Ok(queries::check_queries(
            text,
            format,
            "queries",
            &options,
            &self.rules,
        ))
    }

    /// The queries `check` reads, with those helper calls pass.
    fn queries_of<'t>(&self, text: &'t str) -> Result<locate::Queries<'t>, String> {
        let starts: Vec<usize> = Source::new(text, VERSION)
            .map(|parsed| {
                check::sites(&parsed, self.helpers.as_deref(), None, None)
                    .into_iter()
                    .filter_map(|site| site.start)
                    .collect()
            })
            .unwrap_or_default();
        Ok(locate::Queries::new(text, &self.style()?, &starts))
    }

    fn cursor(&self, unit: &Unit, kind: &str) -> Result<usize, String> {
        let (line, column) = unit
            .cursor()
            .ok_or_else(|| format!("`# {kind}:` needs a <CURSOR> in the code"))?;
        let text = unit.text();
        let start: usize = text
            .split_inclusive('\n')
            .take(line - 1)
            .map(str::len)
            .sum();
        Ok(start + column - 1)
    }

    /// The imports rewrite adds, which `# rewritten:` leaves out.
    fn imports(&self) -> HashSet<String> {
        let style = self
            .import_style
            .unwrap_or("from sqlty import SQL")
            .trim()
            .to_string();
        let mut imports: HashSet<String> = ["import datetime", "import typing", "import uuid"]
            .into_iter()
            .map(str::to_string)
            .collect();
        if style.starts_with("from ") {
            imports.insert("from sqlty import UNTYPED".to_string());
            imports.insert("from sqlty import SQLTemplate".to_string());
        }
        imports.insert(style);
        imports
    }
}

/// The labelled `.py` files as modules (`pkg/db.py` is `pkg.db`), plus `functions`.
fn helpers(section: &Section) -> Option<Arc<HelperIndex>> {
    let modules: Vec<(String, &String)> = section
        .files
        .iter()
        .filter_map(|(path, text)| {
            let module = path.strip_suffix(".py")?.replace('/', ".");
            Some((module, text))
        })
        .collect();
    let function = |f: &toml::Value, database: Option<&str>| {
        let key = |key: &str| f.get(key).and_then(toml::Value::as_str).map(str::to_string);
        (
            key("import-path").unwrap(),
            key("query-param").unwrap(),
            key("return-type-param"),
            database.map(str::to_string),
        )
    };
    let array = |value: Option<&toml::Value>| {
        value
            .and_then(toml::Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let mut functions: Vec<_> = array(section.setting("functions"))
        .iter()
        .map(|f| function(f, None))
        .collect();
    let databases = section.setting("databases").and_then(toml::Value::as_table);
    for (name, database) in databases.into_iter().flatten() {
        for f in array(database.get("functions")) {
            functions.push(function(&f, Some(name)));
        }
    }
    if modules.is_empty() && functions.is_empty() {
        return None;
    }
    let mut index = Builder::new(VERSION);
    for (module, text) in modules {
        let source = Module::new(Arc::from(text.as_str()), &module, VERSION);
        let found = defines_helpers(text)
            .then(|| source.source().map(|parsed| helpers_of(parsed)))
            .flatten()
            .unwrap_or_default();
        let name = module.strip_suffix(".__init__").unwrap_or(&module);
        index.add_found(name, Arc::new(source), &found);
    }
    for (path, query, row, database) in functions {
        let roots = Roots {
            paths: &[],
            read: &|_| None,
        };
        index.register(&path, &query, row.as_deref(), roots, database.as_deref());
    }
    Some(index.build())
}

/// Whether `requires-python` rules out every Python before 3.15, which reads
/// `TypedDict(closed=True)`.
fn requires_closed(requires: &str) -> bool {
    let bound = regex::Regex::new(r"(?:>=|~=|==)\s*(\d+)\.(\d+)").unwrap();
    let closed = bound.captures_iter(requires).any(|found| {
        let part = |i: usize| found[i].parse::<u32>().unwrap();
        (part(1), part(2)) >= (3, 15)
    });
    closed
}

/// The 1-based line and column (in characters) of byte `offset`.
fn position(text: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(text.len());
    let before = &text[..offset];
    let start = before.rfind('\n').map_or(0, |at| at + 1);
    (
        before.matches('\n').count() + 1,
        before[start..].chars().count() + 1,
    )
}

fn candidate_kind(kind: CandidateKind) -> &'static str {
    match kind {
        CandidateKind::Table => "table",
        CandidateKind::View => "view",
        CandidateKind::Cte => "cte",
        CandidateKind::Column => "column",
        CandidateKind::Join => "join",
    }
}

fn highlight_kind(kind: Highlight) -> &'static str {
    match kind {
        Highlight::Keyword => "keyword",
        Highlight::String => "string",
        Highlight::Number => "number",
        Highlight::Comment => "comment",
        Highlight::Operator => "operator",
        Highlight::Parameter => "parameter",
    }
}

/// `output`, which must be valid Python with the same comments when `source` is; with
/// `same_strings`, the same strings too: rewrite and strip only add or remove wrappers.
fn parses(source: &str, output: String, same_strings: bool) -> Result<String, String> {
    if ruff_python_parser::parse_module(source).is_err() {
        return Ok(output);
    }
    if let Err(error) = ruff_python_parser::parse_module(&output) {
        return Err(format!("output is not valid Python ({error}):\n{output}"));
    }
    let (written, original) = (without_generated(&output), without_generated(source));
    if comments(&written) != comments(&original) {
        return Err(format!(
            "output changes the comments of the source:\n{output}"
        ));
    }
    if same_strings && strings(&written) != strings(&original) {
        return Err(format!(
            "output changes the strings of the source:\n{output}"
        ));
    }
    Ok(output)
}

/// `source` without the classes `json-types = "typing.TypedDict"` generates, which are new
/// code with comments and strings of their own.
fn without_generated(source: &str) -> String {
    let generated = regex::Regex::new(
        r"# sqlty: generated\r?\n(?:class [^\r\n]*:\r?\n(?:    [^\r\n]*\r?\n)+|\w+ = typing\.TypedDict\([^\r\n]*\)\r?\n)",
    )
    .unwrap();
    generated.replace_all(source, "").into_owned()
}

fn comments(source: &str) -> Vec<String> {
    let Ok(parsed) = ruff_python_parser::parse_module(source) else {
        return Vec::new();
    };
    let mut found: Vec<String> = parsed
        .tokens()
        .iter()
        .filter(|token| token.kind() == TokenKind::Comment)
        .map(|token| source[token.range()].to_string())
        .collect();
    found.sort();
    found
}

/// Every string value, implicit concatenation applied, sorted; those in the subscript of a
/// called wrapper (`SQL[...](...)`) are the row type rewrite writes and strip removes.
fn strings(source: &str) -> Vec<String> {
    #[derive(Default)]
    struct Strings {
        found: Vec<String>,
    }
    impl<'a> Visitor<'a> for Strings {
        fn visit_expr(&mut self, expr: &'a Expr) {
            match expr {
                Expr::StringLiteral(literal) => self.found.push(literal.value.to_str().to_string()),
                Expr::Call(call) if call.func.is_subscript_expr() => {
                    if let Expr::Subscript(wrapper) = &*call.func {
                        self.visit_expr(&wrapper.value);
                    }
                    self.visit_arguments(&call.arguments);
                }
                _ => walk_expr(self, expr),
            }
        }
    }
    let Ok(parsed) = ruff_python_parser::parse_module(source) else {
        return Vec::new();
    };
    let mut strings = Strings::default();
    strings.visit_body(&parsed.syntax().body);
    strings.found.sort();
    strings.found
}

/// `source` without the import lines rewrite adds, also where `;` joins them.
fn without_imports(source: &str, imports: &HashSet<String>) -> String {
    source
        .lines()
        .filter_map(|line| {
            let mut line = line;
            while let Some((head, rest)) = line.split_once("; ") {
                if !imports.contains(head) {
                    break;
                }
                line = rest;
            }
            (!imports.contains(line.trim_end())).then_some(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `# signature:` lines: each overload, the active one after `> ` with its active parameter
/// in `**`.
fn signature_lines(help: &analyzer::SignatureHelp) -> String {
    let lines: Vec<String> = help
        .signatures
        .iter()
        .enumerate()
        .map(|(at, signature)| {
            if at != help.active_signature {
                return signature.label.to_string();
            }
            let label = match help.active_parameter.map(|p| signature.parameters[p].label) {
                Some((start, end)) => {
                    let label = signature.label;
                    format!(
                        "{}**{}**{}",
                        &label[..start],
                        &label[start..end],
                        &label[end..]
                    )
                }
                None => signature.label.to_string(),
            };
            format!("> {label}")
        })
        .collect();
    lines.join("\n")
}
