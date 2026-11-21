//! The files `sqlty check` reads: the `.py` files under a directory, without what the
//! excludes and `.gitignore` leave out, and the files a `schema` or `queries` entry names.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use toml::{Table, Value};

use super::glob::{self, Excludes, Segment};
use super::route::{self, Route};
use super::{find_in, normalized, Files, FindError, Found, Kind, Listing, Read};

/// The globs `exclude` has when the configuration does not set it.
pub const DEFAULT_EXCLUDE: [&str; 25] = [
    ".bzr",
    ".direnv",
    ".eggs",
    ".git",
    ".git-rewrite",
    ".hg",
    ".ipynb_checkpoints",
    ".mypy_cache",
    ".nox",
    ".pants.d",
    ".pyenv",
    ".pytest_cache",
    ".pytype",
    ".ruff_cache",
    ".svn",
    ".tox",
    ".venv",
    ".vscode",
    "__pypackages__",
    "_build",
    "buck-out",
    "dist",
    "node_modules",
    "site-packages",
    "venv",
];

/// How a host picks the files to check: by default the excludes and `respect-gitignore` of
/// each directory's configuration; `fixed` (`--config-file`) gives one configuration for
/// every directory, `isolated` none; `exclude` replaces the configured excludes and
/// `extend_exclude` adds to them, both relative to `cwd`.
pub struct Discovery {
    files: Box<dyn Files + Send + Sync>,
    cwd: PathBuf,
    isolated: bool,
    fixed: Option<Table>,
    exclude: Option<Vec<String>>,
    extend_exclude: Vec<String>,
    respect_gitignore: Option<bool>,
    state: Mutex<State>,
    excludes: Excludes,
}

#[derive(Default)]
struct State {
    configs: HashMap<PathBuf, Option<(Table, PathBuf)>>,
    found: HashMap<PathBuf, Result<Option<Arc<Found>>, FindError>>,
    chains: HashMap<PathBuf, Option<Chain>>,
}

type Rules = Vec<(Option<String>, Vec<String>)>;

/// The `.gitignore` specs of a directory, from the work tree's root down.
type Chain = Arc<Vec<(PathBuf, Gitignore)>>;

impl Discovery {
    /// `cwd` is the absolute directory relative globs start from.
    pub fn new(files: Box<dyn Files + Send + Sync>, cwd: &Path) -> Self {
        Self {
            files,
            cwd: normalized(cwd),
            isolated: false,
            fixed: None,
            exclude: None,
            extend_exclude: Vec::new(),
            respect_gitignore: None,
            state: Mutex::default(),
            excludes: Excludes::default(),
        }
    }

    /// No configuration file at all (`--isolated`).
    pub fn isolated(mut self) -> Self {
        self.isolated = true;
        self
    }

    /// One configuration for every directory (`--config-file`).
    pub fn fixed(mut self, table: Table) -> Self {
        self.fixed = Some(table);
        self
    }

    /// `--exclude`, replacing the configured excludes.
    pub fn exclude(mut self, globs: &[String]) -> Self {
        self.exclude = Some(self.relative_globs(globs));
        self
    }

    /// `--extend-exclude`, added to the configured excludes.
    pub fn extend_exclude(mut self, globs: &[String]) -> Self {
        self.extend_exclude = self.relative_globs(globs);
        self
    }

    /// `--respect-gitignore` or `--no-respect-gitignore`.
    pub fn respect_gitignore(mut self, respect: bool) -> Self {
        self.respect_gitignore = Some(respect);
        self
    }

    /// Command-line globs relative to `cwd`: `./gen` and `$PWD/gen` are `gen`.
    fn relative_globs(&self, globs: &[String]) -> Vec<String> {
        globs
            .iter()
            .map(|glob| {
                let mut glob = match Path::new(glob).strip_prefix(&self.cwd) {
                    Ok(relative) if Path::new(glob).is_absolute() => {
                        let relative = relative.to_string_lossy().replace('\\', "/");
                        if relative.is_empty() {
                            ".".to_string()
                        } else {
                            relative
                        }
                    }
                    _ => glob.clone(),
                };
                while let Some(rest) = glob.strip_prefix("./") {
                    glob = rest.to_string();
                }
                glob
            })
            .collect()
    }

