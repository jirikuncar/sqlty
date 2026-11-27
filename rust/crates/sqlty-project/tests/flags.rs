//! The command line's `-c` assignments and rule flags, and how a string is spelled in its
//! messages.

use std::collections::BTreeMap;
use std::sync::Arc;

use sqlty_project::{parse_overrides, repr, Configs, Disk, Flags, Options, Shared, Workspace};
use sqlty_python::project::Discovery;
use toml::Table;

fn overrides(assignments: &[&str]) -> Result<Table, String> {
    let assignments: Vec<String> = assignments.iter().map(ToString::to_string).collect();
    parse_overrides(&assignments)
}

#[test]
fn merges_the_assignments_key_by_key() {
    let table = overrides(&[
        "rules.select-star = \"warn\"",
        "rules = { unknown-column = \"ignore\" }",
        "dialect = \"mysql\"",
        "dialect = \"sqlite\"",
    ])
    .expect("parses");
    assert_eq!(
        table.to_string(),
        "dialect = \"sqlite\"\n\n[rules]\nselect-star = \"warn\"\nunknown-column = \"ignore\"\n"
    );
}

#[test]
fn reports_what_is_not_an_assignment() {
    assert_eq!(
        overrides(&["ci.toml"]).err().as_deref(),
        Some(
            "--config expects a TOML assignment such as 'dialect = \"mysql\"', got 'ci.toml'; \
             use --config-file for a file"
        )
    );
    let error = overrides(&["dialect = mysql"]).err().unwrap_or_default();
    assert!(
        error.starts_with("--config 'dialect = mysql': ")
            && error.ends_with(
                "; quote strings, as in -c 'dialect = \"mysql\"', or use --config-file for a file"
            ),
        "{error}"
    );
    let error = overrides(&["dialet = \"mysql\""]).err().unwrap_or_default();
    assert!(error.contains("dialect"), "{error}");
}

#[test]
fn spells_strings_as_python_does() {
    assert_eq!(repr("it's"), "\"it's\"");
    assert_eq!(repr("a'\"b"), "'a\\'\"b'");
    assert_eq!(repr("\\\n\r\t\u{7}é"), "'\\\\\\n\\r\\t\\x07é'");
}

#[test]
fn a_severity_flag_keeps_the_options_of_a_rule() {
    let root = std::env::temp_dir().join(format!("sqlty-flags-{}", std::process::id()));
    std::fs::create_dir_all(&root).expect("creates");
    std::fs::write(root.join("q.py"), "x = 1\n").expect("writes");
    let rules = |config: &str, severities: &[(&str, &str)]| {
        let flags = Flags {
            configs: Configs::File {
                table: config.parse().expect("parses"),
                label: "sqlty.toml".to_string(),
            },
            severities: severities
                .iter()
                .map(|(rule, severity)| (rule.to_string(), severity.to_string()))
                .collect(),
            cwd: root.clone(),
            ..Flags::default()
        };
        let options = Options {
            flags,
            discovery: Discovery::new(Box::new(Disk), &root),
            version: (3, 14),
            extra_roots: Vec::new(),
            force_exclude: false,
            parse: false,
        };
        let files = Shared(Arc::new(Disk));
        let mut workspace =
            Workspace::open(std::slice::from_ref(&root), files, options).expect("opens");
        let settings = workspace.settings(&root.join("q.py")).expect("reads");
        let lock: BTreeMap<String, String> = settings
            .rules
            .iter()
            .filter(|(rule, _)| rule.starts_with("missing-lock-timeout"))
            .map(|(rule, value)| (rule.clone(), value.clone()))
            .collect();
        lock
    };
    let pairs = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    };
    assert_eq!(
        rules(
            "[rules]\nmissing-lock-timeout = { max = \"2s\" }\n",
            &[("missing-lock-timeout", "error")]
        ),
        pairs(&[
            ("missing-lock-timeout.max", "2s"),
            ("missing-lock-timeout", "error")
        ])
    );
    assert_eq!(
        rules(
            "[rules]\nmissing-lock-timeout = \"ignore\"\n",
            &[("missing-lock-timeout.max", "1s")]
        ),
        pairs(&[
            ("missing-lock-timeout", "ignore"),
            ("missing-lock-timeout.max", "1s")
        ])
    );
    assert_eq!(
        rules("", &[("missing-lock-timeout.max", "1s")]),
        pairs(&[("missing-lock-timeout.max", "1s")])
    );
    std::fs::remove_dir_all(&root).expect("removes");
}
