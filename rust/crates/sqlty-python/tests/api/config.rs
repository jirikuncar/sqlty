use crate::common::*;
use sqlty_python::config::*;

fn invalid(toml: &str) -> String {
    validate(toml.parse().unwrap(), "sqlty.toml").unwrap_err().0
}

#[test]
fn suggests_like_difflib() {
    assert_eq!(suggest("dialet", &TOOL_KEYS), " (did you mean 'dialect'?)");
    assert_eq!(
        suggest("import_style", &TOOL_KEYS),
        " (did you mean 'import-style'?)"
    );
    assert_eq!(
        suggest("postgress", &Dialect::NAMES),
        " (did you mean 'postgres'?)"
    );
    assert_eq!(suggest("bogus", &TOOL_KEYS), "");
    // Similarity 0.75 passes the 0.6 cutoff, 0.25 does not; two empty words are the same.
    assert_eq!(suggest("abcd", &["bcde"]), " (did you mean 'bcde'?)");
    assert_eq!(suggest("abcd", &["bxyz"]), "");
    assert_eq!(suggest("", &[""]), " (did you mean ''?)");
}

#[test]
fn reports_unknown_keys_rules_and_severities() {
    assert_eq!(
        invalid("type-modules = []"),
        "sqlty.toml: unknown key 'type-modules' in [tool.sqlty]: removed, since `check` \
             reads row types from source and never imports a module"
    );
    assert!(invalid("[rules]\nstale-typ = 'warn'").contains("did you mean 'stale-type'?"));
    assert!(invalid("[rules]\nstale-type = 'loud'").contains("invalid severity 'loud'"));
    assert!(invalid("[rules]\nstale-type = 1").contains("invalid severity 1"));
    assert!(invalid("[[overrides]]\nrule = {}").contains("did you mean 'rules'?"));
    assert!(invalid("[[functions]]\nquery = 'q'").contains("unknown key 'query'"));
    assert!(invalid("dialect = 'postgress'").contains("did you mean 'postgres'?"));
    assert!(invalid("dialect = 1").contains("dialect must be a string"));
    assert!(invalid("extend = []").contains("extend must be a string"));
    assert!(invalid("schema = 1").contains("schema must be a path"));
    assert!(invalid("max-query-variants = 0").contains("positive integer"));
    assert!(invalid("coerce-row-types = \"app.User\"").contains("list of row type names"));
    assert!(invalid("driver = \"asyncpg\"").contains("must be \"psycopg\""));
    assert!(invalid("types = { DATE = 1 }").contains("types must be a table"));
    assert!(invalid("databases = 1").contains("databases must be a table of tables"));
    assert_eq!(
        invalid("types = { DATE = \"list[\" }"),
        "sqlty.toml: [tool.sqlty.types] DATE = 'list[' is not a Python type expression"
    );
    assert!(invalid("[databases.default]").contains("not 'default'"));
    assert!(invalid("[databases.'a b']").contains("letters, digits"));
    assert_eq!(
        invalid("[databases.analytics]\nimport-style = 'import sqlty'"),
        "sqlty.toml: unknown key 'import-style' in [tool.sqlty.databases.analytics]; \
             expected one of: dialect, schema, queries, query-format, rules, types, functions, \
             include, exclude"
    );
    assert!(invalid("[databases.a]\ninclude = 'x'").contains("include must be a list"));
    assert!(invalid("[databases.a]\ndialect = 'duck'").starts_with(
        "sqlty.toml: [tool.sqlty.databases.a] unknown dialect 'duck' (did you mean 'duckdb'?)"
    ));
}

#[test]
fn fills_the_globs_of_each_database() {
    let config = validate(
        "[databases.analytics]\ndialect = 'duckdb'\nfunctions = [{ import-path = 'r' }]"
            .parse()
            .unwrap(),
        "sqlty.toml",
    )
    .unwrap();
    let expected: Table = "[databases.analytics]\ndialect = 'duckdb'\ninclude = []\nexclude = []"
        .parse()
        .unwrap();
    assert_eq!(config.table, expected);
    assert_eq!(config.skipped_functions, ["{ import-path = \"r\" }"]);
}

#[test]
fn fills_overrides_and_skips_incomplete_functions() {
    let config = validate(
        "functions = ['junk', { import-path = 'db.run' }, { import-path = 'db.ok', query-param = 'q' }]\n\
             [[overrides]]\ninclude = ['a']"
            .parse()
            .unwrap(),
        "sqlty.toml",
    )
    .unwrap();
    let expected: Table = "functions = [{ import-path = 'db.ok', query-param = 'q' }]\n\
             [[overrides]]\ninclude = ['a']\nexclude = []\nrules = {}"
        .parse()
        .unwrap();
    assert_eq!(config.table, expected);
    assert_eq!(
        config.skipped_functions,
        ["\"junk\"", "{ import-path = \"db.run\" }"]
    );
}

#[test]
fn loads_the_table_of_each_file() {
    let load = |data: &str, name| load(data.as_bytes(), name, name, false);
    let dialect = |config: Option<Config>| {
        config.unwrap().table["dialect"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(
        dialect(load("[tool.sqlty]\ndialect = 'mysql'", "pyproject.toml").unwrap()),
        "mysql"
    );
    assert_eq!(
        dialect(
            load(
                "[workspace.metadata.sqlty]\ndialect = 'sqlite'",
                "Cargo.toml"
            )
            .unwrap()
        ),
        "sqlite"
    );
    assert_eq!(load("[project]\nname = 'x'", "pyproject.toml"), Ok(None));
    assert_eq!(load("[package]\nmetadata = 1", "Cargo.toml"), Ok(None));
    assert!(matches!(
        load("[tool.sqlty", "pyproject.toml"),
        Err(LoadError::Syntax(_))
    ));
    assert!(matches!(
        sqlty_python::config::load(b"\xff", "sqlty.toml", "x", true),
        Err(LoadError::Syntax(_))
    ));
    assert_eq!(
        sqlty_python::config::load(b"", "Cargo.toml", "Cargo.toml", true)
            .unwrap_err()
            .to_string(),
        "Cargo.toml: no [package.metadata.sqlty] or [workspace.metadata.sqlty] table"
    );
}

#[test]
fn rarer_configs() {
    assert!(invalid("query-format = 1").contains("query-format must be a string"));
    assert!(driver_types("psycopg", "mysql").is_empty());
    let loaded = load(
        b"[tool]\nsqlty = 1\n",
        "pyproject.toml",
        "pyproject.toml",
        false,
    );
    assert!(matches!(loaded, Ok(None)));
}
