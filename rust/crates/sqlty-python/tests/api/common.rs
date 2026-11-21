//! Names most test modules use.

pub use std::collections::{HashMap, HashSet};
pub use std::path::{Path, PathBuf};
pub use std::sync::Arc;

pub use sqlty_python::checker::Diagnostic;
pub use sqlty_python::options::{Dialect, Options};
pub use sqlty_python::rewriter::ImportStyle;
pub use sqlty_python::rules::{Rule, RuleConfig, Severity};
pub use sqlty_python::types::symbols::Source;
pub use sqlty_python::types::ty::Lit;
pub use sqlty_sql::analyzer::SchemaData;
pub use toml::Table;

/// The style the setting names, the default for an unsupported one.
pub fn style_or_default(style: Option<&str>) -> ImportStyle {
    ImportStyle::parse(style).unwrap_or_default()
}

/// The default configuration with every rule but `rules` ignored.
pub fn only(rules: &[Rule]) -> RuleConfig {
    let overrides = Rule::ALL
        .into_iter()
        .filter(|rule| !rules.contains(rule))
        .map(|rule| (rule.name().to_string(), "ignore".to_string()))
        .collect();
    RuleConfig::from_overrides(Some(overrides)).unwrap()
}
