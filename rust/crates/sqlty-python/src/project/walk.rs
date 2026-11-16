//! The files `sqlty check` reads: the `.py` files under a directory, without what the
//! excludes and `.gitignore` leave out, and the files a `schema` or `queries` entry names.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use toml::{Table, Value};

use super::glob::{self, Excludes, Segment};
use super::route::{self, Route};
use super::{find, normalized, Files, FindError, Kind, Listing, Read};

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
    chains: HashMap<PathBuf, Option<Vec<(PathBuf, Gitignore)>>>,
}

type Rules = Vec<(Option<String>, Vec<String>)>;

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
        let found = match find(&directory, &*self.files) {
            Err(FindError::Syntax(_)) => None,
            found => found?,
        };
        let found = found.map(|found| {
            let base = found.file.parent().unwrap_or(Path::new("/")).to_path_buf();
            (found.config.table, base)
        });
        self.state().configs.insert(directory, found.clone());
        Ok(found)
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
            let resolved = self.resolved(&current);
            let entries = match self.files.list(&self.absolute(&current)) {
                Listing::Entries(entries) => entries,
                Listing::Missing => continue,
                Listing::Unread => return Err(FindError::Need(self.absolute(&current))),
            };
            let rules: Rules =
                self.exclusions(&current)?
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
            let respects = self.respects_gitignore(&current)?;
            let mut directories = Vec::new();
            for (name, kind) in entries {
                let is_dir = matches!(kind, Kind::Dir | Kind::LinkedDir);
                if !is_dir && !name.ends_with(".py") {
                    continue;
                }
                let excluded = rules.iter().any(|(directory, globs)| {
                    self.excluded_entry(&name, directory.as_deref(), globs)
                });
                if excluded || (respects && self.ignored(&resolved, &name, is_dir)?) {
                    continue;
                }
                match kind {
                    Kind::Dir => directories.push(current.join(&name)),
                    Kind::LinkedDir => {}
                    Kind::File | Kind::Other => found.push(current.join(&name)),
                }
            }
            directories.sort();
            todo.extend(directories.into_iter().rev());
        }
        found.sort();
        Ok(found)
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
        for (_, spec) in chain.iter().flatten() {
            let matched = spec.matched(&path, is_dir);
            if matched.is_ignore() {
                verdict = true;
            } else if matched.is_whitelist() {
                verdict = false;
            }
        }
        Ok(verdict)
    }

    fn chain(&self, directory: &Path) -> Result<Option<Vec<(PathBuf, Gitignore)>>, FindError> {
        if let Some(chain) = self.state().chains.get(directory) {
            return Ok(chain.clone());
        }
        let in_repository = match self.files.list(directory) {
            Listing::Entries(entries) => entries.iter().any(|(name, _)| name == ".git"),
            Listing::Missing => false,
            Listing::Unread => return Err(FindError::Need(directory.to_path_buf())),
        };
        let mut chain = match (in_repository, directory.parent()) {
            (true, _) => Some(self.spec(directory, &directory.join(".git/info/exclude"))?),
            (false, None) => None,
            (false, Some(parent)) => self.chain(parent)?,
        };
        if let Some(chain) = chain.as_mut() {
            chain.extend(self.spec(directory, &directory.join(".gitignore"))?);
        }
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
            if !found.contains(&file) {
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
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// Files by absolute path; a directory is every prefix of one, a `None` file a link to
    /// a directory, `/unread`, a directory `unread` and a file `<unread>` are not read yet,
    /// a directory `gone` is missing and a file `<binary>` is not UTF-8.
    struct Tree(BTreeMap<PathBuf, Option<&'static str>>);

    impl Tree {
        fn new(files: &[(&str, Option<&'static str>)]) -> Self {
            Self(
                files
                    .iter()
                    .map(|(path, text)| (PathBuf::from(path), *text))
                    .collect(),
            )
        }
    }

    impl Files for Tree {
        fn read(&self, path: &Path) -> Read {
            match self.0.get(path) {
                Some(Some("<unread>")) => Read::Unread,
                Some(Some("<binary>")) => Read::Text(vec![0xff]),
                Some(Some(text)) => Read::Text(text.as_bytes().to_vec()),
                _ if path.starts_with("/unread") => Read::Unread,
                _ => Read::Missing,
            }
        }

        fn list(&self, path: &Path) -> Listing {
            let path = &normalized(path);
            if path.starts_with("/unread") || path.ends_with("unread") {
                return Listing::Unread;
            }
            if path.ends_with("gone") {
                return Listing::Missing;
            }
            let mut entries: Vec<(String, Kind)> = Vec::new();
            for (file, text) in &self.0 {
                let Ok(rest) = file.strip_prefix(path) else {
                    continue;
                };
                let mut parts = rest.components();
                let Some(name) = parts.next() else {
                    continue;
                };
                let name = name.as_os_str().to_string_lossy().into_owned();
                let kind = match (parts.next(), text) {
                    (Some(_), _) => Kind::Dir,
                    (None, Some(_)) => Kind::File,
                    (None, None) => Kind::LinkedDir,
                };
                if !entries.iter().any(|(seen, _)| *seen == name) {
                    entries.push((name, kind));
                }
            }
            match entries.is_empty() {
                true => Listing::Missing,
                false => Listing::Entries(entries),
            }
        }
    }

    fn project() -> Tree {
        Tree::new(&[
            ("/repo/.git/info/exclude", Some("local.py\n")),
            (
                "/repo/.gitignore",
                Some("generated/\n*_pb2.py\n!keep_pb2.py\n"),
            ),
            (
                "/repo/pyproject.toml",
                Some("[tool.sqlty]\nextend-exclude = [\"gen_*.py\", \"a/b\"]\n"),
            ),
            ("/repo/local.py", Some("")),
            ("/repo/generated/a.py", Some("")),
            ("/repo/app/.gitignore", Some("build/\n")),
            ("/repo/app/b.py", Some("")),
            ("/repo/app/c_pb2.py", Some("")),
            ("/repo/app/keep_pb2.py", Some("")),
            ("/repo/app/build/d.py", Some("")),
            ("/repo/app/gen_e.py", Some("")),
            ("/repo/app/notes.txt", Some("")),
            ("/repo/a/b/f.py", Some("")),
            ("/repo/a/g.py", Some("")),
            ("/repo/.venv/v.py", Some("")),
            ("/repo/linked", None),
        ])
    }

    fn walked(discovery: &Discovery, root: &str) -> Vec<String> {
        let found = discovery.walk(Path::new(root)).unwrap();
        found
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn walks_what_excludes_and_gitignore_leave() {
        let discovery = Discovery::new(Box::new(project()), Path::new("/repo"));
        assert_eq!(
            walked(&discovery, "."),
            ["./a/g.py", "./app/b.py", "./app/keep_pb2.py"]
        );
        assert_eq!(walked(&discovery, "app"), ["app/b.py", "app/keep_pb2.py"]);
        assert_eq!(discovery.excludes.sets(), 1);
        assert!(discovery.is_excluded(Path::new("a/b/f.py")).unwrap());
        assert!(!discovery.is_excluded(Path::new("a/g.py")).unwrap());

        let all = Discovery::new(Box::new(project()), Path::new("/repo"))
            .isolated()
            .respect_gitignore(false)
            .exclude(&["/repo/./app".to_string()]);
        assert_eq!(walked(&all, "/repo/a"), ["/repo/a/b/f.py", "/repo/a/g.py"]);
        let everything = walked(&all, "/repo");
        assert!(everything.contains(&"/repo/generated/a.py".to_string()));
        assert!(!everything.iter().any(|path| path.starts_with("/repo/app")));
    }

    #[test]
    fn a_fixed_configuration_rules_every_directory() {
        let fixed: Table = toml::from_str("exclude = []\nrespect-gitignore = false").unwrap();
        let discovery = Discovery::new(Box::new(project()), Path::new("/repo"))
            .fixed(fixed)
            .extend_exclude(&["./generated".to_string()]);
        assert_eq!(
            discovery.exclusions(Path::new("app")).unwrap(),
            [(PathBuf::from("/repo"), vec!["generated".to_string()])]
        );
        assert!(walked(&discovery, ".").contains(&"./.venv/v.py".to_string()));
        assert!(!discovery.respects_gitignore(Path::new(".")).unwrap());
        assert!(discovery.matches(
            Path::new("a/b/f.py"),
            Path::new("/repo"),
            &["a".to_string()]
        ));
        assert!(!discovery.matches(
            Path::new("/elsewhere"),
            Path::new("/repo"),
            &["a".to_string()]
        ));
    }

    #[test]
    fn a_host_reads_what_it_has_not() {
        let discovery = Discovery::new(Box::new(project()), Path::new("/unread"));
        assert!(matches!(
            discovery.walk(Path::new(".")),
            Err(FindError::Need(_))
        ));
        let invalid = Tree::new(&[
            ("/x/sqlty.toml", Some("dialect = 1")),
            ("/x/a.py", Some("")),
        ]);
        assert!(Discovery::new(Box::new(invalid), Path::new("/x"))
            .walk(Path::new("."))
            .is_err());
        let broken = Tree::new(&[("/x/pyproject.toml", Some("[")), ("/x/.git/a.py", Some(""))]);
        let discovery = Discovery::new(Box::new(broken), Path::new("/x"));
        assert!(discovery.is_excluded(Path::new(".git/a.py")).unwrap());
        let lazy = Tree::new(&[
            ("/y/.git/HEAD", Some("")),
            ("/y/.gitignore", Some("<unread>")),
            ("/y/a.py", Some("")),
        ]);
        let discovery = Discovery::new(Box::new(lazy), Path::new("/y"));
        assert!(matches!(
            discovery.walk(Path::new(".")),
            Err(FindError::Need(path)) if path == Path::new("/y/.gitignore")
        ));
        let nested = Tree::new(&[("/z/unread/sub/a.py", Some(""))]);
        let discovery = Discovery::new(Box::new(nested), Path::new("/z/unread/sub"));
        assert!(matches!(
            discovery.walk(Path::new(".")),
            Err(FindError::Need(path)) if path == Path::new("/z/unread")
        ));
    }

    #[test]
    fn a_missing_root_and_an_unreadable_gitignore_are_nothing() {
        let tree = Tree::new(&[
            ("/w/.git/HEAD", Some("")),
            ("/w/.gitignore", Some("<binary>")),
            ("/w/a.py", Some("")),
        ]);
        let discovery = Discovery::new(Box::new(tree), Path::new("/w"));
        assert_eq!(walked(&discovery, "."), ["./a.py"]);
        assert_eq!(walked(&discovery, "nope"), Vec::<String>::new());
        let gone = Tree::new(&[("/z/gone/sub/a.py", Some(""))]);
        let discovery = Discovery::new(Box::new(gone), Path::new("/z/gone/sub"));
        assert_eq!(walked(&discovery, "."), ["./a.py"]);
        let all =
            Discovery::new(Box::new(project()), Path::new("/repo")).exclude(&["/repo".to_string()]);
        assert_eq!(walked(&all, "app"), ["app/b.py", "app/keep_pb2.py"]);
    }

    #[test]
    fn schema_entries_name_files_directories_and_globs() {
        let tree = Tree::new(&[
            ("/p/db/b.sql", Some("")),
            ("/p/db/a.sql", Some("")),
            ("/p/db/sub/c.sql", Some("")),
            ("/p/db/.hidden/h.sql", Some("")),
            ("/p/db/notes.md", Some("")),
            ("/p/db/link", None),
        ]);
        let entries = |entries: &[&str]| {
            let entries: Vec<String> = entries.iter().map(ToString::to_string).collect();
            let (found, empty) = schema_files(&entries, Path::new("/p"), &tree).unwrap();
            let found: Vec<String> = found
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect();
            (found, empty)
        };
        assert_eq!(
            entries(&["db/a.sql", "db", "missing.sql"]).0,
            [
                "/p/db/a.sql",
                "/p/db/.hidden/h.sql",
                "/p/db/b.sql",
                "/p/db/sub/c.sql",
                "/p/missing.sql"
            ]
        );
        assert_eq!(
            entries(&[
                "db/**/*.sql",
                "db/*.s?l",
                "../p/db/[a]*",
                "/p/db/sub/c.sql",
                "nope/*"
            ]),
            (
                vec![
                    "/p/db/a.sql".to_string(),
                    "/p/db/b.sql".to_string(),
                    "/p/db/sub/c.sql".to_string(),
                    "/p/../p/db/a.sql".to_string(),
                ],
                vec!["nope/*".to_string()]
            )
        );
        assert_eq!(
            entries(&["db/.*/*.sql", "db/**"]).0,
            [
                "/p/db/.hidden/h.sql",
                "/p/db/a.sql",
                "/p/db/b.sql",
                "/p/db/notes.md",
                "/p/db/sub/c.sql"
            ]
        );
        assert_eq!(entries(&["/p/db/*.sql"]).0, ["/p/db/a.sql", "/p/db/b.sql"]);
        assert_eq!(
            entries(&["db/a.sql/*"]),
            (vec![], vec!["db/a.sql/*".to_string()])
        );
        let unread = Tree::new(&[("/q/db/unread/a.sql", Some(""))]);
        for entry in ["*.sql", "a.sql", "db"] {
            let need = schema_files(&[entry.to_string()], Path::new("/unread"), &unread);
            assert!(matches!(need, Err(FindError::Need(_))), "{entry}");
        }
        let need = schema_files(&["db".to_string()], Path::new("/q"), &unread);
        assert!(matches!(need, Err(FindError::Need(path)) if path == Path::new("/q/db/unread")));
        let gone = Tree::new(&[("/q/db/gone/b.sql", Some("")), ("/q/db/a.sql", Some(""))]);
        let found = schema_files(&["db".to_string()], Path::new("/q"), &gone).unwrap();
        assert_eq!(found, (vec![PathBuf::from("/q/db/a.sql")], vec![]));
    }
}
