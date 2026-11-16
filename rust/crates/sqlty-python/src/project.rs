//! The configuration of a directory, found as `sqlty check` finds it: the nearest of
//! [`CONFIG_FILES`] with a sqlty table, on top of the file its `extend` names.
//!
//! The engine reads no file: [`Files`] is the host's. A host that reads lazily, such as
//! the playground, answers [`Read::Unread`], gets [`FindError::Need`] with the path, reads it
//! and asks again.
//!
//! ```
//! use std::collections::HashMap;
//! use std::path::{Path, PathBuf};
//! use sqlty_python::project::{find, Read};
//!
//! let files: HashMap<PathBuf, &str> = HashMap::from([
//!     (PathBuf::from("/app/pyproject.toml"), "[tool.sqlty]\nextend = \"../base.toml\"\n"),
//!     (PathBuf::from("/base.toml"), "dialect = \"mysql\"\nschema = \"db/schema.sql\"\n"),
//! ]);
//! let read = |path: &Path| match files.get(path) {
//!     Some(text) => Read::Text(text.as_bytes().to_vec()),
//!     None => Read::Missing,
//! };
//! let found = find(Path::new("/app/pkg"), &read).unwrap().unwrap();
//! assert_eq!(found.file, Path::new("/app/pyproject.toml"));
//! assert_eq!(found.config.table["dialect"].as_str(), Some("mysql"));
//! assert_eq!(found.config.table["schema"].as_array().unwrap()[0].as_str(), Some("../db/schema.sql"));
//! ```

use std::path::{Component, Path, PathBuf};

use toml::{Table, Value};

use crate::config::{load, Config, LoadError, CONFIG_FILES};

mod glob;
mod walk;

pub use walk::{schema_files, Discovery, DEFAULT_EXCLUDE};

/// What the host has of a file.
#[derive(Clone)]
pub enum Read {
    /// Its bytes.
    Text(Vec<u8>),
    /// No such file, or not a file.
    Missing,
    /// Not read yet: [`find`] answers [`FindError::Need`].
    Unread,
}

/// What a directory entry is, as `os.walk` and `Path.is_file` tell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// A file, or a link to one.
    File,
    /// A directory.
    Dir,
    /// A link to a directory: listed, never walked into.
    LinkedDir,
    /// Anything else, such as a broken link.
    Other,
}

/// What the host has of a directory.
pub enum Listing {
    /// Its entries, by name.
    Entries(Vec<(String, Kind)>),
    /// No such directory.
    Missing,
    /// Not listed yet: the walk answers [`FindError::Need`].
    Unread,
}

/// The files of the host, by absolute path.
pub trait Files {
    /// The file at `path`.
    fn read(&self, path: &Path) -> Read;

    /// The entries of the directory at `path`.
    fn list(&self, _path: &Path) -> Listing {
        Listing::Missing
    }

    /// `path` with its links resolved, as `Path.resolve` gives it.
    fn canonical(&self, path: &Path) -> PathBuf {
        normalized(path)
    }
}

impl<F: Fn(&Path) -> Read> Files for F {
    fn read(&self, path: &Path) -> Read {
        self(path)
    }
}

/// The configuration of a directory, and the file it came from.
#[derive(Debug, PartialEq)]
pub struct Found {
    /// The table, `extend` applied: the `schema` and `queries` an extended file names are
    /// relative to the directory of `file`, like its own.
    pub config: Config,
    /// The file.
    pub file: PathBuf,
}

/// Why [`find`] has no configuration.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FindError {
    /// The host has not read this file.
    #[error("{}: not read", .0.display())]
    Need(PathBuf),
    /// A configuration file is invalid, or `extend` names one that is missing or cycles.
    #[error("{0}")]
    Invalid(String),
    /// A `pyproject.toml` or `Cargo.toml` is not TOML: `sqlty check` warns and checks with
    /// no configuration.
    #[error("{0}")]
    Syntax(String),
}

