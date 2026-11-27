//! The `sqlty` executable where `tests/cli` cannot follow it: without git, and the language server.

#![expect(clippy::expect_used, reason = "a failed setup fails the test")]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("sqlty-exe-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("creates");
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("creates");
        std::fs::write(path, text).expect("writes");
    }
    root
}

fn sqlty(root: &Path, args: &[&str], env: &[(&str, &str)]) -> (i32, String, String) {
    let Output {
        status,
        stdout,
        stderr,
    } = Command::new(env!("CARGO_BIN_EXE_sqlty"))
        .args(args)
        .current_dir(root)
        .env_remove("SQLTY_CONFIG_FILE")
        .env_remove("VIRTUAL_ENV")
        .env_remove("CONDA_PREFIX")
        .envs(env.iter().copied())
        .output()
        .expect("runs");
    let text = |data: Vec<u8>| String::from_utf8(data).expect("UTF-8");
    (status.code().expect("exits"), text(stdout), text(stderr))
}

const SCHEMA: (&str, &str) = ("schema.sql", "CREATE TABLE users (id INT NOT NULL);");
const QUERIES: (&str, &str) = (
    "app/a.py",
    "q = \"--sql\\nSELECT nope FROM users\"\nr = \"--sql\\nSELECT id FROM users\"\n",
);

