//! The Python files `sqlty check PATH...` reads, each with its settings, schema and the
//! helpers of the others; an editor's buffers replace their files.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use rayon::prelude::*;
use sqlty_python::calls::Helper;
use sqlty_python::checker::{self, Diagnostic, SiteKind};
use sqlty_python::index::{
    defines_helpers, helpers_of, Builder, HelperIndex, Module as Indexed, Roots,
};
use sqlty_python::project::{
    self, helper_roots, normalized, Discovery, Files, FindError, Kind, Listing, Packages, Read,
};
use sqlty_python::queries;
use sqlty_python::rewriter::ImportStyle;
use sqlty_python::types::info::Dict;
use sqlty_python::types::stdlib::Version;
use sqlty_python::types::symbols::Source;
use sqlty_sql::analyzer::{self, Schema};

use crate::flags::Flags;
use crate::module::{self, check_module, Checker};
use crate::settings::{Directory, FileSettings, Function, Schemas};
use crate::Shared;

/// The stack the analyzer's recursion keeps free, and the stack it grows by.
const STACK_RED_ZONE: usize = 2 << 20;
const STACK_SEGMENT: usize = 16 << 20;

/// The settings each configuration (label, base, table) gives its directories.
type Configured = Vec<(
    (String, Option<PathBuf>, toml::Table),
    Result<Arc<Directory>, String>,
)>;

/// A module with its settings and helpers.
type Prepared = (PathBuf, Arc<FileSettings>, Arc<HelperIndex>);

/// A checked module: its text, its names and what is read of it.
struct Module {
    text: Arc<str>,
    names: Vec<String>,
    resolved: PathBuf,
    source: Arc<Indexed>,
    helpers: Dict<Helper>,
}

impl Module {
    /// The module parsed, `None` where Rust does not read it: kept only when the index
    /// has read it, so a checked module's tree goes with its check.
    fn parsed(&self) -> Option<Arc<Source>> {
        self.source.load()
    }
}

/// How [`Workspace::open`] reads the paths.
pub struct Options {
    /// The command line's settings over the configuration files.
    pub flags: Flags,
    /// Which files a directory's walk keeps.
    pub discovery: Discovery,
    /// The Python the sources are read as.
    pub version: Version,
    /// Where `[[tool.sqlty.functions]]` modules are also looked for: the host's `sys.path`.
    pub extra_roots: Vec<PathBuf>,
    /// `--force-exclude`: the excludes apply to the paths named too.
    pub force_exclude: bool,
    /// Whether the modules are parsed for checks; a rewrite reads only their text.
    pub parse: bool,
}

/// The project of the paths of a command line, or of an editor's root.
pub struct Workspace {
    files: Shared,
    paths: Vec<PathBuf>,
    roots: Vec<PathBuf>,
    version: Version,
    extra_roots: Vec<PathBuf>,
    flags: Flags,
    discovery: Discovery,
    packages: Packages,
    schemas: Schemas,
    directories: HashMap<PathBuf, Result<Arc<Directory>, String>>,
    order: Vec<PathBuf>,
    modules: HashMap<PathBuf, Module>,
    unreadable: Vec<(PathBuf, String)>,
    index: Option<Arc<HelperIndex>>,
    configured: HashMap<(Vec<Function>, Option<PathBuf>), Arc<HelperIndex>>,
    resolved: Mutex<HashMap<PathBuf, PathBuf>>,
    configs: Mutex<Configured>,
}

/// A schema of a configuration under the paths: one database, and what each of its
/// files is checked with.
pub struct SchemaSource {
    /// The directory whose configuration names it.
    pub directory: PathBuf,
    /// Its `[tool.sqlty.databases]` name, `default` for the project's own.
    pub database: String,
    /// What the database is checked with.
    pub settings: Arc<FileSettings>,
    /// For each of `settings.schema_files`: `settings` with the overrides its path matches.
    pub files: Vec<Arc<FileSettings>>,
}

/// What completion reads of a buffer.
pub struct Completion {
    /// The schema the buffer is checked against.
    pub schema: Option<Schema>,
    /// The SQL dialect.
    pub dialect: &'static str,
    /// `import-style`.
    pub import_style: Option<String>,
    /// Where the helper calls of the buffer pass a query: byte offsets of the literals.
    pub helper_starts: Vec<usize>,
}

