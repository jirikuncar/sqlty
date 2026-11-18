//! The `sqlty` executable on a directory of the disk.

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
        .env_remove("SQLTY_AST")
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
fn prints_its_version() {
    let root = project("version", &[]);
    let version = format!("sqlty {}\n", env!("CARGO_PKG_VERSION"));
    assert_eq!(sqlty(&root, &["version"], &[]), (0, version, String::new()));
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn checks_the_files_of_the_disk() {
    let config = (
        "pyproject.toml",
        "[tool.sqlty]\nschema = [\"schema.sql\", \"none/*.sql\"]\n",
    );
    let root = project("check", &[config, SCHEMA, QUERIES]);
    std::fs::write(root.join("app/b.py"), b"\xff").expect("writes");
    let (code, out, err) = sqlty(&root, &["check"], &[]);
    assert_eq!(code, 1);
    assert!(
        out.contains("app/a.py:1: [unknown-column] column `nope` does not exist"),
        "{out}"
    );
    assert!(
        err.starts_with("Schema pattern matches no files: none/*.sql\n"),
        "{err}"
    );
    let (code, _, err) = sqlty(&root, &["check", "gone.py"], &[]);
    assert_eq!(
        (code, err.as_str()),
        (2, "sqlty: error: path does not exist: gone.py\n")
    );
    std::fs::remove_dir_all(&root).expect("removes");
}

#[test]
fn rewrites_the_files_and_runs_the_format_command() {
    for (command, warning) in [
        ("true", None),
        ("false", Some("format-command false failed on app/a.py: \n")),
        (
            "/nonexistent/fmt",
            Some("format-command /nonexistent/fmt failed on app/a.py: "),
        ),
    ] {
        let config =
            format!("[tool.sqlty]\nschema = \"schema.sql\"\nformat-command = [\"{command}\"]\n");
        let root = project("rewrite", &[("pyproject.toml", &config), SCHEMA, QUERIES]);
        let (code, _, err) = sqlty(&root, &["rewrite", "app"], &[]);
        assert_eq!(code, 0, "{err}");
        assert_eq!(
            warning.is_some_and(|warning| err.starts_with(warning)),
            warning.is_some(),
            "{err}"
        );
        let rewritten = std::fs::read_to_string(root.join("app/a.py")).expect("reads");
        assert!(rewritten.contains("r = SQL[tuple[int]]("), "{rewritten}");
        std::fs::remove_dir_all(&root).expect("removes");
    }
}

#[test]
fn hands_what_it_does_not_run_to_python() {
    let root = project("python", &[]);
    let missing = [("PATH", "/nonexistent")];
    let error = "sqlty: cannot run python3: No such file or directory (os error 2)\n";
    for (args, env) in [
        (&["breaking"][..], &missing[..]),
        (&["check"][..], &[missing[0], ("SQLTY_AST", "python")][..]),
    ] {
        assert_eq!(
            sqlty(&root, args, env),
            (2, String::new(), error.to_string())
        );
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
        .env_remove("SQLTY_AST")
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
    let config = ("pyproject.toml", "[tool.sqlty]\nschema = \"schema.sql\"\n");
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
    let baseline = r#"{"breaking":{"against":"main"}}"#;
    let (messages, code) = serve(
        &root,
        &[initialize(&root, baseline), initialized, open],
        "sqlty.breaking.against needs",
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
