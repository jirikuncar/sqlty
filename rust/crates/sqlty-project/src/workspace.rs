//! The Python files `sqlty check PATH...` reads, each with its settings, schema and the
//! helpers of the others; an editor's buffers replace their files.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use rayon::prelude::*;
use sqlty_python::calls::Helper;
use sqlty_python::checker::{self, Diagnostic, SiteKind};
use sqlty_python::index::{helpers_of, Builder, HelperIndex, Roots};
use sqlty_python::project::{self, helper_roots, Discovery, Files, Kind, Listing, Packages, Read};
use sqlty_python::queries;
use sqlty_python::rewriter::ImportStyle;
use sqlty_python::suppress;
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

/// A module with its settings and helpers.
type Prepared = (PathBuf, Arc<FileSettings>, Arc<HelperIndex>);

/// A checked module: its text, its names and what is read of it.
struct Module {
    text: Arc<str>,
    names: Vec<String>,
    resolved: PathBuf,
    parsed: Option<Arc<Source>>,
    helpers: Dict<Helper>,
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
        };
        workspace.paths = workspace.project_paths(paths)?;
        let collected = workspace.collect(force_exclude)?;
        let mut read = Vec::new();
        for path in collected {
            workspace.settings(&path)?;
            match workspace.decoded(&path) {
                Ok(text) => read.push((path, Arc::<str>::from(text))),
                Err(why) => workspace.unreadable.push((path, why)),
            }
        }
        let modules: Vec<(PathBuf, Module)> = pool().install(|| {
            read.into_par_iter()
                .map(|(path, text)| {
                    let module = workspace.module(&path, text, parse);
                    (path, module)
                })
                .collect()
        });
        for (path, module) in modules {
            workspace.order.push(path.clone());
            workspace.modules.insert(path, module);
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
        let names = self.packages.names(&resolved, &self.paths, &self.flags.cwd);
        let init = path.file_name().is_some_and(|name| name == "__init__.py");
        let name = format!("{}{}", names[0], if init { ".__init__" } else { "" });
        let parsed = parse
            .then(|| {
                stacker::maybe_grow(STACK_RED_ZONE, STACK_SEGMENT, || {
                    Source::new(&text, self.version).map(|source| {
                        let source = source.named(Some(&name));
                        source.targets();
                        Arc::new(source)
                    })
                })
            })
            .flatten();
        let helpers = parsed.as_deref().map(helpers_of).unwrap_or_default();
        Module {
            text,
            names,
            resolved,
            parsed,
            helpers,
        }
    }

    fn absolute(&self, path: &Path) -> PathBuf {
        self.flags.cwd.join(path)
    }

    /// `path` with its links resolved: those of its directory, or its own when it is one.
    fn resolved(&self, path: &Path) -> PathBuf {
        self.files.canonical(&self.absolute(path))
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
                let loaded = self.flags.load(&directory, &self.files, &mut warn)?;
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
                    let walked = self
                        .discovery
                        .walk(path)
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

    /// The `.sql` files of named queries under the paths: those a `queries` key of their
    /// projects matches, relative to the current directory when under it, sorted.
    pub fn query_files(&mut self) -> Result<Vec<PathBuf>, String> {
        let mut directories: Vec<PathBuf> = self
            .paths
            .clone()
            .into_iter()
            .map(|path| match self.kind(&path) {
                Some(Kind::Dir) => path,
                _ => path.parent().map(Path::to_path_buf).unwrap_or_default(),
            })
            .collect();
        directories.extend(
            self.order
                .iter()
                .map(|path| path.parent().map(Path::to_path_buf).unwrap_or_default()),
        );
        let roots: Vec<PathBuf> = self.paths.iter().map(|path| self.resolved(path)).collect();
        let mut seen = HashSet::new();
        let mut found = std::collections::BTreeSet::new();
        for directory in directories {
            if !seen.insert(directory.clone()) {
                continue;
            }
            let all = self.directory(&directory.join("_"))?;
            for settings in all.all() {
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
            let Some(parsed) = &module.parsed else {
                continue;
            };
            for name in &module.names {
                builder.add_found(name, Arc::clone(parsed), &module.helpers);
            }
        }
        let index = builder.build();
        self.index = Some(Arc::clone(&index));
        index
    }

    /// The index with the `functions` of `settings` registered.
    fn helpers(&mut self, settings: &FileSettings) -> Arc<HelperIndex> {
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
        let directory = self
            .resolved(path)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        if !self.directories.contains_key(&directory) {
            let mut warn = |warning| self.schemas.warn(warning);
            let made = self
                .flags
                .load(&directory, &self.files, &mut warn)
                .and_then(|loaded| {
                    Directory::new(
                        loaded,
                        &self.flags,
                        &self.files,
                        &self.schemas,
                        self.version,
                    )
                    .map(Arc::new)
                });
            self.directories.insert(directory.clone(), made);
        }
        self.directories[&directory].clone()
    }

    /// The settings of the file at `path`.
    pub fn settings(&mut self, path: &Path) -> Result<Arc<FileSettings>, String> {
        let directory = self.directory(path)?;
        Ok(directory.file(&self.resolved(path), &self.discovery))
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
                        let name = module
                            .parsed
                            .as_ref()
                            .and_then(|parsed| parsed.module.clone());
                        Ok(check_module(
                            &module.text,
                            module.parsed.as_ref(),
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
                        let name = module
                            .parsed
                            .as_ref()
                            .and_then(|parsed| parsed.module.clone());
                        Ok(module::fix(
                            &module.text,
                            module.parsed.as_deref(),
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
                        let parsed = module.parsed.as_deref();
                        let name = parsed.and_then(|parsed| parsed.module.as_deref());
                        Ok(module::statistics(&module.text, parsed, name, &checker))
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
                let settings = self.settings(&path)?;
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
        let name = module
            .parsed
            .as_ref()
            .and_then(|parsed| parsed.module.clone());
        let found = check_module(text, module.parsed.as_ref(), name.as_deref(), &checker);
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
        let format = settings
            .query_format
            .parse::<queries::Format>()
            .expect("query-format is validated");
        let checker = Checker::of(&settings, None);
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let found = queries::check_queries(text, format, &name, &checker.options, &checker.rules);
        let ignores = suppress::sql_ignores(text);
        Ok(suppress::apply_sql(&ignores, found, &checker.rules))
    }

    /// `name(params) -> row` of each named query of the `.sql` file `path`, as
    /// `sqlty check -v` prints them.
    pub fn query_types(&mut self, path: &Path, text: &str) -> Result<Vec<String>, String> {
        let settings = self.settings(path)?;
        let format = settings
            .query_format
            .parse::<queries::Format>()
            .expect("query-format is validated");
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (dialect, schema) = (settings.dialect, settings.schema.as_deref());
        Ok(queries::split(text, format, &name)
            .into_iter()
            .map(|query| {
                let sql = &query.sql;
                let row = analyzer::infer_type(sql, dialect, schema).or_else(|| {
                    analyzer::returns_rows(sql, dialect).then(|| "unknown".to_string())
                });
                let mut params = analyzer::analyze_params(sql, dialect, schema).unwrap_or_default();
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
        let name = module
            .parsed
            .as_ref()
            .and_then(|parsed| parsed.module.clone());
        Ok(module::fix(
            text,
            module.parsed.as_deref(),
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
        let helper_starts = match &module.parsed {
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::flags::Configs;
    use sqlty_python::project::normalized;
    use std::collections::BTreeMap;

    struct Tree(BTreeMap<PathBuf, &'static [u8]>);

    impl Files for Tree {
        fn read(&self, path: &Path) -> Read {
            self.0
                .get(path)
                .filter(|data| **data != b"<unreadable>")
                .map_or(Read::Missing, |data| Read::Text(data.to_vec()))
        }

        fn list(&self, path: &Path) -> Listing {
            let path = &normalized(path);
            let mut entries: Vec<(String, Kind)> = Vec::new();
            for file in self.0.keys() {
                let Ok(rest) = file.strip_prefix(path) else {
                    continue;
                };
                let mut parts = rest.components();
                let Some(name) = parts.next() else { continue };
                let name = name.as_os_str().to_string_lossy().into_owned();
                let kind = if parts.next().is_some() {
                    Kind::Dir
                } else {
                    Kind::File
                };
                if !entries.iter().any(|(seen, _)| *seen == name) {
                    entries.push((name, kind));
                }
            }
            if entries.is_empty() {
                Listing::Missing
            } else {
                Listing::Entries(entries)
            }
        }
    }

    const CONFIG: &str = "[tool.sqlty]\nschema = \"schema.sql\"\n\n[[tool.sqlty.functions]]\nimport-path = \"app.db.fetch\"\nquery-param = \"query\"\n";

    fn opened(files: &[(&str, &'static str)]) -> Workspace {
        open(files).expect("opens")
    }

    fn open(files: &[(&str, &'static str)]) -> Result<Workspace, String> {
        let tree = Tree(
            files
                .iter()
                .map(|(path, text)| (PathBuf::from(path), text.as_bytes()))
                .collect(),
        );
        Workspace::root(Path::new("/repo"), Arc::new(tree), (3, 14), Vec::new())
    }

    fn project() -> Workspace {
        opened(&[
            ("/repo/pyproject.toml", CONFIG),
            ("/repo/schema.sql", "CREATE TABLE users (id INT NOT NULL);"),
            ("/repo/app/__init__.py", ""),
            ("/repo/app/db.py", "def fetch(query: str) -> list: ...\n"),
            (
                "/repo/queries.sql",
                "-- name: one\nSELECT nope FROM users;\n",
            ),
        ])
    }

    fn rules(found: &[Diagnostic]) -> Vec<&'static str> {
        found.iter().map(|found| found.rule.name()).collect()
    }

    #[test]
    fn checks_a_buffer_against_the_schema_and_its_ignores() {
        let mut workspace = project();
        let path = Path::new("/repo/app/q.py");
        let found = workspace
            .check(path, "q = \"--sql\\nSELECT nope FROM users\"\n")
            .expect("checks");
        assert_eq!(rules(&found), ["unknown-column"]);
        let ignored = "q = \"--sql\\nSELECT nope FROM users\"  # sqlty: ignore[unknown-column]\n";
        assert_eq!(
            rules(&workspace.check(path, ignored).expect("checks")),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn checks_the_calls_of_configured_and_project_helpers() {
        let mut workspace = project();
        let text = "from app.db import fetch\n\nfetch(\"SELECT nope FROM users\")\n";
        let found = workspace
            .check(Path::new("/repo/app/use.py"), text)
            .expect("checks");
        assert_eq!(rules(&found), ["unknown-column"]);
        assert_eq!(
            workspace
                .completion(Path::new("/repo/app/use.py"))
                .expect("completes")
                .helper_starts,
            [32]
        );
    }

    #[test]
    fn checks_only_the_sql_files_queries_names() {
        let mut workspace = project();
        let text = "-- name: one\nSELECT nope FROM users;\n";
        assert_eq!(
            rules(
                &workspace
                    .check(Path::new("/repo/queries.sql"), text)
                    .expect("checks")
            ),
            Vec::<&str>::new()
        );
        let mut named = opened(&[
            (
                "/repo/pyproject.toml",
                "[tool.sqlty]\nschema = \"schema.sql\"\nqueries = [\"queries.sql\"]\n",
            ),
            ("/repo/schema.sql", "CREATE TABLE users (id INT NOT NULL);"),
        ]);
        let found = named
            .check(Path::new("/repo/queries.sql"), text)
            .expect("checks");
        assert_eq!(rules(&found), ["unknown-column"]);
    }

    #[test]
    fn fixes_rewrites_and_strips() {
        let mut workspace = project();
        let path = Path::new("/repo/app/q.py");
        let text = "q = \"--sql\\nSELECT id FROM users\"\n";
        let fixed = workspace.fix(path, text).expect("fixes");
        assert!(
            fixed.ends_with("q = SQL[tuple[int]](\"--sql\\nSELECT id FROM users\")\n"),
            "{fixed}"
        );
        assert_eq!(workspace.rewrite(path, text).expect("rewrites"), fixed);
        assert_eq!(workspace.strip(path, &fixed).expect("strips"), text);
    }

    #[test]
    fn reports_an_invalid_config() {
        let error = open(&[("/repo/pyproject.toml", "[tool.sqlty]\ndialect = \"nope\"\n")]).err();
        assert!(error.is_some_and(|error| error.contains("unknown dialect 'nope'")));
        let error = open(&[("/repo/pyproject.toml", "[tool.sqlty]\nuntyped = \"nope\"\n")]).err();
        assert!(error.is_some_and(|error| error.ends_with("untyped must be 'skip' or 'sentinel'")));
        let mut excluded = opened(&[("/repo/.venv/sqlty.toml", "dialect = \"nope\"\n")]);
        let error = excluded
            .check(Path::new("/repo/.venv/q.py"), "x = 1\n")
            .err();
        assert!(error.is_some_and(|error| error.contains("unknown dialect 'nope'")));
    }

    #[test]
    fn routes_files_to_databases_and_overrides() {
        const CONFIG: &str = "[project]\nrequires-python = \">=3.15\"\n\n[tool.sqlty]\nschema = \"app.sql\"\n\n[tool.sqlty.databases.analytics]\ndialect = \"sqlite\"\nschema = \"analytics.sql\"\ninclude = [\"jobs/**\"]\n\n[tool.sqlty.databases.lake]\ndialect = \"duckdb\"\n\n[[tool.sqlty.overrides]]\ninclude = [\"legacy/**\"]\nrules = { unknown-column = \"ignore\" }\n";
        let mut workspace = opened(&[
            ("/repo/pyproject.toml", CONFIG),
            ("/repo/app.sql", "CREATE TABLE users (id TEXT NOT NULL);"),
            (
                "/repo/analytics.sql",
                "CREATE TABLE events (n INTEGER NOT NULL);",
            ),
        ]);
        let events = "q = \"--sql\\nSELECT nope FROM events\"\n";
        let found = workspace
            .check(Path::new("/repo/jobs/q.py"), events)
            .expect("checks");
        assert_eq!(rules(&found), ["unknown-column"]);
        assert_eq!(
            workspace
                .settings(Path::new("/repo/jobs/q.py"))
                .expect("settings")
                .dialect,
            "sqlite"
        );
        let users = "q = \"--sql\\nSELECT nope FROM users\"\n";
        assert_eq!(
            rules(
                &workspace
                    .check(Path::new("/repo/app/q.py"), users)
                    .expect("checks")
            ),
            ["unknown-column"]
        );
        let ignored = workspace
            .check(Path::new("/repo/legacy/q.py"), users)
            .expect("checks");
        assert_eq!(rules(&ignored), Vec::<&str>::new());
        assert!(
            workspace
                .settings(Path::new("/repo/q.py"))
                .expect("settings")
                .closed_params
        );
        assert_eq!(
            workspace.warnings(),
            ["Table and column names are not checked for duckdb"]
        );
    }

    #[test]
    fn warns_once_about_what_the_schemas_do_not_read() {
        let workspace = opened(&[
            (
                "/repo/pyproject.toml",
                "[tool.sqlty]\nschema = [\"schema.sql\", \"gone.sql\", \"none/*.sql\"]\n\
                 [tool.sqlty.databases.logs]\ntypes = { \"t.c\" = \"int\" }\n",
            ),
            (
                "/repo/schema.sql",
                "CREATE VIEW bad (a) AS SELECT id FROM (;\nCREATE TYPE mood AS ENUM ('a') FOO;",
            ),
        ]);
        assert_eq!(
            workspace.warnings(),
            [
                "Schema pattern matches no files: none/*.sql",
                "Schema file not found: /repo/gone.sql",
                "Schema view bad could not be parsed; its columns are unknown",
                "Schema statement could not be parsed and is ignored: CREATE TYPE mood AS ENUM ('a') FOO",
            ]
        );
    }

    #[test]
    fn reads_the_project_without_a_configuration() {
        let mut workspace = opened(&[
            ("/repo/pyproject.toml", "[tool"),
            ("/repo/broken.py", "x = (\n"),
            ("/repo/gone.py", "<unreadable>"),
        ]);
        let path = Path::new("/repo/broken.py");
        let found = workspace
            .check(path, "q = \"--sql\\nSELECT 1\"  # sqlty: ignore\nx = (\n")
            .expect("checks");
        assert_eq!(rules(&found), Vec::<&str>::new());
        let completion = workspace.completion(path).expect("completes");
        assert_eq!(completion.helper_starts, Vec::<usize>::new());
        assert_eq!(completion.dialect, "postgres");
    }

    #[test]
    fn writes_the_types_the_settings_ask_for() {
        let mut workspace = opened(&[
            (
                "/repo/pyproject.toml",
                "[tool.sqlty]\nschema = \"schema.sql\"\nuntyped = \"sentinel\"\n\
                 json-types = \"typing.TypedDict\"\nparam-types = true\ndriver = \"psycopg\"\n\
                 types = { \"users.id\" = \"app.UserId\" }\nrules = { sqruff = \"warn\" }\n",
            ),
            (
                "/repo/schema.sql",
                "CREATE TABLE users (id INT NOT NULL, meta JSON NOT NULL);",
            ),
        ]);
        let path = Path::new("/repo/q.py");
        let text = "q = \"--sql\\nSELECT id, json_build_object('a', 1) AS m FROM users WHERE id = %(id)s\"\n";
        let rewritten = workspace.rewrite(path, text).expect("rewrites");
        assert!(rewritten.contains("app.UserId"), "{rewritten}");
        assert!(rewritten.contains("TypedDict"), "{rewritten}");
        let found = workspace
            .check(path, "q = \"--sql\\nselect distinct(id) from users\"\n")
            .expect("checks");
        assert_eq!(rules(&found), ["stale-rewrite", "sqruff"]);
    }

    #[test]
    fn reports_invalid_settings() {
        for (config, error) in [
            ("json-types = \"nope\"", "json-types must be 'typing.Any' or 'typing.TypedDict'"),
            ("import-style = \"nope\"", "Unsupported import-style: 'nope'"),
            ("schema = \"schema.sql\"\ntypes = { \"(\" = \"int\" }", "[tool.sqlty.types]: "),
            ("[tool.sqlty.databases.logs]\nschema = \"schema.sql\"\n[tool.sqlty.types]\n\"(\" = \"int\"", "[tool.sqlty.types]: "),
        ] {
            let text: &'static str = Box::leak(format!("[tool.sqlty]\n{config}\n").into_boxed_str());
            let found = open(&[("/repo/pyproject.toml", text), ("/repo/schema.sql", "CREATE TABLE t (a INT);")]);
            let found = found.err().unwrap_or_default();
            assert!(found.contains(error), "{config}: {found}");
        }
    }

    #[test]
    fn merges_the_rules_of_databases_and_overrides() {
        let mut workspace = opened(&[
            (
                "/repo/pyproject.toml",
                "[tool.sqlty]\nrules = { select-star = \"error\" }\n\
                 [tool.sqlty.databases.logs]\ndialect = \"sqlite\"\ninclude = [\"logs/**\"]\n\
                 rules = { select-star = \"ignore\" }\n",
            ),
            (
                "/repo/sub/pyproject.toml",
                "[tool.sqlty]\n[[tool.sqlty.overrides]]\ninclude = [\"old/**\"]\n",
            ),
        ]);
        let text = "q = \"--sql\\nSELECT * FROM t\"\n";
        let logs = workspace
            .check(Path::new("/repo/logs/q.py"), text)
            .expect("checks");
        assert_eq!(rules(&logs), Vec::<&str>::new());
        let old = workspace
            .check(Path::new("/repo/sub/old/q.py"), text)
            .expect("checks");
        assert_eq!(rules(&old), Vec::<&str>::new());
    }

    #[test]
    fn registers_helpers_found_on_the_extra_roots() {
        let tree = Tree(BTreeMap::from([
            (
                PathBuf::from("/repo/pyproject.toml"),
                b"[tool.sqlty]\n[[tool.sqlty.functions]]\nimport-path = \"lib.fetch\"\nquery-param = \"query\"\n".as_slice(),
            ),
            (PathBuf::from("/site/lib.py"), b"def fetch(query: str) -> None: ...\n".as_slice()),
        ]));
        let mut workspace = Workspace::root(
            Path::new("/repo"),
            Arc::new(tree),
            (3, 14),
            vec![PathBuf::from("/site")],
        )
        .expect("opens");
        let text = "from lib import fetch\n\nfetch(\"SELEC 1\")\n";
        let found = workspace
            .check(Path::new("/repo/q.py"), text)
            .expect("checks");
        assert_eq!(rules(&found), ["unparsed-query"]);
    }

    fn tree(files: &[(&str, &'static [u8])]) -> Shared {
        let tree = files
            .iter()
            .map(|(path, data)| (PathBuf::from(path), *data))
            .collect();
        Shared(Arc::new(Tree(tree)))
    }

    fn command(files: &Shared, paths: &[&str], flags: Flags) -> Result<Workspace, String> {
        let mut discovery = Discovery::new(Box::new(files.clone()), &flags.cwd);
        if let Some(fixed) = flags.fixed_discovery() {
            discovery = discovery.fixed(fixed);
        }
        if matches!(flags.configs, Configs::Isolated) {
            discovery = discovery.isolated();
        }
        let options = Options {
            discovery,
            flags,
            version: (3, 14),
            extra_roots: Vec::new(),
            force_exclude: true,
            parse: true,
        };
        let paths: Vec<PathBuf> = paths.iter().map(PathBuf::from).collect();
        Workspace::open(&paths, files.clone(), options)
    }

    fn flags(files: &Shared, config_file: Option<&str>, assignments: &[&str]) -> Flags {
        let assignments: Vec<String> = assignments.iter().map(ToString::to_string).collect();
        let severities = vec![("select-star".to_string(), "error".to_string())];
        Flags::new(
            Path::new("/repo"),
            config_file.map(Path::new),
            false,
            &assignments,
            severities,
            files,
        )
        .expect("flags")
    }

    const COMMAND: &[(&str, &[u8])] = &[
        (
            "/repo/pyproject.toml",
            b"[tool.sqlty]\nschema = \"schema.sql\"\nqueries = [\"sql/*.sql\"]\nextend-exclude = [\"skip\"]\n\n[[tool.sqlty.functions]]\nimport-path = \"app.db.fetch\"\nquery-param = \"query\"\n",
        ),
        ("/repo/app/db.py", b"def fetch(query: str) -> None: ...\n"),
        ("/repo/app/d.py", b"from app.db import fetch\n\nfetch(\"SELECT id FROM users\")\nq = \"--sql\\nSELEC 1\"\nr = \"--sql\\nUPDATE users SET name = NULL\"\n"),
        ("/repo/sub/pyproject.toml", b"[tool.sqlty]\n"),
        ("/repo/schema.sql", b"CREATE TABLE users (id INT NOT NULL, name TEXT);"),
        ("/repo/sql/users.sql", b"-- name: by_id\nSELECT id, name FROM users WHERE id = :id;\n-- name: count\nSELECT count(*) FROM users WHERE id = $1;\n-- name: touch\nUPDATE users SET name = NULL;\n"),
        ("/repo/sql/notes.txt", b""),
        ("/repo/app/a.py", b"\xef\xbb\xbfq = \"--sql\\nSELECT nope FROM users\"\r\nr = \"--sql\\nSELECT *, id FROM users\"\r\nt = f\"--sql\\nSELECT {1}\"\nu = \"--sql\\nSELECT sum(id) AS total FROM users\"\n"),
        ("/repo/app/b.py", b"from app.a import q\nx = (\n"),
        ("/repo/app/c.py", b"<unreadable>"),
        ("/repo/app/start.py", b"q = \"\x80\""),
        ("/repo/app/continued.py", b"q = \"\xc3(\""),
        ("/repo/app/ended.py", b"q = \"\xc3"),
        ("/repo/app/notes.md", b""),
        ("/repo/skip/s.py", b"q = \"--sql\\nSELECT nope FROM users\"\n"),
    ];

    #[test]
    fn reads_the_paths_of_a_command_line() {
        let files = tree(COMMAND);
        let flags = flags(&files, None, &[]);
        let paths = [
            "app",
            "pyproject.toml",
            "sub/pyproject.toml",
            "app/notes.md",
            "app/a.py",
            "skip/s.py",
        ];
        let mut workspace = command(&files, &paths, flags).expect("opens");
        let read = ["app/a.py", "app/b.py", "app/d.py", "app/db.py"];
        assert_eq!(workspace.files(), read.map(PathBuf::from));
        let unreadable: Vec<String> = workspace
            .unreadable()
            .iter()
            .map(|(path, why)| format!("{}: {why}", path.display()))
            .collect();
        assert_eq!(
            unreadable,
            [
                "app/c.py: unreadable",
                "app/continued.py: invalid continuation byte at byte 5",
                "app/ended.py: unexpected end of data at byte 5",
                "app/start.py: invalid start byte at byte 5",
            ]
        );
        let text = workspace
            .text(Path::new("app/a.py"))
            .expect("read")
            .to_string();
        assert!(text.starts_with("q = ") && !text.contains('\r'), "{text:?}");
        workspace.update(Path::new("app/a.py"), &text);
        let found: Vec<_> = workspace
            .check_all()
            .expect("checks")
            .iter()
            .map(|found| rules(found))
            .collect();
        let a = vec![
            "unknown-column",
            "stale-rewrite",
            "fstring-skipped",
            "stale-rewrite",
        ];
        assert_eq!(found, [a, vec![], vec!["unparsed-query"], vec![]]);
        let fixed = workspace.fix_all().expect("fixes");
        assert!(fixed[0].contains("SQL["), "{}", fixed[0]);
        let counts: Vec<Vec<_>> = workspace
            .statistics_all()
            .expect("counts")
            .into_iter()
            .map(|counts| {
                counts
                    .into_iter()
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .collect()
            })
            .collect();
        assert_eq!(
            counts,
            [
                vec![("fstring-skipped", 1), ("partially typed", 1), ("typed", 2)],
                vec![],
                vec![("typed", 1), ("unparsed", 1)],
                vec![],
            ]
        );
        assert_eq!(
            workspace.query_files().expect("lists"),
            [PathBuf::from("sql/users.sql")]
        );
        let explained = workspace
            .explain(Path::new("app/a.py"), 9)
            .expect("explains");
        assert_eq!(
            explained[0],
            "  total: type and nullability by inference rule postgres.aggregate.sum-integers"
        );
        assert_eq!(
            workspace
                .explain(Path::new("app/a.py"), 0)
                .expect("explains"),
            Vec::<String>::new()
        );
        assert_eq!(
            workspace
                .explain(Path::new("app/z.py"), 9)
                .expect("explains"),
            Vec::<String>::new()
        );
        let queries = files.text(Path::new("/repo/sql/users.sql")).expect("text");
        assert_eq!(
            workspace
                .query_types(Path::new("sql/users.sql"), &queries)
                .expect("types"),
            [
                "by_id(id: int) -> tuple[int, str | None]",
                "count($1: int) -> tuple[int]",
                "touch() -> None",
            ]
        );
        assert_eq!(
            workspace
                .format(
                    Path::new("app/a.py"),
                    "q = \"\"\"--sql\nselect id from users where id=%s\"\"\"\n"
                )
                .expect("formats"),
            "q = \"\"\"--sql\nselect id from users\nwhere id = %s\"\"\"\n"
        );
    }

    #[test]
    fn writes_a_module_as_the_commands_do() {
        let files = tree(COMMAND);
        let mut workspace =
            command(&files, &["app/b.py"], flags(&files, None, &[])).expect("opens");
        let settings = workspace.settings(Path::new("app/b.py")).expect("settings");
        let text = "q = \"--sql\\nSELECT id FROM users\"\nt = f\"--sql\\nSELECT {1}\"\n";
        let rewritten = crate::rewritten(text, &settings);
        assert!(rewritten.contains("q = SQL[tuple[int]]("), "{rewritten}");
        assert_eq!(crate::stripped(&rewritten, &settings), text);
        let formatted = crate::formatted(
            "q = \"\"\"--sql\nselect 1 from t where b=%s\"\"\"\n",
            &settings,
        );
        assert_eq!(
            formatted,
            "q = \"\"\"--sql\nselect 1 from t\nwhere b = %s\"\"\"\n"
        );
        let skipped = crate::skipped_strings(text, &settings).expect("checks");
        assert_eq!(rules(&skipped), ["fstring-skipped"]);
    }

    #[test]
    fn reports_paths_it_cannot_read() {
        let files = tree(COMMAND);
        let error = command(&files, &["gone.py"], flags(&files, None, &[])).err();
        assert_eq!(error.as_deref(), Some("path does not exist: gone.py"));
        let files = tree(&[
            (
                "/repo/pyproject.toml",
                b"[tool.sqlty]\nqueries = [\"none/*.sql\"]\n",
            ),
            ("/repo/q.sql", b""),
        ]);
        let mut workspace = command(&files, &["."], flags(&files, None, &[])).expect("opens");
        assert_eq!(
            workspace.query_files().expect("lists"),
            Vec::<PathBuf>::new()
        );
        assert_eq!(
            workspace.warnings(),
            ["Schema pattern matches no files: none/*.sql"]
        );
    }

    #[test]
    fn reads_the_configuration_the_flags_name() {
        let files = tree(&[
            (
                "/repo/pyproject.toml",
                b"[tool.sqlty]\ndialect = \"nope\"\n",
            ),
            (
                "/repo/ci.toml",
                b"dialect = \"mysql\"\nexclude = [\"gen\"]\nschema = \"schema.sql\"\n",
            ),
            ("/repo/schema.sql", b"CREATE TABLE t (a INT);"),
            ("/repo/q.py", b"q = \"--sql\\nSELECT * FROM t\"\n"),
        ]);
        let mut flags = flags(
            &files,
            Some("ci.toml"),
            &["rules.unknown-column = \"warn\"", "exclude = [\"x\"]"],
        );
        assert_eq!(
            flags.take_discovery().keys().collect::<Vec<_>>(),
            ["exclude"]
        );
        assert_eq!(
            flags
                .fixed_discovery()
                .expect("fixed")
                .keys()
                .collect::<Vec<_>>(),
            ["exclude"]
        );
        flags.dialect = Some("sqlite".to_string());
        flags.schema = Some(toml::Value::Array(vec![toml::Value::String(
            "schema.sql".to_string(),
        )]));
        let mut workspace = command(&files, &["schema.sql", "q.py"], flags).expect("opens");
        assert_eq!(workspace.files(), [PathBuf::from("q.py")]);
        assert_eq!(
            workspace.query_files().expect("lists"),
            Vec::<PathBuf>::new()
        );
        let settings = workspace.settings(Path::new("q.py")).expect("settings");
        assert_eq!(settings.dialect, "sqlite");
        assert_eq!(
            settings.rules.get("unknown-column").map(String::as_str),
            Some("warn")
        );
        assert_eq!(
            settings.rules.get("select-star").map(String::as_str),
            Some("error")
        );
        let isolated =
            Flags::new(Path::new("/repo"), None, true, &[], Vec::new(), &files).expect("flags");
        assert!(isolated.fixed_discovery().is_none());
        let mut workspace = command(&files, &["q.py", "ci.toml"], isolated).expect("opens");
        assert_eq!(
            workspace
                .settings(Path::new("q.py"))
                .expect("settings")
                .dialect,
            "postgres"
        );
    }

    #[test]
    fn reports_flags_it_cannot_take() {
        let files = tree(&[
            ("/repo/sqlty.toml", b"extend = \"gone.toml\"\n"),
            ("/repo/sub/pyproject.toml", b"[tool"),
            ("/repo/sub/q.py", b"x = 1\n"),
            ("/repo/dir/x.py", b""),
        ]);
        let new = |config: Option<&str>, assignments: &[&str], severities: &[(&str, &str)]| {
            let assignments: Vec<String> = assignments.iter().map(ToString::to_string).collect();
            let severities = severities
                .iter()
                .map(|(rule, severity)| (rule.to_string(), severity.to_string()))
                .collect();
            Flags::new(
                Path::new("/repo"),
                config.map(Path::new),
                false,
                &assignments,
                severities,
                &files,
            )
            .err()
        };
        assert_eq!(
            new(Some("gone.toml"), &[], &[]).as_deref(),
            Some("config file does not exist: gone.toml")
        );
        assert_eq!(
            new(Some("/repo/dir"), &[], &[]).as_deref(),
            Some("config path is not a file: /repo/dir")
        );
        assert_eq!(
            new(Some("sub/pyproject.toml"), &[], &[]).map(|error| error.is_empty()),
            Some(false)
        );
        assert_eq!(
            new(None, &[], &[("select-str", "warn")]).as_deref(),
            Some("--warn: unknown rule 'select-str' (did you mean 'select-star'?); run `sqlty rule` for the list")
        );
        let error = new(None, &["import-style = \"nope\""], &[]);
        assert!(error.is_none());
        let flags = Flags::new(
            Path::new("/repo"),
            None,
            false,
            &["import-style = \"nope\"".to_string()],
            Vec::new(),
            &files,
        )
        .expect("flags");
        let error = command(&files, &["sub/q.py"], flags)
            .err()
            .unwrap_or_default();
        assert!(error.starts_with("--config: "), "{error}");
        let flags =
            Flags::new(Path::new("/repo"), None, false, &[], Vec::new(), &files).expect("flags");
        let mut workspace = command(&files, &["sub/q.py"], flags).expect("opens");
        let warnings = workspace.warnings();
        assert!(warnings[0].starts_with("Failed to parse "), "{warnings:?}");
        let error = workspace
            .settings(Path::new("dir/x.py"))
            .err()
            .unwrap_or_default();
        assert!(error.contains("gone.toml"), "{error}");
        let flags =
            Flags::new(Path::new("/repo"), None, false, &[], Vec::new(), &files).expect("flags");
        let error = command(&files, &["dir/x.py"], flags)
            .err()
            .unwrap_or_default();
        assert!(error.contains("gone.toml"), "{error}");
        assert!(workspace.check_all().expect("checks")[0].is_empty());
    }

    #[test]
    fn maps_items_on_all_cores() {
        assert_eq!(par_map(&[1, 2, 3], |item| item * 2), [2, 4, 6]);
    }
}