impl Workspace {
    /// The project under `root`, as the language server reads it: the nearest
    /// configuration of each directory, no flags.
    pub fn root(
        root: &Path,
        files: Arc<dyn Files + Send + Sync>,
        version: Version,
        extra_roots: Vec<PathBuf>,
    ) -> Result<Self, String> {
        let files = Shared(files);
        let root = files.canonical(root);
        let options = Options {
            flags: Flags {
                cwd: root.clone(),
                ..Flags::default()
            },
            discovery: Discovery::new(Box::new(files.clone()), &root),
            version,
            extra_roots,
            force_exclude: false,
            parse: true,
        };
        let mut workspace = Self::open(std::slice::from_ref(&root), files, options)?;
        workspace.settings(&root.join("__init__.py"))?;
        Ok(workspace)
    }

    /// The Python files of `paths` (relative to `options.flags.cwd`) and their project: a
    /// file is read whatever its excludes (without `force_exclude`), a directory walked. A
    /// configuration or schema file stands for its project's directory.
    pub fn open(paths: &[PathBuf], files: Shared, options: Options) -> Result<Self, String> {
        let Options {
            flags,
            discovery,
            version,
            extra_roots,
            force_exclude,
            parse,
        } = options;
        let mut workspace = Self {
            packages: Packages::new(Box::new(files.clone())),
            files,
            paths: Vec::new(),
            roots: Vec::new(),
            version,
            extra_roots,
            flags,
            discovery,
            schemas: Schemas::default(),
            directories: HashMap::new(),
            order: Vec::new(),
            modules: HashMap::new(),
            unreadable: Vec::new(),
            index: None,
            configured: HashMap::new(),
            resolved: Mutex::default(),
            configs: Mutex::default(),
        };
        workspace.paths = workspace.project_paths(paths)?;
        workspace.roots = workspace
            .packages
            .roots(&workspace.paths, &workspace.flags.cwd);
        let collected = workspace.collect(force_exclude)?;
        // The settings and schemas of each directory load on one thread while the others
        // read and parse the modules.
        let (directories, read) = pool().install(|| {
            rayon::join(
                || workspace.directories_of(&collected),
                || {
                    collected
                        .par_iter()
                        .map(|path| {
                            let text = workspace.decoded(path)?;
                            Ok(workspace.module(path, Arc::from(text), parse))
                        })
                        .collect::<Vec<Result<Module, String>>>()
                },
            )
        });
        workspace.directories.extend(directories);
        for (path, read) in collected.into_iter().zip(read) {
            match read {
                Ok(module) => {
                    workspace.settings_at(&module.resolved)?;
                    workspace.order.push(path.clone());
                    workspace.modules.insert(path, module);
                }
                Err(why) => {
                    workspace.settings(&path)?;
                    workspace.unreadable.push((path, why));
                }
            }
        }
        Ok(workspace)
    }

    /// The Python files read, in the order of the command line.
    pub fn files(&self) -> &[PathBuf] {
        &self.order
    }

    /// The files that are not UTF-8, with why: `invalid start byte at byte 3`.
    pub fn unreadable(&self) -> &[(PathBuf, String)] {
        &self.unreadable
    }

    /// The text of a read file.
    pub fn text(&self, path: &Path) -> Option<&str> {
        self.modules.get(path).map(|module| &*module.text)
    }

    /// The module names a read file is imported by, the first its own.
    pub fn names(&self, path: &Path) -> Option<&[String]> {
        self.modules.get(path).map(|module| &*module.names)
    }

    /// What the schemas and configurations could not read since the last call.
    pub fn warnings(&self) -> Vec<String> {
        self.schemas.warnings()
    }

    /// Replace the text of the module at `path`, as an editor's buffer does.
    pub fn update(&mut self, path: &Path, text: &str) {
        if self
            .modules
            .get(path)
            .is_some_and(|module| *module.text == *text)
        {
            return;
        }
        let module = self.module(path, Arc::from(text), true);
        if self.modules.insert(path.to_path_buf(), module).is_none() {
            self.order.push(path.to_path_buf());
        }
        self.index = None;
        self.configured.clear();
    }

