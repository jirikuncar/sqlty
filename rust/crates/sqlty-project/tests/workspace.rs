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
