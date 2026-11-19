//! The rule page compiler: `src/sqlty/rules/*.md` to Rust source. `build.rs` runs it;
//! `tests/registry_compiler.rs` tests it.

use std::collections::BTreeMap;

use serde::Deserialize;

/// The most rules a `RuleSet` (one `u64`) holds.
const MAX_RULES: usize = 64;

/// The groups a rule may name, with the `Group` variant of each.
const GROUPS: [(&str, &str); 7] = [
    ("row-type", "RowType"),
    ("query", "Query"),
    ("names", "Names"),
    ("rewrite", "Rewrite"),
    ("params", "Params"),
    ("suppression", "Suppression"),
    ("internal", "Internal"),
];

/// The severities a rule may name, with the `Severity` variant of each.
const SEVERITIES: [(&str, &str); 3] = [("error", "Error"), ("warn", "Warn"), ("ignore", "Ignore")];

/// Where a rule is reported: by the SQL analyzer, or by the Python checks.
const ORIGINS: [&str; 2] = ["SQL", "Python"];

/// What a pass reads, with the `FactKind` variant and the documentation of each.
const FACTS: [(&str, &str, &str); 6] = [
    ("tokens", "Tokens", "The lexed source."),
    ("module", "Module", "The parsed module."),
    (
        "helper-calls",
        "HelperCalls",
        "The calls of the configured helpers.",
    ),
    ("calls", "Calls", "Helper calls and driver calls."),
    ("declared", "Declared", "The `SQL[R, P]` calls."),
    (
        "row-types",
        "RowTypes",
        "The row types of `SQL[Row](...)` calls.",
    ),
];

/// The passes file: its `[[pass]]` entries, in the order the passes run.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PassFile {
    pass: Vec<PassDef>,
}

/// One `[[pass]]` entry.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PassDef {
    name: String,
    facts: String,
    pipeline: bool,
    gated: bool,
    doc: String,
}

struct PassInfo {
    variant: String,
    def: PassDef,
    facts: &'static str,
}

/// The front matter of a rule page.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Front {
    order: u32,
    group: String,
    severity: String,
    fixable: bool,
    summary: String,
    origin: String,
    #[serde(rename = "reported-by")]
    reported_by: Vec<String>,
}

struct Page<'a> {
    file: &'a str,
    variant: String,
    front: Front,
    group: &'static str,
    severity: &'static str,
    body: &'a str,
}

/// Rust source of the registry of `pages`, each a file name and its text, and of the passes
/// file `passes`.
pub fn compile(pages: &[(String, String)], passes: &str) -> Result<String, String> {
    let passes = parse_passes(passes).map_err(|e| format!("passes.toml: {e}"))?;
    let mut sorted: Vec<&(String, String)> = pages.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut parsed = Vec::with_capacity(sorted.len());
    for (file, text) in sorted {
        parsed.push(parse(file, text).map_err(|e| format!("{file}: {e}"))?);
    }
    if parsed.len() > MAX_RULES {
        return Err(format!(
            "{} rule pages, but a RuleSet holds at most {MAX_RULES}",
            parsed.len()
        ));
    }
    let mut orders: BTreeMap<u32, &str> = BTreeMap::new();
    for page in &parsed {
        if let Some(other) = orders.insert(page.front.order, page.file) {
            return Err(format!(
                "{}: order {} is also the order of {other}",
                page.file, page.front.order
            ));
        }
    }
    let mut rules: Vec<&Page> = parsed.iter().collect();
    rules.sort_by_key(|page| page.front.order);
    for page in &parsed {
        if page.front.reported_by.is_empty() {
            return Err(format!("{}: reported-by names no pass", page.file));
        }
        for name in &page.front.reported_by {
            if !passes.iter().any(|pass| pass.def.name == *name) {
                let names: Vec<&str> = passes.iter().map(|pass| pass.def.name.as_str()).collect();
                return Err(format!(
                    "{}: unknown pass {name:?} in reported-by (expected {})",
                    page.file,
                    names.join(", ")
                ));
            }
        }
    }
    for pass in &passes {
        if !parsed
            .iter()
            .any(|page| page.front.reported_by.contains(&pass.def.name))
        {
            return Err(format!(
                "passes.toml: pass {:?} is named by no rule page",
                pass.def.name
            ));
        }
    }
    Ok(generate(&rules, &parsed, &passes))
}

fn parse_passes(text: &str) -> Result<Vec<PassInfo>, String> {
    let file: PassFile = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut passes: Vec<PassInfo> = Vec::with_capacity(file.pass.len());
    for def in file.pass {
        if passes.iter().any(|known| known.def.name == def.name) {
            return Err(format!("duplicate pass {:?}", def.name));
        }
        let variant = variant(&def.name).map_err(|e| format!("pass {:?}: {e}", def.name))?;
        let facts = lookup(
            &FACTS.map(|(name, variant, _)| (name, variant)),
            &def.facts,
            "facts",
        )
        .map_err(|e| format!("pass {:?}: {e}", def.name))?;
        passes.push(PassInfo {
            variant,
            def,
            facts,
        });
    }
    Ok(passes)
}

