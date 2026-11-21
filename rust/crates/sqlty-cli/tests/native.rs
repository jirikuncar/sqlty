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
