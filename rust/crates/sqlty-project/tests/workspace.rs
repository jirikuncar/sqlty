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
    open_with(root, false)
}

fn open_with(root: &Path, parse: bool) -> Workspace {
    let options = Options {
        flags: Flags::default(),
        discovery: Discovery::new(Box::new(Disk), root),
        version: (3, 14),
        extra_roots: Vec::new(),
        force_exclude: false,
        parse,
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
fn modules_no_check_can_report_in_are_skipped_but_not_the_rest() {
    let root = project(
        "prefilter",
        &[
            ("pyproject.toml", "[tool.sqlty]\nschema = \"schema.sql\"\n"),
            ("schema.sql", "CREATE TABLE users (id INTEGER NOT NULL);"),
            ("app/__init__.py", ""),
            (
                "app/db.py",
                "from typing import Annotated, LiteralString\n\nfrom sqlty import SQL\n\n\ndef value[T](query: Annotated[LiteralString, SQL], form: type[T]) -> T | None: ...\n",
            ),
            ("app/plain.py", "x = 1\n"),
            ("app/ignored.py", "x = 1  # sqlty: ignore\n"),
            (
                "app/calls.py",
                "from app.db import value as v\n\nfound = v(\"SELECT id FROM users\", str)\n",
            ),
        ],
    );
    let mut rules: Vec<&str> = open_with(&root, true)
        .check_all()
        .expect("checks")
        .iter()
        .flatten()
        .map(|found| found.rule.name())
        .collect();
    fs::remove_dir_all(&root).expect("removes");
    rules.sort_unstable();
    assert_eq!(rules, ["column-type-mismatch", "unused-ignore"]);
}
