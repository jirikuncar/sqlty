//! The language server of `sqlty server`: diagnostics, quick fixes and fix-all for Python files.
//!
//! The protocol runs on the thread that calls [`run`]. Checks run on one worker thread that
//! owns a [`Workspace`] per project root: the Python passes hold the GIL, so two checks could
//! not overlap anyway. The worker checks the newest text of each buffer and publishes the
//! result only while that text is current.

use std::collections::HashMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, PoisonError};

use crossbeam_channel::Sender;
use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::notification::{
    DidChangeTextDocument, DidChangeWatchedFiles, DidCloseTextDocument, DidOpenTextDocument,
    DidSaveTextDocument, LogMessage, Notification as _, PublishDiagnostics, ShowMessage,
};
use lsp_types::request::{CodeActionRequest, Request as _};
use lsp_types::{
    CodeActionKind, CodeActionOptions, CodeActionParams, CodeActionProviderCapability,
    InitializeParams, LogMessageParams, MessageType, PositionEncodingKind,
    PublishDiagnosticsParams, ServerCapabilities, ServerInfo, ShowMessageParams,
    TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions,
    TextDocumentSyncSaveOptions, Url,
};

mod actions;
mod text;
mod worker;

#[cfg(test)]
mod tests;

/// The code action kind of `sqlty check --fix`, run by `editor.codeActionsOnSave`.
pub const FIX_ALL: &str = "source.fixAll.sqlty";

/// One diagnostic of a buffer, with byte offsets into its text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// The lint rule, as in `# sqlty: ignore[rule]`.
    pub rule: String,
    /// What is wrong.
    pub message: String,
    /// `error` fails `sqlty check`; `warn` does not.
    pub severity: Severity,
    /// The first byte of what the diagnostic is about.
    pub start: usize,
    /// The byte after the last.
    pub end: usize,
    /// The edits of the fix, with the imports they need; empty when none applies.
    pub fix: Vec<Edit>,
    /// The check it comes from, when that is not `sqlty check`.
    pub source: Option<String>,
}

/// How a rule is configured to report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// `error`: fails `sqlty check`.
    Error,
    /// `warn`: printed, does not fail.
    Warning,
}

/// Replace the bytes `start..end` with `replacement`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// The first byte replaced.
    pub start: usize,
    /// The byte after the last replaced.
    pub end: usize,
    /// The new text.
    pub replacement: String,
}

/// The checks of one project, as `sqlty check ROOT` would run them on its buffers.
pub trait Workspace {
    /// Diagnostics of `text`, the buffer of the file `path`.
    fn check(&mut self, path: &Path, text: &str) -> Result<Vec<Diagnostic>, String>;
    /// `text` as `sqlty check --fix` would write it.
    fn fix(&mut self, path: &Path, text: &str) -> Result<String, String>;
    /// Diagnostics by file that do not depend on the buffers: what the baseline's queries
    /// get against this schema. Asked once, after the workspace loads.
    fn breaking(&mut self) -> Result<Vec<(PathBuf, Vec<Diagnostic>)>, String> {
        Ok(Vec::new())
    }
}

/// What the client configures, from the `initializationOptions` of `initialize`:
/// `{"breaking": {"against": "main", "direction": "both"}}`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// `sqlty.breaking.against`: the baseline of `sqlty breaking`; `None` checks no baseline.
    pub against: Option<String>,
    /// `sqlty.breaking.direction`: `backward`, `forward` or `both`.
    pub direction: Option<String>,
}

impl Settings {
    fn new(options: Option<&serde_json::Value>) -> Self {
        let text = |pointer: &str| {
            options
                .and_then(|options| options.pointer(pointer))
                .and_then(serde_json::Value::as_str)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
        };
        Self {
            against: text("/breaking/against"),
            direction: text("/breaking/direction"),
        }
    }
}

/// Serve on standard input and output, as `sqlty server` does.
pub fn stdio<W, F>(open: F) -> Result<(), Box<dyn Error + Send + Sync>>
where
    W: Workspace,
    F: FnMut(&Path, &Settings) -> Result<W, String> + Send + 'static,
{
    let (connection, io) = Connection::stdio();
    run(&connection, open)?;
    drop(connection);
    io.join()?;
    Ok(())
}

