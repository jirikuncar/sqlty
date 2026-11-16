//! One module checked, fixed or rewritten with the settings of its file.

use std::collections::HashSet;
use std::sync::Arc;

use sqlty_python::check::{self, Lint, Run};
use sqlty_python::index::HelperIndex;
use sqlty_python::options::ConfigError;
use sqlty_python::options::Options;
use sqlty_python::rows::{self, Found};
use sqlty_python::rules::RuleConfig;
use sqlty_python::types::symbols::{Index, Source};
use sqlty_python::{fixer, json_types, params, rewriter, suppress};
use sqlty_sql::analyzer::SchemaData;

use crate::settings::FileSettings;

/// What a module is read with, read once for every module that shares it.
pub struct Checker<'a> {
    /// How queries are parsed and spelled.
    pub options: Options<'a>,
    /// The severity of each rule.
    pub rules: RuleConfig,
    /// The helpers and types of the checked modules.
    pub helpers: Option<&'a HelperIndex>,
    types: Option<Arc<dyn Index>>,
    has_schema: bool,
    dialect: &'static str,
    coerce: &'a HashSet<String>,
    variants: Option<usize>,
}

impl<'a> Checker<'a> {
    /// A checker of `schema` in `dialect`, with the helpers of `helpers`.
    #[expect(clippy::too_many_arguments, reason = "each is a setting of the check")]
    pub fn new(
        dialect: &'static str,
        schema: Option<&'a SchemaData>,
        import_style: Option<&str>,
        untyped: bool,
        rules: RuleConfig,
        helpers: Option<&'a HelperIndex>,
        coerce: &'a HashSet<String>,
        variants: Option<usize>,
    ) -> Result<Self, ConfigError> {
        Ok(Self {
            options: Options::new(dialect, schema, import_style, untyped)?,
            rules,
            types: helpers.map(HelperIndex::types),
            has_schema: schema
                .is_some_and(|schema| !schema.databases().is_some_and(|all| all.absent)),
            helpers,
            dialect,
            coerce,
            variants,
        })
    }

    /// A checker of the settings of a file, whose rules and import style its configuration
    /// checked.
    pub fn of(settings: &'a FileSettings, helpers: Option<&'a HelperIndex>) -> Self {
        let rules = RuleConfig::from_overrides(Some(settings.rules.clone()))
            .expect("the configuration checks its rules");
        Self::new(
            settings.dialect,
            settings.schema.as_deref(),
            settings.import_style.as_deref(),
            settings.untyped,
            rules,
            helpers,
            &settings.coerce,
            settings.variants,
        )
        .expect("the settings check the import style")
    }
}

/// The diagnostics of `text`, parsed as `parsed` when it parses, as the module `module`,
/// without what its `# sqlty: ignore` comments drop and with their `unused-ignore`.
pub fn check_module(
    text: &str,
    parsed: Option<&Arc<Source>>,
    module: Option<&str>,
    checker: &Checker<'_>,
) -> Found {
    let settings = rows::Settings {
        options: &checker.options,
        rules: &checker.rules,
        index: checker.types.as_ref(),
        module,
        coerce: checker.coerce,
    };
    let lint = |text: &str| {
        sqlty_format::lint_source(text, checker.dialect, &checker.options.style)
            .into_iter()
            .map(|found| Lint {
                message: format!("{}: {}", found.code, found.message),
                span: found.span,
                query_start: found.query_start,
            })
            .collect()
    };
    let run = Run {
        settings: &settings,
        has_schema: checker.has_schema,
        lint: &lint,
        helpers: checker.helpers,
        variants: checker.variants,
    };
    let mut found = match parsed {
        Some(parsed) => check::diagnostics(parsed, &run),
        None => check::unparsed(text, &run),
    };
    let ignores = match parsed {
        Some(parsed) => suppress::ignores(parsed),
        None => suppress::unparsed_ignores(text),
    };
    found.diagnostics = suppress::apply(&ignores, found.diagnostics, &checker.rules);
    found
}

/// `text` as `sqlty check --fix` writes it.
pub fn fix(
    text: &str,
    parsed: Option<&Source>,
    module: Option<&str>,
    checker: &Checker<'_>,
    settings: &FileSettings,
) -> String {
    let options = &checker.options;
    let fixed = fixer::fix_module(
        text,
        parsed,
        options,
        &checker.rules,
        checker.helpers,
        module,
    );
    typed(text, fixed, options, settings)
}

/// `text` as `sqlty rewrite` writes it.
pub fn rewrite(text: &str, checker: &Checker<'_>, settings: &FileSettings) -> String {
    let rewritten = rewriter::rewrite_source_with(text, &checker.options, false);
    typed(text, rewritten, &checker.options, settings)
}

/// `text` as `sqlty strip` writes it.
pub fn strip(text: &str, checker: &Checker<'_>) -> String {
    rewriter::strip_source(text, &checker.options.style)
}

/// `text` as `sqlty format` writes it.
pub fn format(text: &str, checker: &Checker<'_>) -> String {
    sqlty_format::format_source(text, checker.dialect, &checker.options.style)
}

/// The byte offsets of the string literals the helper calls of `parsed` pass as their query.
pub fn helper_starts(parsed: &Source, module: Option<&str>, checker: &Checker<'_>) -> Vec<usize> {
    check::sites(parsed, checker.helpers, module, checker.variants)
        .into_iter()
        .filter_map(|site| site.start)
        .collect()
}

/// `changed`, a rewrite of `text`, with JSON and parameter types when the settings ask.
fn typed(text: &str, changed: String, options: &Options<'_>, settings: &FileSettings) -> String {
    let typing = Options::new(
        settings.dialect,
        options.schema,
        settings.import_style.as_deref(),
        false,
    );
    let Ok(options) = typing else { return changed };
    let mut changed = changed;
    if settings.json_typed_dicts && changed != text {
        changed = json_types::add_json_types(&changed, &options, settings.version);
    }
    if settings.param_types {
        changed = params::add_param_types(&changed, &options, settings.closed_params);
    }
    changed
}
