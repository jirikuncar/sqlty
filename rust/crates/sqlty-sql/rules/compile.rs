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

/// Canonical SQL types the analyzer gives operands, which `left` and `right` may name.
/// `ARRAY` is any array.
const SQL_TYPES: [&str; 41] = [
    "UDECIMAL",
    "UTINYINT",
    "USMALLINT",
    "UMEDIUMINT",
    "UINT",
    "UBIGINT",
    "UDOUBLE",
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
    "ARRAY",
];

/// Named sets of SQL types, `@name` in `left` and `right`.
const CLASSES: [(&str, &[&str]); 6] = [
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
    ("@array", &["ARRAY"]),
];

/// Result types other than a SQL type name.
const RESULTS: [(&str, &str); 9] = [
    ("@any", "RuleType::Any"),
    ("@unknown", "RuleType::Unknown"),
    ("@code", "RuleType::Code"),
    ("@left", "RuleType::Left"),
    ("@right", "RuleType::Right"),
    ("@common", "RuleType::Common"),
    ("@promote", "RuleType::Promote"),
    ("@promote-decimal", "RuleType::PromoteToDecimal"),
    ("@promote-left", "RuleType::PromoteLeft"),
];

/// How a rule's result can be NULL.
const NULLABILITIES: [(&str, &str); 5] = [
    ("operands", "RuleNullability::Operands"),
    ("nullable", "RuleNullability::Nullable"),
    ("non-null", "RuleNullability::NonNull"),
    ("short-circuit", "RuleNullability::ShortCircuit"),
    ("nonzero-divisor", "RuleNullability::NonzeroDivisor"),
];

/// Result types of a function rule other than a SQL type name; `@argument-as <TYPE>` keeps
/// the argument's Python type and values under another SQL type.
const FUNCTION_RESULTS: [(&str, &str); 3] = [
    ("@unknown", "FunctionType::Unknown"),
    ("@code", "FunctionType::Code"),
    ("@argument", "FunctionType::Argument"),
];