/// Serve `connection` until the client shuts it down. `open` loads the workspace of a
/// project root with the client's settings; its error is shown to the user, and the
/// root's files go unchecked.
pub fn run<W, F>(connection: &Connection, open: F) -> Result<(), Box<dyn Error + Send + Sync>>
where
    W: Workspace,
    F: FnMut(&Path, &Settings) -> Result<W, String> + Send + 'static,
{
    let (id, params) = connection.initialize_start()?;
    let params: InitializeParams = serde_json::from_value(params)?;
    let encoding = encoding(&params);
    connection.initialize_finish(
        id,
        serde_json::to_value(lsp_types::InitializeResult {
            capabilities: capabilities(encoding.clone()),
            server_info: Some(ServerInfo {
                name: "sqlty".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
        })?,
    )?;
    let shared = Shared {
        documents: Arc::default(),
        background: Arc::default(),
        sender: connection.sender.clone(),
        encoding,
    };
    let (jobs, queue) = mpsc::channel();
    let worker = {
        let shared = shared.clone();
        let roots = roots(&params);
        let settings = Settings::new(params.initialization_options.as_ref());
        std::thread::spawn(move || worker::work(&queue, &shared, &roots, &settings, open))
    };
    for message in &connection.receiver {
        match message {
            Message::Request(request) => {
                if connection.handle_shutdown(&request)? {
                    break;
                }
                shared.request(request, &jobs);
            }
            Message::Notification(notification) => shared.notify(notification, &jobs),
            Message::Response(_) => {}
        }
    }
    drop(jobs);
    worker.join().map_err(|_| "the check worker panicked")?;
    Ok(())
}

/// What the worker does next.
enum Job {
    Check(Url),
    FixAll(lsp_server::RequestId, Url),
    Reload,
}

struct Document {
    text: String,
    version: i32,
    /// The diagnostics of `version`, the base of its quick fixes.
    diagnostics: Option<(i32, Vec<Diagnostic>)>,
}

#[derive(Clone)]
struct Shared {
    documents: Arc<Mutex<HashMap<Url, Document>>>,
    /// What [`Workspace::breaking`] found, by file: published while the file is not open.
    background: Arc<Mutex<HashMap<Url, Vec<lsp_types::Diagnostic>>>>,
    sender: Sender<Message>,
    encoding: PositionEncodingKind,
}

impl Shared {
    fn documents(&self) -> std::sync::MutexGuard<'_, HashMap<Url, Document>> {
        self.documents
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn background(&self) -> std::sync::MutexGuard<'_, HashMap<Url, Vec<lsp_types::Diagnostic>>> {
        self.background
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn send(&self, message: impl Into<Message>) {
        let _ = self.sender.send(message.into());
    }

    /// A message the user sees.
    fn message(&self, typ: MessageType, message: String) {
        self.send(Notification::new(
            ShowMessage::METHOD.to_string(),
            ShowMessageParams { typ, message },
        ));
    }

    /// A line in the output channel of the client.
    fn log(&self, message: String) {
        self.send(Notification::new(
            LogMessage::METHOD.to_string(),
            LogMessageParams {
                typ: MessageType::ERROR,
                message,
            },
        ));
    }

    fn publish(&self, uri: Url, diagnostics: Vec<lsp_types::Diagnostic>, version: Option<i32>) {
        self.send(Notification::new(
            PublishDiagnostics::METHOD.to_string(),
            PublishDiagnosticsParams::new(uri, diagnostics, version),
        ));
    }

    fn notify(&self, notification: Notification, jobs: &mpsc::Sender<Job>) {
        let params = notification.params;
        match notification.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let Ok(params) =
                    serde_json::from_value::<lsp_types::DidOpenTextDocumentParams>(params)
                else {
                    return;
                };
                let document = params.text_document;
                if document.language_id != "python" {
                    return;
                }
                self.documents().insert(
                    document.uri.clone(),
                    Document {
                        text: document.text,
                        version: document.version,
                        diagnostics: None,
                    },
                );
                let _ = jobs.send(Job::Check(document.uri));
            }
            DidChangeTextDocument::METHOD => {
                let Ok(params) =
                    serde_json::from_value::<lsp_types::DidChangeTextDocumentParams>(params)
                else {
                    return;
                };
                let uri = params.text_document.uri;
                let Some(change) = params.content_changes.into_iter().last() else {
                    return;
                };
                if let Some(document) = self.documents().get_mut(&uri) {
                    document.text = change.text;
                    document.version = params.text_document.version;
                    let _ = jobs.send(Job::Check(uri));
                }
            }
            DidCloseTextDocument::METHOD => {
                let Ok(params) =
                    serde_json::from_value::<lsp_types::DidCloseTextDocumentParams>(params)
                else {
                    return;
                };
                let uri = params.text_document.uri;
                if self.documents().remove(&uri).is_some() {
                    let background = self.background().get(&uri).cloned();
                    self.publish(uri, background.unwrap_or_default(), None);
                }
            }
            DidSaveTextDocument::METHOD => {
                let Ok(params) =
                    serde_json::from_value::<lsp_types::DidSaveTextDocumentParams>(params)
                else {
                    return;
                };
                if is_config(&params.text_document.uri) {
                    self.reload(jobs);
                }
            }
            DidChangeWatchedFiles::METHOD => {
                let Ok(params) =
                    serde_json::from_value::<lsp_types::DidChangeWatchedFilesParams>(params)
                else {
                    return;
                };
                if params.changes.iter().any(|change| is_config(&change.uri)) {
                    self.reload(jobs);
                }
            }
            _ => {}
        }
    }

    /// Read the projects again, then check every open buffer.
    fn reload(&self, jobs: &mpsc::Sender<Job>) {
        let _ = jobs.send(Job::Reload);
        for uri in self.documents().keys() {
            let _ = jobs.send(Job::Check(uri.clone()));
        }
    }

    fn request(&self, request: Request, jobs: &mpsc::Sender<Job>) {
        if request.method != CodeActionRequest::METHOD {
            self.send(Response::new_err(
                request.id,
                ErrorCode::MethodNotFound as i32,
                format!("sqlty does not handle {}", request.method),
            ));
            return;
        }
        let Ok(params) = serde_json::from_value::<CodeActionParams>(request.params) else {
            self.send(Response::new_err(
                request.id,
                ErrorCode::InvalidParams as i32,
                "invalid code action parameters".to_string(),
            ));
            return;
        };
        let wants = |kind: &str| {
            params.context.only.as_ref().is_none_or(|only| {
                only.iter().any(|asked| {
                    kind == asked.as_str() || kind.starts_with(&format!("{}.", asked.as_str()))
                })
            })
        };
        if params.context.only.is_some() && wants(FIX_ALL) {
            let _ = jobs.send(Job::FixAll(request.id, params.text_document.uri));
            return;
        }
        let actions = if wants(CodeActionKind::QUICKFIX.as_str()) {
            actions::quick_fixes(&self.documents(), &params, &self.encoding)
        } else {
            Vec::new()
        };
        self.send(Response::new_ok(request.id, actions));
    }
}