#[test]
fn checks_against_a_schema_file_without_git() {
    let config = ("pyproject.toml", "[tool.sqlty]\nschema = \"schema.sql\"\n");
    let schema = (
        "schema.sql",
        "CREATE TABLE users (id INT NOT NULL, bio TEXT);",
    );
    let old = ("old.sql", "CREATE TABLE users (id INT NOT NULL);");
    let query = ("app/a.py", "q = \"--sql\\nSELECT bio FROM users\"\n");
    let root = project("breaking", &[config, schema, old, query]);
    let (code, out, err) = sqlty(
        &root,
        &["breaking", "--against", "old.sql"],
        &[("PATH", "/nonexistent")],
    );
    assert_eq!(code, 1, "{out}{err}");
    assert!(
        out.contains("column `bio` does not exist in `users`"),
        "{out}"
    );
    assert_eq!(
        err,
        "sqlty: old.sql has no queries; skipping the backward check\n"
    );
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn keeps_json_output_apart_from_what_fix_reports() {
    let config = ("pyproject.toml", "[tool.sqlty]\nschema = \"schema.sql\"\n");
    let stale = (
        "app/a.py",
        "from sqlty import SQL\nq = SQL[tuple[str]](\"--sql\\nSELECT id FROM users\")\n",
    );
    let root = project("json", &[config, SCHEMA, stale]);
    let args = ["check", "--fix", "-v", "--output-format", "json"];
    assert_eq!(
        sqlty(&root, &args, &[]),
        (0, "[]\n".to_string(), "Fixed app/a.py\n".to_string())
    );
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn writes_ascii_json_and_sarif_uris_for_any_file_name() {
    let config = ("pyproject.toml", "[tool.sqlty]\nschema = \"schema.sql\"\n");
    let query = "q = \"--sql\\nSELECT é😀 FROM users\"\n";
    let root = project("escapes", &[config, SCHEMA, ("a b%.py", query)]);
    let (code, out, _) = sqlty(&root, &["check", "--output-format", "json"], &[]);
    assert_eq!(code, 1, "{out}");
    assert!(
        out.contains(r#""message": "column `\u00e9\ud83d\ude00` does not exist in `users`""#),
        "{out}"
    );
    assert!(out.is_ascii(), "{out}");
    // An absolute path is a file URI with the characters of a URI escaped.
    let absolute = root.join("a b%.py");
    let args = [
        "check",
        "--output-format",
        "sarif",
        absolute.to_str().expect("UTF-8"),
    ];
    let (_, out, _) = sqlty(&root, &args, &[]);
    let uri = format!("\"uri\": \"file://{}/a%20b%25.py\"", root.display());
    assert!(out.contains(&uri), "{uri}\n{out}");
    assert!(!out.contains("\"fixes\""), "{out}");
    std::fs::remove_dir_all(&root).expect("removes");
}

/// A backslash in a file name is a character on Unix, and a separator in a relative URI.
#[cfg(unix)]
#[test]
fn escapes_a_backslash_in_a_file_name_and_writes_it_as_a_slash_in_sarif() {
    let config = ("pyproject.toml", "[tool.sqlty]\nschema = \"schema.sql\"\n");
    let root = project("backslash", &[config, SCHEMA, (r"app\q.py", QUERIES.1)]);
    let (_, out, _) = sqlty(&root, &["check", "--output-format", "json"], &[]);
    assert!(out.contains(r#""file": "app\\q.py""#), "{out}");
    let (_, out, _) = sqlty(&root, &["check", "--output-format", "sarif"], &[]);
    assert!(out.contains(r#""uri": "app/q.py""#), "{out}");
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn walks_as_the_discovery_flags_say() {
    let config = ("pyproject.toml", "[tool.sqlty]\nschema = \"schema.sql\"\n");
    let generated = ("app/gen.py", "q = \"--sql\\nSELECT nope FROM users\"\n");
    let files = [
        config,
        SCHEMA,
        ("app/q.py", "q = \"--sql\\nSELECT id FROM users\"\n"),
        ("app/.gitignore", "gen.py\n"),
        generated,
        // A directory with a `.git` is a repository, whose `.gitignore` files apply.
        ("app/.git/HEAD", ""),
    ];
    let root = project("walk", &files);
    for (args, found) in [
        (&["check", "--respect-gitignore"][..], 0),
        (&["check", "--no-respect-gitignore"][..], 1),
        (&["check", "--exclude", "gen.py"][..], 0),
        (&["check", "-c", "exclude = [\"gen.py\"]"][..], 0),
        (&["check", "-c", "extend-exclude = [\"gen.py\"]"][..], 0),
    ] {
        let (code, out, err) = sqlty(&root, args, &[]);
        assert_eq!(code, found, "{args:?}: {out}{err}");
    }
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn a_diff_marks_a_missing_last_newline_with_a_backslash() {
    let config = ("pyproject.toml", "[tool.sqlty]\nschema = \"schema.sql\"\n");
    let files = [
        config,
        SCHEMA,
        ("q.py", "q = \"--sql\\nSELECT id FROM users\""),
    ];
    let root = project("no-newline", &files);
    let (code, out, _) = sqlty(&root, &["rewrite", "--diff"], &[]);
    assert_eq!(code, 1, "{out}");
    assert_eq!(
        out.matches("\n\\ No newline at end of file\n").count(),
        2,
        "{out}"
    );
    std::fs::remove_dir_all(&root).expect("removes");
}

const OLD_SCHEMA: &str = "CREATE TABLE users (id INT, bio TEXT);\n";
const NEW_SCHEMA: &str = "CREATE TABLE users (id INT, handle TEXT);\n";
const NEW_CODE: &str = "handles = \"--sql\\nSELECT handle FROM users\"\n";
const USERS: &str = "CREATE TABLE users (id INT, bio TEXT);\n";
const EVENTS: &str = "CREATE TABLE events (id INT, at TIMESTAMP);\n";
const INDEX: &str = "SET lock_timeout = '1s';\nCREATE INDEX events_at ON events (at);\n";

const DATABASES: &str = "[tool.sqlty]\nschema = \"migrations/\"\n\
    [tool.sqlty.databases.events]\nschema = \"events/\"\ninclude = [\"events/**\"]\n";
const MIGRATIONS: (&str, &str) = ("pyproject.toml", "[tool.sqlty]\nschema = \"migrations/\"\n");

/// `sqlty breaking` against the snapshot `snapshot.json`; its exit code, output and errors.
fn breaking(root: &Path, args: &[&str]) -> (i32, String, String) {
    let args: Vec<&str> = ["breaking"]
        .into_iter()
        .chain(args.iter().copied())
        .collect();
    sqlty(root, &args, &[])
}

fn save_snapshot(root: &Path) {
    let saved = breaking(root, &["--save", "snapshot.json"]);
    assert_eq!(saved, (0, String::new(), String::new()));
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("creates");
    std::fs::write(path, text).expect("writes");
}

#[test]
fn a_snapshot_reports_only_what_the_schema_of_the_working_tree_breaks() {
    let config = ("pyproject.toml", "[tool.sqlty]\nschema = \"schema.sql\"\n");
    let files = [config, ("schema.sql", NEW_SCHEMA), ("app/q.py", NEW_CODE)];
    let root = project("snapshot", &files);
    save_snapshot(&root);
    let unchanged = breaking(&root, &["--against", "snapshot.json"]);
    assert_eq!(unchanged, (0, String::new(), String::new()));
    write(&root, "schema.sql", OLD_SCHEMA);
    let (code, out, err) = breaking(&root, &["--against", "snapshot.json"]);
    assert_eq!((code, err.as_str()), (1, ""));
    assert_eq!(
        out,
        "app/q.py:1: [unknown-column] column `handle` does not exist in `users` \
         (backward: the queries of snapshot.json against the schema of the working tree)\n\
         Found 1 breaking change(s)\n"
    );
    let forward = breaking(
        &root,
        &["--against", "snapshot.json", "--direction", "forward"],
    );
    assert_eq!(forward, (0, String::new(), String::new()));
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn a_snapshot_without_a_schema_has_nothing_to_report() {
    let files = [("pyproject.toml", "[tool.sqlty]\n"), ("app/q.py", NEW_CODE)];
    let root = project("schemaless", &files);
    save_snapshot(&root);
    write(&root, "app/new.py", NEW_CODE);
    let found = breaking(&root, &["--against", "snapshot.json"]);
    assert_eq!(found, (0, String::new(), String::new()));
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn a_snapshot_keeps_the_settings_of_its_files() {
    let config = (
        "pyproject.toml",
        "[tool.sqlty]\nschema = \"schema.sql\"\n[tool.sqlty.rules]\nselect-star = \"ignore\"\n\
         unknown-table = \"ignore\"\n[tool.sqlty.types]\nINT = \"str\"\n\
         [[tool.sqlty.functions]]\nimport-path = \"app.db.fetch\"\nquery-param = \"sql\"\n",
    );
    let helper = (
        "app/db.py",
        "from typing import Annotated\nfrom sqlty import SQL\n\
         def run(q: Annotated[str, SQL]): ...\ndef fetch(sql): ...\n",
    );
    let code = (
        "app/q.py",
        "from app.db import fetch, run\nrun(\"SELECT handle FROM users\")\n\
         fetch(\"SELECT handle FROM users\")\nfetch(\"SELECT id FROM audit\")\n\
         from sqlty import SQL\nn = SQL[tuple[str]](\"--sql\\nSELECT n FROM notes\")\n",
    );
    let notes = "CREATE TABLE notes (n INT NOT NULL);\n";
    let schema = format!("{NEW_SCHEMA}{notes}CREATE TABLE audit (id INT);\n");
    let root = project("settings", &[config, ("schema.sql", &schema), helper, code]);
    save_snapshot(&root);
    write(
        &root,
        "pyproject.toml",
        "[tool.sqlty]\nschema = \"schema.sql\"\n",
    );
    let bigint = "CREATE TABLE notes (n BIGINT NOT NULL);\n";
    write(&root, "schema.sql", &format!("{OLD_SCHEMA}{bigint}"));
    let (code, out, _) = breaking(&root, &["--against", "snapshot.json"]);
    assert_eq!(code, 1, "{out}");
    // The helper, the function and the type of INT come from the snapshot, and so does
    // ignoring the dropped table.
    let found: Vec<&str> = out.lines().map(|line| &line[..9]).collect();
    assert_eq!(
        found,
        ["app/q.py:", "app/q.py:", "app/q.py:", "Found 3 b"],
        "{out}"
    );
    assert!(out.contains("app/q.py:2: [unknown-column]"), "{out}");
    assert!(out.contains("app/q.py:3: [unknown-column]"), "{out}");
    assert!(out.contains("app/q.py:6: ["), "{out}");
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn a_snapshot_judges_the_migrations_of_databases_it_recorded() {
    let files = [
        ("pyproject.toml", DATABASES),
        ("migrations/0001.sql", USERS),
        ("events/0001.sql", EVENTS),
        ("events/q.py", "x = 1\n"),
        ("app/q.py", "x = 1\n"),
    ];
    let root = project("recorded", &files);
    save_snapshot(&root);
    write(&root, "events/0002.sql", INDEX);
    let (code, out, err) = breaking(&root, &["--against", "snapshot.json"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert_eq!(out.lines().count(), 2, "{out}");
    assert!(
        out.starts_with("events/0002.sql:2: [missing-concurrently]"),
        "{out}"
    );
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn a_snapshot_saved_before_databases_were_recorded_knows_the_default_one() {
    let files = [MIGRATIONS, ("migrations/0001.sql", EVENTS)];
    let root = project("unrecorded", &files);
    save_snapshot(&root);
    let path = root.join("snapshot.json");
    let saved = std::fs::read_to_string(&path).expect("reads");
    let mut data: serde_json::Value = serde_json::from_str(&saved).expect("parses");
    let removed = data
        .as_object_mut()
        .and_then(|data| data.remove("databases"));
    assert!(removed.is_some());
    std::fs::write(&path, data.to_string()).expect("writes");
    write(&root, "migrations/0002.sql", INDEX);
    let (code, out, err) = breaking(&root, &["--against", "snapshot.json"]);
    assert_eq!((code, err.as_str()), (0, ""));
    assert!(
        out.starts_with("migrations/0002.sql:2: [missing-concurrently]"),
        "{out}"
    );
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn a_database_the_baseline_lacks_has_no_migrations_to_judge() {
    let root = project("lacks", &[MIGRATIONS, ("migrations/0001.sql", USERS)]);
    save_snapshot(&root);
    write(&root, "pyproject.toml", DATABASES);
    write(&root, "events/0001.sql", EVENTS);
    write(&root, "events/0002.sql", INDEX);
    let found = breaking(&root, &["--against", "snapshot.json"]);
    assert_eq!(found, (0, String::new(), String::new()));
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn a_schema_shared_by_two_configurations_is_judged_with_the_files_and_rules_of_both() {
    let jobs = "[tool.sqlty]\nschema = \"../migrations/\"\n\
        [tool.sqlty.rules]\nmissing-concurrently = \"error\"\n";
    let files = [
        ("jobs/pyproject.toml", jobs),
        ("jobs/j.py", "x = 1\n"),
        ("migrations/0001.sql", EVENTS),
    ];
    let root = project("siblings", &files);
    save_snapshot(&root);
    // `app` sorts first and is new: only the probe of `jobs` finds a baseline.
    write(
        &root,
        "app/pyproject.toml",
        "[tool.sqlty]\nschema = \"../migrations/\"\n",
    );
    write(&root, "app/q.py", "x = 1\n");
    write(&root, "migrations/0002.sql", INDEX);
    let (code, out, err) = breaking(&root, &["--against", "snapshot.json"]);
    assert_eq!((code, err.as_str()), (1, ""), "{out}");
    assert_eq!(out.matches("migrations/0002.sql").count(), 1, "{out}");
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn a_backfill_is_judged_against_the_queries_of_the_head() {
    let files = [
        (
            "pyproject.toml",
            "[tool.sqlty]\nschema = \"migrations/\"\nqueries = \"sql/\"\n",
        ),
        ("migrations/0001.sql", USERS),
        (
            "app/q.py",
            "q = \"--sql\\nINSERT INTO users (id, bio) VALUES (1, '')\"\n",
        ),
        (
            "sql/users.sql",
            "-- name: add!\r\nINSERT INTO users (id) VALUES (:id);\r\n",
        ),
        ("other/pyproject.toml", "[tool.sqlty]\n"),
        (
            "other/q.py",
            "q = \"--sql\\nINSERT INTO users (id) VALUES (1)\"\n",
        ),
    ];
    let root = project("writers", &files);
    save_snapshot(&root);
    let backfill = "UPDATE users SET bio = '' WHERE id BETWEEN 1 AND 9;\n";
    write(&root, "migrations/0002.sql", backfill);
    let (code, out, err) = breaking(&root, &["--against", "snapshot.json"]);
    assert_eq!((code, err.as_str()), (0, ""), "{out}");
    let [finding, "Found 1 breaking change(s)"] = out.lines().collect::<Vec<_>>()[..] else {
        panic!("{out}");
    };
    assert!(
        finding.starts_with("migrations/0002.sql:1: [backfill-without-writer]"),
        "{out}"
    );
    assert!(finding.contains("`sql/users.sql:2`"), "{out}");
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn migrations_are_judged_against_the_schema_of_the_baseline() {
    let files = [
        MIGRATIONS,
        ("migrations/0001.sql", EVENTS),
        ("migrations/0002.sql", INDEX),
    ];
    let root = project("judged", &files);
    save_snapshot(&root);
    let same = breaking(&root, &["--against", "snapshot.json"]);
    assert_eq!(same, (0, String::new(), String::new()));
    // A warning does not fail the command. The baseline that has only the first migration finds the second one new.
    std::fs::remove_file(root.join("migrations/0002.sql")).expect("removes");
    save_snapshot(&root);
    write(&root, "migrations/0002.sql", INDEX);
    let (code, out, _) = breaking(&root, &["--against", "snapshot.json"]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out.contains("migrations/0002.sql:2: [missing-concurrently]"),
        "{out}"
    );
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn a_snapshot_names_the_query_format_of_its_files() {
    for format in ["auto", "aiosql", "sqlc", "pugsql", "file"] {
        let config =
            format!("[tool.sqlty]\nschema = \"schema.sql\"\nquery-format = \"{format}\"\n");
        let root = project(&format!("format-{format}"), &[("schema.sql", NEW_SCHEMA)]);
        write(&root, "pyproject.toml", &config);
        write(&root, "app.py", NEW_CODE);
        save_snapshot(&root);
        let saved = std::fs::read_to_string(root.join("snapshot.json")).expect("reads");
        let saved: serde_json::Value = serde_json::from_str(&saved).expect("parses");
        assert_eq!(saved["settings"][0]["query_format"], format);
        std::fs::remove_dir_all(&root).expect("removes");
    }
}

#[test]
fn rejects_a_snapshot_that_is_not_json() {
    let root = project("broken", &[("broken.json", "{")]);
    let (code, out, err) = breaking(&root, &["--against", "broken.json"]);
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(
        err.starts_with("sqlty: error: broken.json is not a sqlty snapshot: EOF"),
        "{err}"
    );
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn rejects_a_snapshot_of_another_shape() {
    let root = project("shapes", &[]);
    let file = r#""files":{"a.py":{"settings":9}}"#;
    for (body, why) in [
        (r#""schemas":{}"#, "invalid schemas"),
        (
            r#""schemas":[{"sql":"","dialect":"nope"}]"#,
            "invalid dialect",
        ),
        (r#""schemas":[],"settings":{}"#, "invalid settings"),
        (
            r#""schemas":[],"settings":[{"dialect":"postgres","schema":3}]"#,
            "invalid schema index",
        ),
        (
            r#""schemas":[],"settings":[{"dialect":"postgres","rules":{"nope":"error"}}]"#,
            "nope",
        ),
        (
            r#""schemas":[],"settings":[],"directories":[]"#,
            "invalid directories",
        ),
        (
            &format!(r#""schemas":[],"settings":[],"directories":{{}},{file}"#),
            "invalid settings index",
        ),
        (
            r#""schemas":[],"settings":[],"directories":{},"databases":[]"#,
            "invalid databases",
        ),
        (
            r#""schemas":[],"settings":[],"directories":{},"databases":{".":[]}"#,
            "invalid databases",
        ),
        (
            r#""schemas":[],"settings":[],"directories":{},"databases":{".":{"default":0}}"#,
            "invalid schema index",
        ),
    ] {
        write(&root, "s.json", &format!(r#"{{"sqlty":1,{body}}}"#));
        let (code, _, err) = breaking(&root, &["--against", "s.json"]);
        assert_eq!(code, 2, "{body}: {err}");
        assert!(err.contains(why), "{body}: {err}");
    }
    std::fs::remove_dir_all(&root).expect("removes");
}

/// A `PATH` of one directory that holds a `git` it may not run.
#[cfg(unix)]
#[test]
fn names_why_git_did_not_start() {
    let root = project("no-git", &[("bin/git", ""), MIGRATIONS]);
    let bin = root.join("bin");
    let (code, _, err) = sqlty(
        &root,
        &["breaking", "--against", "main"],
        &[("PATH", bin.to_str().expect("UTF-8"))],
    );
    assert_eq!(code, 2);
    assert_eq!(err, "sqlty: error: git: Permission denied (os error 13)\n");
    std::fs::remove_dir_all(&root).expect("removes");
}

fn git(root: &Path, args: &[&str]) {
    let config = [
        "-c",
        "user.name=t",
        "-c",
        "user.email=t@t",
        "-c",
        "commit.gpgsign=false",
    ];
    let status = Command::new("git")
        .args(config)
        .args(args)
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .status()
        .expect("runs git");
    assert!(status.success(), "git {args:?}");
}

#[test]
fn names_a_revision_the_repository_lacks() {
    let root = project("no-revision", &[MIGRATIONS]);
    git(&root, &["init", "--quiet"]);
    let (code, _, err) = breaking(&root, &["--against", "nope"]);
    assert_eq!(code, 2);
    assert_eq!(err, "sqlty: error: unknown revision 'nope'\n");
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn checks_out_the_schema_a_configuration_reaches_outside_its_directory() {
    let config = (
        "app/pyproject.toml",
        "[tool.sqlty]\nschema = \"../db/schema.sql\"\n",
    );
    let files = [
        config,
        ("db/schema.sql", NEW_SCHEMA),
        ("app/q.py", NEW_CODE),
    ];
    let root = project("outside", &files);
    git(&root, &["init", "--quiet"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "--quiet", "-m", "one"]);
    write(&root, "db/schema.sql", OLD_SCHEMA);
    let (code, out, err) = breaking(&root, &["--against", "HEAD", "app"]);
    assert_eq!((code, err.as_str()), (1, ""));
    assert!(
        out.contains("app/q.py:1: [unknown-column] column `handle`"),
        "{out}"
    );
    std::fs::remove_dir_all(&root).expect("removes");
}

/// Messages of the language server until one holds `wanted`, after sending `requests`.
fn serve(root: &Path, requests: &[String], wanted: &str) -> (Vec<String>, i32) {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::process::Stdio;

    let mut server = Command::new(env!("CARGO_BIN_EXE_sqlty"))
        .arg("server")
        .current_dir(root)
        .env_remove("VIRTUAL_ENV")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("starts");
    let mut input = server.stdin.take().expect("piped");
    let send = |input: &mut std::process::ChildStdin, body: &str| {
        write!(input, "Content-Length: {}\r\n\r\n{body}", body.len()).expect("sends");
    };
    for request in requests {
        send(&mut input, request);
    }
    let mut output = BufReader::new(server.stdout.take().expect("piped"));
    let mut messages = Vec::new();
    while !messages
        .last()
        .is_some_and(|message: &String| message.contains(wanted))
    {
        let mut header = String::new();
        let mut length = 0;
        while header != "\r\n" {
            header.clear();
            output.read_line(&mut header).expect("reads");
            if let Some(value) = header.strip_prefix("Content-Length: ") {
                length = value.trim().parse().expect("a length");
            }
        }
        let mut body = vec![0; length];
        output.read_exact(&mut body).expect("reads");
        messages.push(String::from_utf8(body).expect("UTF-8"));
    }
    send(
        &mut input,
        r#"{"jsonrpc":"2.0","id":2,"method":"shutdown"}"#,
    );
    send(&mut input, r#"{"jsonrpc":"2.0","method":"exit"}"#);
    let code = server.wait().expect("exits").code().expect("a code");
    (messages, code)
}

fn initialize(root: &Path, options: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"capabilities":{{}},"rootUri":"file://{}","initializationOptions":{options}}}}}"#,
        root.display()
    )
}

#[test]
fn serves_the_language_server() {
    // A schema pattern that matches nothing: the server logs the warning and goes on.
    let config = (
        "pyproject.toml",
        "[tool.sqlty]\nschema = [\"schema.sql\", \"none/*.sql\"]\n",
    );
    let root = project("server", &[config, SCHEMA, QUERIES]);
    let root = root.canonicalize().expect("exists");
    let open = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"file://{}/app/a.py","languageId":"python","version":1,"text":{:?}}}}}}}"#,
        root.display(),
        QUERIES.1
    );
    let initialized = r#"{"jsonrpc":"2.0","method":"initialized","params":{}}"#.to_string();
    let (messages, code) = serve(
        &root,
        &[initialize(&root, "null"), initialized.clone(), open.clone()],
        "unknown-column",
    );
    assert_eq!(code, 0);
    assert!(messages
        .last()
        .is_some_and(|message| message.contains("publishDiagnostics")));
    let actions = format!(
        r#"{{"jsonrpc":"2.0","id":3,"method":"textDocument/codeAction","params":{{"textDocument":{{"uri":"file://{}/app/a.py"}},"range":{{"start":{{"line":0,"character":0}},"end":{{"line":0,"character":0}}}},"context":{{"diagnostics":[],"only":["source.fixAll.sqlty","source.format.sqlty","source.rewrite.sqlty","source.strip.sqlty"]}}}}}}"#,
        root.display()
    );
    let (messages, code) = serve(
        &root,
        &[
            initialize(&root, "null"),
            initialized.clone(),
            open.clone(),
            actions,
        ],
        r#""id":3"#,
    );
    assert_eq!(code, 0);
    let answer = messages.last().expect("an answer");
    for (title, changes) in [
        ("sqlty: fix all", true),
        ("sqlty: rewrite", true),
        ("sqlty: format", false),
        ("sqlty: strip", false),
    ] {
        assert_eq!(answer.contains(title), changes, "{title}: {answer}");
    }
    let baseline = r#"{"breaking":{"against":"main"}}"#;
    let (messages, code) = serve(
        &root,
        &[initialize(&root, baseline), initialized, open],
        "sqlty.breaking.against: main does not exist",
    );
    assert_eq!(code, 0);
    assert!(messages
        .last()
        .is_some_and(|message| message.contains("showMessage")));
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn reports_a_server_without_a_client() {
    let root = project("no-client", &[]);
    let (code, out, err) = sqlty(&root, &["server"], &[]);
    assert_eq!((code, out.as_str()), (2, ""));
    assert!(err.starts_with("sqlty: error: "), "{err}");
    std::fs::remove_dir_all(&root).expect("removes");
}
