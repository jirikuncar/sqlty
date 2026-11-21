//! One module checked, fixed or rewritten with the settings of its file.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use sqlty_python::check::{self, Lint, Run};
use sqlty_python::checker::{query_sites, Diagnostic, SiteKind};
use sqlty_python::index::HelperIndex;
use sqlty_python::options::ConfigError;
use sqlty_python::options::Options;
use sqlty_python::rows::{self, Found};
use sqlty_python::rules::{Rule, RuleConfig};
use sqlty_python::types::symbols::{Index, Source};
use sqlty_python::{fixer, json_types, params, queries, rewriter, suppress};
use sqlty_sql::analyzer::{self, SchemaData};

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
    #[expect(
        clippy::expect_used,
        reason = "the configuration checked the rules and import style"
    )]
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
    let mut found = check::module(text, parsed, &run);
    let ignores = match parsed {
        Some(parsed) => suppress::ignores(parsed),
        None => suppress::unparsed_ignores(text),
    };
    found.diagnostics = suppress::apply(&ignores, found.diagnostics, &checker.rules);
    found
}

/// What `against` breaks in `text`: the diagnostics it gets with `against` but not with
/// `own`, as `sqlty breaking` reports them.
pub fn breaking(
    text: &str,
    parsed: Option<&Arc<Source>>,
    module: Option<&str>,
    own: &Checker<'_>,
    against: &Checker<'_>,
) -> Vec<Diagnostic> {
    let check = |checker| {
        check_module(text, parsed, module, checker)
            .diagnostics
            .into_iter()
            .map(|reported| reported.diagnostic)
            .collect::<Vec<_>>()
    };
    let key = |found: &Diagnostic| (found.line, found.rule, found.message.clone());
    sqlty_python::breaking::breaking(&check(own), check(against), key, |found| found.rule.name())
}

/// The diagnostics of the `.sql` file of named queries `text`, called `name`, without what
/// its `-- sqlty: ignore` comments drop.
pub fn check_queries(text: &str, name: &str, settings: &FileSettings) -> Vec<Diagnostic> {
    let checker = Checker::of(settings, None);
    let found = queries::check_queries(
        text,
        settings.query_format,
        name,
        &checker.options,
        &checker.rules,
    );
    let ignores = suppress::sql_ignores(text);
    suppress::apply_sql(&ignores, found, &checker.rules)
}

/// What `against` breaks in the `.sql` file of named queries `text`, as [`breaking`].
pub fn breaking_queries(
    text: &str,
    name: &str,
    own: &FileSettings,
    against: &FileSettings,
) -> Vec<Diagnostic> {
    let key = |found: &Diagnostic| (found.line, found.rule, found.message.clone());
    sqlty_python::breaking::breaking(
        &check_queries(text, name, own),
        check_queries(text, name, against),
        key,
        |found| found.rule.name(),
    )
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

/// How the queries of `text` fare, as `sqlty check --statistics` counts them: `typed`,
/// `partially typed`, `unparsed`, and skipped strings by rule. Queries that return no
/// rows are not counted.
pub fn statistics(
    text: &str,
    parsed: Option<&Source>,
    module: Option<&str>,
    checker: &Checker<'_>,
) -> HashMap<&'static str, usize> {
    let (dialect, schema) = (checker.dialect, checker.options.schema);
    let mut counts = HashMap::new();
    let mut outcomes = Vec::new();
    let outcome = |query: &str| {
        let (routed, routed_schema) = analyzer::route(query, dialect, schema);
        let parsed = analyzer::parse(query, dialect);
        if parsed.error().is_some() {
            Some("unparsed")
        } else if !parsed.returns_rows() {
            None
        } else if (if routed == dialect {
            parsed.columns(routed_schema)
        } else {
            analyzer::analyze_select(query, dialect, schema)
        })
        .is_some_and(|columns| columns.iter().all(|column| column.python_type.is_some()))
        {
            Some("typed")
        } else {
            Some("partially typed")
        }
    };
    let style = &checker.options.style;
    let any_calls = parsed.map_or_else(Vec::new, |parsed| check::any_calls(parsed, style));
    for (_, kind, query) in query_sites(text, style, &any_calls) {
        outcomes.push(match kind {
            SiteKind::Query => outcome(&query),
            SiteKind::Skipped(rule) => Some(rule.name()),
        });
    }
    if let Some(parsed) = parsed {
        for site in check::sites(parsed, checker.helpers, module, checker.variants) {
            outcomes.push(site.unchecked.map(Rule::name));
            outcomes.extend(site.queries.iter().map(|query| outcome(query)));
        }
    }
    for found in outcomes.into_iter().flatten() {
        *counts.entry(found).or_insert(0) += 1;
    }
    counts
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

/// What `sqlty rewrite` warns about in `text`: the f-, t- and other strings it leaves alone,
/// at the severities `settings` gives them.
#[expect(
    clippy::expect_used,
    reason = "every rule and severity is known; the configuration checked the import style"
)]
pub fn skipped_strings(text: &str, settings: &FileSettings) -> Vec<Diagnostic> {
    const SKIPPED: [Rule; 4] = [
        Rule::FstringSkipped,
        Rule::TstringSkipped,
        Rule::UncheckedQuery,
        Rule::DynamicQuery,
    ];
    let severities = Rule::ALL.into_iter().map(|rule| {
        let severity = match SKIPPED.contains(&rule) {
            true => settings
                .rules
                .get(rule.name())
                .cloned()
                .unwrap_or_else(|| rule.default_severity().as_str().to_string()),
            false => "ignore".to_string(),
        };
        (rule.name().to_string(), severity)
    });
    let rules = RuleConfig::from_overrides(Some(severities.collect()))
        .expect("the rules and severities are known");
    let coerce = HashSet::new();
    let import_style = settings.import_style.as_deref();
    let checker = Checker::new(
        "postgres",
        None,
        import_style,
        false,
        rules,
        None,
        &coerce,
        None,
    )
    .expect("the settings are validated");
    let parsed = Source::new(text, settings.version).map(Arc::new);
    let found = check_module(text, parsed.as_ref(), None, &checker);
    found
        .diagnostics
        .into_iter()
        .map(|reported| reported.diagnostic)
        .collect()
}

/// `text` as `sqlty rewrite` writes it with `settings`.
pub fn rewritten(text: &str, settings: &FileSettings) -> String {
    rewrite(text, &Checker::of(settings, None), settings)
}

/// `text` as `sqlty strip` writes it with `settings`.
pub fn stripped(text: &str, settings: &FileSettings) -> String {
    strip(text, &Checker::of(settings, None))
}

/// `text` as `sqlty format` writes it with `settings`.
pub fn formatted(text: &str, settings: &FileSettings) -> String {
    format(text, &Checker::of(settings, None))
}
