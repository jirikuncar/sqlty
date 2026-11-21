//! What `sqlty.core` answers, called without Python.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlty_project::{check_module, Checker, Workspace};
use sqlty_python::options::Options;
use sqlty_python::params;
use sqlty_python::project::{Files, Kind, Listing, Read};
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
    check_with(source, dialect, schema, style, &[])
}

/// [`check`] with the severities of `rules` changed.
pub fn check_with(
    source: &str,
    dialect: &'static str,
    schema: &Schema,
    style: Option<&str>,
    rules: &[(&str, &str)],
) -> Vec<(&'static str, String)> {
    let coerce = HashSet::new();
    let overrides = rules
        .iter()
        .map(|(rule, severity)| (rule.to_string(), severity.to_string()));
    let rules = RuleConfig::from_overrides(Some(overrides.collect())).expect("known rules");
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

/// Files held in memory, by absolute path.
struct Tree(BTreeMap<PathBuf, String>);

impl Files for Tree {
    fn read(&self, path: &Path) -> Read {
        self.0
            .get(path)
            .map_or(Read::Missing, |text| Read::Text(text.clone().into_bytes()))
    }

    fn list(&self, path: &Path) -> Listing {
        let mut entries: Vec<(String, Kind)> = Vec::new();
        for file in self.0.keys() {
            let Ok(rest) = file.strip_prefix(path) else {
                continue;
            };
            let mut parts = rest.components();
            let Some(name) = parts.next() else { continue };
            let name = name.as_os_str().to_string_lossy().into_owned();
            let kind = if parts.next().is_some() {
                Kind::Dir
            } else {
                Kind::File
            };
            if !entries.iter().any(|(seen, _)| *seen == name) {
                entries.push((name, kind));
            }
        }
        if entries.is_empty() {
            Listing::Missing
        } else {
            Listing::Entries(entries)
        }
    }

    fn is_link(&self, _path: &Path) -> bool {
        false
    }
}

/// `(rule, message, line)` of each diagnostic `sqlty check` reports for each of `texts` as
/// the module `path` of the project of `files` (paths relative to its root).
pub fn check_project(
    files: &[(&str, String)],
    path: &str,
    texts: &[String],
) -> Vec<Vec<(&'static str, String, usize)>> {
    let root = Path::new("/project");
    let tree = Tree(
        files
            .iter()
            .map(|(name, text)| (root.join(name), text.clone()))
            .collect(),
    );
    let mut workspace =
        Workspace::root(root, Arc::new(tree), VERSION, Vec::new()).expect("the project opens");
    texts
        .iter()
        .map(|text| {
            workspace
                .check(&root.join(path), text)
                .expect("the module is checked")
                .into_iter()
                .map(|found| (found.rule.name(), found.message, found.line))
                .collect()
        })
        .collect()
}
