//! The `sqlty` executable: the commands on the real terminal and disk, reading sources as
//! the Python environment it is installed in would. The commands Rust does not run yet
//! (and every command with `SQLTY_AST=python`) run as `python -m sqlty` of that
//! environment.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use sqlty_python::project::Files;
use sqlty_python::types::stdlib::Version;

use crate::{Host, EXIT_ERROR};

/// Run `sqlty` with the arguments of this process; the exit code.
pub fn main() -> i32 {
    let args: Vec<OsString> = std::env::args_os().collect();
    let environment = Environment::of_executable();
    if std::env::var("SQLTY_AST").is_ok_and(|ast| ast == "python") {
        return python(&environment, &args);
    }
    let mut host = Native {
        environment: &environment,
    };
    let code = crate::run(args.clone(), &mut host);
    let _ = std::io::stdout().flush();
    code.unwrap_or_else(|| python(&environment, &args))
}

/// `python -m sqlty ARGS...` of `environment`, in place of this process where the platform
/// allows.
fn python(environment: &Environment, args: &[OsString]) -> i32 {
    let mut command = Command::new(&environment.interpreter);
    command.args(["-m", "sqlty"]).args(args.iter().skip(1));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let error = command.exec();
        eprintln!(
            "sqlty: cannot run {}: {error}",
            environment.interpreter.display()
        );
        EXIT_ERROR
    }
    #[cfg(not(unix))]
    match command.status() {
        Ok(status) => status.code().unwrap_or(EXIT_ERROR),
        Err(error) => {
            eprintln!(
                "sqlty: cannot run {}: {error}",
                environment.interpreter.display()
            );
            EXIT_ERROR
        }
    }
}

/// The Python environment `sqlty` is installed in: what its `sys.path` and `sys.version_info`
/// would be.
#[derive(Debug, PartialEq, Eq)]
pub struct Environment {
    /// The interpreter of the environment, or `python3` from `PATH`.
    pub interpreter: PathBuf,
    /// `sys.version_info[:2]`.
    pub version: Version,
    /// `sys.path` as far as modules are found in it: the scripts directory, site-packages
    /// and the directories its `.pth` files add.
    pub path: Vec<PathBuf>,
}

impl Environment {
    fn of_executable() -> Self {
        let executable = std::env::current_exe().and_then(std::fs::canonicalize);
        let executable = executable.unwrap_or_default();
        Self::of_scripts(executable.parent().unwrap_or(Path::new(".")))
    }

    /// The environment whose scripts directory (`bin` or `Scripts`) is `scripts`.
    pub fn of_scripts(scripts: &Path) -> Self {
        let prefix = scripts.parent().unwrap_or(scripts);
        let interpreter = ["python", "python3", "python.exe"]
            .into_iter()
            .map(|name| scripts.join(name))
            .find(|path| path.is_file())
            .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "python" } else { "python3" }));
        let site = site_packages(prefix);
        let version = configured_version(prefix)
            .or_else(|| site.as_deref().and_then(version_of_site))
            .or_else(|| asked_version(&interpreter))
            .unwrap_or(DEFAULT_VERSION);
        let mut path = vec![scripts.to_path_buf()];
        if let Some(site) = site {
            path.extend(pth_paths(&site));
            path.insert(1, site);
        }
        Self {
            interpreter,
            version,
            path,
        }
    }
}

/// The oldest Python sqlty supports.
const DEFAULT_VERSION: Version = (3, 14);

/// The `site-packages` of `prefix` that holds this sqlty, else the first there is.
fn site_packages(prefix: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = ["Lib", "lib"]
        .into_iter()
        .filter_map(|lib| std::fs::read_dir(prefix.join(lib)).ok())
        .flatten()
        .flatten()
        .map(|entry| entry.path().join("site-packages"))
        .chain([
            prefix.join("Lib").join("site-packages"),
            prefix.join("site-packages"),
        ])
        .filter(|path| path.is_dir())
        .collect();
    found.sort();
    found.dedup();
    let ours = format!("sqlty-{}.dist-info", env!("CARGO_PKG_VERSION"));
    found
        .iter()
        .find(|site| site.join(&ours).is_dir())
        .or(found.first())
        .cloned()
}