/// The nearest configuration of `directory`, an absolute path: in it and each directory
/// above, `sqlty.toml`, then `pyproject.toml` with a `[tool.sqlty]` table, then
/// `Cargo.toml` with `[package.metadata.sqlty]` or `[workspace.metadata.sqlty]`. A native
/// file without the table is skipped, so a packaging-only `pyproject.toml` does not hide
/// the workspace's configuration. Files are never merged, except through `extend`.
pub fn find(directory: &Path, files: &dyn Files) -> Result<Option<Found>, FindError> {
    let mut current = Some(normalized(directory));
    while let Some(directory) = current {
        for name in CONFIG_FILES {
            let path = directory.join(name);
            let data = match files.read(&path) {
                Read::Text(data) => data,
                Read::Missing => continue,
                Read::Unread => return Err(FindError::Need(path)),
            };
            let label = path.display().to_string();
            let config = match load(&data, name, &label, name == CONFIG_FILES[0]) {
                Ok(Some(config)) => config,
                Ok(None) => continue,
                Err(LoadError::Syntax(message)) if name != CONFIG_FILES[0] => {
                    return Err(FindError::Syntax(message))
                }
                Err(error) => return Err(FindError::Invalid(error.to_string())),
            };
            let config = extended(config, &path, &directory, &mut Vec::new(), files)?;
            return Ok(Some(Found { config, file: path }));
        }
        current = directory.parent().map(Path::to_path_buf);
    }
    Ok(None)
}

/// The configuration of the file at `path`, as `--config-file` names it: a `pyproject.toml`
/// or `Cargo.toml` must have its table; `extend` is applied, its paths relative to `base`.
pub fn load_file(path: &Path, base: &Path, files: &dyn Files) -> Result<Config, FindError> {
    let path = normalized(path);
    let config = read_required(&path, &path, files, |path| {
        format!("config file does not exist: {}", path.display())
    })?;
    extended(config, &path, &normalized(base), &mut Vec::new(), files)
}

fn read_required(
    path: &Path,
    shown: &Path,
    files: &dyn Files,
    missing: impl FnOnce(&Path) -> String,
) -> Result<Config, FindError> {
    let data = match files.read(path) {
        Read::Text(data) => data,
        Read::Missing => return Err(FindError::Invalid(missing(path))),
        Read::Unread => return Err(FindError::Need(path.to_path_buf())),
    };
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    match load(&data, name, &shown.display().to_string(), true) {
        Ok(found) => Ok(found.expect("a required table")),
        Err(error) => Err(FindError::Invalid(error.to_string())),
    }
}

/// `config` of `path` on top of the file its `extend` names, recursively. `rules`, `types`
/// and `databases` merge key by key; any other key replaces the extended one. The
/// `schema` and `queries` of an extended file are rebased onto `base`, the directory the
/// relative paths of `config` start from.
fn extended(
    mut config: Config,
    path: &Path,
    base: &Path,
    chain: &mut Vec<PathBuf>,
    files: &dyn Files,
) -> Result<Config, FindError> {
    let Some(Value::String(extend)) = config.table.remove("extend") else {
        return Ok(config);
    };
    chain.push(path.to_path_buf());
    let directory = path.parent().unwrap_or(Path::new("/"));
    let target = normalized(&directory.join(&extend));
    let shown = path.display();
    if chain.contains(&target) {
        return Err(FindError::Invalid(format!(
            "{shown}: extend = '{extend}' extends itself in a cycle"
        )));
    }
    let found = read_required(&target, &target, files, |target| {
        format!(
            "{shown}: extend = '{extend}': config file does not exist: {}",
            target.display()
        )
    })?;
    let target_directory = target.parent().unwrap_or(Path::new("/")).to_path_buf();
    let mut parent = extended(found, &target, &target_directory, chain, files)?;
    rebase(&mut parent.table, &target_directory, base);
    if let Some(Value::Table(databases)) = parent.table.get_mut("databases") {
        for (_, database) in databases.iter_mut() {
            if let Value::Table(database) = database {
                rebase(database, &target_directory, base);
            }
        }
    }
    let mut merged = parent.table;
    for (key, value) in config.table {
        match (merged.get_mut(&key), value) {
            (Some(Value::Table(under)), Value::Table(over))
                if matches!(key.as_str(), "rules" | "types" | "databases") =>
            {
                under.extend(over);
            }
            (_, value) => {
                merged.insert(key, value);
            }
        }
    }
    let mut skipped = parent.skipped_functions;
    skipped.extend(config.skipped_functions);
    Ok(Config {
        table: merged,
        skipped_functions: skipped,
    })
}