    fn module(&self, path: &Path, text: Arc<str>, parse: bool) -> Module {
        let resolved = self.resolved(path);
        let names = self.packages.names(&resolved, &self.roots);
        let init = path.file_name().is_some_and(|name| name == "__init__.py");
        let name = format!("{}{}", names[0], if init { ".__init__" } else { "" });
        let source = Arc::new(Indexed::new(Arc::clone(&text), &name, self.version));
        // A module that may define helpers is read now; any other when it is checked or a
        // lookup reaches it.
        let helpers = (parse && defines_helpers(&text))
            .then(|| source.source().map(|parsed| helpers_of(parsed)))
            .flatten()
            .unwrap_or_default();
        Module {
            text,
            names,
            resolved,
            source,
            helpers,
        }
    }

    fn absolute(&self, path: &Path) -> PathBuf {
        self.flags.cwd.join(path)
    }

    /// `path` with its links resolved: those of its directory, or its own when it is one.
    /// A directory is resolved once.
    fn resolved(&self, path: &Path) -> PathBuf {
        let absolute = normalized(&self.absolute(path));
        let Some(name) = absolute.parent().and(absolute.file_name()) else {
            return self.files.canonical(&absolute);
        };
        if self.files.is_link(&absolute) {
            return self.files.canonical(&absolute);
        }
        self.resolved_directory(&absolute).join(name)
    }

    /// The directory of the absolute `path`, its links resolved once.
    fn resolved_directory(&self, path: &Path) -> PathBuf {
        let directory = parent(path);
        let mut known = self.resolved.lock().unwrap_or_else(PoisonError::into_inner);
        known
            .entry(directory.to_path_buf())
            .or_insert_with(|| self.files.canonical(directory))
            .clone()
    }

    fn kind(&self, path: &Path) -> Option<Kind> {
        let path = self.absolute(path);
        if let Listing::Entries(_) = self.files.list(&path) {
            return Some(Kind::Dir);
        }
        match self.files.read(&path) {
            Read::Text(_) => Some(Kind::File),
            Read::Missing | Read::Unread => None,
        }
    }

