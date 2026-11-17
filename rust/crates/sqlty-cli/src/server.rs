//! `sqlty server` on [`sqlty_project`]: the language server of `sqlty-lsp`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlty_lsp::{CompletionContext, Diagnostic, Edit, Settings, Severity, Workspace};
use sqlty_python::types::stdlib::Version;

/// What a breaking baseline needs, which only the Python package runs yet.
pub const NO_BASELINE: &str =
    "sqlty.breaking.against needs sqlty server of the sqlty Python package (pip install sqlty)";

/// Serve the language server on standard input and output until the client exits; a
/// breaking baseline is refused.
pub fn serve(version: Version, extra_roots: Vec<PathBuf>) -> Result<(), String> {
    let open = move |root: &Path, settings: &Settings| match settings.against {
        Some(_) => Err(NO_BASELINE.to_string()),
        None => RustWorkspace::root(root, version, extra_roots.clone()),
    };
    sqlty_lsp::stdio(open).map_err(|error| error.to_string())
}

/// A `sqlty_project::Workspace`; what its schemas could not read goes to standard error.
pub struct RustWorkspace(sqlty_project::Workspace);

impl RustWorkspace {
    /// The workspace of the project at `root`, on the disk.
    pub fn root(root: &Path, version: Version, extra_roots: Vec<PathBuf>) -> Result<Self, String> {
        let files = Arc::new(sqlty_project::Disk);
        sqlty_project::Workspace::root(root, files, version, extra_roots).map(Self)
    }

    fn warned<T>(&mut self, found: T) -> T {
        for warning in self.0.warnings() {
            eprintln!("{warning}");
        }
        found
    }
}

impl Workspace for RustWorkspace {
    fn check(&mut self, path: &Path, text: &str) -> Result<Vec<Diagnostic>, String> {
        let found = self.0.check(path, text);
        let found = self.warned(found)?;
        Ok(found.into_iter().map(diagnostic).collect())
    }

    fn fix(&mut self, path: &Path, text: &str) -> Result<String, String> {
        let fixed = self.0.fix(path, text);
        self.warned(fixed)
    }

    fn format(&mut self, path: &Path, text: &str) -> Result<String, String> {
        let formatted = self.0.format(path, text);
        self.warned(formatted)
    }

    fn rewrite(&mut self, path: &Path, text: &str) -> Result<String, String> {
        let rewritten = self.0.rewrite(path, text);
        self.warned(rewritten)
    }

    fn strip(&mut self, path: &Path, text: &str) -> Result<String, String> {
        let stripped = self.0.strip(path, text);
        self.warned(stripped)
    }

    fn completion(&mut self, path: &Path) -> Option<CompletionContext> {
        let found = self.0.completion(path)?;
        Some(CompletionContext {
            schema: found.schema,
            dialect: found.dialect.to_string(),
            import_style: found.import_style,
            helper_starts: found.helper_starts,
        })
    }
}

fn diagnostic(found: sqlty_python::checker::Diagnostic) -> Diagnostic {
    Diagnostic {
        rule: found.rule.name().to_string(),
        message: found.message,
        severity: if found.severity == sqlty_python::rules::Severity::Error {
            Severity::Error
        } else {
            Severity::Warning
        },
        start: found.span.start,
        end: found.span.end,
        fix: found
            .fix
            .into_iter()
            .map(|edit| Edit {
                start: edit.range.start,
                end: edit.range.end,
                replacement: edit.replacement,
            })
            .collect(),
        source: None,
    }
}
