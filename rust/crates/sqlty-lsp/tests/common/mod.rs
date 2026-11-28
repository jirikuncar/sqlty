//! A client that drives `sqlty_lsp::run` over an in-memory connection.

#![expect(clippy::unwrap_used, reason = "a broken connection fails the test")]

use std::path::{Path, PathBuf};
use std::thread::JoinHandle;
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId};
use serde_json::{json, Value};
use sqlty_lsp::{run, Diagnostic, Settings, Workspace};

/// A workspace whose files have no diagnostics and whose commands change nothing.
pub struct Plain;

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
        let (server, connection) = Connection::memory();
        let server = std::thread::spawn(move || {
            run(&server, |_: &Path, _: &Settings| Ok(Plain)).map_err(|error| error.to_string())
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
            json!({"capabilities": {}, "workspaceFolders": folders, "initializationOptions": options}),
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
            let message = self
                .connection
                .receiver
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            match message {
                Message::Response(response) if response.id == id => {
                    return response.response_result.unwrap_or(Value::Null);
                }
                _ => {}
            }
        }
    }

    /// Semantic tokens of `uri` as `(line, character, length, type)`, positions absolute.
    pub fn tokens(&mut self, uri: &str) -> Vec<(u64, u64, u64, u64)> {
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
                (line, character, token[2], token[3])
            })
            .collect()
    }

    pub fn shutdown(mut self) {
        self.request("shutdown", Value::Null);
        self.notify("exit", Value::Null);
        assert_eq!(self.server.take().unwrap().join().unwrap(), Ok(()));
    }
}
