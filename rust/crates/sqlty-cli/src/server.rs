//! `sqlty server` on [`sqlty_project`]: the language server of `sqlty-lsp`, with
//! `sqlty breaking` against the baseline `sqlty.breaking.against` names.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlty_lsp::{CompletionContext, Diagnostic, Edit, Settings, Severity, Workspace};
use sqlty_python::types::stdlib::Version;

use crate::breaking::{self, Direction, Finding, Revision};

/// Serve the language server on standard input and output until the client exits.
pub fn serve(version: Version, extra_roots: Vec<PathBuf>) -> Result<(), String> {
    let open = move |root: &Path, settings: &Settings| {
        RustWorkspace::open(root, settings, version, &extra_roots)
    };
    sqlty_lsp::stdio(open).map_err(|error| error.to_string())
}

/// A `sqlty_project::Workspace`, with the baseline its settings name; what its schemas
/// could not read goes to standard error.
pub struct RustWorkspace {
    workspace: sqlty_project::Workspace,
    baseline: Option<Baseline>,
}

impl RustWorkspace {
    /// The workspace of the project at `root`, on the disk, without a baseline.
    pub fn root(root: &Path, version: Version, extra_roots: Vec<PathBuf>) -> Result<Self, String> {
        let files = Arc::new(sqlty_project::Disk);
        let workspace = sqlty_project::Workspace::root(root, files, version, extra_roots)?;
        Ok(Self {
            workspace,
            baseline: None,
        })
    }

    /// The workspace of the project at `root`, with the baseline of `settings`.
    pub fn open(
        root: &Path,
        settings: &Settings,
        version: Version,
        extra_roots: &[PathBuf],
    ) -> Result<Self, String> {
        let mut workspace = Self::root(root, version, extra_roots.to_vec())?;
        if let Some(against) = &settings.against {
            let direction = Direction::parse(settings.direction.as_deref().unwrap_or("both"));
            let baseline = Baseline::new(root, against, direction, version, extra_roots)?;
            workspace.baseline = Some(baseline);
        }
        Ok(workspace)
    }

    fn warned<T>(&mut self, found: T) -> T {
        for warning in self.workspace.warnings() {
            eprintln!("{warning}");
        }
        found
    }
}

impl Workspace for RustWorkspace {
    fn check(&mut self, path: &Path, text: &str) -> Result<Vec<Diagnostic>, String> {
        let found = self.workspace.check(path, text);
        let mut found: Vec<Diagnostic> = self.warned(found)?.into_iter().map(diagnostic).collect();
        if let Some(baseline) = &mut self.baseline {
            if path.extension().is_none_or(|extension| extension != "sql") {
                found.extend(baseline.forward(&mut self.workspace, path, text)?);
            }
            found.extend(baseline.backward_of(path, text));
        }
        Ok(found)
    }

    fn fix(&mut self, path: &Path, text: &str) -> Result<String, String> {
        let fixed = self.workspace.fix(path, text);
        self.warned(fixed)
    }

    fn format(&mut self, path: &Path, text: &str) -> Result<String, String> {
        let formatted = self.workspace.format(path, text);
        self.warned(formatted)
    }

    fn rewrite(&mut self, path: &Path, text: &str) -> Result<String, String> {
        let rewritten = self.workspace.rewrite(path, text);
        self.warned(rewritten)
    }

    fn strip(&mut self, path: &Path, text: &str) -> Result<String, String> {
        let stripped = self.workspace.strip(path, text);
        self.warned(stripped)
    }

    fn breaking(&mut self) -> Result<Vec<(PathBuf, Vec<Diagnostic>)>, String> {
        let Some(baseline) = &mut self.baseline else {
            return Ok(Vec::new());
        };
        let paths = baseline.check_backward(&mut self.workspace)?;
        let disk = sqlty_project::Shared(Arc::new(sqlty_project::Disk));
        Ok(paths
            .into_iter()
            .map(|path| {
                let text = match self.workspace.text(&path) {
                    Some(text) => text.to_string(),
                    None => disk.text(&path).unwrap_or_default().replace("\r\n", "\n"),
                };
                let found = baseline.backward_of(&path, &text);
                (path, found)
            })
            .collect())
    }

    fn completion(&mut self, path: &Path) -> Option<CompletionContext> {
        let found = self.workspace.completion(path)?;
        Some(CompletionContext {
            schema: found.schema,
            dialect: found.dialect.to_string(),
            import_style: found.import_style,
            helper_starts: found.helper_starts,
        })
    }
}

/// `sqlty.breaking.against` in the editor: `sqlty breaking` for the buffers of a workspace.
///
/// The forward check (the buffer's queries against the baseline's schema) runs with every
/// check. The backward check (the baseline's queries against the workspace's schema) does
/// not depend on the buffers: it runs once, when the workspace is loaded, which a saved
/// schema or config does again.
struct Baseline {
    root: PathBuf,
    repository: PathBuf,
    prefix: PathBuf,
    direction: Direction,
    baselines: Vec<breaking::Baseline>,
    backward: HashMap<PathBuf, Vec<Finding>>,
    version: Version,
}

