//! `sqlty server`: the language server of `sqlty-lsp`, checking with `sqlty._lsp_bridge`.

use std::path::Path;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use sqlty_lsp::{Diagnostic, Edit, Severity, Workspace};

/// Serve the language server on standard input and output until the client exits.
#[pyfunction]
pub(crate) fn serve(py: Python<'_>) -> PyResult<()> {
    py.detach(|| sqlty_lsp::stdio(BridgeWorkspace::open).map_err(|error| error.to_string()))
        .map_err(PyRuntimeError::new_err)
}

/// A `sqlty._lsp_bridge.Workspace`.
struct BridgeWorkspace(Py<PyAny>);

impl BridgeWorkspace {
    fn open(root: &Path) -> Result<Self, String> {
        Python::attach(|py| {
            py.import("sqlty._lsp_bridge")
                .and_then(|bridge| bridge.getattr("Workspace"))
                .and_then(|workspace| workspace.call1((root.to_string_lossy(),)))
                .map(|workspace| Self(workspace.unbind()))
                .map_err(|error| error.value(py).to_string())
        })
    }

    fn call<T: for<'a, 'py> FromPyObject<'a, 'py>>(
        &self,
        method: &str,
        path: &Path,
        text: &str,
    ) -> Result<T, String> {
        Python::attach(|py| {
            self.0
                .bind(py)
                .call_method1(method, (path.to_string_lossy(), text))
                .and_then(|found| found.extract().map_err(Into::into))
                .map_err(|error| traceback(py, &error))
        })
    }
}

impl Workspace for BridgeWorkspace {
    fn check(&mut self, path: &Path, text: &str) -> Result<Vec<Diagnostic>, String> {
        let found: Vec<BridgeDiagnostic> = self.call("check", path, text)?;
        Ok(found.into_iter().map(Diagnostic::from).collect())
    }

    fn fix(&mut self, path: &Path, text: &str) -> Result<String, String> {
        self.call("fix", path, text)
    }
}

/// A `sqlty._lint.Diagnostic`.
#[derive(FromPyObject)]
struct BridgeDiagnostic {
    #[pyo3(item)]
    rule: String,
    #[pyo3(item)]
    message: String,
    #[pyo3(item)]
    severity: String,
    #[pyo3(item)]
    start: usize,
    #[pyo3(item)]
    end: usize,
    #[pyo3(item)]
    fix: Vec<(usize, usize, String)>,
}

impl From<BridgeDiagnostic> for Diagnostic {
    fn from(found: BridgeDiagnostic) -> Self {
        Self {
            rule: found.rule,
            message: found.message,
            severity: if found.severity == "error" {
                Severity::Error
            } else {
                Severity::Warning
            },
            start: found.start,
            end: found.end,
            fix: found
                .fix
                .into_iter()
                .map(|(start, end, replacement)| Edit {
                    start,
                    end,
                    replacement,
                })
                .collect(),
        }
    }
}

/// The traceback of `error`, for the output channel of the editor.
fn traceback(py: Python<'_>, error: &PyErr) -> String {
    py.import("traceback")
        .and_then(|module| module.call_method1("format_exception", (error.value(py),)))
        .and_then(|lines| lines.extract::<Vec<String>>())
        .map_or_else(|_| error.to_string(), |lines| lines.concat())
}
