//! What `sqlty.core` answers, called without Python.

use std::collections::HashSet;
use std::sync::Arc;

use sqlty_project::{check_module, Checker};
use sqlty_python::options::Options;
use sqlty_python::params;
use sqlty_python::rewriter::{self, ImportStyle};
use sqlty_python::rules::RuleConfig;
use sqlty_python::types::symbols::Source;
use sqlty_sql::analyzer::{self, Column, Param, Schema};

pub const VERSION: (u8, u8) = (3, 14);

pub fn schema(sql: &str, dialect: &str) -> Schema {
    analyzer::load_schema_with_skipped(sql, dialect).0
}

pub fn analyze_select(sql: &str, dialect: &str, schema: Option<&Schema>) -> Option<Vec<Column>> {
    analyzer::analyze_select(sql, dialect, schema.map(Arc::as_ref))
}

pub fn analyze_params(sql: &str, dialect: &str, schema: &Schema) -> Option<Vec<Param>> {
    analyzer::analyze_params(sql, dialect, Some(schema))
}

/// `(rule, message)` of each diagnostic `sqlty check` reports for `source`.
pub fn check(
    source: &str,
    dialect: &'static str,
    schema: &Schema,
    style: Option<&str>,
) -> Vec<(&'static str, String)> {
    let coerce = HashSet::new();
    let rules = RuleConfig::from_overrides(None).expect("the default rules");
    let checker = Checker::new(
        dialect,
        Some(schema),
        style,
        false,
        rules,
        None,
        &coerce,
        None,
    )
    .expect("a known import style");
    let parsed = Source::new(source, VERSION).map(Arc::new);
    check_module(source, parsed.as_ref(), None, &checker)
        .diagnostics
        .into_iter()
        .map(|found| (found.diagnostic.rule.name(), found.diagnostic.message))
        .collect()
}

/// `core.rewrite_source`: row types, then parameter types.
pub fn rewrite(source: &str, schema: &Schema, style: Option<&str>) -> String {
    let options =
        Options::new("postgres", Some(schema), style, false).expect("a known import style");
    let rewritten = rewriter::rewrite_source_with(source, &options, false);
    params::add_param_types(&rewritten, &options, false)
}

pub fn strip(source: &str, style: Option<&str>) -> String {
    rewriter::strip_source(
        source,
        &ImportStyle::parse(style).expect("a known import style"),
    )
}
