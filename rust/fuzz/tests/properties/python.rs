//! Python itself: string literals, and one interpreter that runs generated code.

use std::io::{BufRead, BufReader, Write};
use std::process::{ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{LazyLock, Mutex};

/// A Python string literal of `text`: a JSON string is one.
pub fn literal(text: &str) -> String {
    serde_json::to_string(text).expect("a JSON string")
}

const SERVER: &str = r#"
import json, sys
for line in sys.stdin:
    try:
        exec(compile(json.loads(line), "<rewritten>", "exec"), {})
        print("null", flush=True)
    except BaseException as error:
        print(json.dumps(repr(error)), flush=True)
"#;

/// `SQLTY_FUZZ_PYTHON` (`python3`), with this checkout's `sqlty` importable.
static INTERPRETER: LazyLock<Mutex<(ChildStdin, BufReader<ChildStdout>)>> = LazyLock::new(|| {
    let python = std::env::var("SQLTY_FUZZ_PYTHON").unwrap_or_else(|_| "python3".into());
    #[expect(
        clippy::zombie_processes,
        reason = "it exits once the tests close its stdin"
    )]
    let mut child = Command::new(&python)
        .args(["-c", SERVER])
        .env(
            "PYTHONPATH",
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../src"),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("{python} runs (set SQLTY_FUZZ_PYTHON): {error}"));
    let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
    Mutex::new((child.stdin.take().expect("piped stdin"), stdout))
});

/// Run `source` as a module of its own; the error it raises, if any.
pub fn run(source: &str) -> Result<(), String> {
    let (stdin, stdout) = &mut *INTERPRETER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    writeln!(stdin, "{}", literal(source)).expect("Python reads");
    let mut reply = String::new();
    stdout.read_line(&mut reply).expect("Python answers");
    match serde_json::from_str::<Option<String>>(&reply) {
        Ok(None) => Ok(()),
        Ok(Some(error)) => Err(error),
        Err(_) => Err("Python exited".into()),
    }
}
