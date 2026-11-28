//! A client that drives `sqlty_lsp::run` over an in-memory connection.

#![expect(clippy::unwrap_used, reason = "a broken connection fails the test")]

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId, Response};
use serde_json::{json, Value};
use sqlty_lsp::{run, CompletionContext, Diagnostic, Settings, Workspace};
use sqlty_sql::analyzer::{load_schema_with_skipped, Schema};

/// A workspace whose files have no diagnostics and whose commands change nothing, checked
/// against `schema` once a check has read them.
pub struct Plain {
    schema: Option<Schema>,
}

impl Workspace for Plain {
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

    fn breaking(&mut self) -> Result<Vec<(PathBuf, Vec<Diagnostic>)>, String> {
        Ok(Vec::new())
    }

    fn completion(&mut self, _: &Path) -> Option<CompletionContext> {
        Some(CompletionContext {
            schema: self.schema.clone(),
            dialect: "postgres".to_string(),
            import_style: None,
            helper_starts: Vec::new(),
        })
    }
}

pub struct Client {
    /// The result of `initialize`.
    pub initialized: Value,
    connection: Connection,
    server: Option<JoinHandle<Result<(), String>>>,
    next: i32,
}

impl Client {
    /// A server for the folder `file:///work`, initialized with `options`.
    pub fn start(options: &Value) -> Self {
        Self::with(&json!({}), options, None)
    }

    /// [`Client::start`] for a client with `capabilities`, its files checked against the
    /// PostgreSQL `schema`.
    pub fn with(capabilities: &Value, options: &Value, schema: Option<&str>) -> Self {
        let schema = schema.map(|sql| load_schema_with_skipped(sql, "postgres").0);
        let (server, connection) = Connection::memory();
        let server = std::thread::spawn(move || {
            run(&server, move |_: &Path, _: &Settings| {
                Ok(Plain {
                    schema: schema.clone(),
                })
            })
            .map_err(|error| error.to_string())
        });
        let mut client = Self {
            initialized: Value::Null,
            connection,
            server: Some(server),
            next: 0,
        };
        let folders = json!([{"uri": "file:///work", "name": "work"}]);
        client.initialized = client.request(
            "initialize",
            json!({"capabilities": capabilities, "workspaceFolders": folders, "initializationOptions": options}),
        );
        client.notify("initialized", json!({}));
        client
    }

    pub fn notify(&self, method: &str, params: Value) {
        let notification = Notification::new(method.to_string(), params);
        self.connection.sender.send(notification.into()).unwrap();
    }

    pub fn open(&self, uri: &str, text: &str) {
        let document = json!({"uri": uri, "languageId": "python", "version": 1, "text": text});
        self.notify("textDocument/didOpen", json!({"textDocument": document}));
    }

    /// The result of `method`, skipping what the server sends meanwhile.
    pub fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = RequestId::from(self.next);
        let request = Request::new(id.clone(), method.to_string(), params);
        self.connection.sender.send(request.into()).unwrap();
        loop {
            match self.receive() {
                Message::Response(response) if response.id == id => {
                    return response.response_result.unwrap_or(Value::Null);
                }
                _ => {}
            }
        }
    }

    /// The params of the next `method` notification, skipping what the server sends before.
    pub fn notification(&self, method: &str) -> Value {
        loop {
            match self.receive() {
                Message::Notification(notification) if notification.method == method => {
                    return notification.params;
                }
                _ => {}
            }
        }
    }

    /// Waits for the server to send the request `method`, and answers it.
    pub fn asked(&self, method: &str) {
        loop {
            if let Message::Request(request) = self.receive() {
                let response = Response::new_ok(request.id, Value::Null);
                self.connection.sender.send(response.into()).unwrap();
                if request.method == method {
                    return;
                }
            }
        }
    }

    fn receive(&self) -> Message {
        self.connection
            .receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
    }

    /// Semantic tokens of `uri` as `(line, character, length, type, modifiers)`, positions
    /// absolute.
    pub fn tokens(&mut self, uri: &str) -> Vec<(u64, u64, u64, u64, u64)> {
        let tokens = self.request(
            "textDocument/semanticTokens/full",
            json!({"textDocument": {"uri": uri}}),
        );
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
                (line, character, token[2], token[3], token[4])
            })
            .collect()
    }

    pub fn shutdown(mut self) {
        self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        assert_eq!(self.server.take().unwrap().join().unwrap(), Ok(()));
    }
}