/// How a function rule's result can be NULL.
const FUNCTION_NULLABILITIES: [(&str, &str); 2] = [
    ("non-null", "FunctionNullability::NonNull"),
    ("aggregate", "FunctionNullability::Aggregate"),
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleFile {
    #[serde(default)]
    rule: Vec<RuleDef>,
    #[serde(default)]
    function: Vec<FunctionDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FunctionDef {
    id: String,
    reference: String,
    reasoning: String,
    #[serde(default)]
    function: Option<String>,
    #[serde(default)]
    functions: Vec<String>,
    #[serde(default)]
    dialects: Vec<String>,
    #[serde(default)]
    argument: Vec<String>,
    #[serde(rename = "type")]
    result: String,
    nullability: String,
    #[serde(default)]
    shadows: Vec<String>,
}

impl From<FunctionDef> for RuleDef {
    fn from(def: FunctionDef) -> Self {
        RuleDef {
            id: def.id,
            reference: def.reference,
            reasoning: def.reasoning,
            operator: def.function,
            operators: def.functions,
            dialects: def.dialects,
            left: def.argument,
            right: Vec::new(),
            result: def.result,
            nullability: def.nullability,
            shadows: def.shadows,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuleDef {
    id: String,
    reference: String,
    reasoning: String,
    #[serde(default)]
    operator: Option<String>,
    #[serde(default)]
    operators: Vec<String>,
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
    /// Ids of later rules of the same tier this one takes precedence over where both match.
    #[serde(default)]
    shadows: Vec<String>,
}

fn operands() -> String {
    "operands".to_string()
}

/// An operand predicate: SQL types, and whether any typed operand is admitted (`@known`);
/// `None` admits any operand, typed or not.
#[derive(Clone, PartialEq, Eq)]
struct Pred {
    types: BTreeSet<&'static str>,
    known: bool,
}

type Types = Option<Pred>;

/// A rule as read, with where it came from and its order.
struct Rule {
    def: RuleDef,
    /// A function rule (`[[function]]`), whose operators are function names.
    function: bool,
    file: String,
    /// 0 for a dialect directory, 1 for `common/`: dialect rules are tried first.
    tier: u8,
    operators: Vec<String>,
    dialects: Vec<&'static str>,
    left: Types,
    right: Types,
}

impl Rule {
    fn outcome(&self) -> (&str, &str) {
        (&self.def.result, &self.def.nullability)
    }
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
        let defs = parsed
            .rule
            .into_iter()
            .map(|def| (def, false))
            .chain(parsed.function.into_iter().map(|def| (def.into(), true)));
        for (def, function) in defs {
            let rule = check(def, function, file, tier)?;
            if let Some(other) = rules.iter().find(|r| r.def.id == rule.def.id) {
                return Err(format!(
                    "rule {} ({file}): duplicate id, also in {}",
                    rule.def.id, other.file
                ));
            }
            rules.push(rule);
        }
    }
    for rule in &rules {
        if let Some(id) = rule
            .def
            .shadows
            .iter()
            .find(|id| !rules.iter().any(|r| &r.def.id == *id))
        {
            return Err(format!(
                "rule {} ({}): `shadows` names unknown rule {id}",
                rule.def.id, rule.file
            ));
        }
    }
    rules.sort_by_key(|rule| rule.tier);
    check_overlaps(&rules)?;
    Ok(generate(&rules))
}

/// Validate one rule: everything it names must exist.
fn check(def: RuleDef, function: bool, file: &str, tier: u8) -> Result<Rule, String> {
    let fail = |message: String| Err(format!("rule {} ({file}): {message}", def.id));
    if def.id.trim().is_empty() {
        return Err(format!("{file}: a rule has an empty id"));
    }
    if def.reference.trim().is_empty() || def.reasoning.trim().is_empty() {
        return fail("`reference` and `reasoning` must not be empty".to_string());
    }
    let operators: Vec<String> = def.operator.iter().chain(&def.operators).cloned().collect();
    let (one, many) = if function {
        ("function", "functions")
    } else {
        ("operator", "operators")
    };
    if operators.is_empty() || (def.operator.is_some() && !def.operators.is_empty()) {
        return fail(format!("give `{one}` or `{many}`, not both"));
    }
    if function {
        let named = |name: &&String| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        };
        if let Some(name) = operators.iter().find(|name| !named(name)) {
            return fail(format!("function {name:?} is not an upper-case name"));
        }
    } else if let Some(name) = operators.iter().find(|name| !is_operator(name)) {
        return fail(format!(
            "operator {name:?} is neither a sqlparser `BinaryOperator` variant nor `Custom:<text>`"
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
    let results: &[(&str, &str)] = if function {
        &FUNCTION_RESULTS
    } else {
        &RESULTS
    };
    let argument_as = function
        && def
            .result
            .strip_prefix("@argument-as ")
            .is_some_and(is_sql_type);
    if !(results.iter().any(|(name, _)| *name == def.result)
        || is_sql_type(&def.result)
        || argument_as)
    {
        return fail(format!("unknown result type {:?}", def.result));
    }
    let nullabilities: &[(&str, &str)] = if function {
        &FUNCTION_NULLABILITIES
    } else {
        &NULLABILITIES
    };
    if !nullabilities
        .iter()
        .any(|(name, _)| *name == def.nullability)
    {
        return fail(format!("unknown nullability {:?}", def.nullability));
    }
    Ok(Rule {
        def,
        function,
        file: file.to_string(),
        tier,
        operators,
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
    SQL_TYPES.contains(&name) && name != "ARRAY"
}

/// The SQL types an operand predicate admits; `None` for any operand.
fn operand_types(names: &[String]) -> Result<Types, String> {
    if names.is_empty() {
        return Ok(None);
    }
    let mut pred = Pred {
        types: BTreeSet::new(),
        known: false,
    };
    for name in names {
        if name == "@known" {
            pred.known = true;
        } else if let Some((_, members)) = CLASSES.iter().find(|(class, _)| class == name) {
            pred.types.extend(members.iter().copied());
        } else if let Some(known) = SQL_TYPES.iter().find(|known| *known == name) {
            pred.types.insert(*known);
        } else {
            return Err(format!("unknown operand type {name:?}"));
        }
    }
    Ok(Some(pred))
}

/// The operands two rules both match, with their dialects; `None` when they share none.
struct Region<'r> {
    dialects: Vec<&'r str>,
    left: Types,
    right: Types,
}

fn intersect<'r>(a: &'r Rule, b: &'r Rule) -> Option<Region<'r>> {
    let dialects: Vec<&str> = match (a.dialects.is_empty(), b.dialects.is_empty()) {
        (true, _) => b.dialects.clone(),
        (_, true) => a.dialects.clone(),
        _ => a
            .dialects
            .iter()
            .filter(|d| b.dialects.contains(d))
            .copied()
            .collect(),
    };
    if dialects.is_empty() && !(a.dialects.is_empty() && b.dialects.is_empty()) {
        return None;
    }
    let both = |x: &Types, y: &Types| match (x, y) {
        (None, other) | (other, None) => Some(other.clone()),
        (Some(x), Some(y)) => {
            let types: BTreeSet<&'static str> = match (x.known, y.known) {
                (true, true) => x.types.union(&y.types).copied().collect(),
                (true, false) => y.types.clone(),
                (false, true) => x.types.clone(),
                (false, false) => x.types.intersection(&y.types).copied().collect(),
            };
            let known = x.known && y.known;
            (known || !types.is_empty()).then_some(Some(Pred { types, known }))
        }
    };
    Some(Region {
        dialects,
        left: both(&a.left, &b.left)?,
        right: both(&a.right, &b.right)?,
    })
}

/// Whether `rule` matches every operand of `region`.
fn covers(rule: &Rule, region: &Region<'_>) -> bool {
    within(&region.left, &rule.left)
        && within(&region.right, &rule.right)
        && (rule.dialects.is_empty()
            || (!region.dialects.is_empty()
                && region.dialects.iter().all(|d| rule.dialects.contains(d))))
}

/// Within one tier, rules for one operator that match the same operands must agree, unless
/// an earlier rule decides those operands first: the earlier rule wins, so a later one it
/// covers would never apply, and a partial overlap would let file order decide silently.
fn check_overlaps(rules: &[Rule]) -> Result<(), String> {
    for (i, later) in rules.iter().enumerate() {
        let whole = Region {
            dialects: later.dialects.clone(),
            left: later.left.clone(),
            right: later.right.clone(),
        };
        for operator in &later.operators {
            let shadowing = rules[..i].iter().find(|earlier| {
                earlier.function == later.function
                    && earlier.tier == later.tier
                    && earlier.operators.contains(operator)
                    && covers(earlier, &whole)
            });
            if let Some(earlier) = shadowing {
                return Err(format!(
                    "rule {} ({}) never applies to {operator}: rule {} ({}) comes first and \
                     matches every operand it does",
                    later.def.id, later.file, earlier.def.id, earlier.file
                ));
            }
        }
        for (j, earlier) in rules[..i].iter().enumerate() {
            if earlier.function != later.function
                || earlier.tier != later.tier
                || earlier.outcome() == later.outcome()
            {
                continue;
            }
            for operator in later
                .operators
                .iter()
                .filter(|op| earlier.operators.contains(op))
            {
                let Some(region) = intersect(earlier, later) else {
                    continue;
                };
                let decided = rules[..j].iter().any(|first| {
                    first.function == later.function
                        && first.tier == later.tier
                        && first.operators.contains(operator)
                        && covers(first, &region)
                });
                let narrower = covers(
                    later,
                    &Region {
                        dialects: earlier.dialects.clone(),
                        left: earlier.left.clone(),
                        right: earlier.right.clone(),
                    },
                ) && !covers(
                    earlier,
                    &Region {
                        dialects: later.dialects.clone(),
                        left: later.left.clone(),
                        right: later.right.clone(),
                    },
                );
                let intended = earlier.def.shadows.contains(&later.def.id);
                if !(decided || narrower || intended) {
                    return Err(format!(
                        "rule {} ({}) overlaps rule {} ({}) for the same operands of {operator}; \
                         make the earlier rule narrower, the operand types disjoint, or list the \
                         later rule in the earlier one's `shadows`",
                        later.def.id, later.file, earlier.def.id, earlier.file
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Whether every operand `a` admits, `b` admits too.
fn within(a: &Types, b: &Types) -> bool {
    match (a, b) {
        (_, None) => true,
        (None, Some(_)) => false,
        (Some(_), Some(b)) if b.known => true,
        (Some(a), Some(b)) => !a.known && a.types.is_subset(&b.types),
    }
}

fn generate(rules: &[Rule]) -> String {
    let binary: Vec<&Rule> = rules.iter().filter(|rule| !rule.function).collect();
    let functions: Vec<&Rule> = rules.iter().filter(|rule| rule.function).collect();
    let mut out = generate_binary(&binary);
    out.push('\n');
    out.push_str(&generate_functions(&functions));
    out
}

fn generate_functions(rules: &[&Rule]) -> String {
    let mut entries: Vec<(&str, &Rule)> = rules
        .iter()
        .flat_map(|rule| {
            rule.operators
                .iter()
                .map(move |name| (name.as_str(), *rule))
        })
        .collect();
    entries.sort_by(|a, b| (a.0, a.1.tier).cmp(&(b.0, b.1.tier)));
    let mut out = String::from(
        "/// Every function inference rule, dialect rules first, each name's together.\n",
    );
    out.push_str("pub(crate) static FUNCTION_RULES: &[FunctionRule] = &[\n");
    let mut ranges: Vec<(&str, usize, usize)> = Vec::new();
    for (i, (name, rule)) in entries.iter().enumerate() {
        match ranges.last_mut() {
            Some((last, _, end)) if last == name => *end = i + 1,
            _ => ranges.push((name, i, i + 1)),
        }
        let def = &rule.def;
        let result = match def.result.strip_prefix("@argument-as ") {
            Some(sql) => format!("FunctionType::ArgumentAs({sql:?})"),
            None => FUNCTION_RESULTS
                .iter()
                .find(|(known, _)| *known == def.result)
                .map_or_else(
                    || format!("FunctionType::Sql({:?})", def.result),
                    |(_, path)| (*path).to_string(),
                ),
        };
        let nullability = FUNCTION_NULLABILITIES
            .iter()
            .find(|(known, _)| *known == def.nullability)
            .unwrap()
            .1;
        let _ = writeln!(
            out,
            "    // rule {} ({}), {name}\n    FunctionRule {{\n        id: {:?},\n        \
             reference: {:?},\n        reasoning: {:?},\n        dialects: &[{}],\n        \
             argument: {},\n        result: {result},\n        nullability: {nullability},\n    }},",
            def.id,
            rule.file,
            def.id,
            def.reference.trim(),
            def.reasoning.trim(),
            rule.dialects.join(", "),
            operand(&rule.left),
        );
    }
    out.push_str("];\n\n");
    out.push_str(
        "/// The rules of the function `name` (upper case), in the order they are tried.\n",
    );
    out.push_str("pub(crate) fn function_rules(name: &str) -> &'static [FunctionRule] {\n");
    out.push_str("    match name {\n");
    for (name, start, end) in &ranges {
        let _ = writeln!(out, "        {name:?} => &FUNCTION_RULES[{start}..{end}],");
    }
    out.push_str("        _ => &[],\n    }\n}\n");
    out
}

fn generate_binary(rules: &[&Rule]) -> String {
    let mut entries: Vec<(&str, &Rule)> = rules
        .iter()
        .flat_map(|rule| rule.operators.iter().map(move |op| (op.as_str(), *rule)))
        .collect();
    entries.sort_by(|a, b| (a.0, a.1.tier).cmp(&(b.0, b.1.tier)));
    let mut out = String::from("// @generated by build.rs from rules/**/*.inference.toml\n\n");
    out.push_str(
        "/// Every inference rule per operator, dialect rules first, each operator's together.\n",
    );
    out.push_str("pub(crate) static BINARY_RULES: &[BinaryRule] = &[\n");
    let mut ranges: Vec<(&str, usize, usize)> = Vec::new();
    for (i, (op, rule)) in entries.iter().enumerate() {
        match ranges.last_mut() {
            Some((last, _, end)) if last == op => *end = i + 1,
            _ => ranges.push((op, i, i + 1)),
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
            "    // rule {} ({}), {op}\n    BinaryRule {{\n        id: {:?},\n        reference: {:?},\n        \
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
            Some(text) => custom.push((text, *start, *end)),
            None => {
                let _ = writeln!(
                    out,
                    "        BinaryOperator::{op} {{ .. }} => &BINARY_RULES[{start}..{end}], // {}",
                    entries[*start].1.def.id
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

fn operand(types: &Types) -> String {
    match types {
        None => "None".to_string(),
        Some(pred) => format!(
            "Some(&[{}])",
            pred.known
                .then_some("@known")
                .into_iter()
                .chain(pred.types.iter().copied())
                .map(|t| format!("{t:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}
