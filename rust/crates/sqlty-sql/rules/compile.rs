//! The inference rule compiler: `rules/**/*.inference.toml` to Rust source. `build.rs`
//! runs it; `tests/inference_compiler.rs` tests it.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use serde::Deserialize;

/// The dialects a rule may name, as `SqlDialect` spells them in Rust.
const DIALECTS: [(&str, &str); 4] = [
    ("postgres", "SqlDialect::Postgres"),
    ("mysql", "SqlDialect::MySql"),
    ("sqlite", "SqlDialect::Sqlite"),
    ("clickhouse", "SqlDialect::ClickHouse"),
];

/// Canonical SQL types the analyzer gives operands (`operand_type`), which `left` and
/// `right` may name.
const SQL_TYPES: [&str; 33] = [
    "SMALLINT",
    "TINYINT",
    "MEDIUMINT",
    "INT",
    "BIGINT",
    "DECIMAL",
    "NUMERIC",
    "REAL",
    "FLOAT",
    "DOUBLE",
    "TEXT",
    "VARCHAR",
    "CHAR",
    "BOOLEAN",
    "DATE",
    "TIME",
    "TIMETZ",
    "TIMESTAMP",
    "TIMESTAMPTZ",
    "DATETIME",
    "INTERVAL",
    "JSON",
    "JSONB",
    "UUID",
    "BYTEA",
    "BLOB",
    "VARBINARY",
    "INET",
    "CIDR",
    "MACADDR",
    "TSVECTOR",
    "TSQUERY",
    "MONEY",
];

/// Named sets of SQL types, `@name` in `left` and `right`.
const CLASSES: [(&str, &[&str]); 5] = [
    (
        "@integer",
        &["SMALLINT", "TINYINT", "MEDIUMINT", "INT", "BIGINT"],
    ),
    (
        "@numeric",
        &[
            "SMALLINT",
            "TINYINT",
            "MEDIUMINT",
            "INT",
            "BIGINT",
            "DECIMAL",
            "NUMERIC",
            "REAL",
            "FLOAT",
            "DOUBLE",
        ],
    ),
    ("@text", &["TEXT", "VARCHAR", "CHAR"]),
    ("@json", &["JSON", "JSONB"]),
    ("@address", &["INET", "CIDR"]),
];

/// Result types other than a SQL type name.
const RESULTS: [(&str, &str); 2] = [("@any", "RuleType::Any"), ("@left", "RuleType::Left")];

