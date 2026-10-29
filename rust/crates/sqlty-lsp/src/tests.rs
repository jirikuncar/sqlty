use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use lsp_types::Url;
use serde_json::{json, Value};

use crate::{run, Diagnostic, Edit, Settings, Severity, Workspace};

/// Reports each `bad`; its fix writes `good` and imports it. With a baseline, `old.py`
/// of its root has a breaking change.
struct Fake {
    root: PathBuf,
    settings: Settings,
}

impl Workspace for Fake {
    fn check(&mut self, _: &Path, text: &str) -> Result<Vec<Diagnostic>, String> {
        if text.contains("crash") {
            return Err("Traceback: crash".to_string());
        }
        Ok(text
            .match_indices("bad")
            .map(|(at, _)| Diagnostic {
                rule: "stale-type".to_string(),
                message: "bad should be good".to_string(),
                severity: Severity::Error,
                start: at,
                end: at + 3,
                fix: vec![
                    Edit {
                        start: 0,
                        end: 0,
                        replacement: "import good\n".to_string(),
                    },
                    Edit {
                        start: at,
                        end: at + 3,
                        replacement: "good".to_string(),
                    },
                ],
                source: None,
            })
            .collect())
    }

    fn fix(&mut self, _: &Path, text: &str) -> Result<String, String> {
        Ok(text.replace("bad", "good"))
    }

    fn breaking(&mut self) -> Result<Vec<(PathBuf, Vec<Diagnostic>)>, String> {
        let Some(against) = &self.settings.against else {
            return Ok(Vec::new());
        };
        let finding = Diagnostic {
            rule: "unknown-column".to_string(),
            message: format!("bio is gone (in the code of {against})"),
            severity: Severity::Error,
            start: 4,
            end: 7,
            fix: Vec::new(),
            source: Some("sqlty breaking (backward)".to_string()),
        };
        Ok(vec![(self.root.join("old.py"), vec![finding])])
    }
}

struct Client {
    connection: Connection,
    server: Option<JoinHandle<Result<(), String>>>,
    pending: Vec<Notification>,
    next: i32,
    opened: Arc<AtomicUsize>,
}

impl Client {
    fn start(capabilities: &Value) -> Self {
        let folders = json!([
            {"uri": "file:///work", "name": "work"},
            {"uri": "file:///broken", "name": "broken"},
        ]);
        Self::with(capabilities, &folders, &Value::Null)
    }