    fn decoded(&self, path: &Path) -> Result<String, String> {
        let Read::Text(data) = self.files.read(&self.absolute(path)) else {
            return Err("unreadable".to_string());
        };
        let data = data.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&data);
        match std::str::from_utf8(data) {
            Ok(text) => Ok(text.replace("\r\n", "\n").replace('\r', "\n")),
            Err(error) => {
                let at = error.valid_up_to();
                let reason = match (error.error_len(), data[at]) {
                    (None, _) => "unexpected end of data",
                    (Some(_), 0x80..=0xc1 | 0xf5..) => "invalid start byte",
                    (Some(_), _) => "invalid continuation byte",
                };
                Err(format!("{reason} at byte {at}"))
            }
        }
    }

    fn project_paths(&mut self, paths: &[PathBuf]) -> Result<Vec<PathBuf>, String> {
        let mut projects: Vec<PathBuf> = Vec::new();
        for path in paths {
            let mut path = path.clone();
            if self.kind(&path) == Some(Kind::File)
                && path.extension().is_none_or(|ext| ext != "py")
            {
                let directory = self
                    .resolved(&path)
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_default();
                let mut warn = |warning| self.schemas.warn(warning);
                let loaded = self.flags.load(&directory, &self.discovery, &mut warn)?;
                if let Some(base) = loaded.base {
                    let schema = loaded
                        .table
                        .get("schema")
                        .map(crate::settings::entries_of)
                        .unwrap_or_default();
                    let (schema, _) = project::schema_files(&schema, &base, &self.files)
                        .map_err(|error| error.to_string())?;
                    let resolved = self.resolved(&path);
                    let owned = self.resolved(Path::new(&loaded.label)) == resolved
                        || schema
                            .iter()
                            .any(|file| self.files.canonical(file) == resolved);
                    if owned {
                        path =
                            base.strip_prefix(&self.flags.cwd)
                                .map_or(base.clone(), |relative| {
                                    match relative.as_os_str().is_empty() {
                                        true => PathBuf::from("."),
                                        false => relative.to_path_buf(),
                                    }
                                });
                    }
                }
            }
            if !projects.contains(&path) {
                projects.push(path);
            }
        }
        Ok(projects)
    }

    fn collect(&self, force_exclude: bool) -> Result<Vec<PathBuf>, String> {
        let mut found: Vec<PathBuf> = Vec::new();
        for path in &self.paths {
            let kind = self.kind(path);
            if kind.is_some()
                && force_exclude
                && self
                    .discovery
                    .is_excluded(path)
                    .map_err(|error| error.to_string())?
            {
                continue;
            }
            match kind {
                Some(Kind::File) if path.extension().is_some_and(|ext| ext == "py") => {
                    found.push(path.clone());
                }
                Some(Kind::File) => {}
                Some(_) => {
                    let walked = pool()
                        .install(|| walk(&self.discovery, path))
                        .map_err(|error| error.to_string())?;
                    found.extend(walked.into_iter().map(|path| {
                        path.components()
                            .filter(|part| *part != std::path::Component::CurDir)
                            .collect()
                    }));
                }
                None => return Err(format!("path does not exist: {}", path.display())),
            }
        }
        let mut seen = HashSet::new();
        found.retain(|path| seen.insert(path.clone()));
        Ok(found)
    }

    /// The directories of the paths, then of the files read, once each.
    fn directories(&self) -> Vec<PathBuf> {
        let mut directories: Vec<PathBuf> = self
            .paths
            .iter()
            .map(|path| match self.kind(path) {
                Some(Kind::Dir) => path.clone(),
                _ => path.parent().map(Path::to_path_buf).unwrap_or_default(),
            })
            .collect();
        directories.extend(
            self.order
                .iter()
                .map(|path| path.parent().map(Path::to_path_buf).unwrap_or_default()),
        );
        let mut seen = HashSet::new();
        directories.retain(|directory| seen.insert(directory.clone()));
        directories
    }

    /// The schemas of the configurations under the paths, one per database that has schema
    /// files, each configuration once.
    pub fn schema_sources(&mut self) -> Result<Vec<SchemaSource>, String> {
        let mut known: Vec<Arc<Directory>> = Vec::new();
        let mut sources = Vec::new();
        for directory in self.directories() {
            let all = self.directory(&directory.join("_"))?;
            if known.iter().any(|seen| Arc::ptr_eq(seen, &all)) {
                continue;
            }
            for (database, settings) in all.named() {
                if settings.schema_files.is_empty() {
                    continue;
                }
                let files = settings
                    .schema_files
                    .iter()
                    .map(|file| all.schema_file(settings, file, &self.discovery))
                    .collect();
                sources.push(SchemaSource {
                    directory: directory.clone(),
                    database: database.to_string(),
                    settings: Arc::clone(settings),
                    files,
                });
            }
            known.push(all);
        }
        Ok(sources)
    }

    /// The databases of the configuration of the directory of `path`, each with its
    /// settings: the project's own first, under [`analyzer::DEFAULT`].
    pub fn databases(&mut self, path: &Path) -> Result<Vec<(String, Arc<FileSettings>)>, String> {
        let all = self.directory(path)?;
        let named = all.named();
        Ok(named
            .map(|(name, settings)| (name.to_string(), Arc::clone(settings)))
            .collect())
    }

    /// The settings of the database `name` of the configuration of the directory of
    /// `path`, `None` when it has no such database.
    pub fn database(
        &mut self,
        path: &Path,
        name: &str,
    ) -> Result<Option<Arc<FileSettings>>, String> {
        let mut all = self.databases(path)?.into_iter();
        Ok(all
            .find(|(database, _)| database == name)
            .map(|(_, settings)| settings))
    }

    /// The `.sql` files of named queries under the paths: those a `queries` key of their
    /// projects matches, relative to the current directory when under it, sorted.
    pub fn query_files(&mut self) -> Result<Vec<PathBuf>, String> {
        let directories = self.directories();
        let roots: Vec<PathBuf> = self.paths.iter().map(|path| self.resolved(path)).collect();
        let mut found = std::collections::BTreeSet::new();
        for directory in directories {
            let all = self.directory(&directory.join("_"))?;
            for (_, settings) in all.named() {
                let Some(base) = &settings.base else { continue };
                if settings.queries.is_empty() {
                    continue;
                }
                let (files, empty) = project::schema_files(&settings.queries, base, &self.files)
                    .map_err(|error| error.to_string())?;
                for entry in empty {
                    self.schemas
                        .warn(format!("Schema pattern matches no files: {entry}"));
                }
                for file in files {
                    let resolved = self.files.canonical(&file);
                    let under = roots.iter().any(|root| resolved.starts_with(root));
                    if file.extension().is_some_and(|ext| ext == "sql") && under {
                        let cwd = self.files.canonical(&self.flags.cwd);
                        found.insert(
                            resolved
                                .strip_prefix(&cwd)
                                .map_or(file.clone(), Path::to_path_buf),
                        );
                    }
                }
            }
        }
        Ok(found.into_iter().collect())
    }

    fn index(&mut self) -> Arc<HelperIndex> {
        if let Some(index) = &self.index {
            return Arc::clone(index);
        }
        let mut builder = Builder::new(self.version);
        for path in &self.order {
            let module = &self.modules[path];
            let source = &module.source;
            for name in &module.names {
                builder.add_found(name, Arc::clone(source), &module.helpers);
            }
        }
        let index = builder.build();
        self.index = Some(Arc::clone(&index));
        index
    }

    /// The index with the `functions` of `settings` registered.
    pub fn helpers(&mut self, settings: &FileSettings) -> Arc<HelperIndex> {
        let index = self.index();
        if settings.functions.is_empty() {
            return index;
        }
        let key = (settings.functions.clone(), settings.base.clone());
        if let Some(found) = self.configured.get(&key) {
            return Arc::clone(found);
        }
        let resolved: Vec<PathBuf> = self
            .order
            .iter()
            .map(|path| self.modules[path].resolved.clone())
            .collect();
        let checked = self
            .packages
            .checked_roots(&self.paths, &resolved, &self.flags.cwd);
        let base = settings
            .base
            .as_deref()
            .map(|base| self.files.canonical(base));
        let roots = helper_roots(base.as_deref(), &checked, &self.extra_roots);
        let files = self.files.clone();
        let read = move |path: &Path| files.text(path);
        let mut forked = index.fork();
        for function in &settings.functions {
            let roots = Roots {
                paths: &roots,
                read: &read,
            };
            let row = function.row.as_deref();
            forked.register(
                &function.import_path,
                &function.query,
                row,
                roots,
                function.database.as_deref(),
            );
        }
        let forked = forked.build();
        self.configured.insert(key, Arc::clone(&forked));
        forked
    }

    fn directory(&mut self, path: &Path) -> Result<Arc<Directory>, String> {
        let resolved = self.resolved(path);
        self.directory_at(parent(&resolved))
    }

    /// The settings of the resolved directory `directory`, loaded once.
    fn directory_at(&mut self, directory: &Path) -> Result<Arc<Directory>, String> {
        if !self.directories.contains_key(directory) {
            let made = self.load_directory(directory);
            self.directories.insert(directory.to_path_buf(), made);
        }
        self.directories[directory].clone()
    }

    /// The settings of `directory`, built once per configuration the directories share.
    fn load_directory(&self, directory: &Path) -> Result<Arc<Directory>, String> {
        let mut warn = |warning| self.schemas.warn(warning);
        let loaded = self.flags.load(directory, &self.discovery, &mut warn)?;
        let mut configs = self.configs.lock().unwrap_or_else(PoisonError::into_inner);
        let key = (
            loaded.label.clone(),
            loaded.base.clone(),
            loaded.table.clone(),
        );
        if let Some((_, made)) = configs.iter().find(|(known, _)| *known == key) {
            return made.clone();
        }
        let made = Directory::new(
            loaded,
            &self.flags,
            &self.files,
            &self.schemas,
            self.version,
        )
        .map(Arc::new);
        configs.push((key, made.clone()));
        made
    }

    /// The settings of the directories of `paths`, in their order; a linked file's own
    /// directory is loaded when its settings are asked.
    fn directories_of(&self, paths: &[PathBuf]) -> Vec<(PathBuf, Result<Arc<Directory>, String>)> {
        let mut seen = HashSet::new();
        paths
            .iter()
            .map(|path| self.resolved_directory(&normalized(&self.absolute(path))))
            .filter(|directory| seen.insert(directory.clone()))
            .map(|directory| {
                let made = self.load_directory(&directory);
                (directory, made)
            })
            .collect()
    }

    /// The settings of the file at `path`.
    pub fn settings(&mut self, path: &Path) -> Result<Arc<FileSettings>, String> {
        let resolved = self.resolved(path);
        self.settings_at(&resolved)
    }

    /// The settings of the file at the resolved path `resolved`.
    fn settings_at(&mut self, resolved: &Path) -> Result<Arc<FileSettings>, String> {
        let directory = self.directory_at(parent(resolved))?;
        Ok(directory.file(resolved, &self.discovery))
    }

    /// The diagnostics of every module, in order, on all cores, without the
    /// `redundant-optional` reports another module's query refutes.
    pub fn check_all(&mut self) -> Result<Vec<Vec<Diagnostic>>, String> {
        let work = self.prepared()?;
        let found: Vec<_> = pool().install(|| {
            work.par_iter()
                .map(|(path, settings, helpers)| {
                    stacker::maybe_grow(STACK_RED_ZONE, STACK_SEGMENT, || {
                        let module = &self.modules[path];
                        let checker = Checker::of(settings, Some(helpers));
                        let parsed = module.parsed();
                        let name = parsed.as_ref().and_then(|parsed| parsed.module.clone());
                        Ok(check_module(
                            &module.text,
                            parsed.as_ref(),
                            name.as_deref(),
                            &checker,
                        ))
                    })
                })
                .collect::<Result<_, String>>()
        })?;
        let needed: HashSet<_> = found
            .iter()
            .flat_map(|found| found.needed.iter().cloned())
            .collect();
        Ok(found
            .into_iter()
            .map(|found| {
                found
                    .diagnostics
                    .into_iter()
                    .filter(|reported| {
                        reported
                            .redundant
                            .as_ref()
                            .is_none_or(|key| !needed.contains(key))
                    })
                    .map(|reported| reported.diagnostic)
                    .collect()
            })
            .collect())
    }

    /// The text `sqlty check --fix` writes of every module, in order, on all cores.
    pub fn fix_all(&mut self) -> Result<Vec<String>, String> {
        let work = self.prepared()?;
        pool().install(|| {
            work.par_iter()
                .map(|(path, settings, helpers)| {
                    stacker::maybe_grow(STACK_RED_ZONE, STACK_SEGMENT, || {
                        let module = &self.modules[path];
                        let checker = Checker::of(settings, Some(helpers));
                        let parsed = module.parsed();
                        let name = parsed.as_ref().and_then(|parsed| parsed.module.clone());
                        Ok(module::fix(
                            &module.text,
                            parsed.as_deref(),
                            name.as_deref(),
                            &checker,
                            settings,
                        ))
                    })
                })
                .collect()
        })
    }

    /// [`module::statistics`] of every module, in order, on all cores.
    pub fn statistics_all(&mut self) -> Result<Vec<HashMap<&'static str, usize>>, String> {
        let work = self.prepared()?;
        pool().install(|| {
            work.par_iter()
                .map(|(path, settings, helpers)| {
                    stacker::maybe_grow(STACK_RED_ZONE, STACK_SEGMENT, || {
                        let module = &self.modules[path];
                        let checker = Checker::of(settings, Some(helpers));
                        let parsed = module.parsed();
                        let name = parsed.as_ref().and_then(|parsed| parsed.module.as_deref());
                        Ok(module::statistics(
                            &module.text,
                            parsed.as_deref(),
                            name,
                            &checker,
                        ))
                    })
                })
                .collect()
        })
    }

    /// The inference rules behind the columns of the last query that starts on or before
    /// `line` of the module at `path`, indented, as `sqlty check --explain` prints them.
    pub fn explain(&mut self, path: &Path, line: usize) -> Result<Vec<String>, String> {
        let settings = self.settings(path)?;
        let Some(module) = self.modules.get(path) else {
            return Ok(Vec::new());
        };
        let style = ImportStyle::parse(None).map_err(|error| error.to_string())?;
        let query = checker::query_sites(&module.text, &style, &[])
            .into_iter()
            .rfind(|(start, kind, _)| *kind == SiteKind::Query && *start <= line);
        let Some((_, _, query)) = query else {
            return Ok(Vec::new());
        };
        let columns =
            analyzer::explain_select(&query, settings.dialect, settings.schema.as_deref());
        let mut lines = Vec::new();
        for column in columns.unwrap_or_default() {
            let by = [
                ("type", column.type_rule),
                ("nullability", column.nullability_rule),
            ];
            let mut rules: Vec<_> = by.iter().filter_map(|(_, rule)| *rule).collect();
            rules.sort_by_key(|rule| rule.id);
            rules.dedup();
            for rule in rules {
                let parts: Vec<&str> = by
                    .iter()
                    .filter(|(_, other)| *other == Some(rule))
                    .map(|(part, _)| *part)
                    .collect();
                lines.push(format!(
                    "  {}: {} by inference rule {}",
                    column.name,
                    parts.join(" and "),
                    rule.id
                ));
                lines.push(format!("    {}", rule.reasoning));
                lines.push(format!("    Reference: {}", rule.reference));
            }
        }
        Ok(lines)
    }

    /// Every module with its settings and helpers.
    fn prepared(&mut self) -> Result<Vec<Prepared>, String> {
        self.order
            .clone()
            .into_iter()
            .map(|path| {
                let resolved = self.modules[&path].resolved.clone();
                let settings = self.settings_at(&resolved)?;
                let helpers = self.helpers(&settings);
                Ok((path, settings, helpers))
            })
            .collect()
    }

    /// The diagnostics of `text`, the buffer of `path`: a Python module, or a `.sql` file
    /// whose project's `queries` name it (any other has none).
    pub fn check(&mut self, path: &Path, text: &str) -> Result<Vec<Diagnostic>, String> {
        let settings = self.settings(path)?;
        if path.extension().is_some_and(|extension| extension == "sql") {
            return self.check_queries(path, text);
        }
        self.update(path, text);
        let helpers = self.helpers(&settings);
        let module = &self.modules[path];
        let checker = Checker::of(&settings, Some(&helpers));
        let parsed = module.parsed();
        let name = parsed.as_ref().and_then(|parsed| parsed.module.clone());
        let found = check_module(text, parsed.as_ref(), name.as_deref(), &checker);
        Ok(found
            .diagnostics
            .into_iter()
            .map(|reported| reported.diagnostic)
            .collect())
    }

    /// The diagnostics of the `.sql` file of named queries at `path`, or none when its
    /// project's `queries` do not name it.
    pub fn check_queries(&mut self, path: &Path, text: &str) -> Result<Vec<Diagnostic>, String> {
        let settings = self.settings(path)?;
        let resolved = self.resolved(path);
        let named = settings.base.as_ref().is_some_and(|base| {
            project::names_query_file(
                path,
                &resolved,
                &self.files.canonical(base),
                &settings.queries,
            )
        });
        if !named {
            return Ok(Vec::new());
        }
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(module::check_queries(text, &name, &settings))
    }

    /// `name(params) -> row` of each named query of the `.sql` file `path`, as
    /// `sqlty check -v` prints them.
    pub fn query_types(&mut self, path: &Path, text: &str) -> Result<Vec<String>, String> {
        let settings = self.settings(path)?;
        let format = settings.query_format;
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (dialect, schema) = (settings.dialect, settings.schema.as_deref());
        Ok(queries::split(text, format, &name)
            .into_iter()
            .map(|query| {
                let sql = &query.sql;
                let (routed, routed_schema) = analyzer::route(sql, dialect, schema);
                let parsed = analyzer::parse(sql, routed);
                let row = parsed
                    .columns(routed_schema)
                    .as_deref()
                    .and_then(analyzer::row_type)
                    .or_else(|| {
                        let rows = if routed == dialect {
                            parsed.returns_rows()
                        } else {
                            analyzer::returns_rows(sql, dialect)
                        };
                        rows.then(|| "unknown".to_string())
                    });
                let mut params = parsed.params(sql, routed_schema).unwrap_or_default();
                params.sort_by_key(|param| param.index);
                let listed: Vec<String> = params
                    .iter()
                    .map(|param| {
                        let spelled = match &param.name {
                            Some(name) => query
                                .renames
                                .iter()
                                .find(|(renamed, _)| renamed == name)
                                .map_or(name.clone(), |(_, original)| original.clone()),
                            None => format!("${}", param.index),
                        };
                        format!(
                            "{spelled}: {}",
                            param.python_type.as_deref().unwrap_or("Any")
                        )
                    })
                    .collect();
                let row = row.unwrap_or_else(|| "None".to_string());
                format!("{}({}) -> {row}", query.name, listed.join(", "))
            })
            .collect())
    }

    /// `text` as `sqlty check --fix` writes it.
    pub fn fix(&mut self, path: &Path, text: &str) -> Result<String, String> {
        let settings = self.settings(path)?;
        self.update(path, text);
        let helpers = self.helpers(&settings);
        let module = &self.modules[path];
        let checker = Checker::of(&settings, Some(&helpers));
        let parsed = module.parsed();
        let name = parsed.as_ref().and_then(|parsed| parsed.module.clone());
        Ok(module::fix(
            text,
            parsed.as_deref(),
            name.as_deref(),
            &checker,
            &settings,
        ))
    }

    /// `text` as `sqlty rewrite` writes it.
    pub fn rewrite(&mut self, path: &Path, text: &str) -> Result<String, String> {
        let settings = self.settings(path)?;
        Ok(module::rewrite(
            text,
            &Checker::of(&settings, None),
            &settings,
        ))
    }

    /// `text` as `sqlty strip` writes it.
    pub fn strip(&mut self, path: &Path, text: &str) -> Result<String, String> {
        let settings = self.settings(path)?;
        Ok(module::strip(text, &Checker::of(&settings, None)))
    }

    /// `text` as `sqlty format` writes it.
    pub fn format(&mut self, path: &Path, text: &str) -> Result<String, String> {
        let settings = self.settings(path)?;
        Ok(module::format(text, &Checker::of(&settings, None)))
    }

    /// What completion reads of the buffer [`Workspace::check`] last read for `path`.
    pub fn completion(&mut self, path: &Path) -> Option<Completion> {
        let settings = self.settings(path).ok()?;
        let helpers = self.helpers(&settings);
        let module = self.modules.get(path)?;
        let checker = Checker::of(&settings, Some(&helpers));
        let helper_starts = match &module.parsed() {
            Some(parsed) => module::helper_starts(parsed, parsed.module.as_deref(), &checker),
            None => Vec::new(),
        };
        Some(Completion {
            schema: settings.schema.clone(),
            dialect: settings.dialect,
            import_style: settings.import_style.clone(),
            helper_starts,
        })
    }
}

