use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use std::thread::JoinHandle;
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use serde_json::{json, Value};

use sqlty_sql::analyzer::{load_schema_with_skipped, Schema};

use crate::{run, CompletionContext, Diagnostic, Edit, Settings, Severity, Workspace};

/// Users and orders, and 540 tables `t0`..`t539`.
static SCHEMA: LazyLock<Schema> = LazyLock::new(|| {
    let mut sql = String::from(
        "CREATE TABLE users (id INT NOT NULL, name TEXT);\nCREATE TABLE orders (id INT, user_id INT);\n",
    );
    for i in 0..540 {
        sql += &format!("CREATE TABLE t{i} (id INT);\n");
    }
    load_schema_with_skipped(&sql, "postgres").0
});

/// Lets a check of a buffer holding `slow` finish.
static RELEASE: AtomicBool = AtomicBool::new(false);

/// Reports each `bad`; its fix writes `good` and imports it. With a baseline, `old.py`
/// of its root has a breaking change. Completes with [`SCHEMA`] and the table `gen{N}`
/// of its `N`th load; the strings `run(` is called with are helper queries.
struct Fake {
    root: PathBuf,
    settings: Settings,
    generation: usize,
    checked: String,
}

impl Workspace for Fake {
    fn check(&mut self, _: &Path, text: &str) -> Result<Vec<Diagnostic>, String> {
        if text.contains("crash") {
            return Err("Traceback: crash".to_string());
        }
        while text.contains("slow") && !RELEASE.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(5));
        }
        self.checked = text.to_string();
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

    fn format(&mut self, _: &Path, text: &str) -> Result<String, String> {
        Ok(text.replace("x=1", "x = 1"))
    }

    fn rewrite(&mut self, _: &Path, text: &str) -> Result<String, String> {
        Ok(text.replace("= \"--sql", "= SQL(\"--sql"))
    }

    fn strip(&mut self, _: &Path, text: &str) -> Result<String, String> {
        Ok(text.replace("SQL(\"--sql", "\"--sql"))
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

    fn completion(&mut self, _: &Path) -> Option<CompletionContext> {
        let schema = if self.generation == 1 {
            SCHEMA.clone()
        } else {
            let sql = format!("CREATE TABLE gen{} (id INT);", self.generation);
            load_schema_with_skipped(&sql, "postgres").0
        };
        Some(CompletionContext {
            schema: Some(schema),
            dialect: "postgres".to_string(),
            import_style: None,
            helper_starts: self
                .checked
                .match_indices("run(")
                .map(|(at, _)| at + 4)
                .collect(),
        })
    }
}