    /// The configuration governing `directory` and the directory its paths start from.
    fn config(&self, directory: &Path) -> Result<Option<(Table, PathBuf)>, FindError> {
        let directory = self.resolved(directory);
        if let Some(found) = self.state().configs.get(&directory) {
            return Ok(found.clone());
        }
        // A file that is not TOML excludes nothing; checking reports it.
        let found = match self.find(&directory) {
            Err(FindError::Syntax(_)) => None,
            found => found?,
        };
        let found = found.map(|found| {
            let base = found.file.parent().unwrap_or(Path::new("/")).to_path_buf();
            (found.config.table.clone(), base)
        });
        self.state().configs.insert(directory, found.clone());
        Ok(found)
    }

    /// [`find`] of the absolute normalized `directory`, each directory read once; a file
    /// the host has not read yet is asked for again.
    pub fn find(&self, directory: &Path) -> Result<Option<Arc<Found>>, FindError> {
        if let Some(found) = self.state().found.get(directory) {
            return found.clone();
        }
        let found = match find_in(directory, &*self.files) {
            Ok(None) => directory
                .parent()
                .map_or(Ok(None), |parent| self.find(parent)),
            found => found.map(|found| found.map(Arc::new)),
        };
        if !matches!(found, Err(FindError::Need(_))) {
            self.state()
                .found
                .insert(directory.to_path_buf(), found.clone());
        }
        found
    }

