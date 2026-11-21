//! `sqlty_cli::run` on a host of its own: what a host that fails does to the command line.

#![expect(clippy::panic, reason = "the host fails on purpose")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlty_cli::{run, Host, EXIT_ERROR};
use sqlty_python::project::Files;
use sqlty_python::types::stdlib::Version;

/// A host that keeps what the commands print and panics when asked its version.
struct Crashing {
    err: String,
}

impl Host for Crashing {
    fn out(&mut self, _: &str) {}

    fn err(&mut self, text: &str) {
        self.err.push_str(text);
    }

    fn warn(&mut self, _: &str) {}

    fn error(&mut self, _: &str) {}

    fn files(&self) -> Arc<dyn Files + Send + Sync> {
        Arc::new(sqlty_project::Disk)
    }

    fn write(&mut self, _: &Path, _: &[u8]) -> Result<(), String> {
        Ok(())
    }

    fn run(&mut self, _: &[String]) -> Result<(bool, String), String> {
        Ok((true, String::new()))
    }

    fn cwd(&self) -> PathBuf {
        std::env::temp_dir()
    }

    fn env(&self, _: &str) -> Option<String> {
        None
    }

    fn python(&self) -> Version {
        (3, 14)
    }

    fn extra_roots(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn version(&self) -> String {
        panic!("boom")
    }
}

#[test]
fn a_panic_prints_a_bug_report_link() {
    let mut host = Crashing { err: String::new() };
    let code = run(["sqlty", "version"].map(OsString::from), &mut host);
    assert_eq!(code, EXIT_ERROR);
    assert_eq!(
        host.err,
        format!(
            "sqlty: crashed: boom\nThis is a bug in sqlty, please report it at {}\n",
            sqlty_python::report::issue_url("boom")
        )
    );
}
