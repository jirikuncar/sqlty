//! Markdown to test sections, as `tests/_mdtest/parser.py` reads them.
//!
//! A heading opens a section; sections with assertions are tests. Setup (`toml` config
//! and labelled files) is inherited by nested sections, code never is. Each code block with
//! assertions is a unit: a `sql` block on its own, a `python` block together with the
//! assertion-free python blocks right before it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;

pub const CURSOR: &str = "<CURSOR>";

const SQL_KINDS: &str = "inferred|expanded|columns|json|params|rules|error";
const PY_KINDS: &str =
    "rewritten|rewritten-file|stripped|fixed|formatted|error|decoded|completions|hover|signature|highlighted";

static PY_ASSERTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^(\s*)#\s+({PY_KINDS})(\[[^\]]*\])?:(?: (.*))?$")).unwrap()
});
static SQL_ASSERTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"^(\s*)--\s+({SQL_KINDS}):(?: (.*))?$")).unwrap());
static TRAILING_ERROR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(.*?\S)\s+#\s+error(\[[^\]]*\])?:(?: (.*))?$").unwrap());
static FENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*)(`{3,}|~{3,})\s*([\w+-]*)\s*(.*)$").unwrap());
static HEADING: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(#{1,6})\s+(.+?)\s*#*\s*$").unwrap());
static LABEL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^`([^`]+)`:\s*$").unwrap());
static DIRECTIVE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^<!--\s*mdtest:([\w-]+)(?:\s+(.*?))?\s*-->$").unwrap());

#[derive(Clone, Debug)]
pub struct Assertion {
    pub kind: String,
    pub expected: String,
    /// The Markdown line of the first comment line.
    pub line: usize,
    /// Consecutive comment lines merged into `expected`.
    pub span: usize,
    pub comment: &'static str,
    pub indent: String,
    /// `error`: the Markdown line of the code it applies to.
    pub target: Option<usize>,
    pub trailing: bool,
    /// `error[9:19]`: the columns the diagnostic underlines.
    pub range: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Unit {
    pub lang: &'static str,
    /// `(Markdown line, text)`, assertion comments removed.
    pub lines: Vec<(usize, String)>,
    pub assertions: Vec<Assertion>,
    /// A ```` ```sql migration ```` block: a migration run on top of `against.sql`.
    pub migration: bool,
    /// A ```` ```sql schema ```` block: the schema itself, whose `-- error:` `sqlty check`
    /// reports on it.
    pub schema: bool,
}

impl Unit {
    /// The source, without the `<CURSOR>` marker.
    pub fn text(&self) -> String {
        let text: Vec<&str> = self.lines.iter().map(|(_, line)| line.as_str()).collect();
        text.join("\n").replacen(CURSOR, "", 1)
    }

    /// The Markdown line of each line of [`Unit::text`].
    pub fn md_lines(&self) -> Vec<usize> {
        self.lines.iter().map(|(line, _)| *line).collect()
    }

    /// The 1-based line and column of the `<CURSOR>` marker.
    pub fn cursor(&self) -> Option<(usize, usize)> {
        self.lines
            .iter()
            .enumerate()
            .find_map(|(i, (_, text))| text.find(CURSOR).map(|column| (i + 1, column + 1)))
    }
}

#[derive(Clone, Debug)]
pub struct Section {
    pub path: PathBuf,
    pub titles: Vec<String>,
    pub level: usize,
    pub line: usize,
    pub config: toml::Table,
    pub files: BTreeMap<String, String>,
    pub directives: BTreeMap<String, String>,
    pub units: Vec<Unit>,
    setup: Vec<(usize, String)>,
}

impl Section {
    pub fn name(&self) -> String {
        let titles = if self.titles.len() > 1 {
            &self.titles[1..]
        } else {
            &self.titles[..]
        };
        titles.join(" - ")
    }

    /// `[tool.sqlty]`.
    pub fn settings(&self) -> &toml::Table {
        static EMPTY: LazyLock<toml::Table> = LazyLock::new(toml::Table::new);
        self.config
            .get("tool")
            .and_then(|tool| tool.get("sqlty"))
            .and_then(toml::Value::as_table)
            .unwrap_or(&EMPTY)
    }

    pub fn setting(&self, key: &str) -> Option<&toml::Value> {
        self.settings().get(key)
    }

    pub fn str_setting(&self, key: &str) -> Option<&str> {
        self.setting(key).and_then(toml::Value::as_str)
    }

    /// The files `schema` names, joined in order; `None` when it names none.
    pub fn joined(&self, schema: Option<&toml::Value>) -> Option<String> {
        let names: Vec<&str> = match schema {
            Some(toml::Value::String(name)) => vec![name.as_str()],
            Some(toml::Value::Array(names)) => {
                names.iter().filter_map(toml::Value::as_str).collect()
            }
            _ => Vec::new(),
        };
        (!names.is_empty()).then(|| {
            names
                .iter()
                .map(|name| self.files[*name].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
    }

    /// The `schema` files, joined in order.
    pub fn schema_sql(&self) -> Option<String> {
        self.joined(self.setting("schema"))
    }

    fn child(&self, title: &str, level: usize, line: usize) -> Section {
        let mut titles = self.titles.clone();
        titles.push(title.to_string());
        Section {
            path: self.path.clone(),
            titles,
            level,
            line,
            config: self.config.clone(),
            files: self.files.clone(),
            directives: self.directives.clone(),
            units: Vec::new(),
            setup: Vec::new(),
        }
    }
}

/// The sections of `text` that carry assertions.
pub fn parse(path: &Path, text: &str) -> Vec<Section> {
    let lines: Vec<&str> = text.lines().collect();
    let root = Section {
        path: path.to_path_buf(),
        titles: Vec::new(),
        level: 0,
        line: 0,
        config: toml::Table::new(),
        files: BTreeMap::new(),
        directives: BTreeMap::new(),
        units: Vec::new(),
        setup: Vec::new(),
    };
    // Indices into `sections`; 0 is the root.
    let mut sections = vec![root];
    let mut stack = vec![0];
    let mut label: Option<String> = None;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if let Some(fence) = FENCE.captures(line) {
            let (indent, marker, lang, info) = (&fence[1], &fence[2], &fence[3], &fence[4]);
            let end = closing_fence(&lines, i + 1, indent, marker)
                .unwrap_or_else(|| panic!("{}:{}: unclosed code block", path.display(), i + 1));
            if indent.is_empty() && !info.split_whitespace().any(|word| word == "ignore") {
                let section = &mut sections[*stack.last().unwrap()];
                let tag = |tag: &str| info.split_whitespace().any(|word| word == tag);
                add_block(
                    section,
                    lang,
                    label.take(),
                    &lines[i + 1..end],
                    i + 2,
                    (tag("migration"), tag("schema")),
                );
            }
            label = None;
            i = end + 1;
            continue;
        }
        if let Some(heading) = HEADING.captures(line) {
            let level = heading[1].len();
            while sections[*stack.last().unwrap()].level >= level && stack.len() > 1 {
                stack.pop();
            }
            let child = sections[*stack.last().unwrap()].child(&heading[2], level, i + 1);
            sections.push(child);
            stack.push(sections.len() - 1);
            label = None;
        } else if let Some(directive) = DIRECTIVE.captures(line.trim()) {
            let value = directive.get(2).map_or("", |value| value.as_str());
            let section = &mut sections[*stack.last().unwrap()];
            section
                .directives
                .insert(directive[1].to_string(), value.to_string());
        } else if let Some(found) = LABEL.captures(line) {
            label = Some(found[1].to_string());
        } else if !line.trim().is_empty() {
            label = None;
        }
        i += 1;
    }
    sections.remove(0);
    sections.retain(|section| section.units.iter().any(|unit| !unit.assertions.is_empty()));
    sections
}

fn closing_fence(lines: &[&str], start: usize, indent: &str, marker: &str) -> Option<usize> {
    let fence = marker.chars().next().unwrap();
    (start..lines.len()).find(|&j| {
        let stripped = lines[j].trim();
        lines[j].starts_with(indent)
            && stripped.starts_with(marker)
            && stripped.trim_matches(fence).is_empty()
    })
}

fn add_block(
    section: &mut Section,
    lang: &str,
    label: Option<String>,
    body: &[&str],
    start: usize,
    tags: (bool, bool),
) {
    if let Some(label) = label {
        section.files.insert(label, body.join("\n"));
    } else if lang == "toml" {
        let config: toml::Table = body.join("\n").parse().unwrap();
        merge(&mut section.config, config);
    } else if lang == "python" || lang == "py" {
        add_python(section, body, start);
    } else if lang == "sql" {
        add_sql(section, body, start, tags);
    }
}

fn assertion(kind: &str, value: Option<&str>, line: usize, comment: &'static str) -> Assertion {
    Assertion {
        kind: kind.to_string(),
        expected: value.unwrap_or("").trim_end().to_string(),
        line,
        span: 1,
        comment,
        indent: String::new(),
        target: None,
        trailing: false,
        range: None,
    }
}

fn add_python(section: &mut Section, body: &[&str], start: usize) {
    let mut lines = Vec::new();
    let mut assertions: Vec<Assertion> = Vec::new();
    let mut pending: Vec<usize> = Vec::new();
    for (offset, text) in body.iter().enumerate() {
        let md = start + offset;
        if let Some(found) = PY_ASSERTION.captures(text) {
            let mut new = assertion(&found[2], found.get(4).map(|v| v.as_str()), md, "#");
            new.indent = found[1].to_string();
            new.range = found.get(3).map(|range| range.as_str().to_string());
            if new.kind == "error" {
                pending.push(assertions.len());
            }
            append(&mut assertions, new);
            continue;
        }
        let mut text = (*text).to_string();
        if let Some(found) = TRAILING_ERROR.captures(&text.clone()) {
            let mut new = assertion("error", found.get(3).map(|v| v.as_str()), md, "#");
            new.target = Some(md);
            new.trailing = true;
            new.range = found.get(2).map(|range| range.as_str().to_string());
            assertions.push(new);
            text = found[1].to_string();
        }
        if !text.trim().is_empty() {
            for index in pending.drain(..) {
                assertions[index].target = Some(md);
            }
        }
        lines.push((md, text));
    }
    if assertions.is_empty() {
        section.setup.extend(lines);
        return;
    }
    let mut all = std::mem::take(&mut section.setup);
    all.extend(lines);
    section.units.push(Unit {
        lang: "python",
        lines: all,
        assertions,
        migration: false,
        schema: false,
    });
}

fn add_sql(section: &mut Section, body: &[&str], start: usize, (migration, schema): (bool, bool)) {
    let mut lines = Vec::new();
    let mut assertions: Vec<Assertion> = Vec::new();
    let mut pending: Vec<usize> = Vec::new();
    for (offset, text) in body.iter().enumerate() {
        let md = start + offset;
        if let Some(found) = SQL_ASSERTION.captures(text) {
            let mut new = assertion(&found[2], found.get(3).map(|v| v.as_str()), md, "--");
            new.indent = found[1].to_string();
            if new.kind == "error" {
                pending.push(assertions.len());
            }
            append(&mut assertions, new);
        } else {
            if !text.trim().is_empty() {
                for index in pending.drain(..) {
                    assertions[index].target = Some(md);
                }
            }
            lines.push((md, (*text).to_string()));
        }
    }
    if !assertions.is_empty() {
        section.units.push(Unit {
            lang: "sql",
            lines,
            assertions,
            migration,
            schema,
        });
    }
}

/// Merges a comment continuing the previous assertion of the same kind.
fn append(assertions: &mut Vec<Assertion>, new: Assertion) {
    if let Some(prev) = assertions.last_mut() {
        if new.kind != "error" && prev.kind == new.kind && prev.line + prev.span == new.line {
            prev.expected.push('\n');
            prev.expected.push_str(&new.expected);
            prev.span += 1;
            return;
        }
    }
    assertions.push(new);
}

fn merge(base: &mut toml::Table, over: toml::Table) {
    for (key, value) in over {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(old)), toml::Value::Table(new)) => merge(old, new),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

/// Every Markdown file the suite reads: `mdtest_paths` of `pyproject.toml`.
pub fn files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in ["tests/mdtest", "docs", "README.md", "skills"] {
        walk(&root.join(entry), &mut found);
    }
    found.sort();
    found
}

fn walk(path: &Path, found: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().is_some_and(|ext| ext == "md") {
            found.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        walk(&entry.path(), found);
    }
}
