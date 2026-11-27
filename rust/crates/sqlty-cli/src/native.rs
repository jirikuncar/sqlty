//! The `sqlty` executable: the commands on the real terminal and disk, reading sources as
//! the Python environment it is installed in would, or, installed outside one (npm,
//! Homebrew), the environment of the project it runs in.

use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use sqlty_python::project::Files;
use sqlty_python::types::stdlib::Version;

use crate::Host;

/// Run `sqlty` with the arguments of this process; the exit code.
pub fn main() -> i32 {
    let args: Vec<OsString> = std::env::args_os().collect();
    let executable = std::env::current_exe().and_then(std::fs::canonicalize);
    let cwd = std::env::current_dir().unwrap_or_default();
    let environment = Environment::find(&executable.unwrap_or_default(), &cwd, |name| {
        std::env::var_os(name)
    });
    let mut host = Native {
        environment: &environment,
    };
    let code = crate::run(args, &mut host);
    let _ = std::io::stdout().flush();
    code
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
    /// The environment `executable` is installed in, else the one a project in `cwd` uses,
    /// looked for as ty does: `VIRTUAL_ENV`, a non-base Conda environment, the nearest
    /// `.venv`, the base Conda environment, then `python3` from `PATH`.
    pub fn find(executable: &Path, cwd: &Path, env: impl Fn(&str) -> Option<OsString>) -> Self {
        let scripts = executable.parent().unwrap_or(Path::new("."));
        if installed_in(scripts.parent().unwrap_or(scripts)) {
            return Self::read(scripts, true);
        }
        let conda = env("CONDA_PREFIX").map(PathBuf::from);
        let base = env("CONDA_DEFAULT_ENV").is_some_and(|name| name == "base");
        let venv = || {
            cwd.ancestors()
                .map(|directory| directory.join(".venv"))
                .find(|venv| venv.join("pyvenv.cfg").is_file())
        };
        // A `.venv` comes with the checkout, so its interpreter is never run to ask a version.
        let active = env("VIRTUAL_ENV")
            .map(PathBuf::from)
            .or_else(|| conda.clone().filter(|_| !base));
        match (active, venv()) {
            (Some(prefix), _) => Self::read(&scripts_of(&prefix), true),
            (None, Some(prefix)) => Self::read(&scripts_of(&prefix), false),
            (None, None) => Self::read(
                &conda.map_or(scripts.to_path_buf(), |prefix| scripts_of(&prefix)),
                true,
            ),
        }
    }

    /// The environment whose scripts directory (`bin` or `Scripts`) is `scripts`.
    pub fn of_scripts(scripts: &Path) -> Self {
        Self::read(scripts, true)
    }

    /// The environment of `scripts`; its interpreter is asked its version only when `ask`.
    fn read(scripts: &Path, ask: bool) -> Self {
        let prefix = scripts.parent().unwrap_or(scripts);
        let interpreter = ["python", "python3", "python.exe"]
            .into_iter()
            .map(|name| scripts.join(name))
            .find(|path| path.is_file())
            .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "python" } else { "python3" }));
        let site = site_packages(prefix);
        let version = configured_version(prefix)
            .or_else(|| site.as_deref().and_then(version_of_site))
            .or_else(|| ask.then(|| asked_version(&interpreter)).flatten())
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

/// Whether `prefix` is a virtual environment, or holds this sqlty's Python package.
fn installed_in(prefix: &Path) -> bool {
    prefix.join("pyvenv.cfg").is_file()
        || site_packages(prefix).is_some_and(|site| site.join(DIST_INFO).is_dir())
}

/// Where the environment at `prefix` keeps its executables.
fn scripts_of(prefix: &Path) -> PathBuf {
    let windows = prefix.join("Scripts");
    if windows.is_dir() {
        windows
    } else {
        prefix.join("bin")
    }
}

/// The metadata directory of this sqlty's Python package.
const DIST_INFO: &str = concat!("sqlty-", env!("CARGO_PKG_VERSION"), ".dist-info");

/// The oldest Python sqlty supports.
const DEFAULT_VERSION: Version = (3, 14);

/// The `site-packages` of `prefix` that holds this sqlty, else the first there is.
fn site_packages(prefix: &Path) -> Option<PathBuf> {
    // `Lib` (Windows) or `lib`, as the disk spells it: probing both names would list one
    // directory twice where case is ignored (Windows, macOS).
    let libs: Vec<PathBuf> = std::fs::read_dir(prefix)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().eq_ignore_ascii_case("lib"))
        .map(|entry| entry.path())
        .collect();
    let mut found: Vec<PathBuf> = libs
        .iter()
        .filter_map(|lib| std::fs::read_dir(lib).ok())
        .flatten()
        .flatten()
        .map(|entry| entry.path().join("site-packages"))
        .chain(libs.iter().map(|lib| lib.join("site-packages")))
        .chain([prefix.join("site-packages")])
        .filter(|path| path.is_dir())
        .collect();
    found.sort();
    found.dedup();
    found
        .iter()
        .find(|site| site.join(DIST_INFO).is_dir())
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