    fn with(capabilities: &Value, folders: &Value, options: &Value) -> Self {
        let (server, connection) = Connection::memory();
        let opened = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&opened);
        let server = std::thread::spawn(move || {
            run(&server, move |root: &Path, settings: &Settings| {
                count.fetch_add(1, Ordering::SeqCst);
                if root.ends_with("broken") {
                    Err("schema.sql does not exist".to_string())
                } else {
                    Ok(Fake {
                        root: root.to_path_buf(),
                        settings: settings.clone(),
                    })
                }
            })
            .map_err(|error| error.to_string())
        });
        let mut client = Self {
            connection,
            server: Some(server),
            pending: Vec::new(),
            next: 0,
            opened,
        };
        let result = client.request(
            "initialize",
            json!({"capabilities": capabilities, "workspaceFolders": folders, "initializationOptions": options}),
        );
        assert_eq!(result["serverInfo"]["name"], "sqlty");
        client.notify("initialized", json!({}));
        client
    }

    fn notify(&self, method: &str, params: Value) {
        let notification = Notification::new(method.to_string(), params);
        self.connection.sender.send(notification.into()).unwrap();
    }

    fn open(&self, uri: &str, text: &str) {
        let document = json!({"uri": uri, "languageId": "python", "version": 1, "text": text});
        self.notify("textDocument/didOpen", json!({"textDocument": document}));
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = RequestId::from(self.next);
        let request = Request::new(id.clone(), method.to_string(), params);
        self.connection.sender.send(request.into()).unwrap();
        loop {
            match self.receive() {
                Message::Response(response) if response.id == id => {
                    return response
                        .response_result
                        .unwrap_or_else(|error| json!({"error": error.code}));
                }
                Message::Notification(notification) => self.pending.push(notification),
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    fn notification(&mut self, method: &str) -> Value {
        if let Some(at) = self.pending.iter().position(|n| n.method == method) {
            return self.pending.remove(at).params;
        }
        loop {
            match self.receive() {
                Message::Notification(n) if n.method == method => return n.params,
                Message::Notification(n) => self.pending.push(n),
                other => panic!("unexpected {other:?}"),
            }
        }
    }

    fn receive(&self) -> Message {
        self.connection
            .receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("the server answers")
    }

    /// The notifications not read yet, once the server has stopped.
    fn shutdown(mut self) -> Vec<String> {
        assert_eq!(self.request("shutdown", Value::Null), Value::Null);
        self.notify("exit", Value::Null);
        let server = self.server.take().unwrap();
        assert_eq!(server.join().unwrap(), Ok(()));
        let rest = self.connection.receiver.try_iter().filter_map(|m| match m {
            Message::Notification(n) => Some(n),
            _ => None,
        });
        self.pending
            .drain(..)
            .chain(rest)
            .map(|n| n.method)
            .collect()
    }
}

fn utf8() -> Value {
    json!({"general": {"positionEncodings": ["utf-8", "utf-16"]}})
}

#[test]
fn diagnostics_are_published_in_utf16_units() {
    let mut client = Client::start(&json!({}));
    client.open("file:///work/q.py", "é = bad\n");
    let published = client.notification("textDocument/publishDiagnostics");
    assert_eq!(published["version"], 1);
    let diagnostic = &published["diagnostics"][0];
    assert_eq!(
        diagnostic["range"]["start"],
        json!({"line": 0, "character": 4})
    );
    assert_eq!(
        diagnostic["range"]["end"],
        json!({"line": 0, "character": 7})
    );
    assert_eq!(diagnostic["code"], "stale-type");
    assert_eq!(diagnostic["source"], "sqlty");
    assert_eq!(diagnostic["severity"], 1);
    assert_eq!(
        diagnostic["codeDescription"]["href"],
        "https://jirikuncar.github.io/sqlty/reference/rules/#stale-type"
    );
    client.shutdown();
}

#[test]
fn utf8_positions_when_the_client_offers_them() {
    let mut client = Client::start(&utf8());
    client.open("file:///work/q.py", "é = bad\n");
    let published = client.notification("textDocument/publishDiagnostics");
    assert_eq!(
        published["diagnostics"][0]["range"]["start"]["character"],
        5
    );
    client.shutdown();
}

#[test]
fn a_quick_fix_carries_its_imports() {
    let mut client = Client::start(&json!({}));
    client.open("file:///work/q.py", "x = 1\ny = bad\n");
    client.notification("textDocument/publishDiagnostics");
    let range = json!({"start": {"line": 1, "character": 5}, "end": {"line": 1, "character": 5}});
    let actions = client.request(
        "textDocument/codeAction",
        json!({"textDocument": {"uri": "file:///work/q.py"}, "range": range, "context": {"diagnostics": []}}),
    );
    let action = &actions[0];
    assert_eq!(action["title"], "Fix stale-type");
    assert_eq!(action["kind"], "quickfix");
    let change = &action["edit"]["documentChanges"][0];
    assert_eq!(change["textDocument"]["version"], 1);
    let edits: Vec<_> = change["edits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["newText"])
        .collect();
    assert_eq!(edits, ["import good\n", "good"]);

    let elsewhere =
        json!({"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 1}});
    let none = client.request(
        "textDocument/codeAction",
        json!({"textDocument": {"uri": "file:///work/q.py"}, "range": elsewhere, "context": {"diagnostics": []}}),
    );
    assert_eq!(none, json!([]));
    client.shutdown();
}

#[test]
fn fix_all_edits_the_changed_span() {
    let mut client = Client::start(&json!({}));
    client.open("file:///work/q.py", "x = 1\ny = bad\n");
    let range = json!({"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}});
    let only = |kinds: Value| json!({"textDocument": {"uri": "file:///work/q.py"}, "range": range, "context": {"diagnostics": [], "only": kinds}});
    let actions = client.request("textDocument/codeAction", only(json!(["source.fixAll"])));
    assert_eq!(actions[0]["kind"], "source.fixAll.sqlty");
    let edit = &actions[0]["edit"]["documentChanges"][0]["edits"][0];
    assert_eq!(edit["newText"], "goo", "`d` is common");
    assert_eq!(edit["range"]["start"], json!({"line": 1, "character": 4}));
    assert_eq!(edit["range"]["end"], json!({"line": 1, "character": 6}));
    let other = client.request("textDocument/codeAction", only(json!(["refactor"])));
    assert_eq!(other, json!([]));
    client.shutdown();
}

#[test]
fn an_edit_is_checked_at_its_version() {
    let mut client = Client::start(&json!({}));
    client.open("file:///work/q.py", "y = bad\n");
    client.notification("textDocument/publishDiagnostics");
    let change = json!({"textDocument": {"uri": "file:///work/q.py", "version": 2}, "contentChanges": [{"text": "y = good\n"}]});
    client.notify("textDocument/didChange", change);
    let published = client.notification("textDocument/publishDiagnostics");
    assert_eq!(published["version"], 2);
    assert_eq!(published["diagnostics"], json!([]));
    client.notify(
        "textDocument/didClose",
        json!({"textDocument": {"uri": "file:///work/q.py"}}),
    );
    let closed = client.notification("textDocument/publishDiagnostics");
    assert_eq!(closed["diagnostics"], json!([]));
    client.shutdown();
}

#[test]
fn a_project_that_fails_to_load_is_reported_once() {
    let mut client = Client::start(&json!({}));
    client.open("file:///broken/a.py", "bad\n");
    let shown = client.notification("window/showMessage");
    assert_eq!(shown["message"], "sqlty: schema.sql does not exist");
    assert_eq!(shown["type"], 1);
    client.open("file:///broken/b.py", "bad\n");
    client.open("file:///work/q.py", "bad\n");
    let published = client.notification("textDocument/publishDiagnostics");
    assert_eq!(published["uri"], "file:///work/q.py");
    let rest = client.shutdown();
    assert!(
        !rest.iter().any(|method| method == "window/showMessage"),
        "{rest:?}"
    );
}

#[test]
fn a_failed_check_is_logged() {
    let mut client = Client::start(&json!({}));
    client.open("file:///work/q.py", "crash\n");
    let logged = client.notification("window/logMessage");
    assert_eq!(
        logged["message"],
        "sqlty: checking /work/q.py: Traceback: crash"
    );
    client.shutdown();
}

#[test]
fn saving_the_config_reloads_the_project() {
    let mut client = Client::start(&json!({}));
    client.open("file:///work/q.py", "bad\n");
    client.notification("textDocument/publishDiagnostics");
    assert_eq!(client.opened.load(Ordering::SeqCst), 1);
    client.notify(
        "textDocument/didSave",
        json!({"textDocument": {"uri": "file:///work/pyproject.toml"}}),
    );
    client.notification("textDocument/publishDiagnostics");
    assert_eq!(client.opened.load(Ordering::SeqCst), 2);
    client.shutdown();
}

#[test]
fn other_languages_and_requests_are_left_alone() {
    let mut client = Client::start(&json!({}));
    let document =
        json!({"uri": "file:///work/q.sql", "languageId": "sql", "version": 1, "text": "bad"});
    client.notify("textDocument/didOpen", json!({"textDocument": document}));
    let hover = client.request(
        "textDocument/hover",
        json!({"textDocument": {"uri": "file:///work/q.py"}, "position": {"line": 0, "character": 0}}),
    );
    assert_eq!(hover, json!({"error": -32601}));
    let rest = client.shutdown();
    assert!(
        !rest
            .iter()
            .any(|method| method == "textDocument/publishDiagnostics"),
        "{rest:?}"
    );
}

#[test]
fn breaking_changes_in_files_that_are_not_open_are_published() {
    let root = std::env::temp_dir().join(format!("sqlty-lsp-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("old.py"), "q = bio\n").unwrap();
    let folder = Url::from_file_path(&root).unwrap();
    let old = Url::from_file_path(root.join("old.py")).unwrap();
    let options = json!({"breaking": {"against": "main", "direction": "both"}});
    let mut client = Client::with(
        &json!({}),
        &json!([{"uri": folder, "name": "work"}]),
        &options,
    );
    let new = Url::from_file_path(root.join("new.py")).unwrap();
    client.open(new.as_str(), "ok\n");
    let published = client.notification("textDocument/publishDiagnostics");
    assert_eq!(published["uri"], old.as_str());
    let diagnostic = &published["diagnostics"][0];
    assert_eq!(diagnostic["source"], "sqlty breaking (backward)");
    assert_eq!(diagnostic["message"], "bio is gone (in the code of main)");
    assert_eq!(
        diagnostic["range"]["start"],
        json!({"line": 0, "character": 4})
    );

    client.open(old.as_str(), "q = bio\n");
    client.notify(
        "textDocument/didClose",
        json!({"textDocument": {"uri": old}}),
    );
    let closed = loop {
        let published = client.notification("textDocument/publishDiagnostics");
        if published["uri"] == old.as_str() && published["version"].is_null() {
            break published;
        }
    };
    assert_eq!(
        closed["diagnostics"][0]["source"],
        "sqlty breaking (backward)"
    );

    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({"changes": [{"uri": Url::from_file_path(root.join("schema.sql")).unwrap(), "type": 2}]}),
    );
    let reloaded = loop {
        let published = client.notification("textDocument/publishDiagnostics");
        if published["uri"] == old.as_str() {
            break published;
        }
    };
    assert_eq!(
        reloaded["diagnostics"][0]["source"],
        "sqlty breaking (backward)"
    );
    assert_eq!(client.opened.load(Ordering::SeqCst), 2);
    client.shutdown();
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn without_a_baseline_nothing_is_published_in_the_background() {
    let mut client = Client::start(&json!({}));
    client.open("file:///work/q.py", "ok\n");
    let published = client.notification("textDocument/publishDiagnostics");
    assert_eq!(published["uri"], "file:///work/q.py");
    let rest = client.shutdown();
    assert!(
        !rest
            .iter()
            .any(|method| method == "textDocument/publishDiagnostics"),
        "{rest:?}"
    );
}
