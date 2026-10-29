//! One check at a time, the newest buffer first.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use lsp_server::Response;
use lsp_types::{CodeAction, CodeActionKind, CodeActionOrCommand, MessageType, OneOf, Url};

use crate::text::{diff, LineIndex};
use crate::{actions, Job, Shared, Workspace, FIX_ALL};

pub(crate) fn work<W, F>(queue: &Receiver<Job>, shared: &Shared, roots: &[PathBuf], open: F)
where
    W: Workspace,
    F: FnMut(&Path) -> Result<W, String>,
{
    let mut workspaces = Workspaces {
        open,
        roots,
        loaded: HashMap::new(),
    };
    while let Ok(first) = queue.recv() {
        // Jobs that arrived during the last check: the last edit of a buffer is checked once.
        let mut checks: Vec<Url> = Vec::new();
        for job in std::iter::once(first).chain(queue.try_iter()) {
            match job {
                Job::Reload => workspaces.loaded.clear(),
                Job::FixAll(id, uri) => {
                    let action = fix_all(&mut workspaces, shared, &uri);
                    let actions: Vec<_> = action
                        .into_iter()
                        .map(CodeActionOrCommand::CodeAction)
                        .collect();
                    shared.send(Response::new_ok(id, actions));
                }
                Job::Check(uri) => {
                    checks.retain(|queued| *queued != uri);
                    checks.push(uri);
                }
            }
        }
        for uri in checks.into_iter().rev() {
            check(&mut workspaces, shared, uri);
        }
    }
}

struct Workspaces<'a, W, F> {
    open: F,
    roots: &'a [PathBuf],
    /// By project root; an error is shown once, when the root is first loaded.
    loaded: HashMap<PathBuf, Result<W, String>>,
}

impl<W, F> Workspaces<'_, W, F>
where
    W: Workspace,
    F: FnMut(&Path) -> Result<W, String>,
{
    fn of(&mut self, path: &Path, shared: &Shared) -> Option<&mut W> {
        let root = self
            .roots
            .iter()
            .filter(|root| path.starts_with(root))
            .max_by_key(|root| root.components().count())
            .cloned()
            .or_else(|| path.parent().map(Path::to_path_buf))?;
        let open = &mut self.open;
        let loaded = self.loaded.entry(root).or_insert_with_key(|root| {
            let loaded = open(root);
            if let Err(message) = &loaded {
                shared.message(MessageType::ERROR, format!("sqlty: {message}"));
            }
            loaded
        });
        loaded.as_mut().ok()
    }
}

fn check<W, F>(workspaces: &mut Workspaces<'_, W, F>, shared: &Shared, uri: Url)
where
    W: Workspace,
    F: FnMut(&Path) -> Result<W, String>,
{
    let Some((text, version)) = shared
        .documents()
        .get(&uri)
        .map(|d| (d.text.clone(), d.version))
    else {
        return;
    };
    let Ok(path) = uri.to_file_path() else {
        return;
    };
    let Some(workspace) = workspaces.of(&path, shared) else {
        return;
    };
    let diagnostics = match workspace.check(&path, &text) {
        Ok(diagnostics) => diagnostics,
        Err(message) => {
            shared.log(format!("sqlty: checking {}: {message}", path.display()));
            return;
        }
    };
    let mut documents = shared.documents();
    let Some(document) = documents.get_mut(&uri).filter(|d| d.version == version) else {
        return;
    };
    let index = LineIndex::new(&text, &shared.encoding);
    let published = diagnostics
        .iter()
        .map(|d| actions::diagnostic(d, &index))
        .collect();
    document.diagnostics = Some((version, diagnostics));
    drop(documents);
    shared.publish(uri, published, Some(version));
}

fn fix_all<W, F>(
    workspaces: &mut Workspaces<'_, W, F>,
    shared: &Shared,
    uri: &Url,
) -> Option<CodeAction>
where
    W: Workspace,
    F: FnMut(&Path) -> Result<W, String>,
{
    let (text, version) = shared
        .documents()
        .get(uri)
        .map(|d| (d.text.clone(), d.version))?;
    let path = uri.to_file_path().ok()?;
    let fixed = workspaces
        .of(&path, shared)?
        .fix(&path, &text)
        .map_err(|message| shared.log(format!("sqlty: fixing {}: {message}", path.display())))
        .ok()?;
    let edit = diff(&text, &fixed, &LineIndex::new(&text, &shared.encoding))?;
    Some(CodeAction {
        title: "sqlty: fix all".to_string(),
        kind: Some(CodeActionKind::from(FIX_ALL)),
        edit: Some(actions::versioned(uri, version, vec![OneOf::Left(edit)])),
        ..CodeAction::default()
    })
}