/// `version_info` (uv) or `version` (venv) of the environment's `pyvenv.cfg`.
fn configured_version(prefix: &Path) -> Option<Version> {
    let text = std::fs::read_to_string(prefix.join("pyvenv.cfg")).ok()?;
    let value = |wanted: &str| {
        text.lines().find_map(|line| {
            let (key, value) = line.split_once('=')?;
            (key.trim() == wanted).then(|| value.trim().to_string())
        })
    };
    value("version_info")
        .or_else(|| value("version"))
        .and_then(|version| parse_version(&version))
}

/// The version of `lib/python3.14/site-packages`, read from its parent's name.
fn version_of_site(site: &Path) -> Option<Version> {
    let name = site.parent()?.file_name()?.to_str()?;
    parse_version(name.strip_prefix("python")?.trim_end_matches('t'))
}

fn asked_version(interpreter: &Path) -> Option<Version> {
    let output = Command::new(interpreter)
        .args(["-c", "import sys; print('%d.%d' % sys.version_info[:2])"])
        .output()
        .ok()?;
    parse_version(String::from_utf8_lossy(&output.stdout).trim())
}

fn parse_version(text: &str) -> Option<Version> {
    let mut parts = text.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    Some((major, minor))
}

/// The directories the `.pth` files of `site` add to `sys.path`, as `site.addpackage` reads
/// them: in file name order, skipping comments and `import` lines, only those that exist.
fn pth_paths(site: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(site)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|extension| extension == "pth")
                && !path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with('.'))
        })
        .collect();
    files.sort();
    let mut found: Vec<PathBuf> = Vec::new();
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        for line in text.lines() {
            let line = line.trim_end();
            if line.is_empty()
                || line.starts_with('#')
                || line.starts_with("import ")
                || line.starts_with("import\t")
            {
                continue;
            }
            let path = site.join(line);
            if path.is_dir() && !found.contains(&path) {
                found.push(path);
            }
        }
    }
    found
}

struct Native<'a> {
    environment: &'a Environment,
}

impl Host for Native<'_> {
    fn out(&mut self, text: &str) {
        let _ = std::io::stdout().lock().write_all(text.as_bytes());
    }

    fn err(&mut self, text: &str) {
        let _ = std::io::stdout().flush();
        let _ = std::io::stderr().lock().write_all(text.as_bytes());
    }

    fn warn(&mut self, message: &str) {
        self.err(&format!("{message}\n"));
    }

    fn error(&mut self, message: &str) {
        self.err(&format!("{message}\n"));
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
        let output = Command::new(&command[0])
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
        self.environment.version
    }

    fn extra_roots(&self) -> Vec<PathBuf> {
        self.environment.path.clone()
    }

    fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_environment_it_is_installed_in() {
        let root = std::env::temp_dir().join(format!("sqlty-native-{}", std::process::id()));
        let site = root.join("lib/python3.13/site-packages");
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(&site).unwrap();
        std::fs::write(
            root.join("pyvenv.cfg"),
            "home = /usr\nversion_info = 3.14.2\n",
        )
        .unwrap();
        std::fs::write(
            site.join("a.pth"),
            "# comment\nimport x\n../../../src\nmissing\n",
        )
        .unwrap();
        let found = Environment::of_scripts(&root.join("bin"));
        assert_eq!(found.version, (3, 14));
        assert_eq!(found.path[..2], [root.join("bin"), site.clone()]);
        assert_eq!(found.path.len(), 3);
        assert!(found.path[2].ends_with("src"));
        std::fs::write(site.join("b.pth"), b"\xff").unwrap();
        std::fs::remove_file(root.join("pyvenv.cfg")).unwrap();
        assert_eq!(Environment::of_scripts(&root.join("bin")).path.len(), 3);
        assert_eq!(Environment::of_scripts(&root.join("bin")).version, (3, 13));
        std::fs::remove_dir_all(&root).unwrap();
        let bare = Environment::of_scripts(&root.join("bin"));
        assert_eq!(bare.path, [root.join("bin")]);
        assert_eq!(bare.version.0, 3);
    }
}
