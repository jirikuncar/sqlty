//! `sqlty` from Python: the commands of `sqlty-cli`, writing to `sys.stdout` and
//! `sys.stderr` and warning through `logging`.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use pyo3::prelude::*;
use sqlty_python::project::Files;
use sqlty_python::types::stdlib::Version;

/// Run `sqlty ARGS...` (`args[0]` is the program); the exit code, or None for a command
/// the Python CLI still runs.
#[pyfunction]
pub(crate) fn cli(py: Python<'_>, args: Vec<OsString>) -> PyResult<Option<i32>> {
    let sys = py.import("sys")?;
    let info = sys.getattr("version_info")?;
    let mut host = PyHost {
        python: (
            info.getattr("major")?.extract()?,
            info.getattr("minor")?.extract()?,
        ),
        extra_roots: sys.getattr("path")?.extract()?,
        error: None,
    };
    let code = py.detach(|| sqlty_cli::run(args, &mut host));
    match host.error {
        Some(error) => Err(error),
        None => Ok(code),
    }
}

struct PyHost {
    python: Version,
    extra_roots: Vec<PathBuf>,
    error: Option<PyErr>,
}

impl PyHost {
    fn call(&mut self, f: impl FnOnce(Python<'_>) -> PyResult<()>) {
        if self.error.is_none() {
            self.error = Python::attach(f).err();
        }
    }

    fn stream(&mut self, name: &str, text: &str) {
        self.call(|py| {
            py.import("sys")?
                .getattr(name)?
                .call_method1("write", (text,))?;
            Ok(())
        });
    }

    fn log(&mut self, level: &str, message: &str) {
        self.call(|py| {
            let logger = py
                .import("logging")?
                .call_method1("getLogger", ("sqlty._cli",))?;
            logger.call_method1(level, ("%s", message))?;
            Ok(())
        });
    }
}

impl sqlty_cli::Host for PyHost {
    fn out(&mut self, text: &str) {
        self.stream("stdout", text);
    }

    fn err(&mut self, text: &str) {
        self.stream("stderr", text);
    }

    fn warn(&mut self, message: &str) {
        self.log("warning", message);
    }

    fn error(&mut self, message: &str) {
        self.log("error", message);
    }

    fn files(&self) -> Arc<dyn Files + Send + Sync> {
        Arc::new(sqlty_project::Disk)
    }

    fn write(&mut self, path: &Path, data: &[u8]) -> Result<(), String> {
        let parent = path.parent().unwrap_or(Path::new("."));
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        std::fs::write(path, data).map_err(|error| error.to_string())
    }

    fn run(&mut self, command: &[String]) -> Result<(bool, String), String> {
        let output = std::process::Command::new(&command[0])
            .args(&command[1..])
            .stdin(std::process::Stdio::null())
            .output()
            .map_err(|error| error.to_string())?;
        Ok((
            output.status.success(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        ))
    }

    fn cwd(&self) -> PathBuf {
        std::env::current_dir().unwrap_or_default()
    }

    fn env(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn python(&self) -> Version {
        self.python
    }

    fn extra_roots(&self) -> Vec<PathBuf> {
        self.extra_roots.clone()
    }

    fn version(&self) -> String {
        Python::attach(|py| {
            py.import("sqlty")
                .and_then(|sqlty| sqlty.getattr("__version__"))
                .and_then(|version| version.extract())
                .unwrap_or_default()
        })
    }
}