/// UTF-8 when the client offers it, as offsets need no conversion then; else UTF-16.
fn encoding(params: &InitializeParams) -> PositionEncodingKind {
    let offered = params
        .capabilities
        .general
        .as_ref()
        .and_then(|general| general.position_encodings.as_ref());
    if offered.is_some_and(|kinds| kinds.contains(&PositionEncodingKind::UTF8)) {
        PositionEncodingKind::UTF8
    } else {
        PositionEncodingKind::UTF16
    }
}

fn capabilities(encoding: PositionEncodingKind) -> ServerCapabilities {
    ServerCapabilities {
        position_encoding: Some(encoding),
        text_document_sync: Some(TextDocumentSyncCapability::Options(
            TextDocumentSyncOptions {
                open_close: Some(true),
                change: Some(TextDocumentSyncKind::FULL),
                save: Some(TextDocumentSyncSaveOptions::Supported(true)),
                ..TextDocumentSyncOptions::default()
            },
        )),
        code_action_provider: Some(CodeActionProviderCapability::Options(CodeActionOptions {
            code_action_kinds: Some(vec![
                CodeActionKind::QUICKFIX,
                CodeActionKind::from(FIX_ALL),
            ]),
            ..CodeActionOptions::default()
        })),
        ..ServerCapabilities::default()
    }
}

/// The workspace folders, else the root of the client; a file outside them all is its
/// directory's project.
#[allow(deprecated)]
fn roots(params: &InitializeParams) -> Vec<PathBuf> {
    let folders = params
        .workspace_folders
        .iter()
        .flatten()
        .map(|folder| &folder.uri);
    folders
        .chain(params.root_uri.as_ref())
        .filter_map(|uri| uri.to_file_path().ok())
        .collect()
}

/// Whether a change to `uri` changes the settings or the schema of its project.
fn is_config(uri: &Url) -> bool {
    let path = uri.path();
    path.ends_with("/pyproject.toml") || path.ends_with(".sql")
}