    /// `(base directory, globs)` pairs excluding entries of `directory`; empty sets left
    /// out.
    pub fn exclusions(&self, directory: &Path) -> Result<Vec<(PathBuf, Vec<String>)>, FindError> {
        let (config, base) = match &self.fixed {
            Some(table) => (table.clone(), self.cwd.clone()),
            None => {
                let found = match self.isolated {
                    true => None,
                    false => self.config(directory)?,
                };
                let resolved = self.resolved(directory);
                let fallback = match resolved.starts_with(&self.cwd) {
                    true => self.cwd.clone(),
                    false => resolved,
                };
                match found {
                    Some((table, base)) => (table, base),
                    None => (Table::new(), fallback),
                }
            }
        };
        let globs = |key: &str| -> Option<Vec<String>> {
            config.get(key).and_then(Value::as_array).map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect()
            })
        };
        let extend = globs("extend-exclude").unwrap_or_default();
        let mut rules = match &self.exclude {
            None => {
                let mut own = globs("exclude")
                    .unwrap_or_else(|| DEFAULT_EXCLUDE.iter().map(ToString::to_string).collect());
                own.extend(extend);
                vec![(base, own)]
            }
            Some(exclude) => vec![(self.cwd.clone(), exclude.clone()), (base, extend)],
        };
        rules.push((self.cwd.clone(), self.extend_exclude.clone()));
        rules.retain(|(_, globs)| !globs.is_empty());
        Ok(rules)
    }

    /// Whether walking `directory` skips what `.gitignore` ignores.
    pub fn respects_gitignore(&self, directory: &Path) -> Result<bool, FindError> {
        if let Some(respect) = self.respect_gitignore {
            return Ok(respect);
        }
        let config = match (&self.fixed, self.isolated) {
            (Some(table), _) => Some(table.clone()),
            (None, true) => None,
            (None, false) => self.config(directory)?.map(|(table, _)| table),
        };
        Ok(config
            .and_then(|table| table.get("respect-gitignore").and_then(Value::as_bool))
            .unwrap_or(true))
    }

    /// The route of the file at `path` in the project of `table`, whose paths are relative
    /// to `base`.
    pub fn route(&self, table: &Table, base: &Path, path: &Path) -> Route {
        route::route(
            table,
            base,
            &self.absolute(path),
            &*self.files,
            &self.excludes,
        )
    }

    /// Whether `path`, or a directory holding it up to a glob's base, is excluded.
    pub fn is_excluded(&self, path: &Path) -> Result<bool, FindError> {
        let resolved = self.resolved(path);
        let parent = resolved.parent().unwrap_or(Path::new("/"));
        let name = file_name(&resolved);
        for (base, globs) in self.exclusions(parent)? {
            let Ok(relative) = resolved.strip_prefix(&base) else {
                if self.excluded_entry(&name, None, &globs) {
                    return Ok(true);
                }
                continue;
            };
            let parts: Vec<String> = relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();
            for (index, part) in parts.iter().enumerate() {
                let directory = match index {
                    0 => ".".to_string(),
                    _ => parts[..index].join("/"),
                };
                if self.excluded_entry(part, Some(&directory), &globs) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Whether `path` or a directory holding it matches one of `globs` relative to `base`,
    /// as excludes match.
    pub fn matches(&self, path: &Path, base: &Path, globs: &[String]) -> bool {
        self.excludes.path(&self.resolved(path), base, globs)
    }

    /// Whether the entry `name` of `directory` (relative to the globs' base, `None` outside
    /// it, where only globs without a slash match) matches an exclude glob. A glob without
    /// a slash matches a name at any depth; every glob matches the relative path.
    fn excluded_entry(&self, name: &str, directory: Option<&str>, globs: &[String]) -> bool {
        self.excludes.entry(name, directory, globs)
    }

    /// The `.py` files under `root`, sorted, without excluded directories and files.
    pub fn walk(&self, root: &Path) -> Result<Vec<PathBuf>, FindError> {
        let mut found = Vec::new();
        let mut todo = vec![root.to_path_buf()];
        while let Some(current) = todo.pop() {
            let (files, directories) = self.entries(&current)?;
            found.extend(files);
            todo.extend(directories.into_iter().rev());
        }
        found.sort();
        Ok(found)
    }

    /// The Python files and the directories, sorted, that [`Discovery::walk`] takes from
    /// the directory `current`: a host walks the directories on its own threads.
    pub fn entries(&self, current: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>), FindError> {
        let resolved = self.resolved(current);
        let entries = match self.files.list(&self.absolute(current)) {
            Listing::Entries(entries) => entries,
            Listing::Missing => return Ok((Vec::new(), Vec::new())),
            Listing::Unread => return Err(FindError::Need(self.absolute(current))),
        };
        let rules: Rules = self
            .exclusions(current)?
            .into_iter()
            .map(|(base, globs)| {
                let relative = resolved.strip_prefix(&base).ok().map(|relative| {
                    match relative.to_string_lossy().replace('\\', "/") {
                        relative if relative.is_empty() => ".".to_string(),
                        relative => relative,
                    }
                });
                (relative, globs)
            })
            .collect();
        let respects = self.respects_gitignore(current)?;
        let (mut files, mut directories) = (Vec::new(), Vec::new());
        for (name, kind) in entries {
            let is_dir = matches!(kind, Kind::Dir | Kind::LinkedDir);
            if !is_dir && !name.ends_with(".py") {
                continue;
            }
            let excluded = rules
                .iter()
                .any(|(directory, globs)| self.excluded_entry(&name, directory.as_deref(), globs));
            if excluded || (respects && self.ignored(&resolved, &name, is_dir)?) {
                continue;
            }
            match kind {
                Kind::Dir => directories.push(current.join(&name)),
                Kind::LinkedDir => {}
                Kind::File | Kind::Other => files.push(current.join(&name)),
            }
        }
        directories.sort();
        Ok((files, directories))
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// `path` against `cwd`, its links resolved.
    fn resolved(&self, path: &Path) -> PathBuf {
        self.files.canonical(&self.absolute(path))
    }

    fn absolute(&self, path: &Path) -> PathBuf {
        normalized(&self.cwd.join(path))
    }

    /// Whether the `.gitignore` files of its repository ignore the entry `name` of the
    /// resolved `directory`: those from the root of the work tree down, a deeper file and a
    /// later line overriding, `.git/info/exclude` first.
    fn ignored(&self, directory: &Path, name: &str, is_dir: bool) -> Result<bool, FindError> {
        let chain = self.chain(directory)?;
        let path = directory.join(name);
        let mut verdict = false;
        for (_, spec) in chain.iter().flat_map(|chain| chain.iter()) {
            let matched = spec.matched(&path, is_dir);
            if matched.is_ignore() {
                verdict = true;
            } else if matched.is_whitelist() {
                verdict = false;
            }
        }
        Ok(verdict)
    }

    fn chain(&self, directory: &Path) -> Result<Option<Chain>, FindError> {
        if let Some(chain) = self.state().chains.get(directory) {
            return Ok(chain.clone());
        }
        let in_repository = match self.files.list(directory) {
            Listing::Entries(entries) => entries.iter().any(|(name, _)| name == ".git"),
            Listing::Missing => false,
            Listing::Unread => return Err(FindError::Need(directory.to_path_buf())),
        };
        let chain = match (in_repository, directory.parent()) {
            (true, _) => Some(Arc::new(
                self.spec(directory, &directory.join(".git/info/exclude"))?,
            )),
            (false, None) => None,
            (false, Some(parent)) => self.chain(parent)?,
        };
        let chain = match chain {
            Some(chain) => {
                let own = self.spec(directory, &directory.join(".gitignore"))?;
                Some(match own.is_empty() {
                    true => chain,
                    false => Arc::new(chain.iter().cloned().chain(own).collect()),
                })
            }
            None => None,
        };
        self.state()
            .chains
            .insert(directory.to_path_buf(), chain.clone());
        Ok(chain)
    }

    fn spec(&self, base: &Path, path: &Path) -> Result<Vec<(PathBuf, Gitignore)>, FindError> {
        let text = match self.files.read(path) {
            Read::Text(data) => match String::from_utf8(data) {
                Ok(text) => text,
                Err(_) => return Ok(Vec::new()),
            },
            Read::Missing => return Ok(Vec::new()),
            Read::Unread => return Err(FindError::Need(path.to_path_buf())),
        };
        let mut builder = GitignoreBuilder::new(base);
        for line in text.lines() {
            // A line git does not read is left out, as git leaves it out.
            let _ = builder.add_line(None, line);
        }
        Ok(builder
            .build()
            .map(|spec| vec![(base.to_path_buf(), spec)])
            .unwrap_or_default())
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The files `entries` of a `schema` or `queries` key name, relative to `base`, in order,
/// each once: a file, a directory (every `*.sql` under it), or a glob (`*`, `?`, `[`; `**`
/// recurses), whose matches are sorted. A plain path is kept even if it does not exist, so
/// the host reports it. Also the globs that match no file.
pub fn schema_files(
    entries: &[String],
    base: &Path,
    files: &dyn Files,
) -> Result<(Vec<PathBuf>, Vec<String>), FindError> {
    let mut found: Vec<PathBuf> = Vec::new();
    let mut seen = HashSet::new();
    let mut empty = Vec::new();
    for entry in entries {
        let path = base.join(entry);
        let matched = if glob::has_magic(entry) {
            let matched = glob_files(base, entry, files)?;
            if matched.is_empty() {
                empty.push(entry.clone());
            }
            matched
        } else {
            match files.list(&path) {
                Listing::Entries(_) => sql_files(&path, files)?,
                Listing::Unread => return Err(FindError::Need(path)),
                Listing::Missing => vec![path],
            }
        };
        for file in matched {
            if seen.insert(file.clone()) {
                found.push(file);
            }
        }
    }
    Ok((found, empty))
}

/// Every `*.sql` file under `directory`, sorted by path, not walking into linked
/// directories.
fn sql_files(directory: &Path, files: &dyn Files) -> Result<Vec<PathBuf>, FindError> {
    let mut found = Vec::new();
    let mut todo = vec![directory.to_path_buf()];
    while let Some(current) = todo.pop() {
        let entries = match files.list(&current) {
            Listing::Entries(entries) => entries,
            Listing::Missing => continue,
            Listing::Unread => return Err(FindError::Need(current)),
        };
        for (name, kind) in entries {
            match kind {
                Kind::Dir => todo.push(current.join(&name)),
                Kind::File if name.ends_with(".sql") => found.push(current.join(&name)),
                _ => {}
            }
        }
    }
    found.sort();
    Ok(found)
}

/// The files `pattern` matches under `base`, as `glob.glob(pattern, root_dir=base,
/// recursive=True)` lists them, sorted as their relative paths sort.
fn glob_files(base: &Path, pattern: &str, files: &dyn Files) -> Result<Vec<PathBuf>, FindError> {
    let (root, pattern) = match pattern.strip_prefix('/') {
        Some(rest) => (PathBuf::from("/"), rest),
        None => (base.to_path_buf(), pattern),
    };
    let parts: Vec<&str> = pattern.split('/').filter(|part| !part.is_empty()).collect();
    // `(relative path, kind)` of what the parts so far match; the root first.
    let mut matched: Vec<(String, Kind)> = vec![(String::new(), Kind::Dir)];
    for (index, part) in parts.iter().enumerate() {
        let last = index + 1 == parts.len();
        let mut next = Vec::new();
        for (relative, kind) in &matched {
            if !matches!(kind, Kind::Dir | Kind::LinkedDir) {
                continue;
            }
            let directory = root.join(relative);
            let join = |name: &str| match relative.is_empty() {
                true => name.to_string(),
                false => format!("{relative}/{name}"),
            };
            if *part == "**" {
                next.push((relative.clone(), *kind));
                for (path, kind) in descendants(&directory, files)? {
                    next.push((join(&path), kind));
                }
            } else if glob::has_magic(part) {
                let segment = Segment::new(part);
                for (name, kind) in entries(&directory, files)? {
                    if segment.matches(&name) {
                        next.push((join(&name), kind));
                    }
                }
            } else {
                let kind = match entries(&directory, files)?
                    .into_iter()
                    .find(|(name, _)| name == part)
                {
                    Some((_, kind)) => kind,
                    None if *part == ".." || *part == "." => Kind::Dir,
                    None => continue,
                };
                if last || matches!(kind, Kind::Dir | Kind::LinkedDir) {
                    next.push((join(part), kind));
                }
            }
        }
        next.dedup();
        matched = next;
    }
    let mut found: Vec<(String, Kind)> = matched
        .into_iter()
        .filter(|(relative, kind)| *kind == Kind::File && !relative.is_empty())
        .collect();
    found.sort();
    found.dedup();
    Ok(found
        .into_iter()
        .map(|(relative, _)| root.join(relative))
        .collect())
}

fn entries(directory: &Path, files: &dyn Files) -> Result<Vec<(String, Kind)>, FindError> {
    match files.list(directory) {
        Listing::Entries(entries) => Ok(entries),
        Listing::Missing => Ok(Vec::new()),
        Listing::Unread => Err(FindError::Need(directory.to_path_buf())),
    }
}

/// The entries under `directory` that `**` reaches, by path relative to it: every one whose
/// name does not start with `.`, walking into directories and the links to them.
fn descendants(directory: &Path, files: &dyn Files) -> Result<Vec<(String, Kind)>, FindError> {
    let mut found = Vec::new();
    let mut todo = vec![String::new()];
    while let Some(relative) = todo.pop() {
        for (name, kind) in entries(&directory.join(&relative), files)? {
            if name.starts_with('.') {
                continue;
            }
            let path = match relative.is_empty() {
                true => name,
                false => format!("{relative}/{name}"),
            };
            // A link that loops is walked at most this deep.
            if matches!(kind, Kind::Dir | Kind::LinkedDir) && path.matches('/').count() < 64 {
                todo.push(path.clone());
            }
            found.push((path, kind));
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Tree;

    impl Files for Tree {
        fn read(&self, path: &Path) -> Read {
            match path.to_str() {
                Some("/repo/pyproject.toml") => {
                    Read::Text(b"[tool.sqlty]\nextend-exclude = [\"a/b\"]\n".to_vec())
                }
                _ => Read::Missing,
            }
        }

        fn list(&self, path: &Path) -> Listing {
            match path.to_str() {
                Some("/repo") => Listing::Entries(vec![
                    ("a".to_string(), Kind::Dir),
                    ("b".to_string(), Kind::Dir),
                ]),
                Some("/repo/a" | "/repo/b") => {
                    Listing::Entries(vec![("f.py".to_string(), Kind::File)])
                }
                _ => Listing::Missing,
            }
        }
    }

    /// The exclude set is compiled once for a whole walk (check budget), not per directory.
    #[test]
    fn walk_compiles_its_exclude_set_once() {
        let discovery = Discovery::new(Box::new(Tree), Path::new("/repo"));
        discovery.walk(Path::new(".")).unwrap();
        assert_eq!(discovery.excludes.sets(), 1);
    }
}