/// How a rule's result can be NULL.
const NULLABILITIES: [(&str, &str); 3] = [
    ("operands", "RuleNullability::Operands"),
    ("nullable", "RuleNullability::Nullable"),
    ("non-null", "RuleNullability::NonNull"),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleFile {
    #[serde(default)]
    rule: Vec<RuleDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleDef {
    id: String,
    reference: String,
    reasoning: String,
    operator: String,
    #[serde(default)]
    dialects: Vec<String>,
    #[serde(default)]
    left: Vec<String>,
    #[serde(default)]
    right: Vec<String>,
    #[serde(rename = "type")]
    result: String,
    #[serde(default = "operands")]
    nullability: String,
}

fn operands() -> String {
    "operands".to_string()
}

/// A rule as read, with where it came from and its order.
struct Rule {
    def: RuleDef,
    file: String,
    /// 0 for a dialect directory, 1 for `common/`: dialect rules are tried first.
    tier: u8,
    dialects: Vec<&'static str>,
    left: Option<BTreeSet<&'static str>>,
    right: Option<BTreeSet<&'static str>>,
}

/// The Rust source of the rules in `files`, `(path, contents)` pairs with paths relative to
/// the crate (`rules/postgres/json.inference.toml`); a rule in a `common/` directory applies
/// to every dialect. Errors name the rule and its file.
pub fn compile(files: &[(String, String)]) -> Result<String, String> {
    let mut sorted: Vec<&(String, String)> = files.iter().collect();
    sorted.sort();
    let mut rules: Vec<Rule> = Vec::new();
    for (file, text) in sorted {
        let parsed: RuleFile = toml::from_str(text).map_err(|e| format!("{file}: {e}"))?;
        let tier = u8::from(file.contains("/common/"));
        for def in parsed.rule {
            let rule = check(def, file, tier)?;
            if let Some(other) = rules.iter().find(|r| r.def.id == rule.def.id) {
                return Err(format!(
                    "rule {} ({file}): duplicate id, also in {}",
                    rule.def.id, other.file
                ));
            }
            rules.push(rule);
        }
    }
    rules.sort_by_key(|rule| rule.tier);
    check_overlaps(&rules)?;
    Ok(generate(&rules))
}

/// Validate one rule: everything it names must exist.
fn check(def: RuleDef, file: &str, tier: u8) -> Result<Rule, String> {
    let fail = |message: String| Err(format!("rule {} ({file}): {message}", def.id));
    if def.id.trim().is_empty() {
        return Err(format!("{file}: a rule has an empty id"));
    }
    if def.reference.trim().is_empty() || def.reasoning.trim().is_empty() {
        return fail("`reference` and `reasoning` must not be empty".to_string());
    }
    if !is_operator(&def.operator) {
        return fail(format!(
            "operator {:?} is neither a sqlparser `BinaryOperator` variant nor `Custom:<text>`",
            def.operator
        ));
    }
    let mut dialects = Vec::new();
    for name in &def.dialects {
        match DIALECTS.iter().find(|(known, _)| known == name) {
            Some((_, path)) => dialects.push(*path),
            None => return fail(format!("unknown dialect {name:?}")),
        }
    }
    if tier == 1 && !dialects.is_empty() {
        return fail("a rule in `common/` applies to every dialect; move it".to_string());
    }
    if tier == 0 && dialects.is_empty() {
        return fail("a rule outside `common/` must name its `dialects`".to_string());
    }
    let left = operand_types(&def.left).map_err(|e| format!("rule {} ({file}): {e}", def.id))?;
    let right = operand_types(&def.right).map_err(|e| format!("rule {} ({file}): {e}", def.id))?;
    if !(RESULTS.iter().any(|(name, _)| *name == def.result) || is_sql_type(&def.result)) {
        return fail(format!("unknown result type {:?}", def.result));
    }
    if !NULLABILITIES
        .iter()
        .any(|(name, _)| *name == def.nullability)
    {
        return fail(format!("unknown nullability {:?}", def.nullability));
    }
    Ok(Rule {
        def,
        file: file.to_string(),
        tier,
        dialects,
        left,
        right,
    })
}

/// A sqlparser variant name (rustc checks it exists) or `Custom:<<=`.
fn is_operator(name: &str) -> bool {
    match name.strip_prefix("Custom:") {
        Some(text) => !text.is_empty() && !text.contains(['"', '\\']),
        None => {
            name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
                && name.chars().all(|c| c.is_ascii_alphanumeric())
        }
    }
}

fn is_sql_type(name: &str) -> bool {
    SQL_TYPES.contains(&name)
}

/// The SQL types an operand predicate admits; `None` for any operand.
fn operand_types(names: &[String]) -> Result<Option<BTreeSet<&'static str>>, String> {
    if names.is_empty() {
        return Ok(None);
    }
    let mut types = BTreeSet::new();
    for name in names {
        if let Some((_, members)) = CLASSES.iter().find(|(class, _)| class == name) {
            types.extend(members.iter().copied());
        } else if let Some(known) = SQL_TYPES.iter().find(|known| *known == name) {
            types.insert(*known);
        } else {
            return Err(format!("unknown operand type {name:?}"));
        }
    }
    Ok(Some(types))
}

/// Within one tier, two rules for one operator must not overlap unless the earlier one is
/// narrower: an earlier rule that covers a later one makes it unreachable, and a partial
/// overlap makes the order decide silently.
fn check_overlaps(rules: &[Rule]) -> Result<(), String> {
    for (i, later) in rules.iter().enumerate() {
        for earlier in &rules[..i] {
            if earlier.tier != later.tier || earlier.def.operator != later.def.operator {
                continue;
            }
            let dialects = earlier.dialects.is_empty()
                || later.dialects.is_empty()
                || earlier.dialects.iter().any(|d| later.dialects.contains(d));
            if !(dialects
                && overlaps(&earlier.left, &later.left)
                && overlaps(&earlier.right, &later.right))
            {
                continue;
            }
            let narrower = within(&earlier.left, &later.left)
                && within(&earlier.right, &later.right)
                && within_dialects(&earlier.dialects, &later.dialects)
                && (earlier.left != later.left
                    || earlier.right != later.right
                    || earlier.dialects != later.dialects);
            if !narrower {
                return Err(format!(
                    "rule {} ({}) overlaps rule {} ({}) for the same operands; make the earlier \
                     rule narrower or the operand types disjoint",
                    later.def.id, later.file, earlier.def.id, earlier.file
                ));
            }
        }
    }
    Ok(())
}

fn overlaps(a: &Option<BTreeSet<&str>>, b: &Option<BTreeSet<&str>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => !a.is_disjoint(b),
        _ => true,
    }
}

