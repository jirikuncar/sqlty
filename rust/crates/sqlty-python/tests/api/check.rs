use crate::common::*;
use sqlty_python::check::*;
use sqlty_python::rows::{Found, Settings};

fn crash(text: &str, rules: &[(&str, &str)]) -> Vec<Diagnostic> {
    run_module(text, rules, false)
        .diagnostics
        .into_iter()
        .map(|reported| reported.diagnostic)
        .collect()
}

fn run_module(text: &str, rules: &[(&str, &str)], parsed: bool) -> Found {
    let options = Options::new("postgres", None, None, false).unwrap();
    let rules = RuleConfig::from_overrides(Some(
        rules
            .iter()
            .map(|(rule, severity)| (rule.to_string(), severity.to_string()))
            .collect(),
    ))
    .unwrap();
    let coerce = HashSet::new();
    let settings = Settings {
        options: &options,
        rules: &rules,
        index: None,
        module: None,
        coerce: &coerce,
    };
    let lint = |_: &str| -> Vec<Lint> { panic!("lint {}", 1) };
    let run = Run {
        settings: &settings,
        has_schema: parsed,
        lint: &lint,
        helpers: None,
        variants: None,
    };
    let source = parsed
        .then(|| Source::new(text, (3, 14)).map(Arc::new))
        .flatten();
    module(text, source.as_ref(), &run)
}

#[test]
fn a_panic_becomes_an_internal_error() {
    let found = crash("q = 1\n", &[("sqruff", "warn")]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].rule, Rule::InternalError);
    assert_eq!(found[0].severity, Severity::Error);
    assert_eq!(
        found[0].message,
        format!(
            "sqlty crashed checking this file: lint 1; please report it at {}",
            sqlty_python::report::issue_url("lint 1")
        )
    );
    let ignored = [("sqruff", "warn"), ("internal-error", "ignore")];
    assert!(crash("q = 1\n", &ignored).is_empty());
    assert!(crash("q = 1\n", &[]).is_empty());
}
