//! `textDocument/definition` over the protocol, in each dialect: a table, a column, a
//! renamed column and an alias, from a Python string and from a `.sql` query file.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use serde_json::{json, Value};
use sqlty_lsp::{run, CompletionContext, Diagnostic, Settings, Workspace};
use sqlty_python::queries::Format;
use sqlty_sql::analyzer::load_schema_with_skipped;

const TABLES: &str = "CREATE TABLE users (id INT, name TEXT);";
const RENAMES: &str = "-- later\nALTER TABLE users RENAME COLUMN name TO full_name;";

/// The project under `/DIALECT`: its schema is `tables.sql` then `renames.sql`.
struct Project {
    root: PathBuf,
}

impl Workspace for Project {
    fn check(&mut self, _: &Path, _: &str) -> Result<Vec<Diagnostic>, String> {
        Ok(Vec::new())
    }

    fn fix(&mut self, _: &Path, text: &str) -> Result<String, String> {
        Ok(text.to_string())
    }

    fn format(&mut self, _: &Path, text: &str) -> Result<String, String> {
        Ok(text.to_string())
    }

    fn rewrite(&mut self, _: &Path, text: &str) -> Result<String, String> {
        Ok(text.to_string())
    }

    fn strip(&mut self, _: &Path, text: &str) -> Result<String, String> {
        Ok(text.to_string())
    }

    fn completion(&mut self, path: &Path) -> Option<CompletionContext> {
        let dialect = self.root.file_name()?.to_str()?.to_string();
        let sql = format!("{TABLES}\n{RENAMES}");
        let (schema, _) = load_schema_with_skipped(&sql, &dialect);
        Some(CompletionContext {
            schema: Some(schema),
            dialect,
            import_style: None,
            helper_starts: Vec::new(),
            schema_files: vec![
                (self.root.join("tables.sql"), 0),
                (self.root.join("renames.sql"), TABLES.len() + 1),
            ],
            schema_sql: Some(Arc::from(sql)),
            query_format: path
                .extension()
                .is_some_and(|ext| ext == "sql")
                .then_some(Format::Auto),
        })
    }
}

struct Client {
    connection: Connection,
    next: i32,
}

impl Client {
    fn start(dialect: &str, options: &Value) -> Self {
        let (server, connection) = Connection::memory();
        std::thread::spawn(move || {
            run(&server, |root: &Path, _: &Settings| {
                Ok(Project {
                    root: root.to_path_buf(),
                })
            })
            .map_err(|error| error.to_string())
        });
        let mut client = Self {
            connection,
            next: 0,
        };
        let folders = json!([{"uri": format!("file:///{dialect}"), "name": dialect}]);
        let init = json!({"capabilities": {}, "workspaceFolders": folders, "initializationOptions": options});
        let result = client.request("initialize", init);
        assert_eq!(result["serverInfo"]["name"], "sqlty");
        client.notify("initialized", json!({}));
        client
    }

    fn notify(&self, method: &str, params: Value) {
        let notification = Notification::new(method.to_string(), params);
        assert!(self.connection.sender.send(notification.into()).is_ok());
    }

    fn receive(&self) -> Option<Message> {
        self.connection
            .receiver
            .recv_timeout(Duration::from_secs(10))
            .ok()
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = RequestId::from(self.next);
        let request = Request::new(id.clone(), method.to_string(), params);
        assert!(self.connection.sender.send(request.into()).is_ok());
        while let Some(message) = self.receive() {
            if let Message::Response(response) = message {
                if response.id == id {
                    return response.response_result.unwrap_or_default();
                }
            }
        }
        Value::Null
    }

    /// Open `uri` and wait for its first check.
    fn open(&self, uri: &str, language: &str, text: &str) {
        let document = json!({"uri": uri, "languageId": language, "version": 1, "text": text});
        self.notify("textDocument/didOpen", json!({"textDocument": document}));
        while let Some(message) = self.receive() {
            if matches!(&message, Message::Notification(n) if n.method == "textDocument/publishDiagnostics")
            {
                return;
            }
        }
    }

    /// `file line:character-character` of the definition at `needle` in the line `line`.
    fn definition(&mut self, uri: &str, line: u32, character: usize) -> String {
        let position = json!({"line": line, "character": character});
        let found = self.request(
            "textDocument/definition",
            json!({"textDocument": {"uri": uri}, "position": position}),
        );
        if found.is_null() {
            return "None".to_string();
        }
        let range = &found["range"];
        let file = found["uri"].as_str().unwrap_or_default();
        let file = file.rsplit('/').next().unwrap_or_default();
        format!(
            "{file} {}:{}-{}",
            range["start"]["line"], range["start"]["character"], range["end"]["character"]
        )
    }
}

#[test]
fn names_go_to_the_statement_that_defines_them() {
    for dialect in ["postgres", "mysql", "sqlite"] {
        let mut client = Client::start(dialect, &Value::Null);
        let py = format!("file:///{dialect}/q.py");
        let line = "q = \"--sql\\nSELECT u.id, full_name FROM users u\"";
        client.open(&py, "python", &format!("{line}\n"));
        let at = |word: &str| line.find(word).unwrap_or_default();
        let found = [
            client.definition(&py, 0, at("users")),
            client.definition(&py, 0, at("id")),
            client.definition(&py, 0, at("full_name")),
            client.definition(&py, 0, at("u.")),
            client.definition(&py, 0, 1),
        ];
        assert_eq!(
            found,
            [
                "tables.sql 0:13-18",
                "tables.sql 0:20-22",
                "renames.sql 1:40-49",
                "q.py 0:46-47",
                "None",
            ],
            "{dialect}"
        );
        let sql = format!("file:///{dialect}/q.sql");
        client.open(
            &sql,
            "sql",
            "-- name: get-user^\nSELECT u.id FROM users u;\n",
        );
        assert_eq!(
            client.definition(&sql, 1, 10),
            "tables.sql 0:20-22",
            "{dialect}"
        );
        assert_eq!(client.definition(&sql, 1, 7), "q.sql 1:23-24", "{dialect}");
    }
}

#[test]
fn definition_can_be_turned_off() {
    let mut client = Client::start("postgres", &json!({"definition": {"enable": false}}));
    client.open(
        "file:///postgres/q.py",
        "python",
        "q = \"--sql\\nSELECT 1 FROM users\"\n",
    );
    assert_eq!(client.definition("file:///postgres/q.py", 0, 26), "None");
}