struct Client {
    connection: Connection,
    server: Option<JoinHandle<Result<(), String>>>,
    pending: Vec<Notification>,
    /// What the server asked of the client, by method.
    asked: Vec<String>,
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
                        generation: count.load(Ordering::SeqCst),
                        checked: String::new(),
                    })
                }
            })
            .map_err(|error| error.to_string())
        });
        let mut client = Self {
            connection,
            server: Some(server),
            pending: Vec::new(),
            asked: Vec::new(),
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
                Message::Request(asked) => self.asked.push(asked.method),
                other @ Message::Response(_) => panic!("unexpected {other:?}"),
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
                Message::Request(asked) => self.asked.push(asked.method),
                other @ Message::Response(_) => panic!("unexpected {other:?}"),
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
        "https://sqlty.kuncar.dev/reference/rules/#stale-type"
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
fn format_is_a_source_action_beside_fix_all() {
    let mut client = Client::start(&json!({}));
    client.open("file:///work/q.py", "x=1\ny = bad\n");
    let range = json!({"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}});
    let only = |kinds: Value| json!({"textDocument": {"uri": "file:///work/q.py"}, "range": range, "context": {"diagnostics": [], "only": kinds}});
    let actions = client.request("textDocument/codeAction", only(json!(["source.format"])));
    assert_eq!(actions[0]["kind"], "source.format.sqlty");
    assert_eq!(actions[0]["title"], "sqlty: format");
    let edit = &actions[0]["edit"]["documentChanges"][0]["edits"][0];
    assert_eq!(edit["newText"], " = ");
    let both = client.request("textDocument/codeAction", only(json!(["source"])));
    let kinds: Vec<_> = both
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["kind"].clone())
        .collect();
    assert_eq!(
        kinds,
        [json!("source.fixAll.sqlty"), json!("source.format.sqlty")]
    );
    client.open("file:///work/clean.py", "x = 1\n");
    let clean = json!({"textDocument": {"uri": "file:///work/clean.py"}, "range": range, "context": {"diagnostics": [], "only": ["source.format"]}});
    assert_eq!(client.request("textDocument/codeAction", clean), json!([]));
    client.shutdown();
}

#[test]
fn rewrite_and_strip_are_source_actions() {
    let mut client = Client::start(&json!({}));
    client.open("file:///work/q.py", "a = \"--sql\"\nb = SQL(\"--sql\")\n");
    let range = json!({"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}});
    let only = |kinds: Value| json!({"textDocument": {"uri": "file:///work/q.py"}, "range": range, "context": {"diagnostics": [], "only": kinds}});
    let rewrite = client.request(
        "textDocument/codeAction",
        only(json!(["source.rewrite.sqlty"])),
    );
    assert_eq!(rewrite[0]["title"], "sqlty: rewrite");
    assert_eq!(
        rewrite[0]["edit"]["documentChanges"][0]["edits"][0]["newText"],
        "SQL("
    );
    let strip = client.request(
        "textDocument/codeAction",
        only(json!(["source.strip.sqlty"])),
    );
    assert_eq!(strip[0]["title"], "sqlty: strip");
    assert_eq!(strip.as_array().unwrap().len(), 1);
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
    for name in ["pyproject.toml", "sqlty.toml", "Cargo.toml"] {
        let mut client = Client::start(&json!({}));
        client.open("file:///work/q.py", "bad\n");
        client.notification("textDocument/publishDiagnostics");
        assert_eq!(client.opened.load(Ordering::SeqCst), 1);
        client.notify(
            "textDocument/didSave",
            json!({"textDocument": {"uri": format!("file:///work/{name}")}}),
        );
        client.notification("textDocument/publishDiagnostics");
        assert_eq!(client.opened.load(Ordering::SeqCst), 2, "{name}");
        client.shutdown();
    }
}

#[test]
fn other_languages_and_requests_are_left_alone() {
    let mut client = Client::start(&json!({}));
    let document = json!({"uri": "file:///work/q.txt", "languageId": "plaintext", "version": 1, "text": "bad"});
    client.notify("textDocument/didOpen", json!({"textDocument": document}));
    let definition = client.request(
        "textDocument/definition",
        json!({"textDocument": {"uri": "file:///work/q.py"}, "position": {"line": 0, "character": 0}}),
    );
    assert_eq!(definition, json!({"error": -32601}));
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
    let folder = crate::uri::from_path(&root).unwrap();
    let old = crate::uri::from_path(&root.join("old.py")).unwrap();
    let options = json!({"breaking": {"against": "main", "direction": "both"}});
    let mut client = Client::with(
        &json!({}),
        &json!([{"uri": folder, "name": "work"}]),
        &options,
    );
    let new = crate::uri::from_path(&root.join("new.py")).unwrap();
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
        json!({"changes": [{"uri": crate::uri::from_path(&root.join("schema.sql")).unwrap(), "type": 2}]}),
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

/// The labels of a completion response, or `null`.
fn labels(found: &Value) -> Value {
    if found.is_null() {
        return Value::Null;
    }
    json!(found["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["label"].clone())
        .collect::<Vec<_>>())
}

impl Client {
    fn complete(&mut self, uri: &str, line: u32, character: u32) -> Value {
        let position = json!({"line": line, "character": character});
        self.request(
            "textDocument/completion",
            json!({"textDocument": {"uri": uri}, "position": position, "context": {"triggerKind": 2, "triggerCharacter": "."}}),
        )
    }

    fn change(&self, uri: &str, version: i32, text: &str) {
        let change = json!({"textDocument": {"uri": uri, "version": version}, "contentChanges": [{"text": text}]});
        self.notify("textDocument/didChange", change);
    }
}

#[test]
fn completion_hover_and_highlighting_are_advertised_unless_off() {
    for (options, advertised, hover, highlighting) in [
        (Value::Null, true, true, true),
        (json!({"completion": {"enable": true}}), true, true, true),
        (json!({"completion": {"enable": false}}), false, true, true),
        (json!({"hover": {"enable": false}}), true, false, true),
        (
            json!({"highlighting": {"enable": false}}),
            true,
            true,
            false,
        ),
    ] {
        let (server, connection) = Connection::memory();
        let thread = std::thread::spawn(move || {
            run(&server, |_: &Path, _: &Settings| -> Result<Fake, String> {
                Err(String::new())
            })
            .map_err(|error| error.to_string())
        });
        let mut client = Client {
            connection,
            server: Some(thread),
            pending: Vec::new(),
            asked: Vec::new(),
            next: 0,
            opened: Arc::default(),
        };
        let result = client.request(
            "initialize",
            json!({"capabilities": {}, "initializationOptions": options}),
        );
        let tokens = &result["capabilities"]["semanticTokensProvider"];
        if highlighting {
            assert_eq!(tokens["full"], true);
            assert_eq!(
                tokens["legend"]["tokenTypes"],
                json!([
                    "keyword",
                    "string",
                    "number",
                    "comment",
                    "operator",
                    "parameter"
                ])
            );
        } else {
            assert!(tokens.is_null(), "{tokens}");
        }
        assert_eq!(
            result["capabilities"]["hoverProvider"],
            if hover { json!(true) } else { Value::Null }
        );
        let provider = &result["capabilities"]["completionProvider"];
        if advertised {
            assert_eq!(provider["triggerCharacters"], json!(["."]));
            assert_eq!(provider["resolveProvider"], true);
        } else {
            assert!(provider.is_null(), "{provider}");
        }
        client.notify("initialized", json!({}));
        client.shutdown();
    }
}

#[test]
fn a_request_before_initialized_is_answered() {
    let (server, connection) = Connection::memory();
    let thread = std::thread::spawn(move || {
        run(&server, |_: &Path, _: &Settings| -> Result<Fake, String> {
            Err(String::new())
        })
        .map_err(|error| error.to_string())
    });
    let mut client = Client {
        connection,
        server: Some(thread),
        pending: Vec::new(),
        asked: Vec::new(),
        next: 0,
        opened: Arc::default(),
    };
    client.request("initialize", json!({"capabilities": {}}));
    let range = json!({"start": {"line": 0, "character": 0}, "end": {"line": 1, "character": 0}});
    let answer = client.request(
        "textDocument/semanticTokens/range",
        json!({"textDocument": {"uri": "file:///work/q.py"}, "range": range}),
    );
    assert_eq!(answer, json!({"error": -32601}), "method not found");
    client.notify("initialized", json!({}));
    client.shutdown();
}

#[test]
fn completion_after_a_dot_lists_the_columns() {
    let mut client = Client::start(&json!({}));
    let text = "q = \"--sql\\nSELECT u. FROM users u\"\n";
    client.open("file:///work/q.py", text);
    client.notification("textDocument/publishDiagnostics");
    let found = client.complete("file:///work/q.py", 0, 21);
    assert_eq!(labels(&found), json!(["id", "name"]));
    assert_eq!(found["isIncomplete"], false);
    let item = &found["items"][0];
    assert_eq!(item["kind"], 5, "a field");
    assert_eq!(item["sortText"], "0000");
    assert_eq!(item["labelDetails"], json!({"description": "u"}));
    assert_eq!(
        item["textEdit"]["range"],
        json!({"start": {"line": 0, "character": 21}, "end": {"line": 0, "character": 21}})
    );
    assert!(item.get("detail").is_none(), "left to resolve");

    let resolved = client.request("completionItem/resolve", item.clone());
    assert_eq!(resolved["detail"], "INT NOT NULL");
    assert_eq!(
        resolved["documentation"]["value"],
        "Column of `u`: `INT NOT NULL`"
    );
    client.shutdown();
}

#[test]
fn completion_in_helper_strings_only() {
    let mut client = Client::start(&json!({}));
    let text = "run(\"SELECT * FROM us\")\nother(\"SELECT * FROM us\")\n";
    client.open("file:///work/q.py", text);
    client.notification("textDocument/publishDiagnostics");
    assert_eq!(
        labels(&client.complete("file:///work/q.py", 0, 21)),
        json!(["users"])
    );
    assert_eq!(client.complete("file:///work/q.py", 1, 23), Value::Null);
    assert_eq!(client.complete("file:///work/q.py", 0, 1), Value::Null);
    assert_eq!(client.complete("file:///nowhere/q.py", 0, 1), Value::Null);

    let edited = format!("# a comment\n{text}");
    client.change("file:///work/q.py", 2, &edited);
    assert_eq!(
        labels(&client.complete("file:///work/q.py", 1, 21)),
        json!(["users"]),
        "the helper string moved with the edit, before the edit was checked"
    );
    client.shutdown();
}

#[test]
fn a_long_list_is_capped_and_incomplete() {
    let mut client = Client::start(&json!({}));
    let text = "q = \"--sql\\nSELECT * FROM t\"\n";
    client.open("file:///work/q.py", text);
    client.notification("textDocument/publishDiagnostics");
    let found = client.complete("file:///work/q.py", 0, 27);
    assert_eq!(found["items"].as_array().unwrap().len(), 200);
    assert_eq!(found["isIncomplete"], true);
    client.change(
        "file:///work/q.py",
        2,
        "q = \"--sql\\nSELECT * FROM t53\"\n",
    );
    let found = client.complete("file:///work/q.py", 0, 29);
    assert_eq!(found["items"].as_array().unwrap().len(), 11);
    assert_eq!(found["isIncomplete"], false);
    client.shutdown();
}

#[test]
fn completion_is_answered_while_a_check_runs() {
    let mut client = Client::start(&json!({}));
    let text = "q = \"--sql\\nSELECT u. FROM users u\"\n";
    client.open("file:///work/q.py", text);
    client.notification("textDocument/publishDiagnostics");
    client.change("file:///work/q.py", 2, &format!("{text}slow = 1\n"));
    std::thread::sleep(Duration::from_millis(50));
    client.notify("$/cancelRequest", json!({"id": 999}));
    assert_eq!(
        labels(&client.complete("file:///work/q.py", 0, 21)),
        json!(["id", "name"])
    );
    RELEASE.store(true, Ordering::SeqCst);
    let published = client.notification("textDocument/publishDiagnostics");
    assert_eq!(published["version"], 2);
    client.shutdown();
}

#[test]
fn a_reload_swaps_the_schema_completion_reads() {
    let mut client = Client::start(&json!({}));
    let text = "q = \"--sql\\nSELECT * FROM gen\"\n";
    client.open("file:///work/q.py", text);
    client.notification("textDocument/publishDiagnostics");
    assert_eq!(
        labels(&client.complete("file:///work/q.py", 0, 29)),
        json!([])
    );
    client.notify(
        "workspace/didChangeWatchedFiles",
        json!({"changes": [{"uri": "file:///work/schema.sql", "type": 2}]}),
    );
    client.notification("textDocument/publishDiagnostics");
    assert_eq!(
        labels(&client.complete("file:///work/q.py", 0, 29)),
        json!(["gen2"])
    );
    client.shutdown();
}

#[test]
fn completion_can_be_turned_off() {
    let mut client = Client::with(
        &json!({}),
        &json!([{"uri": "file:///work", "name": "work"}]),
        &json!({"completion": {"enable": false}}),
    );
    let text = "q = \"--sql\\nSELECT u. FROM users u\"\n";
    client.open("file:///work/q.py", text);
    client.notification("textDocument/publishDiagnostics");
    assert_eq!(client.complete("file:///work/q.py", 0, 21), Value::Null);
    client.shutdown();
}

impl Client {
    fn hover(&mut self, uri: &str, line: u32, character: u32) -> Value {
        let position = json!({"line": line, "character": character});
        self.request(
            "textDocument/hover",
            json!({"textDocument": {"uri": uri}, "position": position}),
        )
    }
}

#[test]
fn hover_says_what_a_word_of_a_query_is() {
    let mut client = Client::start(&json!({}));
    let text =
        "q = \"--sql\\nSELECT 1 FROM users u WHERE u.id = 1\"\nrun(\"SELECT 1 FROM users\")\n";
    client.open("file:///work/q.py", text);
    client.notification("textDocument/publishDiagnostics");
    let found = client.hover("file:///work/q.py", 0, 43);
    assert_eq!(
        found["contents"],
        json!({"kind": "markdown", "value": "Column `id` of `u`: `INT NOT NULL`"})
    );
    assert_eq!(
        found["range"],
        json!({"start": {"line": 0, "character": 42}, "end": {"line": 0, "character": 44}})
    );
    let table = client.hover("file:///work/q.py", 1, 21);
    assert!(
        table["contents"]["value"]
            .as_str()
            .unwrap()
            .starts_with("Table `public.users`\n- `id` `INT NOT NULL`"),
        "{table}"
    );
    assert_eq!(client.hover("file:///work/q.py", 0, 1), Value::Null);
    assert_eq!(client.hover("file:///nowhere/q.py", 0, 1), Value::Null);
    client.shutdown();
}

#[test]
fn hover_can_be_turned_off() {
    let mut client = Client::with(
        &json!({}),
        &json!([{"uri": "file:///work", "name": "work"}]),
        &json!({"hover": {"enable": false}}),
    );
    client.open("file:///work/q.py", "q = \"--sql\\nSELECT 1 FROM users\"\n");
    client.notification("textDocument/publishDiagnostics");
    assert_eq!(client.hover("file:///work/q.py", 0, 26), Value::Null);
    client.shutdown();
}

impl Client {
    /// Wait for the server to ask the client `method`.
    fn asked(&mut self, method: &str) {
        while !self.asked.iter().any(|asked| asked == method) {
            match self.receive() {
                Message::Request(asked) => self.asked.push(asked.method),
                Message::Notification(n) => self.pending.push(n),
                other @ Message::Response(_) => panic!("unexpected {other:?}"),
            }
        }
    }

    fn tokens(&mut self, uri: &str) -> Value {
        self.request(
            "textDocument/semanticTokens/full",
            json!({"textDocument": {"uri": uri}}),
        )
    }
}

/// `data` of semantic tokens as `(line, character, length, type)`, positions made absolute.
fn decoded(tokens: &Value) -> Vec<(u64, u64, u64, u64)> {
    let data: Vec<u64> = serde_json::from_value(tokens["data"].clone()).unwrap();
    let (mut line, mut character) = (0, 0);
    data.chunks(5)
        .map(|token| {
            line += token[0];
            character = if token[0] == 0 {
                character + token[1]
            } else {
                token[1]
            };
            (line, character, token[2], token[3])
        })
        .collect()
}

#[test]
fn semantic_tokens_color_the_sql_of_checked_strings() {
    let mut client = Client::start(&json!({}));
    let text =
        "é = \"--sql\\nSELECT 'é' FROM t\"\nq = \"\"\"--sql\nSELECT 1\"\"\"\nx = \"SELECT 2\"\n";
    client.open("file:///work/q.py", text);
    let (keyword, string, number, comment) = (0, 1, 2, 3);
    assert_eq!(
        decoded(&client.tokens("file:///work/q.py")),
        [
            (0, 5, 5, comment),
            (0, 12, 6, keyword),
            (0, 19, 3, string),
            (0, 23, 4, keyword),
            (1, 7, 5, comment),
            (2, 0, 6, keyword),
            (2, 7, 1, number),
        ],
        "in UTF-16 units, before the first check"
    );
    assert_eq!(client.tokens("file:///nowhere/q.py"), Value::Null);
    client.notification("textDocument/publishDiagnostics");
    assert!(client.asked.is_empty(), "the check found nothing new");
    client.shutdown();
}

#[test]
fn a_check_that_finds_helper_strings_asks_for_the_tokens_again() {
    let capabilities = json!({"workspace": {"semanticTokens": {"refreshSupport": true}}});
    let mut client = Client::start(&capabilities);
    client.open("file:///work/q.py", "run(\"SELECT 1\")\n");
    client.notification("textDocument/publishDiagnostics");
    client.asked("workspace/semanticTokens/refresh");
    assert_eq!(decoded(&client.tokens("file:///work/q.py")).len(), 2);
    assert_eq!(client.asked, ["workspace/semanticTokens/refresh"]);
    client.shutdown();
}

#[test]
fn highlighting_can_be_turned_off() {
    let mut client = Client::with(
        &json!({}),
        &json!([{"uri": "file:///work", "name": "work"}]),
        &json!({"highlighting": {"enable": false}}),
    );
    client.open("file:///work/q.py", "q = \"--sql\\nSELECT 1\"\n");
    assert_eq!(client.tokens("file:///work/q.py"), Value::Null);
    client.shutdown();
}