fn parse<'a>(file: &'a str, text: &'a str) -> Result<Page<'a>, String> {
    let stem = file
        .strip_suffix(".md")
        .ok_or("a rule page must be a .md file")?;
    let variant = variant(stem)?;
    let (toml, body) = split(text)?;
    let front: Front = toml::from_str(toml).map_err(|e| format!("front matter: {e}"))?;
    let group = lookup(&GROUPS, &front.group, "group")?;
    let severity = lookup(&SEVERITIES, &front.severity, "severity")?;
    if !ORIGINS.contains(&front.origin.as_str()) {
        return Err(format!(
            "unknown origin {:?} (expected SQL or Python)",
            front.origin
        ));
    }
    if front.summary.trim().is_empty() {
        return Err("the summary is empty".to_string());
    }
    let title = format!("# {stem}");
    if body.lines().next() != Some(title.as_str()) {
        return Err(format!("the page must start with `{title}`"));
    }
    let default = format!("Default severity: **{}**", front.severity);
    if !body.lines().any(|line| line == default) {
        return Err(format!("missing the line `{default}`"));
    }
    for section in ["## What it does", "## Why is this bad?"] {
        if !body.lines().any(|line| line == section) {
            return Err(format!("missing the section `{section}`"));
        }
    }
    Ok(Page {
        file,
        variant,
        front,
        group,
        severity,
        body,
    })
}

/// The front matter between `+++` lines, and the page after it and one blank line.
fn split(text: &str) -> Result<(&str, &str), String> {
    let rest = text
        .strip_prefix("+++\n")
        .ok_or("missing front matter: the page must start with a `+++` line")?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches('\n') == "+++" {
            let body = rest[offset + line.len()..]
                .strip_prefix('\n')
                .ok_or("front matter must be followed by a blank line")?;
            return Ok((&rest[..offset], body));
        }
        offset += line.len();
    }
    Err("unterminated front matter: no closing `+++` line".to_string())
}

fn lookup(table: &[(&str, &'static str)], key: &str, what: &str) -> Result<&'static str, String> {
    table
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, variant)| *variant)
        .ok_or_else(|| {
            let names: Vec<&str> = table.iter().map(|(name, _)| *name).collect();
            format!("unknown {what} {key:?} (expected {})", names.join(", "))
        })
}

/// `UnknownTable` for `unknown-table`.
fn variant(stem: &str) -> Result<String, String> {
    let mut out = String::new();
    for part in stem.split('-') {
        let mut chars = part.chars();
        let valid = chars.next().is_some_and(|c| c.is_ascii_lowercase())
            && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        if !valid {
            return Err("a rule name is kebab-case: lowercase words joined by `-`".to_string());
        }
        out.push_str(&part[..1].to_ascii_uppercase());
        out.push_str(&part[1..]);
    }
    Ok(out)
}

fn arms(rules: &[&Page], value: impl Fn(&Page) -> String) -> String {
    rules
        .iter()
        .map(|page| format!("            Rule::{} => {},\n", page.variant, value(page)))
        .collect()
}

fn generate(rules: &[&Page], by_file: &[Page], passes: &[PassInfo]) -> String {
    let mut out = String::from("/// A lint rule, one per page of `src/sqlty/rules/`.\n#[repr(u8)]\n#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\n");
    out.push_str("pub enum Rule {\n");
    for page in rules {
        out.push_str(&format!(
            "    #[doc = {:?}]\n    {},\n",
            page.front.summary, page.variant
        ));
    }
    out.push_str("}\n\nimpl Rule {\n");
    out.push_str(&format!(
        "    /// The number of rules.\n    pub const COUNT: usize = {};\n\n",
        rules.len()
    ));
    let list = |keep: &dyn Fn(&Page) -> bool| -> String {
        rules
            .iter()
            .filter(|page| keep(page))
            .map(|page| format!("        Rule::{},\n", page.variant))
            .collect()
    };
    out.push_str(&format!(
        "    /// Every rule, in the order `sqlty rule` lists them.\n    pub const ALL: [Rule; Rule::COUNT] = [\n{}    ];\n\n",
        list(&|_| true)
    ));
    out.push_str(&format!(
        "    /// The rules the SQL analyzer reports, in the order of [`Rule::ALL`].\n    pub const SQL: &'static [Rule] = &[\n{}    ];\n\n",
        list(&|page| page.front.origin == "SQL")
    ));
    let methods = [
        (
            "The kebab-case rule name, as in `[tool.sqlty.rules]`.",
            "name",
            "&'static str",
        ),
        (
            "What the rule reports, in a line.",
            "summary",
            "&'static str",
        ),
        (
            "Severity when `[tool.sqlty.rules]` does not set one.",
            "default_severity",
            "Severity",
        ),
        (
            "Whether `sqlty check --fix` can fix the rule's diagnostics.",
            "fixable",
            "bool",
        ),
        (
            "The group of the rules table of the documentation.",
            "group",
            "Group",
        ),
        (
            "Whether the SQL analyzer reports the rule.",
            "is_sql",
            "bool",
        ),
    ];
    for (doc, method, ty) in methods {
        let table = match method {
            "name" => arms(rules, |page| {
                format!("{:?}", page.file.trim_end_matches(".md"))
            }),
            "summary" => arms(rules, |page| format!("{:?}", page.front.summary)),
            "default_severity" => arms(rules, |page| format!("Severity::{}", page.severity)),
            "fixable" => arms(rules, |page| page.front.fixable.to_string()),
            "group" => arms(rules, |page| format!("Group::{}", page.group)),
            _ => arms(rules, |page| (page.front.origin == "SQL").to_string()),
        };
        out.push_str(&format!(
            "    /// {doc}\n    pub fn {method}(self) -> {ty} {{\n        match self {{\n{table}        }}\n    }}\n\n"
        ));
    }
    let reporters = arms(rules, |page| {
        let list: Vec<String> = page
            .front
            .reported_by
            .iter()
            .map(|name| format!("Pass::{}", pass_variant(passes, name)))
            .collect();
        format!("&[{}]", list.join(", "))
    });
    out.push_str(&format!(
        "    /// The passes that report the rule, in the order of the passes file.\n    pub fn reported_by(self) -> &'static [Pass] {{\n        match self {{\n{reporters}        }}\n    }}\n\n"
    ));
    out.push_str("    /// The rule called `name`, if there is one.\n    pub fn parse(name: &str) -> Option<Rule> {\n        match name {\n");
    for page in rules {
        out.push_str(&format!(
            "            {:?} => Some(Rule::{}),\n",
            page.file.trim_end_matches(".md"),
            page.variant
        ));
    }
    out.push_str("            _ => None,\n        }\n    }\n}\n\n");
    out.push_str("/// The rule pages, by file name, without their front matter.\npub static PAGES: &[(&str, &str)] = &[\n");
    for page in by_file {
        out.push_str(&format!("    ({:?}, {:?}),\n", page.file, page.body));
    }
    out.push_str("];\n\n");
    generate_passes(&mut out, rules, passes);
    out
}

fn pass_variant<'a>(passes: &'a [PassInfo], name: &str) -> &'a str {
    passes
        .iter()
        .find(|pass| pass.def.name == name)
        .map_or("", |pass| pass.variant.as_str())
}