/// [`Discovery::walk`], each directory on the pool.
fn walk(discovery: &Discovery, root: &Path) -> Result<Vec<PathBuf>, FindError> {
    fn visit(discovery: &Discovery, directory: &Path) -> Result<Vec<PathBuf>, FindError> {
        let (mut files, directories) = discovery.entries(directory)?;
        let nested: Vec<_> = directories
            .par_iter()
            .map(|directory| visit(discovery, directory))
            .collect();
        for walked in nested {
            files.extend(walked?);
        }
        Ok(files)
    }
    let mut found = visit(discovery, root)?;
    found.par_sort();
    Ok(found)
}

/// The directory of `path`, empty for a root.
fn parent(path: &Path) -> &Path {
    path.parent().unwrap_or(Path::new(""))
}

/// `work` of every item, in order, on all cores.
pub fn par_map<T: Sync, R: Send>(items: &[T], work: impl Fn(&T) -> R + Sync) -> Vec<R> {
    pool().install(|| {
        items
            .par_iter()
            .map(|item| stacker::maybe_grow(STACK_RED_ZONE, STACK_SEGMENT, || work(item)))
            .collect()
    })
}

/// The threads that parse and check modules, with stacks above the analyzer's red zone.
#[expect(
    clippy::expect_used,
    reason = "a check cannot go on without its threads"
)]
fn pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .thread_name(|i| format!("sqlty-check-{i}"))
            .stack_size(32 << 20)
            .build()
            .expect("the check threads start")
    })
}