/// Whether every operand `a` admits, `b` admits too.
fn within(a: &Option<BTreeSet<&str>>, b: &Option<BTreeSet<&str>>) -> bool {
    match (a, b) {
        (_, None) => true,
        (None, Some(_)) => false,
        (Some(a), Some(b)) => a.is_subset(b),
    }
}

fn within_dialects(a: &[&str], b: &[&str]) -> bool {
    b.is_empty() || (!a.is_empty() && a.iter().all(|d| b.contains(d)))
}

fn generate(rules: &[Rule]) -> String {
    let mut out = String::from("// @generated by build.rs from rules/**/*.inference.toml\n\n");
    out.push_str(
        "/// Every inference rule, dialect rules first, each operator's rules together.\n",
    );
    out.push_str("pub(crate) static BINARY_RULES: &[BinaryRule] = &[\n");
    let mut order: Vec<&Rule> = rules.iter().collect();
    order.sort_by(|a, b| (&a.def.operator, a.tier).cmp(&(&b.def.operator, b.tier)));
    let mut ranges: Vec<(String, usize, usize)> = Vec::new();
    for (i, rule) in order.iter().enumerate() {
        match ranges.last_mut() {
            Some((op, _, end)) if *op == rule.def.operator => *end = i + 1,
            _ => ranges.push((rule.def.operator.clone(), i, i + 1)),
        }
        let def = &rule.def;
        let result = RESULTS
            .iter()
            .find(|(name, _)| *name == def.result)
            .map_or_else(
                || format!("RuleType::Sql({:?})", def.result),
                |(_, path)| (*path).to_string(),
            );
        let nullability = NULLABILITIES
            .iter()
            .find(|(name, _)| *name == def.nullability)
            .unwrap()
            .1;
        let _ = writeln!(
            out,
            "    // rule {} ({})\n    BinaryRule {{\n        id: {:?},\n        reference: {:?},\n        \
             reasoning: {:?},\n        dialects: &[{}],\n        left: {},\n        right: {},\n        \
             result: {result},\n        nullability: {nullability},\n    }},",
            def.id,
            rule.file,
            def.id,
            def.reference.trim(),
            def.reasoning.trim(),
            rule.dialects.join(", "),
            operand(&rule.left),
            operand(&rule.right),
        );
    }
    out.push_str("];\n\n");
    out.push_str("/// The rules of `op`, in the order they are tried.\n");
    // `Variant { .. }` matches unit and tuple variants alike.
    out.push_str("#[allow(clippy::unneeded_struct_pattern)]\n");
    out.push_str("pub(crate) fn binary_rules(op: &BinaryOperator) -> &'static [BinaryRule] {\n");
    out.push_str("    match op {\n");
    let mut custom = Vec::new();
    for (op, start, end) in &ranges {
        match op.strip_prefix("Custom:") {
            Some(text) => custom.push((text.to_string(), *start, *end)),
            None => {
                let _ = writeln!(
                    out,
                    "        BinaryOperator::{op} {{ .. }} => &BINARY_RULES[{start}..{end}], // {}",
                    order[*start].def.id
                );
            }
        }
    }
    if !custom.is_empty() {
        out.push_str("        BinaryOperator::Custom(name) => match name.as_str() {\n");
        for (text, start, end) in custom {
            let _ = writeln!(
                out,
                "            {text:?} => &BINARY_RULES[{start}..{end}],"
            );
        }
        out.push_str("            _ => &[],\n        },\n");
    }
    out.push_str("        _ => &[],\n    }\n}\n");
    out
}

fn operand(types: &Option<BTreeSet<&str>>) -> String {
    match types {
        None => "None".to_string(),
        Some(types) => format!(
            "Some(&[{}])",
            types
                .iter()
                .map(|t| format!("{t:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
