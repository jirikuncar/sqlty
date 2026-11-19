//! Python itself: `repr` of a string, and one interpreter that runs generated code.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{LazyLock, Mutex};

/// `repr(text)` for printable text and newlines, as Python spells it.
pub fn repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

const SERVER: &str = r#"
import json, sys
for line in sys.stdin:
    try:
        exec(compile(json.loads(line), "<rewritten>", "exec"), {})
        print("ok", flush=True)
    except BaseException as error:
        print(json.dumps(repr(error)), flush=True)
"#;

struct Interpreter {
    _child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
}

/// `SQLTY_FUZZ_PYTHON` (`python3` by default), with this checkout's `sqlty` importable.
static INTERPRETER: LazyLock<Mutex<Interpreter>> = LazyLock::new(|| {
    let python = std::env::var("SQLTY_FUZZ_PYTHON").unwrap_or_else(|_| "python3".into());
    let src = concat!(env!("CARGO_MANIFEST_DIR"), "/../../src");
    let mut child = Command::new(&python)
        .args(["-c", SERVER])
        .env("PYTHONPATH", src)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|error| panic!("{python} runs (set SQLTY_FUZZ_PYTHON): {error}"));
    let stdin = child.stdin.take().expect("piped stdin");
    let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
    Mutex::new(Interpreter {
        _child: child,
        stdin,
        stdout,
    })
});

/// Run `source` as a module of its own; the error it raises, if any.
pub fn run(source: &str) -> Result<(), String> {
    let mut python = INTERPRETER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let request = serde_json::to_string(source).expect("a JSON string");
    writeln!(python.stdin, "{request}").expect("Python reads");
    let mut reply = String::new();
    python.stdout.read_line(&mut reply).expect("Python answers");
    match reply.trim_end() {
        "ok" => Ok(()),
        "" => Err("Python exited".into()),
        error => Err(serde_json::from_str(error).unwrap_or_else(|_| error.to_string())),
    }
}