fn generate_passes(out: &mut String, rules: &[&Page], passes: &[PassInfo]) {
    out.push_str("/// What a pass reads.\n#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\npub enum FactKind {\n");
    for (_, variant, doc) in FACTS {
        out.push_str(&format!("    #[doc = {doc:?}]\n    {variant},\n"));
    }
    out.push_str("}\n\n");
    out.push_str("/// A step that reports lint rules, one per `[[pass]]` of `passes.toml`.\n#[repr(u8)]\n#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]\npub enum Pass {\n");
    for pass in passes {
        out.push_str(&format!(
            "    #[doc = {:?}]\n    {},\n",
            pass.def.doc, pass.variant
        ));
    }
    out.push_str("}\n\nimpl Pass {\n");
    let list = |keep: &dyn Fn(&PassInfo) -> bool| -> String {
        passes
            .iter()
            .filter(|pass| keep(pass))
            .map(|pass| format!("        Pass::{},\n", pass.variant))
            .collect()
    };
    out.push_str(&format!(
        "    /// The number of passes.\n    pub const COUNT: usize = {};\n\n",
        passes.len()
    ));
    out.push_str(&format!(
        "    /// Every pass, in file order.\n    pub const ALL: [Pass; Pass::COUNT] = [\n{}    ];\n\n",
        list(&|_| true)
    ));
    out.push_str(&format!(
        "    /// The passes `check::diagnostics` runs, in the order it runs them.\n    pub const PIPELINE: &'static [Pass] = &[\n{}    ];\n\n",
        list(&|pass| pass.def.pipeline)
    ));
    let pass_arms = |value: &dyn Fn(&PassInfo) -> String| -> String {
        passes
            .iter()
            .map(|pass| format!("            Pass::{} => {},\n", pass.variant, value(pass)))
            .collect()
    };
    let name = pass_arms(&|pass| format!("{:?}", pass.def.name));
    let facts = pass_arms(&|pass| format!("FactKind::{}", pass.facts));
    let gated = pass_arms(&|pass| pass.def.gated.to_string());
    let sets = pass_arms(&|pass| {
        let mut set = String::from("RuleSet::EMPTY");
        for page in rules {
            if page.front.reported_by.contains(&pass.def.name) {
                set.push_str(&format!(".with(Rule::{})", page.variant));
            }
        }
        set
    });
    for (doc, method, ty, table) in [
        (
            "The pass name, as in `passes.toml`.",
            "name",
            "&'static str",
            name,
        ),
        ("What the pass reads.", "facts", "FactKind", facts),
        (
            "Whether the pass is skipped when every rule it reports is ignored.",
            "gated",
            "bool",
            gated,
        ),
        ("The rules the pass reports.", "rules", "RuleSet", sets),
    ] {
        out.push_str(&format!(
            "    /// {doc}\n    pub fn {method}(self) -> {ty} {{\n        match self {{\n{table}        }}\n    }}\n\n"
        ));
    }
    out.push_str("}\n");
}