impl Baseline {
    fn new(
        root: &Path,
        against: &str,
        direction: Direction,
        version: Version,
        extra_roots: &[PathBuf],
    ) -> Result<Self, String> {
        // Findings are keyed by the editor's paths, which may run through a symlink.
        let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
        let repository = match breaking::repository(&canonical) {
            Ok(repository) => repository,
            Err(_) if canonical.join(against).exists() => canonical.clone(),
            Err(_) => {
                return Err(format!(
                    "sqlty.breaking.against: {against} does not exist, and {} is in no git \
                     repository",
                    root.display()
                ))
            }
        };
        let prefix = match canonical.strip_prefix(&repository) {
            Ok(prefix) if !prefix.as_os_str().is_empty() => prefix.to_path_buf(),
            _ => PathBuf::from("."),
        };
        let paths = [prefix.clone()];
        let baselines = breaking::baselines(
            against,
            &canonical,
            &repository,
            &paths,
            version,
            extra_roots,
        )
        .map_err(|error| format!("sqlty.breaking.against: {error}"))?;
        Ok(Self {
            root: root.to_path_buf(),
            repository,
            prefix,
            direction,
            baselines,
            backward: HashMap::new(),
            version,
        })
    }

    /// What the baseline's schema breaks in the buffer `text` of `path`.
    fn forward(
        &mut self,
        workspace: &mut sqlty_project::Workspace,
        path: &Path,
        text: &str,
    ) -> Result<Vec<Diagnostic>, String> {
        let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !self.direction.forward() || !resolved.starts_with(&self.repository) {
            return Ok(Vec::new());
        }
        let relative = resolved
            .strip_prefix(&self.repository)
            .unwrap_or(&resolved)
            .to_path_buf();
        let mut queries = Buffer {
            file: breaking::File {
                relative,
                text: Arc::from(text),
                module: None,
                names: Vec::new(),
                settings: workspace.settings(path)?,
            },
            workspace,
        };
        let names = queries.workspace.names(path).unwrap_or_default();
        let init = path.file_name().is_some_and(|name| name == "__init__.py");
        queries.file.module = names
            .first()
            .map(|name| format!("{name}{}", if init { ".__init__" } else { "" }));
        let mut found = Vec::new();
        for baseline in &mut self.baselines {
            let label = baseline.label().to_string();
            let findings = breaking::compare(
                Direction::Forward,
                &label,
                &mut queries,
                baseline.schemas(),
                self.version,
            )?;
            found.extend(findings.into_iter().map(|finding| {
                let mut found = diagnostic(finding.diagnostic);
                found.message = format!("{} (with the schema of {label})", found.message);
                found.fix = Vec::new();
                found.source = Some("sqlty breaking (forward)".to_string());
                found
            }));
        }
        Ok(found)
    }

    /// Check the baseline's queries against the workspace's schema; the files with findings.
    fn check_backward(
        &mut self,
        workspace: &mut sqlty_project::Workspace,
    ) -> Result<Vec<PathBuf>, String> {
        self.backward.clear();
        if !self.direction.backward() {
            return Ok(Vec::new());
        }
        let mut head = Revision {
            label: "the workspace".to_string(),
            root: self.repository.clone(),
            workspace,
        };
        for baseline in &mut self.baselines {
            let label = baseline.label().to_string();
            let Some(queries) = baseline.queries() else {
                continue;
            };
            let findings = breaking::compare(
                Direction::Backward,
                &label,
                queries,
                &mut head,
                self.version,
            )?;
            for finding in findings {
                let inside = match self.prefix == Path::new(".") {
                    true => &finding.path,
                    false => match finding.path.strip_prefix(&self.prefix) {
                        Ok(inside) => inside,
                        Err(_) => continue,
                    },
                };
                let path = self.root.join(inside);
                self.backward.entry(path).or_default().push(finding);
            }
        }
        let mut paths: Vec<PathBuf> = self.backward.keys().cloned().collect();
        paths.sort();
        Ok(paths)
    }

    /// The backward findings of `path`: at their spans while `text` is the baseline's, at
    /// the top of the file otherwise.
    fn backward_of(&self, path: &Path, text: &str) -> Vec<Diagnostic> {
        let Some(findings) = self.backward.get(path) else {
            return Vec::new();
        };
        findings
            .iter()
            .map(|finding| {
                let found = &finding.diagnostic;
                let same = *finding.text == *text;
                let at = match same {
                    true => String::new(),
                    false => format!(", line {}", found.line),
                };
                Diagnostic {
                    rule: found.rule.name().to_string(),
                    message: format!("{} (in the code of {}{at})", found.message, finding.against),
                    severity: severity(found.severity),
                    start: if same { found.span.start } else { 0 },
                    end: if same { found.span.end } else { 0 },
                    fix: Vec::new(),
                    source: Some("sqlty breaking (backward)".to_string()),
                }
            })
            .collect()
    }
}

/// The buffer of one file, as the queries `sqlty breaking` checks forward.
struct Buffer<'a> {
    file: breaking::File,
    workspace: &'a mut sqlty_project::Workspace,
}

impl breaking::Queries for Buffer<'_> {
    fn files(&mut self) -> Result<Vec<breaking::File>, String> {
        Ok(vec![breaking::File {
            relative: self.file.relative.clone(),
            text: Arc::clone(&self.file.text),
            module: self.file.module.clone(),
            names: self.file.names.clone(),
            settings: Arc::clone(&self.file.settings),
        }])
    }

    fn helpers(
        &mut self,
        settings: &sqlty_project::FileSettings,
    ) -> Arc<sqlty_python::index::HelperIndex> {
        self.workspace.helpers(settings)
    }
}

fn severity(severity: sqlty_python::rules::Severity) -> Severity {
    match severity {
        sqlty_python::rules::Severity::Error => Severity::Error,
        _ => Severity::Warning,
    }
}

fn diagnostic(found: sqlty_python::checker::Diagnostic) -> Diagnostic {
    Diagnostic {
        rule: found.rule.name().to_string(),
        message: found.message,
        severity: severity(found.severity),
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
