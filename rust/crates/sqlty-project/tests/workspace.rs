//! The project of a command line's paths or an editor's root: what `Workspace` reads,
//! checks, fixes and reports of its Python files, through the crate's public API.

#![expect(clippy::expect_used, reason = "a failed setup fails the test")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlty_project::{par_map, Configs, Flags, Options, SchemaSource, Shared, Workspace};
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
    const CONFIG: &str = "[project]\nrequires-python = \">=3.15\"\n\n[tool.sqlty]\nschema = \"app.sql\"\n\n[tool.sqlty.databases.lite]\ndialect = \"sqlite\"\nschema = \"lite.sql\"\n";
    let mut workspace = opened(&[
        ("/repo/pyproject.toml", CONFIG),
        ("/repo/app.sql", "CREATE TABLE users (id INT NOT NULL);"),
        ("/repo/lite.sql", "CREATE TABLE events (n INTEGER NOT NULL);"),
        (
            "/repo/q.py",
            "q = \"--sql sqlty: db=lite\\nSELECT n FROM events\"\nu = \"--sql sqlty: db=lite\\nSELECT nope FROM events\"\n",
        ),
    ]);
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

fn tree(files: &[(&str, &'static [u8])]) -> Shared {
    let tree = files
        .iter()
        .map(|(path, data)| (PathBuf::from(path), *data))
        .collect();
    Shared(Arc::new(Tree(tree)))
}

fn command(files: &Shared, paths: &[&str], flags: Flags) -> Result<Workspace, String> {
    let mut discovery = Discovery::new(Box::new(files.clone()), &flags.cwd);
    if let Some(fixed) = flags.fixed_discovery() {
        discovery = discovery.fixed(fixed);
    }
    if matches!(flags.configs, Configs::Isolated) {
        discovery = discovery.isolated();
    }
    let options = Options {
        discovery,
        flags,
        version: (3, 14),
        extra_roots: Vec::new(),
        force_exclude: true,
        parse: true,
    };
    let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
    Workspace::open(&paths, files.clone(), options)
}

fn flags(files: &Shared, config_file: Option<&str>, assignments: &[&str]) -> Flags {
    let assignments: Vec<String> = assignments.iter().map(ToString::to_string).collect();
    let severities = vec![("select-star".to_string(), "error".to_string())];
    Flags::new(
        Path::new("/repo"),
        config_file.map(Path::new),
        false,
        &assignments,
        severities,
        files,
    )
    .expect("flags")
}

const COMMAND: &[(&str, &[u8])] = &[
    (
        "/repo/pyproject.toml",
        b"[tool.sqlty]\nschema = \"schema.sql\"\nqueries = [\"sql/*.sql\"]\nextend-exclude = [\"skip\"]\n\n[[tool.sqlty.functions]]\nimport-path = \"app.db.fetch\"\nquery-param = \"query\"\n",
    ),
    ("/repo/app/db.py", b"def fetch(query: str) -> None: ...\n"),
    ("/repo/app/d.py", b"from app.db import fetch\n\nfetch(\"SELECT id FROM users\")\nq = \"--sql\\nSELEC 1\"\nr = \"--sql\\nUPDATE users SET name = NULL\"\n"),
    ("/repo/sub/pyproject.toml", b"[tool.sqlty]\n"),
    ("/repo/schema.sql", b"CREATE TABLE users (id INT NOT NULL, name TEXT);"),
    ("/repo/sql/users.sql", b"-- name: by_id\nSELECT id, name FROM users WHERE id = :id;\n-- name: count\nSELECT count(*) FROM users WHERE id = $1;\n-- name: touch\nUPDATE users SET name = NULL;\n-- name: by_user\nSELECT name FROM users WHERE id = :user.id;\n"),
    ("/repo/sql/notes.txt", b""),
    ("/repo/app/a.py", b"\xef\xbb\xbfq = \"--sql\\nSELECT nope FROM users\"\r\nr = \"--sql\\nSELECT *, id FROM users\"\r\nt = f\"--sql\\nSELECT {1}\"\nu = \"--sql\\nSELECT sum(id) AS total FROM users\"\n"),
    ("/repo/app/b.py", b"from app.a import q\nx = (\n"),
    ("/repo/app/c.py", b"<unreadable>"),
    ("/repo/app/start.py", b"q = \"\x80\""),
    ("/repo/app/continued.py", b"q = \"\xc3(\""),
    ("/repo/app/ended.py", b"q = \"\xc3"),
    ("/repo/app/notes.md", b""),
    ("/repo/skip/s.py", b"q = \"--sql\\nSELECT nope FROM users\"\n"),
];

#[test]
fn reads_the_paths_of_a_command_line() {
    let files = tree(COMMAND);
    let flags = flags(&files, None, &[]);
    let paths = [
        "app",
        "pyproject.toml",
        "sub/pyproject.toml",
        "app/notes.md",
        "app",
        "app/a.py",
        "skip/s.py",
    ];
    let mut workspace = command(&files, &paths, flags).expect("opens");
    let read = ["app/a.py", "app/b.py", "app/d.py", "app/db.py"];
    assert_eq!(workspace.files(), read.map(PathBuf::from));
    let unreadable: Vec<String> = workspace
        .unreadable()
        .iter()
        .map(|(path, why)| format!("{}: {why}", path.display()))
        .collect();
    assert_eq!(
        unreadable,
        [
            "app/c.py: unreadable",
            "app/continued.py: invalid continuation byte at byte 5",
            "app/ended.py: unexpected end of data at byte 5",
            "app/start.py: invalid start byte at byte 5",
        ]
    );
    let text = workspace
        .text(Path::new("app/a.py"))
        .expect("read")
        .to_string();
    assert!(text.starts_with("q = ") && !text.contains('\r'), "{text:?}");
    workspace.update(Path::new("app/a.py"), &text);
    let found: Vec<_> = workspace
        .check_all()
        .expect("checks")
        .iter()
        .map(|found| rules(found))
        .collect();
    let a = vec![
        "unknown-column",
        "stale-rewrite",
        "fstring-skipped",
        "stale-rewrite",
    ];
    assert_eq!(found, [a, vec![], vec!["unparsed-query"], vec![]]);
    let fixed = workspace.fix_all().expect("fixes");
    assert!(fixed[0].contains("SQL["), "{}", fixed[0]);
    let counts: Vec<Vec<_>> = workspace
        .statistics_all()
        .expect("counts")
        .into_iter()
        .map(|counts| {
            counts
                .into_iter()
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect()
        })
        .collect();
    assert_eq!(
        counts,
        [
            vec![("fstring-skipped", 1), ("partially typed", 1), ("typed", 2)],
            vec![],
            vec![("typed", 1), ("unparsed", 1)],
            vec![],
        ]
    );
    assert_eq!(
        workspace.query_files().expect("lists"),
        [PathBuf::from("sql/users.sql")]
    );
    let explained = workspace
        .explain(Path::new("app/a.py"), 9)
        .expect("explains");
    assert_eq!(
        explained[0],
        "  total: type and nullability by inference rule postgres.aggregate.sum-integers"
    );
    assert_eq!(
        workspace
            .explain(Path::new("app/a.py"), 0)
            .expect("explains"),
        Vec::<String>::new()
    );
    assert_eq!(
        workspace
            .explain(Path::new("app/z.py"), 9)
            .expect("explains"),
        Vec::<String>::new()
    );
    let queries = files.text(Path::new("/repo/sql/users.sql")).expect("text");
    assert_eq!(
        workspace
            .query_types(Path::new("sql/users.sql"), &queries)
            .expect("types"),
        [
            "by_id(id: int) -> tuple[int, str | None]",
            "count($1: int) -> tuple[int]",
            "touch() -> None",
            "by_user(user.id: int) -> tuple[str | None]",
        ]
    );
    assert_eq!(
        workspace
            .format(
                Path::new("app/a.py"),
                "q = \"\"\"--sql\nselect id from users where id=%s\"\"\"\n"
            )
            .expect("formats"),
        "q = \"\"\"--sql\nselect id from users\nwhere id = %s\"\"\"\n"
    );
}

#[test]
fn writes_a_module_as_the_commands_do() {
    let files = tree(COMMAND);
    let mut workspace = command(&files, &["app/b.py"], flags(&files, None, &[])).expect("opens");
    let settings = workspace.settings(Path::new("app/b.py")).expect("settings");
    let text = "q = \"--sql\\nSELECT id FROM users\"\nt = f\"--sql\\nSELECT {1}\"\n";
    let rewritten = sqlty_project::rewritten(text, &settings);
    assert!(rewritten.contains("q = SQL[tuple[int]]("), "{rewritten}");
    assert_eq!(sqlty_project::stripped(&rewritten, &settings), text);
    let formatted = sqlty_project::formatted(
        "q = \"\"\"--sql\nselect 1 from t where b=%s\"\"\"\n",
        &settings,
    );
    assert_eq!(
        formatted,
        "q = \"\"\"--sql\nselect 1 from t\nwhere b = %s\"\"\"\n"
    );
    let skipped = sqlty_project::skipped_strings(text, &settings);
    assert_eq!(rules(&skipped), ["fstring-skipped"]);
    assert_eq!(
        workspace.query_files().expect("lists"),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn reports_what_the_host_has_not_read() {
    let files = tree(&[
        (
            "/repo/pyproject.toml",
            b"[tool.sqlty]\nschema = \"unread\"\n",
        ),
        ("/repo/unread/x.sql", b""),
    ]);
    let error = command(&files, &["pyproject.toml"], flags(&files, None, &[])).err();
    assert_eq!(error.as_deref(), Some("/repo/unread: not read"));
    let files = tree(&[
        (
            "/repo/pyproject.toml",
            b"[tool.sqlty]\nqueries = \"unread\"\n",
        ),
        ("/repo/q.py", b""),
    ]);
    let mut workspace = command(&files, &["q.py"], flags(&files, None, &[])).expect("opens");
    assert_eq!(
        workspace.query_files().err().as_deref(),
        Some("/repo/unread: not read")
    );
    let files = tree(&[("/repo/app/unread/x.py", b"")]);
    let error = command(&files, &["app"], flags(&files, None, &[])).err();
    assert_eq!(error.as_deref(), Some("/repo/app/unread: not read"));
}

#[test]
fn keeps_what_no_module_refutes() {
    let files = tree(&[
        (
            "/repo/pyproject.toml",
            b"[tool.sqlty]\nschema = \"schema.sql\"\nrules = { redundant-optional = \"warn\" }\n",
        ),
        ("/repo/schema.sql", b"CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL);"),
        (
            "/repo/q.py",
            b"from typing import TypedDict\n\nfrom sqlty import SQL\n\n\nclass User(TypedDict):\n    id: int\n    name: str | None\n\n\nq = SQL[User](\"--sql\\nSELECT id, name FROM users\")\n",
        ),
    ]);
    let mut workspace = command(&files, &["q.py"], flags(&files, None, &[])).expect("opens");
    assert_eq!(
        rules(&workspace.check_all().expect("checks")[0]),
        ["redundant-optional"]
    );
}

#[test]
fn reports_paths_it_cannot_read() {
    let files = tree(COMMAND);
    let error = command(&files, &["gone.py"], flags(&files, None, &[])).err();
    assert_eq!(error.as_deref(), Some("path does not exist: gone.py"));
    let files = tree(&[
        (
            "/repo/pyproject.toml",
            b"[tool.sqlty]\nschema = \"none/*.sql\"\nqueries = [\"none/*.sql\"]\n",
        ),
        ("/repo/q.sql", b""),
        ("/repo/sub/q.py", b""),
    ]);
    let mut workspace = command(&files, &["."], flags(&files, None, &[])).expect("opens");
    let warning = "Schema pattern matches no files: none/*.sql";
    assert_eq!(workspace.warnings(), [warning]);
    assert_eq!(
        workspace.query_files().expect("lists"),
        Vec::<PathBuf>::new()
    );
    assert_eq!(workspace.warnings(), Vec::<String>::new());
}

#[test]
fn reads_the_configuration_the_flags_name() {
    let files = tree(&[
        (
            "/repo/pyproject.toml",
            b"[tool.sqlty]\ndialect = \"nope\"\n",
        ),
        (
            "/repo/ci.toml",
            b"dialect = \"mysql\"\nexclude = [\"gen\"]\nschema = \"schema.sql\"\n[databases.logs]\ninclude = [\"logs/**\"]\n",
        ),
        ("/repo/schema.sql", b"CREATE TABLE t (a INT);"),
        ("/repo/q.py", b"q = \"--sql\\nSELECT * FROM t\"\n"),
    ]);
    let mut flags = flags(
        &files,
        Some("ci.toml"),
        &["rules.unknown-column = \"warn\"", "exclude = [\"x\"]"],
    );
    assert_eq!(
        flags.take_discovery().keys().collect::<Vec<_>>(),
        ["exclude"]
    );
    assert_eq!(
        flags
            .fixed_discovery()
            .expect("fixed")
            .keys()
            .collect::<Vec<_>>(),
        ["exclude"]
    );
    flags.dialect = Some("sqlite".to_string());
    flags.schema = Some(toml::Value::Array(vec![toml::Value::String(
        "schema.sql".to_string(),
    )]));
    let mut workspace = command(&files, &["schema.sql", "q.py"], flags).expect("opens");
    assert_eq!(workspace.files(), [PathBuf::from("q.py")]);
    assert_eq!(
        workspace.query_files().expect("lists"),
        Vec::<PathBuf>::new()
    );
    let settings = workspace.settings(Path::new("q.py")).expect("settings");
    assert_eq!(settings.dialect, "sqlite");
    assert_eq!(
        settings.rules.get("unknown-column").map(String::as_str),
        Some("warn")
    );
    assert_eq!(
        settings.rules.get("select-star").map(String::as_str),
        Some("error")
    );
    let isolated =
        Flags::new(Path::new("/repo"), None, true, &[], Vec::new(), &files).expect("flags");
    assert!(isolated.fixed_discovery().is_none());
    let mut workspace = command(&files, &["q.py", "ci.toml"], isolated).expect("opens");
    assert_eq!(
        workspace
            .settings(Path::new("q.py"))
            .expect("settings")
            .dialect,
        "postgres"
    );
}

#[test]
fn reports_flags_it_cannot_take() {
    let files = tree(&[
        ("/repo/sqlty.toml", b"extend = \"gone.toml\"\n"),
        ("/repo/sub/pyproject.toml", b"[tool"),
        ("/repo/sub/q.py", b"x = 1\n"),
        ("/repo/dir/x.py", b""),
    ]);
    let new = |config: Option<&str>, assignments: &[&str], severities: &[(&str, &str)]| {
        let assignments: Vec<String> = assignments.iter().map(ToString::to_string).collect();
        let severities = severities
            .iter()
            .map(|(rule, severity)| (rule.to_string(), severity.to_string()))
            .collect();
        Flags::new(
            Path::new("/repo"),
            config.map(Path::new),
            false,
            &assignments,
            severities,
            &files,
        )
        .err()
    };
    assert_eq!(
        new(Some("gone.toml"), &[], &[]).as_deref(),
        Some("config file does not exist: gone.toml")
    );
    assert_eq!(
        new(Some("/repo/dir"), &[], &[]).as_deref(),
        Some("config path is not a file: /repo/dir")
    );
    assert_eq!(
        new(Some("sub/pyproject.toml"), &[], &[]).map(|error| error.is_empty()),
        Some(false)
    );
    assert_eq!(
        new(None, &[], &[("select-str", "warn")]).as_deref(),
        Some("--warn: unknown rule 'select-str' (did you mean 'select-star'?); run `sqlty rule` for the list")
    );
    let error = new(None, &["import-style = \"nope\""], &[]);
    assert!(error.is_none());
    let flags = Flags::new(
        Path::new("/repo"),
        None,
        false,
        &["import-style = \"nope\"".to_string()],
        Vec::new(),
        &files,
    )
    .expect("flags");
    let error = command(&files, &["sub/q.py"], flags)
        .err()
        .unwrap_or_default();
    assert!(error.starts_with("--config: "), "{error}");
    let flags =
        Flags::new(Path::new("/repo"), None, false, &[], Vec::new(), &files).expect("flags");
    let mut workspace = command(&files, &["sub/q.py"], flags).expect("opens");
    let warnings = workspace.warnings();
    assert!(warnings[0].starts_with("Failed to parse "), "{warnings:?}");
    let flags =
        Flags::new(Path::new("/repo"), None, false, &[], Vec::new(), &files).expect("flags");
    let listed = command(&files, &["sub/pyproject.toml"], flags).expect("opens");
    assert_eq!(listed.files(), Vec::<PathBuf>::new());
    let error = workspace
        .settings(Path::new("dir/x.py"))
        .err()
        .unwrap_or_default();
    assert!(error.contains("gone.toml"), "{error}");
    let flags =
        Flags::new(Path::new("/repo"), None, false, &[], Vec::new(), &files).expect("flags");
    let error = command(&files, &["dir/x.py"], flags)
        .err()
        .unwrap_or_default();
    assert!(error.contains("gone.toml"), "{error}");
    assert!(workspace.check_all().expect("checks")[0].is_empty());
}

#[test]
fn maps_items_on_all_cores() {
    assert_eq!(par_map(&[1, 2, 3], |item| item * 2), [2, 4, 6]);
}
