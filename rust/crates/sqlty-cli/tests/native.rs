//! The `sqlty` executable on a directory of the disk.

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
    let error = "sqlty: cannot run python3: No such file or directory (os error 2)\n";
    assert_eq!(
        sqlty(&root, &["breaking"], &[("PATH", "/nonexistent")]),
        (2, String::new(), error.to_string())
    );
    std::fs::remove_dir_all(&root).expect("removes");
}