/// `schema` and `queries` of `table`, relative to `from`, made relative to `to`.
fn rebase(table: &mut Table, from: &Path, to: &Path) {
    for key in ["schema", "queries"] {
        let entries = match table.get(key) {
            Some(Value::String(entry)) => vec![entry.clone()],
            Some(Value::Array(entries)) => entries
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_string))
                .collect(),
            _ => continue,
        };
        let rebased = entries
            .iter()
            .map(|entry| Value::String(relative(&normalized(&from.join(entry)), to)))
            .collect();
        table.insert(key.into(), Value::Array(rebased));
    }
}

/// `path` without `.` and with each `..` taking off the component before it, as
/// `os.path.normpath` spells it; the file system is not asked.
pub fn normalized(path: &Path) -> PathBuf {
    let mut found = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !found.pop() {
                    found.push(component);
                }
            }
            _ => found.push(component),
        }
    }
    found
}

/// `path` relative to the directory `base`, both absolute and normalized, as
/// `os.path.relpath` spells it with `/`.
fn relative(path: &Path, base: &Path) -> String {
    let path: Vec<_> = path.components().collect();
    let base: Vec<_> = base.components().collect();
    let shared = path.iter().zip(&base).take_while(|(a, b)| a == b).count();
    let parts: Vec<String> = std::iter::repeat_n("..".to_string(), base.len() - shared)
        .chain(
            path[shared..]
                .iter()
                .map(|part| part.as_os_str().to_string_lossy().into_owned()),
        )
        .collect();
    match parts.is_empty() {
        true => ".".into(),
        false => parts.join("/"),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use std::collections::HashMap;

    use super::*;

    fn tree(files: &[(&str, &str)]) -> impl Fn(&Path) -> Read {
        let files: HashMap<PathBuf, String> = files
            .iter()
            .map(|(path, text)| (PathBuf::from(path), text.to_string()))
            .collect();
        move |path: &Path| match files.get(path) {
            Some(text) if text == "<unread>" => Read::Unread,
            Some(text) => Read::Text(text.as_bytes().to_vec()),
            None => Read::Missing,
        }
    }

    fn found(directory: &str, files: &[(&str, &str)]) -> Result<Option<Found>, FindError> {
        find(Path::new(directory), &tree(files))
    }

    #[test]
    fn the_nearest_file_with_a_table_wins() {
        let files = [
            ("/a/sqlty.toml", "dialect = \"mysql\""),
            ("/a/b/pyproject.toml", "[project]\nname = \"b\""),
            ("/a/b/c/Cargo.toml", "[package]\nname = \"c\""),
        ];
        let config = found("/a/b/c/../c/./", &files).unwrap().unwrap();
        assert_eq!(config.file, Path::new("/a/sqlty.toml"));
        assert_eq!(found("/elsewhere", &files), Ok(None));
        let root = found("/a", &[("/sqlty.toml", "dialect = \"mysql\"")]);
        assert_eq!(root.unwrap().unwrap().file, Path::new("/sqlty.toml"));
    }

    #[test]
    fn a_host_reads_what_it_is_asked_for() {
        let files = [("/a/sqlty.toml", "<unread>")];
        assert_eq!(
            found("/a", &files),
            Err(FindError::Need(PathBuf::from("/a/sqlty.toml")))
        );
        let files = [
            ("/a/sqlty.toml", "extend = \"base.toml\""),
            ("/a/base.toml", "<unread>"),
        ];
        assert_eq!(
            found("/a", &files),
            Err(FindError::Need(PathBuf::from("/a/base.toml")))
        );
    }

    #[test]
    fn errors_name_the_file() {
        let syntax = found("/a", &[("/a/pyproject.toml", "[tool")]).unwrap_err();
        assert!(
            matches!(syntax, FindError::Syntax(ref message) if message.starts_with("/a/pyproject.toml: "))
        );
        let invalid = found("/a", &[("/a/sqlty.toml", "[tool")]).unwrap_err();
        assert!(
            matches!(invalid, FindError::Invalid(ref message) if message.starts_with("/a/sqlty.toml: "))
        );
        let unknown = found("/a", &[("/a/pyproject.toml", "[tool.sqlty]\nnope = 1")]).unwrap_err();
        assert!(unknown.to_string().contains("unknown key 'nope'"));
        let missing = found("/a", &[("/a/sqlty.toml", "extend = \"../x.toml\"")]).unwrap_err();
        assert_eq!(
            missing.to_string(),
            "/a/sqlty.toml: extend = '../x.toml': config file does not exist: /x.toml"
        );
        let cycle = found(
            "/a",
            &[
                ("/a/sqlty.toml", "extend = \"b.toml\""),
                ("/a/b.toml", "extend = \"sqlty.toml\""),
            ],
        )
        .unwrap_err();
        assert_eq!(
            cycle.to_string(),
            "/a/b.toml: extend = 'sqlty.toml' extends itself in a cycle"
        );
        let tableless = found(
            "/a",
            &[
                ("/a/sqlty.toml", "extend = \"pyproject.toml\""),
                ("/a/pyproject.toml", ""),
            ],
        )
        .unwrap_err();
        assert!(tableless.to_string().contains("no [tool.sqlty] table"));
    }

    #[test]
    fn extend_merges_tables_and_rebases_paths() {
        let files = [
            (
                "/w/app/sqlty.toml",
                "extend = \"../base/sqlty.toml\"\nrules = { select-star = \"warn\" }\n\
                 [databases.main]\nschema = \"own.sql\"\n",
            ),
            (
                "/w/base/sqlty.toml",
                "dialect = \"mysql\"\nschema = [\"s/*.sql\"]\nqueries = \"q\"\n\
                 rules = { extra-param = \"error\" }\n\
                 [databases.main]\nschema = \"main.sql\"\n[databases.logs]\nschema = \"logs.sql\"\n",
            ),
        ];
        let read = tree(&files);
        let config = find(Path::new("/w/app"), &read)
            .unwrap()
            .unwrap()
            .config
            .table;
        let shown: Table = toml::from_str(
            "dialect = \"mysql\"\nschema = [\"../base/s/*.sql\"]\nqueries = [\"../base/q\"]\n\
             rules = { extra-param = \"error\", select-star = \"warn\" }\n\
             [databases.main]\nschema = \"own.sql\"\ninclude = []\nexclude = []\n\
             [databases.logs]\nschema = [\"../base/logs.sql\"]\ninclude = []\nexclude = []\n",
        )
        .unwrap();
        assert_eq!(config, shown);
    }

    #[test]
    fn a_named_file_must_have_its_table() {
        let files = [("/a/pyproject.toml", "[project]")];
        let error = load_file(
            Path::new("/a/pyproject.toml"),
            Path::new("/"),
            &tree(&files),
        );
        assert!(error
            .unwrap_err()
            .to_string()
            .contains("no [tool.sqlty] table"));
        let error = load_file(Path::new("/a/x.toml"), Path::new("/"), &tree(&files));
        assert_eq!(
            error.unwrap_err().to_string(),
            "config file does not exist: /a/x.toml"
        );
        let files = [
            ("/a/x.toml", "extend = \"b/y.toml\""),
            ("/a/b/y.toml", "schema = \"s.sql\""),
        ];
        let config = load_file(Path::new("/a/x.toml"), Path::new("/c"), &tree(&files)).unwrap();
        assert_eq!(config.table["schema"][0].as_str(), Some("../a/b/s.sql"));
    }

    #[test]
    fn paths_are_spelled_as_python_spells_them() {
        assert_eq!(normalized(Path::new("/a/./b/../c/")), Path::new("/a/c"));
        assert_eq!(normalized(Path::new("../a")), Path::new("../a"));
        assert_eq!(relative(Path::new("/a/b/c"), Path::new("/a/d")), "../b/c");
        assert_eq!(relative(Path::new("/a"), Path::new("/a")), ".");
    }
}
