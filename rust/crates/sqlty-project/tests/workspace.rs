//! The workspace a host opens, read through its public API.

#![expect(
    clippy::expect_used,
    reason = "a test fails on the first unexpected error"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlty_project::{Disk, Flags, Options, Shared, Workspace};
use sqlty_python::project::Discovery;

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sqlty-project-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for (path, text) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().expect("has a parent")).expect("creates");
        fs::write(path, text).expect("writes");
    }
    root
}

fn open(root: &Path) -> Workspace {
    let options = Options {
        flags: Flags::default(),
        discovery: Discovery::new(Box::new(Disk), root),
        version: (3, 14),
        extra_roots: Vec::new(),
        force_exclude: false,
        parse: false,
    };
    Workspace::open(&[root.to_path_buf()], Shared(Arc::new(Disk)), options).expect("opens")
}

#[test]
fn a_database_without_schema_files_has_no_schema_source() {
    let root = project(
        "schema-sources",
        &[
            (
                "pyproject.toml",
                "[tool.sqlty]\nschema = \"db/\"\n\n[tool.sqlty.databases.cache]\ndialect = \"sqlite\"\nschema = []\n",
            ),
            ("db/0001.sql", "CREATE TABLE users (id INT);"),
            ("app/a.py", ""),
        ],
    );
    let sources = open(&root).schema_sources().expect("reads");
    let databases: Vec<_> = sources
        .iter()
        .map(|source| source.database.as_str())
        .collect();
    fs::remove_dir_all(&root).expect("removes");
    assert_eq!(databases, ["default"]);
}

#[test]
fn a_schema_buffer_has_its_tenant_policies_while_it_matches_the_file() {
    let schema = "CREATE TABLE notes (id INT, org_id INT);\nALTER TABLE notes ENABLE ROW LEVEL SECURITY;\nCREATE POLICY t ON notes USING (org_id = current_setting('app.org')::int);\n";
    let root = project(
        "schema-checks",
        &[
            ("sqlty.toml", "schema = \"schema.sql\"\n"),
            ("schema.sql", schema),
        ],
    );
    let mut workspace = open(&root);
    let path = root.join("schema.sql");
    let rules = |text: &str, workspace: &mut Workspace| -> Vec<(String, usize)> {
        let found = workspace.check(&path, text).expect("checks");
        found
            .into_iter()
            .map(|found| (found.rule.name().to_string(), found.line))
            .collect()
    };
    let saved = rules(schema, &mut workspace);
    let edited = rules(&format!("\n{schema}"), &mut workspace);
    fs::remove_dir_all(&root).expect("removes");
    assert_eq!(saved, [("tenant-policy".to_string(), 2)]);
    assert_eq!(edited, []);
}

#[test]
fn schema_diagnostics_are_those_of_the_schema_files_under_the_paths() {
    let rls = "ALTER TABLE notes ENABLE ROW LEVEL SECURITY;\nCREATE POLICY t ON notes USING (org_id = current_setting('app.org')::int);\n";
    let root = project(
        "schema-diagnostics",
        &[
            (
                "sqlty.toml",
                "schema = \"db/\"\n\n[databases.cache]\ndialect = \"sqlite\"\nschema = \"cache.sql\"\n\n[databases.none]\nschema = \"missing.sql\"\n",
            ),
            ("db/1.sql", "CREATE TABLE notes (id INT, org_id INT);\n"),
            ("db/2.sql", rls),
            ("cache.sql", "CREATE TABLE c (id INT);\n"),
            ("app/a.py", ""),
        ],
    );
    let found = |workspace: &mut Workspace| -> Vec<(PathBuf, usize)> {
        let found = workspace.schema_diagnostics().expect("checks");
        found
            .into_iter()
            .map(|(path, diagnostics, _)| (path, diagnostics.len()))
            .collect()
    };
    let mut workspace = open(&root);
    let all = found(&mut workspace);
    let mut app = open(&root.join("app"));
    let none = found(&mut app);
    fs::write(root.join("db/2.sql"), format!("{rls}\n")).expect("writes");
    let changed = found(&mut workspace);
    fs::remove_dir_all(&root).expect("removes");
    let db = fs::canonicalize(std::env::temp_dir())
        .expect("resolves")
        .join(root.file_name().expect("named"))
        .join("db");
    assert_eq!(all, [(db.join("1.sql"), 0), (db.join("2.sql"), 1)]);
    assert_eq!(none, []);
    assert_eq!(changed, []);
}
