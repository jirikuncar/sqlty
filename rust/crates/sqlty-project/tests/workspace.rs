//! The project of a command line's paths or an editor's root: what `Workspace` reads,
//! checks, fixes and reports of its Python files, through the crate's public API.

#![expect(clippy::expect_used, reason = "a failed setup fails the test")]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlty_project::{Disk, FileSettings, Flags, Options, SchemaSource, Shared, Workspace};
use sqlty_python::checker::Diagnostic;
use sqlty_python::project::{normalized, Discovery, Files, Kind, Listing, Read};

struct Tree(BTreeMap<PathBuf, &'static [u8]>);

impl Files for Tree {
    fn read(&self, path: &Path) -> Read {
        self.0
            .get(path)
            .filter(|data| **data != b"<unreadable>")
            .map_or(Read::Missing, |data| Read::Text(data.to_vec()))
    }

    fn list(&self, path: &Path) -> Listing {
        let path = &normalized(path);
        if path.ends_with("unread") {
            return Listing::Unread;
        }
        let mut entries: Vec<(String, Kind)> = Vec::new();
        for file in self.0.keys() {
            let Ok(rest) = file.strip_prefix(path) else {
                continue;
            };
            let mut parts = rest.components();
            let Some(name) = parts.next() else { continue };
            let name = name.as_os_str().to_string_lossy().into_owned();
            let kind = if parts.next().is_some() {
                Kind::Dir
            } else {
                Kind::File
            };
            if !entries.iter().any(|(seen, _)| *seen == name) {
                entries.push((name, kind));
            }
        }
        if entries.is_empty() {
            Listing::Missing
        } else {
            Listing::Entries(entries)
        }
    }

    fn is_link(&self, path: &Path) -> bool {
        path.ends_with("linked.py")
    }
}

const CONFIG: &str = "[tool.sqlty]\nschema = \"schema.sql\"\n\n[[tool.sqlty.functions]]\nimport-path = \"app.db.fetch\"\nquery-param = \"query\"\n";

fn opened(files: &[(&str, &'static str)]) -> Workspace {
    open(files).expect("opens")
}

fn open(files: &[(&str, &'static str)]) -> Result<Workspace, String> {
    let tree = Tree(
        files
            .iter()
            .map(|(path, text)| (PathBuf::from(path), text.as_bytes()))
            .collect(),
    );
    Workspace::root(Path::new("/repo"), Arc::new(tree), (3, 14), Vec::new())
}

fn project() -> Workspace {
    opened(&[
        ("/repo/pyproject.toml", CONFIG),
        ("/repo/schema.sql", "CREATE TABLE users (id INT NOT NULL);"),
        ("/repo/app/__init__.py", ""),
        ("/repo/app/db.py", "def fetch(query: str) -> list: ...\n"),
        (
            "/repo/queries.sql",
            "-- name: one\nSELECT nope FROM users;\n",
        ),
    ])
}

fn rules(found: &[Diagnostic]) -> Vec<&'static str> {
    found.iter().map(|found| found.rule.name()).collect()
}

#[test]
fn checks_a_buffer_against_the_schema_and_its_ignores() {
    let mut workspace = project();
    let path = Path::new("/repo/app/q.py");
    let found = workspace
        .check(path, "q = \"--sql\\nSELECT nope FROM users\"\n")
        .expect("checks");
    assert_eq!(rules(&found), ["unknown-column"]);
    let ignored = "q = \"--sql\\nSELECT nope FROM users\"  # sqlty: ignore[unknown-column]\n";
    assert_eq!(
        rules(&workspace.check(path, ignored).expect("checks")),
        Vec::<&str>::new()
    );
}

#[test]
fn checks_the_calls_of_configured_and_project_helpers() {
    let mut workspace = project();
    let text = "from app.db import fetch\n\nfetch(\"SELECT nope FROM users\")\n";
    let found = workspace
        .check(Path::new("/repo/app/use.py"), text)
        .expect("checks");
    assert_eq!(rules(&found), ["unknown-column"]);
    assert_eq!(
        workspace
            .completion(Path::new("/repo/app/use.py"))
            .expect("completes")
            .helper_starts,
        [32]
    );
}

#[test]
fn checks_the_calls_of_a_helper_annotated_in_the_project() {
    let mut workspace = opened(&[
        ("/repo/pyproject.toml", "[tool.sqlty]\nschema = \"schema.sql\"\n"),
        ("/repo/schema.sql", "CREATE TABLE users (id INT NOT NULL);"),
        (
            "/repo/db.py",
            "from typing import Annotated, LiteralString\nfrom sqlty import SQL\n\ndef run(query: Annotated[LiteralString, SQL]) -> None: ...\n",
        ),
    ]);
    let found = workspace
        .check(
            Path::new("/repo/use.py"),
            "from db import run\n\nrun(\"SELECT nope FROM users\")\n",
        )
        .expect("checks");
    assert_eq!(rules(&found), ["unknown-column"]);
}

/// `Tree`, where each path in `targets` is a link to the file it names.
struct Links {
    tree: Tree,
    targets: BTreeMap<PathBuf, PathBuf>,
}

impl Files for Links {
    fn read(&self, path: &Path) -> Read {
        self.tree.read(path)
    }

    fn list(&self, path: &Path) -> Listing {
        self.tree.list(path)
    }

    fn canonical(&self, path: &Path) -> PathBuf {
        self.targets
            .get(path)
            .cloned()
            .unwrap_or_else(|| normalized(path))
    }

    fn is_link(&self, path: &Path) -> bool {
        self.targets.contains_key(path)
    }
}

#[test]
fn a_linked_file_is_checked_with_the_configuration_of_its_target() {
    let tree = Tree(BTreeMap::from([
        (
            PathBuf::from("/repo/pyproject.toml"),
            b"[tool.sqlty]\n".as_slice(),
        ),
        (
            PathBuf::from("/repo/lib/pyproject.toml"),
            b"[tool.sqlty]\ndialect = \"sqlite\"\n".as_slice(),
        ),
        (PathBuf::from("/repo/app/linked.py"), b"x = 1\n".as_slice()),
        (PathBuf::from("/repo/lib/real.py"), b"x = 1\n".as_slice()),
        (PathBuf::from("/repo/app/plain.py"), b"y = 1\n".as_slice()),
    ]));
    let links = Links {
        tree,
        targets: BTreeMap::from([(
            PathBuf::from("/repo/app/linked.py"),
            PathBuf::from("/repo/lib/real.py"),
        )]),
    };
    let mut workspace =
        Workspace::root(Path::new("/repo"), Arc::new(links), (3, 14), Vec::new()).expect("opens");
    assert_eq!(
        workspace
            .settings(Path::new("/repo/app/linked.py"))
            .expect("settings")
            .dialect,
        "sqlite"
    );
    assert_eq!(
        workspace
            .settings(Path::new("/repo/app/plain.py"))
            .expect("settings")
            .dialect,
        "postgres"
    );
}

#[test]
fn the_root_directory_has_settings_of_its_own() {
    let mut workspace = project();
    assert_eq!(
        workspace
            .settings(Path::new("/"))
            .expect("settings")
            .dialect,
        "postgres"
    );
}

#[test]
fn checks_only_the_sql_files_queries_names() {
    let mut workspace = project();
    let text = "-- name: one\nSELECT nope FROM users;\n";
    assert_eq!(
        rules(
            &workspace
                .check(Path::new("/repo/queries.sql"), text)
                .expect("checks")
        ),
        Vec::<&str>::new()
    );
    let mut named = opened(&[
        (
            "/repo/pyproject.toml",
            "[tool.sqlty]\nschema = \"schema.sql\"\nqueries = [\"queries.sql\"]\n",
        ),
        ("/repo/schema.sql", "CREATE TABLE users (id INT NOT NULL);"),
    ]);
    let found = named
        .check(Path::new("/repo/queries.sql"), text)
        .expect("checks");
    assert_eq!(rules(&found), ["unknown-column"]);
}

#[test]
fn fixes_rewrites_and_strips() {
    let mut workspace = project();
    let path = Path::new("/repo/app/q.py");
    let text = "q = \"--sql\\nSELECT id FROM users\"\n";
    let fixed = workspace.fix(path, text).expect("fixes");
    assert!(
        fixed.ends_with("q = SQL[tuple[int]](\"--sql\\nSELECT id FROM users\")\n"),
        "{fixed}"
    );
    assert_eq!(workspace.rewrite(path, text).expect("rewrites"), fixed);
    assert_eq!(workspace.strip(path, &fixed).expect("strips"), text);
}

#[test]
fn reports_an_invalid_config() {
    let error = open(&[("/repo/pyproject.toml", "[tool.sqlty]\ndialect = \"nope\"\n")]).err();
    assert!(error.is_some_and(|error| error.contains("unknown dialect 'nope'")));
    for config in [
        "[tool.sqlty]\nuntyped = \"nope\"\n",
        "[tool.sqlty]\nuntyped = true\n",
    ] {
        let error = open(&[("/repo/pyproject.toml", config)]).err();
        assert!(error.is_some_and(|error| error.ends_with("must be 'skip' or 'sentinel'")));
    }
    let mut excluded = opened(&[("/repo/.venv/sqlty.toml", "dialect = \"nope\"\n")]);
    let error = excluded
        .check(Path::new("/repo/.venv/q.py"), "x = 1\n")
        .err();
    assert!(error.is_some_and(|error| error.contains("unknown dialect 'nope'")));
}

#[test]
fn routes_files_to_databases_and_overrides() {
    const CONFIG: &str = "[project]\nrequires-python = \">=3.15\"\n\n[tool.sqlty]\nschema = \"app.sql\"\n\n[tool.sqlty.databases.analytics]\ndialect = \"sqlite\"\nschema = \"analytics.sql\"\ninclude = [\"jobs/**\"]\n\n[tool.sqlty.databases.lake]\ndialect = \"duckdb\"\n\n[[tool.sqlty.overrides]]\ninclude = [\"legacy/**\"]\nrules = { unknown-column = \"ignore\" }\n";
    let mut workspace = opened(&[
        ("/repo/pyproject.toml", CONFIG),
        ("/repo/app.sql", "CREATE TABLE users (id TEXT NOT NULL);"),
        (
            "/repo/analytics.sql",
            "CREATE TABLE events (n INTEGER NOT NULL);",
        ),
    ]);
    let events = "q = \"--sql\\nSELECT nope FROM events\"\n";
    let found = workspace
        .check(Path::new("/repo/jobs/q.py"), events)
        .expect("checks");
    assert_eq!(rules(&found), ["unknown-column"]);
    assert_eq!(
        workspace
            .settings(Path::new("/repo/jobs/q.py"))
            .expect("settings")
            .dialect,
        "sqlite"
    );
    let users = "q = \"--sql\\nSELECT nope FROM users\"\n";
    assert_eq!(
        rules(
            &workspace
                .check(Path::new("/repo/app/q.py"), users)
                .expect("checks")
        ),
        ["unknown-column"]
    );
    let ignored = workspace
        .check(Path::new("/repo/legacy/q.py"), users)
        .expect("checks");
    assert_eq!(rules(&ignored), Vec::<&str>::new());
    assert!(
        workspace
            .settings(Path::new("/repo/q.py"))
            .expect("settings")
            .closed_params
    );
    assert_eq!(
        workspace.warnings(),
        ["Table and column names are not checked for duckdb"]
    );
}

#[test]
fn schema_sources_are_each_configuration_once_with_the_overrides_of_their_files() {
    const CONFIG: &str = "[tool.sqlty]\nschema = \"db/\"\n\
        [tool.sqlty.databases.events]\nschema = \"events/\"\ninclude = [\"db/**\"]\n\
        [[tool.sqlty.overrides]]\ninclude = [\"db/**\"]\nrules = { missing-concurrently = \"error\" }\n";
    let files = [
        ("/repo/pyproject.toml", CONFIG),
        ("/repo/db/0001.sql", "CREATE TABLE users (id INT);"),
        ("/repo/events/0001.sql", "CREATE TABLE events (id INT);"),
        ("/repo/app/a.py", ""),
        ("/repo/jobs/j.py", ""),
    ];
    let mut workspace = opened(&files);
    let sources = workspace.schema_sources().expect("reads");
    let named: Vec<_> = sources
        .iter()
        .map(|source| (source.directory.clone(), source.database.as_str()))
        .collect();
    let repo = PathBuf::from("/repo");
    assert_eq!(named, [(repo.clone(), "default"), (repo, "events")]);
    let rule = |source: &SchemaSource| source.files[0].rules.get("missing-concurrently").cloned();
    assert_eq!(rule(&sources[0]), Some("error".to_string()));
    assert_eq!(rule(&sources[1]), None);
    assert_eq!(
        sources[1].files[0].schema_files,
        sources[1].settings.schema_files
    );
    assert!(Arc::ptr_eq(&sources[1].files[0], &sources[1].settings));
    assert!(workspace
        .database(Path::new("/repo/app/_"), "events")
        .expect("reads")
        .is_some_and(|events| Arc::ptr_eq(&events, &sources[1].settings)));
    assert!(workspace
        .database(Path::new("/repo/app/_"), "nope")
        .expect("reads")
        .is_none());

    let plain = [
        ("/repo/pyproject.toml", "[tool.sqlty]\nschema = \"db/\"\n"),
        files[1],
        files[3],
    ];
    let sources = opened(&plain).schema_sources().expect("reads");
    assert!(Arc::ptr_eq(&sources[0].files[0], &sources[0].settings));
}

#[test]
fn queries_of_another_dialect_are_counted_and_typed() {
    const CONFIG: &str = "[project]\nrequires-python = \">=3.15\"\n\n[tool.sqlty]\nschema = \"app.sql\"\n\n[tool.sqlty.databases.lite]\ndialect = \"sqlite\"\nschema = \"lite.sql\"\n\n[tool.sqlty.databases.copy]\ninclude = [\"copy/**\"]\n";
    let mut workspace = opened(&[
        ("/repo/pyproject.toml", CONFIG),
        ("/repo/app.sql", "CREATE TABLE users (id INT NOT NULL);"),
        ("/repo/lite.sql", "CREATE TABLE events (n INTEGER NOT NULL);"),
        (
            "/repo/q.py",
            "q = \"--sql sqlty: db=lite\\nSELECT n FROM events\"\nu = \"--sql sqlty: db=lite\\nSELECT nope FROM events\"\n",
        ),
    ]);
    let schemas: Vec<_> = workspace
        .databases(Path::new("/repo/q.py"))
        .expect("reads")
        .into_iter()
        .map(|(_, settings)| settings.schema.clone().expect("loaded"))
        .collect();
    assert_eq!(schemas.len(), 3);
    assert!(schemas[2].shares_data(&schemas[0]) && !schemas[1].shares_data(&schemas[0]));
    let counts = workspace.statistics_all().expect("counts");
    assert_eq!(counts[0].get("typed"), Some(&1));
    assert_eq!(counts[0].get("partially typed"), Some(&1));

    let sql = "-- name: add\n-- sqlty: db=lite\nINSERT INTO events (n) VALUES (1);\n\n-- name: all\n-- sqlty: db=lite\nSELECT n FROM events;\n\n-- name: odd\n-- sqlty: db=lite\nSELECT nope FROM events;\n";
    let listed = workspace
        .query_types(Path::new("sql/lite.sql"), sql)
        .expect("types");
    assert_eq!(
        listed,
        ["add() -> None", "all() -> tuple[int]", "odd() -> unknown"]
    );
}

#[test]
fn warns_once_about_what_the_schemas_do_not_read() {
    let workspace = opened(&[
        (
            "/repo/pyproject.toml",
            "[tool.sqlty]\nschema = [\"schema.sql\", \"gone.sql\", \"none/*.sql\"]\n\
             types = { DATE = \"str\" }\n\
             [tool.sqlty.databases.logs]\ntypes = { \"t.c\" = \"int\" }\n",
        ),
        (
            "/repo/schema.sql",
            "CREATE VIEW bad (a) AS SELECT id FROM (;\nCREATE TYPE mood AS ENUM ('a') FOO;",
        ),
    ]);
    assert_eq!(
        workspace.warnings(),
        [
            "Schema pattern matches no files: none/*.sql",
            "Schema file not found: /repo/gone.sql",
            "Schema view bad could not be parsed; its columns are unknown",
            "Schema statement could not be parsed and is ignored: CREATE TYPE mood AS ENUM ('a') FOO",
        ]
    );
}

#[test]
fn reads_max_query_variants_with_preview() {
    let mut workspace = opened(&[(
        "/repo/pyproject.toml",
        "[tool.sqlty]\npreview = true\nmax-query-variants = 5\n",
    )]);
    let settings = workspace
        .settings(Path::new("/repo/a.py"))
        .expect("settings");
    assert_eq!(settings.variants, Some(5));
}

#[test]
fn warns_about_a_schema_too_large_to_load() {
    let columns: Vec<String> = (0..=4096).map(|i| format!("c{i} int")).collect();
    let schema = format!("CREATE TABLE wide ({});", columns.join(", "));
    let workspace = opened(&[
        (
            "/repo/pyproject.toml",
            "[tool.sqlty]\nschema = \"schema.sql\"\n",
        ),
        ("/repo/schema.sql", schema.leak()),
    ]);
    assert_eq!(
        workspace.warnings(),
        ["Schema is too large to load; ignored: public.wide"]
    );
}

#[test]
fn reads_the_project_without_a_configuration() {
    let mut workspace = opened(&[
        ("/repo/pyproject.toml", "[tool"),
        ("/repo/broken.py", "x = (\n"),
        ("/repo/gone.py", "<unreadable>"),
    ]);
    let path = Path::new("/repo/broken.py");
    let found = workspace
        .check(path, "q = \"--sql\\nSELECT 1\"  # sqlty: ignore\nx = (\n")
        .expect("checks");
    assert_eq!(rules(&found), Vec::<&str>::new());
    let completion = workspace.completion(path).expect("completes");
    assert_eq!(completion.helper_starts, Vec::<usize>::new());
    assert_eq!(completion.dialect, "postgres");
}

#[test]
fn writes_the_types_the_settings_ask_for() {
    let mut workspace = opened(&[
        (
            "/repo/pyproject.toml",
            "[tool.sqlty]\nschema = \"schema.sql\"\nuntyped = \"sentinel\"\n\
             json-types = \"typing.TypedDict\"\ndriver = \"psycopg\"\n\
             types = { \"users.id\" = \"app.UserId\" }\nrules = { sqruff = \"warn\" }\n",
        ),
        (
            "/repo/schema.sql",
            "CREATE TABLE users (id INT NOT NULL, meta JSON NOT NULL);",
        ),
    ]);
    let path = Path::new("/repo/q.py");
    let text =
        "q = \"--sql\\nSELECT id, json_build_object('a', 1) AS m FROM users WHERE id = %(id)s\"\n";
    let rewritten = workspace.rewrite(path, text).expect("rewrites");
    assert!(rewritten.contains("app.UserId"), "{rewritten}");
    assert!(rewritten.contains("TypedDict"), "{rewritten}");
    let found = workspace
        .check(path, "q = \"--sql\\nselect distinct(id) from users\"\n")
        .expect("checks");
    assert_eq!(rules(&found), ["stale-rewrite", "sqruff"]);
}

#[test]
fn writes_no_parameter_types_when_turned_off() {
    let mut workspace = opened(&[(
        "/repo/pyproject.toml",
        "[tool.sqlty]\nparam-types = false\n",
    )]);
    let text = "q = \"--sql\\nSELECT 1::INTEGER WHERE 1 = %s\"\n";
    let rewritten = workspace
        .rewrite(Path::new("/repo/q.py"), text)
        .expect("rewrites");
    assert!(rewritten.contains("SQL[tuple[int]]("), "{rewritten}");
}

#[test]
fn reports_invalid_settings() {
    for (config, error) in [
        ("json-types = \"nope\"", "json-types must be 'typing.Any' or 'typing.TypedDict'"),
        ("import-style = \"nope\"", "Unsupported import-style: 'nope'"),
        ("schema = \"schema.sql\"\ntypes = { \"(\" = \"int\" }", "[tool.sqlty.types]: "),
        ("[tool.sqlty.databases.logs]\nschema = \"schema.sql\"\n[tool.sqlty.types]\n\"(\" = \"int\"", "[tool.sqlty.types]: "),
    ] {
        let text: &'static str = Box::leak(format!("[tool.sqlty]\n{config}\n").into_boxed_str());
        let found = open(&[("/repo/pyproject.toml", text), ("/repo/schema.sql", "CREATE TABLE t (a INT);")]);
        let found = found.err().unwrap_or_default();
        assert!(found.contains(error), "{config}: {found}");
    }
}

#[test]
fn merges_the_rules_of_databases_and_overrides() {
    let mut workspace = opened(&[
        (
            "/repo/pyproject.toml",
            "[tool.sqlty]\nrules = { select-star = \"error\" }\n\
             [tool.sqlty.databases.logs]\ndialect = \"sqlite\"\ninclude = [\"logs/**\"]\n\
             rules = { select-star = \"ignore\" }\n",
        ),
        (
            "/repo/sub/pyproject.toml",
            "[tool.sqlty]\n[[tool.sqlty.overrides]]\ninclude = [\"old/**\"]\n",
        ),
    ]);
    let text = "q = \"--sql\\nSELECT * FROM t\"\n";
    let logs = workspace
        .check(Path::new("/repo/logs/q.py"), text)
        .expect("checks");
    assert_eq!(rules(&logs), Vec::<&str>::new());
    let old = workspace
        .check(Path::new("/repo/sub/old/q.py"), text)
        .expect("checks");
    assert_eq!(rules(&old), Vec::<&str>::new());
}

#[test]
fn registers_helpers_found_on_the_extra_roots() {
    let tree = Tree(BTreeMap::from([
        (
            PathBuf::from("/repo/pyproject.toml"),
            b"[tool.sqlty]\n[[tool.sqlty.functions]]\nimport-path = \"lib.fetch\"\nquery-param = \"query\"\n".as_slice(),
        ),
        (PathBuf::from("/site/lib.py"), b"def fetch(query: str) -> None: ...\n".as_slice()),
    ]));
    let mut workspace = Workspace::root(
        Path::new("/repo"),
        Arc::new(tree),
        (3, 14),
        vec![PathBuf::from("/site")],
    )
    .expect("opens");
    let text = "from lib import fetch\n\nfetch(\"SELEC 1\")\n";
    let found = workspace
        .check(Path::new("/repo/q.py"), text)
        .expect("checks");
    assert_eq!(rules(&found), ["unparsed-query"]);
}

fn disk_project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sqlty-project-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for (path, text) in files {
        let path = root.join(path);
        fs::create_dir_all(path.parent().expect("has a parent")).expect("creates");
        fs::write(path, text).expect("writes");
    }
    root
}

fn open_disk(root: &Path) -> Workspace {
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
    let root = disk_project(
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
    let sources = open_disk(&root).schema_sources().expect("reads");
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
    let root = disk_project(
        "schema-checks",
        &[
            ("sqlty.toml", "schema = \"schema.sql\"\n"),
            ("schema.sql", schema),
        ],
    );
    let mut workspace = open_disk(&root);
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
    let root = disk_project(
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
    let mut workspace = open_disk(&root);
    let all = found(&mut workspace);
    let mut app = open_disk(&root.join("app"));
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

#[test]
fn files_that_match_the_same_overrides_share_their_settings() {
    let root = disk_project(
        "shared-routes",
        &[
            (
                "pyproject.toml",
                "[tool.sqlty]\n\n[tool.sqlty.databases.logs]\ninclude = [\"logs\"]\n\n[[tool.sqlty.overrides]]\ninclude = [\"app/tests\", \"logs/tests\"]\nrules = { select-star = \"ignore\" }\n",
            ),
            ("app/a.py", ""),
            ("app/b.py", ""),
            ("app/tests/c.py", ""),
            ("app/tests/d.py", ""),
            ("logs/e.py", ""),
            ("logs/tests/f.py", ""),
        ],
    );
    let mut workspace = open_disk(&root);
    let [a, b, c, d, e, f] = [
        "app/a.py",
        "app/b.py",
        "app/tests/c.py",
        "app/tests/d.py",
        "logs/e.py",
        "logs/tests/f.py",
    ]
    .map(|path| workspace.settings(&root.join(path)).expect("settings"));
    fs::remove_dir_all(&root).expect("removes");
    assert!(Arc::ptr_eq(&a, &b) && Arc::ptr_eq(&c, &d));
    assert!(!Arc::ptr_eq(&a, &c) && !Arc::ptr_eq(&a, &e) && !Arc::ptr_eq(&e, &f));
    let ignored = |settings: &Arc<FileSettings>| {
        settings
            .rules
            .get("select-star")
            .is_some_and(|level| level == "ignore")
    };
    assert_eq!([&a, &c, &e, &f].map(ignored), [false, true, false, true]);
}
