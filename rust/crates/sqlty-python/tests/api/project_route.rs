use crate::common::*;
use sqlty_python::project::Read;
use sqlty_python::project::*;

fn routed(path: &str) -> (Route, Table) {
    let table: Table = toml::from_str(
        "queries = ['own']\nrules = {a = 'warn'}\n\
             [databases.logs]\nqueries = 'sql/logs/**/*.sql'\ninclude = ['logs']\nexclude = ['logs/old']\n\
             rules = {b = 'error'}\ndialect = 'duckdb'\n\
             [databases.rest]\ninclude = ['**/*.py']\n\
             [[overrides]]\ninclude = ['tests']\nrules = {a = 'ignore'}\n\
             [[overrides]]\ninclude = []\nexclude = ['tests/keep.py']\nrules = {c = 'warn'}\n",
    )
    .unwrap();
    let files = |_: &Path| Read::Missing;
    let route = route(
        &table,
        Path::new("/p"),
        Path::new(path),
        &files,
        &Excludes::default(),
    );
    let effective = file_table(&table, &route);
    (route, effective)
}

#[test]
fn a_file_takes_its_database_and_overrides() {
    let (route, table) = routed("/p/logs/a.py");
    assert_eq!(route.database.as_deref(), Some("logs"));
    assert_eq!(table["dialect"].as_str(), Some("duckdb"));
    assert_eq!(
        table["rules"].to_string(),
        "{ a = \"warn\", b = \"error\", c = \"warn\" }"
    );
    assert!(!table.contains_key("databases") && !table.contains_key("overrides"));

    assert_eq!(
        routed("/p/sql/logs/x/q.sql").0.database.as_deref(),
        Some("logs")
    );
    assert_eq!(routed("/p/own/q.sql").0.database, None);
    assert_eq!(
        routed("/p/logs/old/a.py").0.database.as_deref(),
        Some("rest")
    );
    assert_eq!(routed("/elsewhere/a.py").0.database, None);
    let (route, table) = routed("/p/tests/keep.py");
    assert_eq!(route.rules.to_string(), "a = \"ignore\"\n");
    assert_eq!(table["rules"].to_string(), "{ a = \"ignore\" }");
    let plain = file_table(&Table::new(), &Route::default());
    assert!(!plain.contains_key("rules"));
}

#[test]
fn queries_name_files_directories_and_globs() {
    let named = |path: &str, queries: &[&str]| {
        let queries: Vec<String> = queries.iter().map(ToString::to_string).collect();
        names_query_file(Path::new(path), Path::new(path), Path::new("/p"), &queries)
    };
    assert!(named("/p/q.sql", &["q.sql"]));
    assert!(named("/p/sql/a/q.sql", &["/sql/"]));
    assert!(named("/p/sql/a/q.sql", &["sql/**"]));
    assert!(!named("/p/sql/a/q.sql", &["sql/*.sql"]));
    assert!(!named("/p/q.py", &["*"]));
    assert!(!named("/p/q.sql", &[]));
    assert!(!named("/x/q.sql", &["*"]));
}
