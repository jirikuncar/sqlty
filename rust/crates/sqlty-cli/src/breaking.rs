//! `sqlty breaking`: the queries and schema of two revisions, checked across.
//!
//! During a rolling deploy the code of one revision runs against the schema of another.
//! `--against` names the baseline `A`; `--head` (the working tree by default) names `B`.
//! The backward check reads the queries of `A` against the schema of `B` (migrate first),
//! the forward check the queries of `B` against the schema of `A` (deploy first). Each
//! reports only what the other schema breaks: diagnostics a query does not already get
//! against its own schema.
//!
//! Every revision but the working tree is checked out with `git worktree add --detach`
//! into a temporary directory, removed afterwards, so the working tree is never touched.
//! Only the analyzed paths, the files of the directories above them and the schema are
//! checked out, unless a configuration reads more (see [`checkout`]). A baseline can also
//! be a snapshot `--save` wrote, which needs no git history, or a schema file, which has
//! no queries and so is only checked forward.

use std::borrow::BorrowMut;
use std::cell::OnceCell;
use std::collections::{BTreeSet, HashMap};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use clap::ValueEnum;
use sqlty_project::{Checker, FileSettings, Flags, Options, Shared, Workspace};
use sqlty_python::checker::Diagnostic;
use sqlty_python::index::{defines_helpers, helpers_of, Builder, HelperIndex, Module, Roots};
use sqlty_python::project::{self, Discovery};
use sqlty_python::rules::{RuleConfig, Severity};
use sqlty_python::types::stdlib::Version;
use sqlty_sql::analyzer::{self, Schema};

use crate::{
    github_annotation, json_diagnostic, pretty, report_unreadable, sarif_log, warn_all, Breaking,
    Host, Lines, OutputFormat, EXIT_FOUND,
};

mod snapshot;

pub use snapshot::Snapshot;

/// The arguments of `sqlty breaking`.
#[derive(clap::Args)]
pub struct Args {
    /// Files or directories to check, in both revisions (default: current directory)
    paths: Vec<PathBuf>,
    /// The baseline: a revision (a branch, tag, SHA or HEAD~1), a snapshot written by
    /// --save, a directory of snapshots (each checked), or a schema .sql file or directory
    /// (forward only, as it has no queries)
    #[arg(long, value_name = "BASELINE")]
    against: Option<String>,
    /// Write a snapshot of --head (its settings, schema and Python files) to PATH, a
    /// baseline for a later --against
    #[arg(long, value_name = "PATH")]
    save: Option<PathBuf>,
    /// The new revision (default: the working tree, uncommitted changes included)
    #[arg(long, value_name = "REF")]
    head: Option<String>,
    /// backward: queries of --against on the schema of --head (migrate first); forward:
    /// queries of --head on the schema of --against (deploy first)
    #[arg(long, value_enum, default_value_t = Direction::Both)]
    direction: Direction,
    /// Print nothing; only the exit code tells
    #[arg(short, long)]
    quiet: bool,
    /// How findings are printed: text (the default), json (an array of objects with file,
    /// line, rule, severity, message, direction and against), github (GitHub Actions
    /// annotations, each at its severity) or sarif (a SARIF 2.1.0 log, for GitHub code
    /// scanning)
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    output_format: OutputFormat,
}

/// Which revision's queries are checked against which schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum Direction {
    /// The queries of the baseline against the schema of the head.
    Backward,
    /// The queries of the head against the schema of the baseline.
    Forward,
    /// Both.
    Both,
    /// The new statements of the migrations of the head, whatever the direction.
    #[value(skip)]
    Migration,
}

impl Direction {
    /// `sqlty.breaking.direction`; anything else is both.
    pub fn parse(name: &str) -> Self {
        match name {
            "backward" => Self::Backward,
            "forward" => Self::Forward,
            _ => Self::Both,
        }
    }

    /// The name of a finding's direction: backward, forward or migration, never both.
    fn name(self) -> &'static str {
        match self {
            Self::Forward => "forward",
            Self::Migration => "migration",
            _ => "backward",
        }
    }

    pub(crate) fn backward(self) -> bool {
        self != Self::Forward
    }

    pub(crate) fn forward(self) -> bool {
        self != Self::Backward
    }
}

/// What a schema change breaks in one file.
pub struct Finding {
    /// [`Direction::Backward`], [`Direction::Forward`] or [`Direction::Migration`].
    pub direction: Direction,
    /// The label of the baseline.
    pub against: String,
    /// The file, relative to its revision's root.
    pub path: PathBuf,
    /// What the other schema reports.
    pub diagnostic: Diagnostic,
    /// The text of the file.
    pub text: Arc<str>,
}

/// A Python file of a revision, or a `.sql` file of named queries (no `module`).
pub struct File {
    /// Relative to the revision's root.
    pub relative: PathBuf,
    /// The text, line endings read as `\n`.
    pub text: Arc<str>,
    /// The module name, `.__init__` for a package.
    pub module: Option<String>,
    /// The module names its helpers are imported by.
    pub names: Vec<String>,
    /// What it is checked with.
    pub settings: Arc<FileSettings>,
}

/// What a schema is loaded from, owned: two equal keys check queries alike.
/// `types` of a configuration, sorted.
pub type Types = Vec<(String, String)>;

type Key = (Option<Arc<str>>, &'static str, Types);

fn key_of(settings: &FileSettings) -> Key {
    let (_, dialect, types) = settings.schema_key();
    (settings.schema_sql.clone(), dialect, types.to_vec())
}

/// The schema another revision gives a file.
pub struct Against {
    key: Key,
    schema: Option<Schema>,
}

impl Against {
    fn of(settings: &FileSettings) -> Self {
        Self {
            key: key_of(settings),
            schema: settings.schema.clone(),
        }
    }

    /// No schema: what a baseline without the database gives.
    fn none(own: &FileSettings) -> Self {
        Self {
            key: (None, own.dialect, own.types.clone()),
            schema: None,
        }
    }

    /// `own` with this schema, `None` when it is `own`'s.
    fn settings(self, own: &FileSettings) -> Option<FileSettings> {
        if self.key == key_of(own) {
            return None;
        }
        let (schema_sql, _, types) = self.key;
        Some(FileSettings {
            schema: self.schema,
            schema_sql,
            types,
            ..own.clone()
        })
    }
}

/// A revision whose files are checked: a git revision or a snapshot.
pub trait Queries {
    /// The files, with their settings.
    fn files(&mut self) -> Result<Vec<File>, String>;
    /// The helpers a file with `settings` calls.
    fn helpers(&mut self, settings: &FileSettings) -> Arc<HelperIndex>;
}

/// A revision whose schema the other revision's queries are checked against.
pub trait Schemas {
    /// The schema of the file `relative`, checked with `own`.
    fn against(&mut self, relative: &Path, own: &FileSettings) -> Result<Against, String>;
    /// The schema of the database `name` of the configuration of `directory`, for a
    /// database no file of the head is checked against.
    fn database(
        &mut self,
        directory: &Path,
        name: &str,
        own: &FileSettings,
    ) -> Result<Against, String>;
}

/// A checkout of the project at one revision, read as `sqlty check` reads it.
pub struct Revision<W> {
    /// How findings name it.
    pub label: String,
    /// Its root.
    pub root: PathBuf,
    /// Its project.
    pub workspace: W,
}

impl Revision<Workspace> {
    /// `paths` (relative to `root`) of the checkout at `root`, read with no flags.
    pub fn open(
        label: &str,
        root: &Path,
        paths: &[PathBuf],
        files: Shared,
        version: Version,
        extra_roots: Vec<PathBuf>,
    ) -> Result<Self, String> {
        let options = Options {
            flags: Flags {
                cwd: root.to_path_buf(),
                ..Flags::default()
            },
            discovery: Discovery::new(Box::new(files.clone()), root),
            version,
            extra_roots,
            force_exclude: false,
            parse: true,
        };
        Ok(Self {
            label: label.to_string(),
            root: root.to_path_buf(),
            workspace: Workspace::open(paths, files, options)?,
        })
    }
}

/// `path` relative to `root`, as it is when it is not under it.
fn relative(root: &Path, path: &Path) -> PathBuf {
    root.join(path)
        .strip_prefix(root)
        .map_or_else(|_| path.to_path_buf(), Path::to_path_buf)
}

impl<W: BorrowMut<Workspace>> Queries for Revision<W> {
    fn files(&mut self) -> Result<Vec<File>, String> {
        let mut found = Vec::new();
        let workspace: &mut Workspace = self.workspace.borrow_mut();
        for path in workspace.files().to_vec() {
            let names = workspace.names(&path).unwrap_or_default().to_vec();
            let init = path.file_name().is_some_and(|name| name == "__init__.py");
            let module = format!("{}{}", names[0], if init { ".__init__" } else { "" });
            found.push(File {
                relative: relative(&self.root, &path),
                text: Arc::from(workspace.text(&path).unwrap_or("")),
                module: Some(module),
                names,
                settings: workspace.settings(&path)?,
            });
        }
        let disk = Shared(Arc::new(sqlty_project::Disk));
        for path in workspace.query_files()? {
            let text = disk.text(&self.root.join(&path)).unwrap_or_default();
            found.push(File {
                relative: relative(&self.root, &path),
                text: Arc::from(text.replace("\r\n", "\n")),
                module: None,
                names: Vec::new(),
                settings: workspace.settings(&path)?,
            });
        }
        Ok(found)
    }

    fn helpers(&mut self, settings: &FileSettings) -> Arc<HelperIndex> {
        self.workspace.borrow_mut().helpers(settings)
    }
}

impl<W: BorrowMut<Workspace>> Schemas for Revision<W> {
    fn against(&mut self, relative: &Path, _own: &FileSettings) -> Result<Against, String> {
        let path = self.root.join(relative);
        let settings = self.workspace.borrow_mut().settings(&path)?;
        Ok(Against::of(&settings))
    }

    fn database(
        &mut self,
        directory: &Path,
        name: &str,
        own: &FileSettings,
    ) -> Result<Against, String> {
        let path = self.root.join(directory).join("_");
        let found = self.workspace.borrow_mut().database(&path, name)?;
        Ok(found.map_or_else(|| Against::none(own), |settings| Against::of(&settings)))
    }
}

/// A schema without queries, such as a dump or migrations: a baseline only forward.
pub struct SchemaFiles {
    sql: Arc<str>,
    loaded: HashMap<(&'static str, Types), Schema>,
}

impl Schemas for SchemaFiles {
    fn against(&mut self, _relative: &Path, own: &FileSettings) -> Result<Against, String> {
        let key = (own.dialect, own.types.clone());
        let schema = match self.loaded.get(&key) {
            Some(schema) => schema.clone(),
            None => {
                let schema = sqlty_project::load_schema(&self.sql, own.dialect, &own.types)?;
                self.loaded.insert(key, schema.clone());
                schema
            }
        };
        Ok(Against {
            key: (Some(Arc::clone(&self.sql)), own.dialect, own.types.clone()),
            schema: Some(schema),
        })
    }

    fn database(
        &mut self,
        _directory: &Path,
        _name: &str,
        own: &FileSettings,
    ) -> Result<Against, String> {
        self.against(Path::new(""), own)
    }
}

/// What `--against` names.
pub enum Baseline {
    /// A git revision, checked out while it is kept.
    Revision {
        /// The revision.
        revision: Box<Revision<Workspace>>,
        /// Its checkout, removed when dropped.
        _checkout: Worktree,
    },
    /// A snapshot `--save` wrote.
    Snapshot(Snapshot),
    /// Schema files.
    SchemaFiles(String, SchemaFiles),
}

impl Baseline {
    /// How findings name it.
    pub fn label(&self) -> &str {
        match self {
            Self::Revision { revision, .. } => &revision.label,
            Self::Snapshot(snapshot) => &snapshot.label,
            Self::SchemaFiles(label, _) => label,
        }
    }

    /// The baseline's queries, `None` for schema files.
    pub fn queries(&mut self) -> Option<&mut dyn Queries> {
        match self {
            Self::Revision { revision, .. } => Some(&mut **revision),
            Self::Snapshot(snapshot) => Some(snapshot),
            Self::SchemaFiles(..) => None,
        }
    }

    /// The baseline's schema.
    pub fn schemas(&mut self) -> &mut dyn Schemas {
        match self {
            Self::Revision { revision, .. } => &mut **revision,
            Self::Snapshot(snapshot) => snapshot,
            Self::SchemaFiles(_, files) => files,
        }
    }
}

/// What `against` names, relative to `cwd`: a git revision of the repository at `root`
/// when no such path exists, else a snapshot, each snapshot of a directory, or schema
/// files. A revision is read from `paths` (relative to `root`).
pub fn baselines(
    against: &str,
    cwd: &Path,
    root: &Path,
    paths: &[PathBuf],
    version: Version,
    extra_roots: &[PathBuf],
) -> Result<Vec<Baseline>, String> {
    let path = cwd.join(against);
    if !path.exists() {
        let worktree = Worktree::add(root, against, paths)?;
        let files = Shared(Arc::new(sqlty_project::Disk));
        let checkout = worktree.checkout.clone();
        let revision = Revision::open(
            against,
            &checkout,
            paths,
            files,
            version,
            extra_roots.to_vec(),
        )?;
        return Ok(vec![Baseline::Revision {
            revision: Box::new(revision),
            _checkout: worktree,
        }]);
    }
    if path.is_file()
        && path
            .extension()
            .is_some_and(|extension| extension == "json")
    {
        let snapshot = Snapshot::load(&path, Path::new(against), against, paths, version)?;
        return Ok(vec![Baseline::Snapshot(snapshot)]);
    }
    if path.is_dir() {
        let mut snapshots: Vec<PathBuf> = std::fs::read_dir(&path)
            .into_iter()
            .flatten()
            .filter_map(|entry| Some(entry.ok()?.path()))
            .filter(|file| file.is_file() && file.extension().is_some_and(|ext| ext == "json"))
            .collect();
        snapshots.sort();
        if !snapshots.is_empty() {
            return snapshots
                .iter()
                .map(|file| {
                    let label = file.file_stem().unwrap_or_default().to_string_lossy();
                    let shown = Path::new(against).join(file.file_name().unwrap_or_default());
                    Snapshot::load(file, &shown, &label, paths, version).map(Baseline::Snapshot)
                })
                .collect();
        }
    }
    let disk = Shared(Arc::new(sqlty_project::Disk));
    // The disk reads every directory, so it never needs one read first.
    let (found, _) = project::schema_files(&[against.to_string()], cwd, &disk).unwrap_or_default();
    let texts: Vec<String> = found.iter().filter_map(|file| disk.text(file)).collect();
    if texts.is_empty() {
        return Err(format!("{against} has no schema .sql files"));
    }
    let files = SchemaFiles {
        sql: Arc::from(texts.join("\n")),
        loaded: HashMap::new(),
    };
    Ok(vec![Baseline::SchemaFiles(against.to_string(), files)])
}

/// A file to check, with the other schema and its helpers.
type Work = (File, FileSettings, Option<Arc<HelperIndex>>);

/// The files of `queries` whose schema `schemas` changes.
fn pending(queries: &mut dyn Queries, schemas: &mut dyn Schemas) -> Result<Vec<Work>, String> {
    let mut work = Vec::new();
    for file in queries.files()? {
        let Some(other) = schemas
            .against(&file.relative, &file.settings)?
            .settings(&file.settings)
        else {
            continue;
        };
        let helpers = file
            .module
            .is_some()
            .then(|| queries.helpers(&file.settings));
        work.push((file, other, helpers));
    }
    Ok(work)
}

/// What the schema of `schemas` breaks in the files of `queries`.
pub fn compare(
    direction: Direction,
    against: &str,
    queries: &mut dyn Queries,
    schemas: &mut dyn Schemas,
    version: Version,
) -> Result<Vec<Finding>, String> {
    let work = pending(queries, schemas)?;
    let found = sqlty_project::par_map(&work, |(file, other, helpers)| {
        let Some(module) = &file.module else {
            let name = file
                .relative
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            return sqlty_project::breaking_queries(&file.text, &name, &file.settings, other);
        };
        let source = Module::new(Arc::clone(&file.text), module, version);
        let parsed = source.source();
        let name = parsed.and_then(|parsed| parsed.module.as_deref());
        let helpers = helpers.as_deref();
        let own = Checker::of(&file.settings, helpers);
        let other = Checker::of(other, helpers);
        sqlty_project::breaking(&file.text, parsed, name, &own, &other)
    });
    Ok(work
        .into_iter()
        .zip(found)
        .flat_map(|((file, ..), found)| {
            found.into_iter().map(move |diagnostic| Finding {
                direction,
                against: against.to_string(),
                path: file.relative.clone(),
                diagnostic,
                text: Arc::clone(&file.text),
            })
        })
        .collect())
}

/// The migrations of one database of the head, in the order they run.
struct Migrations {
    /// A path the baseline is asked for: a file of the head checked against this schema,
    /// or `_` in `directory` for the project's own. `None`: ask by database name.
    probe: Option<PathBuf>,
    directory: PathBuf,
    database: String,
    settings: Arc<FileSettings>,
    /// For each file: its shown path, where it is read from and the strictest rules of
    /// the configurations that name it.
    files: Vec<(PathBuf, PathBuf, RuleConfig)>,
    /// The texts, read when a baseline first differs.
    texts: OnceCell<Vec<Arc<str>>>,
}

/// The PostgreSQL schemas of the head, each list of schema files once, with the strictest
/// rules of the configurations that name it.
fn migration_sets(head: &mut Revision<Workspace>) -> Result<Vec<Migrations>, String> {
    let sources: Vec<_> = head
        .workspace
        .schema_sources()?
        .into_iter()
        .filter(|source| source.settings.dialect == "postgres")
        .collect();
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    let mut paths = head.workspace.files().to_vec();
    paths.extend(head.workspace.query_files()?);
    let mut routed = Vec::new();
    for path in paths {
        let schema = head.workspace.settings(&path)?.schema_sql.clone();
        routed.push((relative(&head.root, &path), schema));
    }
    let mut seen: HashMap<Vec<PathBuf>, usize> = HashMap::new();
    let mut sets: Vec<Migrations> = Vec::new();
    for source in sources {
        let files = &source.settings.schema_files;
        let shown: Vec<PathBuf> = files.iter().map(|path| project::normalized(path)).collect();
        let rules = source
            .files
            .iter()
            .map(|settings| Checker::of(settings, None).rules);
        if let Some(&at) = seen.get(&shown) {
            for ((_, _, known), rules) in sets[at].files.iter_mut().zip(rules) {
                *known = known.strictest(&rules);
            }
            continue;
        }
        let probe = routed
            .iter()
            .find(|(_, schema)| match (schema, &source.settings.schema_sql) {
                (Some(schema), Some(own)) => Arc::ptr_eq(schema, own),
                _ => false,
            })
            .map(|(path, _)| path.clone())
            .or_else(|| (source.database == analyzer::DEFAULT).then(|| source.directory.join("_")));
        let checked = files.iter().zip(&shown).zip(rules);
        let files = checked
            .map(|((path, shown), rules)| (relative_to(shown, &head.root), path.clone(), rules))
            .collect();
        seen.insert(shown, sets.len());
        sets.push(Migrations {
            probe,
            directory: source.directory.clone(),
            database: source.database.clone(),
            settings: Arc::clone(&source.settings),
            files,
            texts: OnceCell::new(),
        });
    }
    Ok(sets)
}

/// What the new statements of the migrations `sets` lock, against the schema `schemas`
/// gives each of their databases.
fn migrations(
    against: &str,
    sets: &[Migrations],
    schemas: &mut dyn Schemas,
) -> Result<Vec<Finding>, String> {
    let disk = Shared(Arc::new(sqlty_project::Disk));
    let mut findings = Vec::new();
    for set in sets {
        let own = &set.settings;
        let other = match &set.probe {
            Some(probe) => schemas.against(probe, own)?,
            None => schemas.database(&set.directory, &set.database, own)?,
        };
        let Some(baseline) = other
            .key
            .0
            .filter(|sql| Some(sql) != own.schema_sql.as_ref())
        else {
            continue;
        };
        let texts = set.texts.get_or_init(|| {
            let read = |(_, path, _): &(PathBuf, PathBuf, RuleConfig)| {
                Arc::from(disk.text(path).unwrap_or_default())
            };
            set.files.iter().map(read).collect()
        });
        let checked: Vec<(&str, &RuleConfig)> = texts
            .iter()
            .zip(&set.files)
            .map(|(text, (_, _, rules))| (&**text, rules))
            .collect();
        let found = sqlty_python::migrations::check_migrations_with(&baseline, &checked);
        for (((path, ..), text), found) in set.files.iter().zip(texts).zip(found) {
            findings.extend(found.into_iter().map(|diagnostic| Finding {
                direction: Direction::Migration,
                against: against.to_string(),
                path: path.clone(),
                diagnostic,
                text: Arc::clone(text),
            }));
        }
    }
    Ok(findings)
}

/// The helpers of `files`, with the `functions` of `settings` found among them.
pub(crate) fn index_of(files: &[File], version: Version) -> Arc<HelperIndex> {
    let mut builder = Builder::new(version);
    for file in files {
        let Some(module) = &file.module else { continue };
        let source = Arc::new(Module::new(Arc::clone(&file.text), module, version));
        let helpers = match defines_helpers(&file.text) {
            true => source.source().map(|parsed| helpers_of(parsed)),
            false => None,
        };
        for name in &file.names {
            builder.add_found(
                name,
                Arc::clone(&source),
                &helpers.clone().unwrap_or_default(),
            );
        }
    }
    builder.build()
}

/// `index` with the `functions` of `settings` registered, found among its modules only.
pub(crate) fn configured(index: &HelperIndex, settings: &FileSettings) -> Arc<HelperIndex> {
    let mut forked = index.fork();
    let read = |_: &Path| None;
    for function in &settings.functions {
        let roots = Roots {
            paths: &[],
            read: &read,
        };
        forked.register(
            &function.import_path,
            &function.query,
            function.row.as_deref(),
            roots,
            function.database.as_deref(),
        );
    }
    forked.build()
}

/// Run `sqlty breaking`.
pub fn run(host: &mut dyn Host, args: &Args) -> Result<i32, String> {
    if args.against.is_none() && args.save.is_none() {
        return Err("sqlty breaking needs --against, --save or both".to_string());
    }
    let cwd = host.cwd();
    let root = repository(&cwd).or_else(|error| {
        let revision = args.head.is_some()
            || args
                .against
                .as_ref()
                .is_some_and(|against| !cwd.join(against).exists());
        match revision {
            true => Err(error),
            false => Ok(std::fs::canonicalize(&cwd).unwrap_or(cwd.clone())),
        }
    })?;
    let named = match args.paths.is_empty() {
        true => vec![cwd.clone()],
        false => args.paths.iter().map(|path| cwd.join(path)).collect(),
    };
    let paths = named
        .iter()
        .map(|path| {
            let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
            let relative = resolved
                .strip_prefix(&root)
                .map_err(|_| format!("{} is not in {}", path.display(), root.display()))?;
            Ok(match relative.as_os_str().is_empty() {
                true => PathBuf::from("."),
                false => relative.to_path_buf(),
            })
        })
        .collect::<Result<Vec<PathBuf>, String>>()?;
    let (version, extra_roots) = (host.python(), host.extra_roots());
    let mut baselines = match &args.against {
        Some(against) => baselines(against, &cwd, &root, &paths, version, &extra_roots)?,
        None => Vec::new(),
    };
    for baseline in &baselines {
        if let Baseline::Revision { revision, .. } = baseline {
            report_unreadable(host, &revision.workspace);
        }
    }
    let head_tree = match &args.head {
        Some(head) => Some(Worktree::add(&root, head, &paths)?),
        None => None,
    };
    let head_root = head_tree.as_ref().map_or(&root, |tree| &tree.checkout);
    let label = args.head.as_deref().unwrap_or("the working tree");
    let files = Shared(host.files());
    let mut head = Revision::open(label, head_root, &paths, files, version, extra_roots)?;
    warn_all(host, &head.workspace);
    report_unreadable(host, &head.workspace);
    if let Some(save) = &args.save {
        let data = snapshot::save(&mut head)?;
        host.write(&cwd.join(save), data.as_bytes())?;
    }
    let sets = migration_sets(&mut head)?;
    let mut findings = Vec::new();
    for baseline in &mut baselines {
        let label = baseline.label().to_string();
        if args.direction.backward() {
            match baseline.queries() {
                Some(queries) => findings.extend(compare(
                    Direction::Backward,
                    &label,
                    queries,
                    &mut head,
                    version,
                )?),
                None => host.err(&format!(
                    "sqlty: {label} has no queries; skipping the backward check\n"
                )),
            }
        }
        if args.direction.forward() {
            let schemas = baseline.schemas();
            findings.extend(compare(
                Direction::Forward,
                &label,
                &mut head,
                schemas,
                version,
            )?);
        }
        findings.extend(migrations(&label, &sets, baseline.schemas())?);
    }
    warn_all(host, &head.workspace);
    for baseline in &baselines {
        if let Baseline::Revision { revision, .. } = baseline {
            warn_all(host, &revision.workspace);
        }
    }
    if !args.quiet {
        report(host, &findings, label, args.output_format);
    }
    let error = findings
        .iter()
        .any(|finding| finding.diagnostic.severity == Severity::Error);
    Ok(if error { EXIT_FOUND } else { 0 })
}

/// Print each finding: `path:line: [rule] message (direction)`, a JSON array with a
/// `direction` and `against` per finding, GitHub Actions annotations at each finding's
/// severity, or a SARIF log.
fn report(host: &mut dyn Host, findings: &[Finding], head: &str, format: OutputFormat) {
    let message = |finding: &Finding| {
        let title = match finding.direction {
            Direction::Backward => format!(
                "the queries of {} against the schema of {head}",
                finding.against
            ),
            Direction::Migration => format!(
                "a statement {head} runs that {} does not have",
                finding.against
            ),
            _ => format!(
                "the queries of {head} against the schema of {}",
                finding.against
            ),
        };
        format!(
            "{} ({}: {title})",
            finding.diagnostic.message,
            finding.direction.name()
        )
    };
    match format {
        OutputFormat::Json | OutputFormat::Sarif => {
            let mut lines = HashMap::new();
            let rows: Vec<_> = findings
                .iter()
                .map(|finding| {
                    let lines = lines
                        .entry(finding.text.as_ptr())
                        .or_insert_with(|| Lines::new(&finding.text));
                    let mut row = json_diagnostic(&finding.path, &finding.diagnostic, lines);
                    if format == OutputFormat::Sarif {
                        row.message = message(finding);
                    }
                    row.breaking = Some(Breaking {
                        direction: finding.direction.name(),
                        against: finding.against.clone(),
                    });
                    row
                })
                .collect();
            match format {
                OutputFormat::Json => host.out(&pretty(&rows)),
                _ => host.out(&pretty(&sarif_log(&host.version(), &rows))),
            }
        }
        OutputFormat::Github => {
            for finding in findings {
                let found = &finding.diagnostic;
                let diagnostic = Diagnostic {
                    rule: found.rule,
                    message: message(finding),
                    severity: found.severity,
                    line: found.line,
                    span: found.span.clone(),
                    fix: Vec::new(),
                };
                host.out(&format!(
                    "{}\n",
                    github_annotation(&finding.path, &diagnostic)
                ));
            }
        }
        OutputFormat::Text => {
            for finding in findings {
                let diagnostic = &finding.diagnostic;
                host.out(&format!(
                    "{}:{}: [{}] {}\n",
                    finding.path.display(),
                    diagnostic.line,
                    diagnostic.rule.name(),
                    message(finding)
                ));
            }
            if !findings.is_empty() {
                host.out(&format!("Found {} breaking change(s)\n", findings.len()));
            }
        }
    }
}

/// The top level of the git repository holding `directory`, resolved.
pub fn repository(directory: &Path) -> Result<PathBuf, String> {
    let top = git(directory, &["rev-parse", "--show-toplevel"])?;
    Ok(std::fs::canonicalize(&top).unwrap_or_else(|_| PathBuf::from(top)))
}

/// A revision checked out into a temporary worktree, removed when dropped.
pub struct Worktree {
    repository: PathBuf,
    directory: PathBuf,
    /// The checkout, resolved.
    pub checkout: PathBuf,
}

impl Worktree {
    /// `reference` of the repository at `root` checked out: the files `paths` need, or all.
    pub fn add(root: &Path, reference: &str, paths: &[PathBuf]) -> Result<Self, String> {
        git(
            root,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("{reference}^{{commit}}"),
            ],
        )?;
        static ADDED: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "sqlty-breaking-{}-{}",
            std::process::id(),
            ADDED.fetch_add(1, Ordering::Relaxed)
        ));
        // A process of the same id left it behind.
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
        // Named like the repository, so its files have the module names they have there.
        let name = root.file_name().unwrap_or_default();
        let mut worktree = Self {
            repository: root.to_path_buf(),
            checkout: directory.join(name),
            directory,
        };
        let checkout = worktree.checkout.to_string_lossy().into_owned();
        git(
            root,
            &[
                "worktree",
                "add",
                "--detach",
                "--no-checkout",
                "--quiet",
                &checkout,
                reference,
            ],
        )?;
        self::checkout(&worktree.checkout, paths)?;
        worktree.checkout =
            std::fs::canonicalize(&worktree.checkout).unwrap_or(worktree.checkout.clone());
        Ok(worktree)
    }
}

impl Drop for Worktree {
    fn drop(&mut self) {
        let checkout = self.checkout.to_string_lossy().into_owned();
        let _ = git(
            &self.repository,
            &["worktree", "remove", "--force", &checkout],
        );
        let _ = std::fs::remove_dir_all(&self.directory);
        let _ = git(&self.repository, &["worktree", "prune"]);
    }
}

const CONFIG_FILES: [&str; 3] = ["sqlty.toml", "pyproject.toml", "Cargo.toml"];

/// Check out what checking `paths` reads: each path, the files of the directories above
/// it (where its configuration is found) and the schema and query files the
/// configurations name. Everything when a path is the root or a configuration reads more:
/// `functions` modules, `databases`, a glob or a file outside the repository.
fn checkout(checkout: &Path, paths: &[PathBuf]) -> Result<(), String> {
    let full = |checkout: &Path| git(checkout, &["reset", "--hard", "--quiet"]).map(|_| ());
    if paths.iter().any(|path| path == Path::new(".")) {
        return full(checkout);
    }
    let listed: Vec<PathBuf> = git(checkout, &["ls-tree", "-r", "--name-only", "-z", "HEAD"])?
        .split('\0')
        .filter(|name| !name.is_empty())
        .map(PathBuf::from)
        .collect();
    let above: BTreeSet<&Path> = paths.iter().flat_map(|path| parents(path)).collect();
    let mut wanted: Vec<PathBuf> = paths.to_vec();
    wanted.extend(
        listed
            .iter()
            .filter(|name| above.contains(parent(name)))
            .cloned(),
    );
    restore(checkout, &wanted, &listed)?;
    let configs: BTreeSet<&Path> = listed
        .iter()
        .filter(|name| {
            name.file_name()
                .is_some_and(|file| CONFIG_FILES.iter().any(|config| file == *config))
                && (above.contains(parent(name)) || paths.iter().any(|path| name.starts_with(path)))
        })
        .map(|name| parent(name))
        .chain(paths.iter().map(PathBuf::as_path))
        .collect();
    let disk = sqlty_project::Disk;
    let base = std::fs::canonicalize(checkout).unwrap_or_else(|_| checkout.to_path_buf());
    let mut needed = Vec::new();
    let found = configs
        .iter()
        .map(|directory| project::find(&base.join(directory), &disk))
        .collect::<Result<Vec<_>, _>>();
    let Ok(found) = found else {
        return full(checkout);
    };
    for found in found.into_iter().flatten() {
        let table = &found.config.table;
        let entries: Vec<String> = ["schema", "queries"]
            .iter()
            .filter_map(|key| table.get(*key))
            .flat_map(|value| match value {
                toml::Value::Array(items) => items
                    .iter()
                    .filter_map(|item| item.as_str().map(str::to_string))
                    .collect::<Vec<String>>(),
                value => value.as_str().map(str::to_string).into_iter().collect(),
            })
            .collect();
        let folder = found.file.parent().unwrap_or(Path::new(""));
        let named: Vec<PathBuf> = entries
            .iter()
            .map(|entry| relative_to(&project::normalized(&folder.join(entry)), &base))
            .collect();
        let magic = |entry: &String| entry.contains(['*', '?', '[']);
        if table.contains_key("functions")
            || table.contains_key("databases")
            || entries.iter().any(magic)
            || named
                .iter()
                .any(|name| name.components().next() == Some(Component::ParentDir))
        {
            return full(checkout);
        }
        needed.extend(named);
    }
    restore(checkout, &needed, &listed)
}

/// The directory of the relative `name`, `.` at the top.
fn parent(name: &Path) -> &Path {
    name.parent().map_or(Path::new("."), dot)
}

/// `path` and the directories above it, as relative paths: `a/b` gives `a` and `.`.
fn parents(path: &Path) -> impl Iterator<Item = &Path> {
    path.ancestors().skip(1).map(dot)
}

fn dot(path: &Path) -> &Path {
    match path.as_os_str().is_empty() {
        true => Path::new("."),
        false => path,
    }
}

/// `path` relative to `base`, through `..` where it is outside.
fn relative_to(path: &Path, base: &Path) -> PathBuf {
    let path: Vec<Component<'_>> = path.components().collect();
    let base: Vec<Component<'_>> = base.components().collect();
    let common = path
        .iter()
        .zip(&base)
        .take_while(|(left, right)| left == right)
        .count();
    let mut relative: PathBuf = base[common..]
        .iter()
        .map(|_| Component::ParentDir)
        .collect();
    relative.extend(&path[common..]);
    match relative.as_os_str().is_empty() {
        true => PathBuf::from("."),
        false => relative,
    }
}

/// Check out the files and directories of `names` that HEAD has.
fn restore(checkout: &Path, names: &[PathBuf], listed: &[PathBuf]) -> Result<(), String> {
    let directories: BTreeSet<&Path> = listed
        .iter()
        .flat_map(|name| parents(name))
        .filter(|directory| *directory != Path::new("."))
        .collect();
    let present: BTreeSet<String> = names
        .iter()
        .filter(|name| listed.contains(name) || directories.contains(name.as_path()))
        .map(|name| name.to_string_lossy().replace('\\', "/"))
        .collect();
    if !present.is_empty() {
        let mut args: Vec<String> = ["checkout", "--quiet", "HEAD", "--"]
            .map(str::to_string)
            .to_vec();
        args.extend(present.iter().map(|name| format!(":(literal){name}")));
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        git(checkout, &args)?;
    }
    Ok(())
}

fn spawn_error(error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::NotFound => "git is not installed".to_string(),
        _ => format!("git: {error}"),
    }
}

/// The standard output of `git ARGS...` in `cwd`, trimmed.
fn git(cwd: &Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git").args(args).current_dir(cwd).output();
    let output = output.map_err(|error| spawn_error(&error))?;
    if !output.status.success() {
        if args.starts_with(&["rev-parse", "--verify"]) {
            let reference = args[args.len() - 1].trim_end_matches("^{commit}");
            return Err(format!(
                "unknown revision {}",
                sqlty_project::repr(reference)
            ));
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let failed = format!("git {} failed", args.join(" "));
        return Err(Some(stderr)
            .filter(|stderr| !stderr.is_empty())
            .unwrap_or(failed));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    const OLD_SCHEMA: &str = "CREATE TABLE users (id INT, bio TEXT);\n";
    const NEW_SCHEMA: &str = "CREATE TABLE users (id INT, handle TEXT);\n";
    const NEW_CODE: &str = "handles = \"--sql\\nSELECT handle FROM users\"\n";

    fn project(name: &str, files: &[(&str, &str)]) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("sqlty-breaking-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (path, text) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("creates");
            std::fs::write(path, text).expect("writes");
        }
        std::fs::canonicalize(root).expect("exists")
    }

    fn open(root: &Path) -> Revision<Workspace> {
        let files = Shared(Arc::new(sqlty_project::Disk));
        let paths = [PathBuf::from(".")];
        Revision::open("head", root, &paths, files, (3, 14), Vec::new()).expect("opens")
    }

    fn snapshot(root: &Path) -> Snapshot {
        let path = root.with_extension("json");
        std::fs::write(&path, snapshot::save(&mut open(root)).expect("saves")).expect("writes");
        let paths = [PathBuf::from(".")];
        Snapshot::load(&path, &path, "snapshot", &paths, (3, 14)).expect("loads")
    }

    fn schema_files(sql: &str) -> SchemaFiles {
        SchemaFiles {
            sql: Arc::from(sql),
            loaded: HashMap::new(),
        }
    }

    #[test]
    fn a_schema_key_shares_the_schema_text() {
        let config = ("pyproject.toml", "[tool.sqlty]\nschema = \"schema.sql\"\n");
        let root = project("shared", &[config, ("schema.sql", NEW_SCHEMA)]);
        let settings = open(&root).workspace.settings(&root.join("q.py"));
        let settings = settings.expect("reads");
        let text = settings.schema_sql.as_ref().expect("a schema");
        let key = key_of(&settings).0.expect("a schema");
        assert!(Arc::ptr_eq(&key, text));
        std::fs::remove_dir_all(&root).expect("removes");
    }

    #[test]
    fn an_unchanged_schema_checks_no_query() {
        let config = ("pyproject.toml", "[tool.sqlty]\nschema = \"schema.sql\"\n");
        let root = project(
            "unchanged",
            &[config, ("schema.sql", NEW_SCHEMA), ("app/q.py", NEW_CODE)],
        );
        let mut saved = snapshot(&root);
        let mut head = open(&root);
        let count = |queries: &mut dyn Queries, schemas: &mut dyn Schemas| {
            pending(queries, schemas).expect("reads").len()
        };
        assert_eq!(count(&mut head, &mut open(&root)), 0);
        assert_eq!(count(&mut head, &mut saved), 0);
        assert_eq!(count(&mut saved, &mut head), 0);
        assert_eq!(count(&mut head, &mut schema_files(NEW_SCHEMA)), 0);
        std::fs::write(root.join("schema.sql"), OLD_SCHEMA).expect("writes");
        let mut head = open(&root);
        assert_eq!(count(&mut head, &mut saved), 1);
        assert_eq!(count(&mut saved, &mut head), 1);
        assert_eq!(count(&mut head, &mut schema_files(NEW_SCHEMA)), 1);
        let findings = compare(Direction::Backward, "v", &mut saved, &mut head, (3, 14));
        let messages: Vec<String> = findings
            .expect("compares")
            .into_iter()
            .map(|finding| finding.diagnostic.message)
            .collect();
        assert_eq!(messages, ["column `handle` does not exist in `users`"]);
        std::fs::remove_dir_all(&root).expect("removes");
    }

    #[test]
    fn a_snapshot_without_a_schema_checks_no_query() {
        let root = project(
            "schemaless",
            &[("pyproject.toml", "[tool.sqlty]\n"), ("app/q.py", NEW_CODE)],
        );
        let mut saved = snapshot(&root);
        std::fs::write(root.join("app/new.py"), NEW_CODE).expect("writes");
        let mut head = open(&root);
        assert_eq!(pending(&mut saved, &mut head).expect("reads").len(), 0);
        assert_eq!(pending(&mut head, &mut saved).expect("reads").len(), 0);
        std::fs::remove_dir_all(&root).expect("removes");
    }

    #[test]
    fn a_snapshot_that_is_not_json_is_an_error() {
        let root = project("broken", &[("broken.json", "{")]);
        let path = root.join("broken.json");
        let error = Snapshot::load(&path, Path::new("broken.json"), "b", &[], (3, 14)).err();
        let error = error.expect("an error");
        assert!(
            error.starts_with("broken.json is not a sqlty snapshot: EOF"),
            "{error}"
        );
        std::fs::remove_dir_all(&root).expect("removes");
    }

    #[test]
    fn a_snapshot_keeps_the_settings_of_its_files() {
        let config = (
            "pyproject.toml",
            "[tool.sqlty]\nschema = \"schema.sql\"\n[tool.sqlty.rules]\nselect-star = \"ignore\"\n\
             [tool.sqlty.types]\nTEXT = \"str\"\n[[tool.sqlty.functions]]\n\
             import-path = \"app.db.fetch\"\nquery-param = \"sql\"\n",
        );
        let helper = (
            "app/db.py",
            "from typing import Annotated\nfrom sqlty import SQL\n\
             def run(q: Annotated[str, SQL]): ...\ndef fetch(sql): ...\n",
        );
        let code = (
            "app/q.py",
            "from app.db import fetch, run\nrun(\"SELECT handle FROM users\")\n\
             fetch(\"SELECT handle FROM users\")\n",
        );
        let root = project(
            "settings",
            &[config, ("schema.sql", NEW_SCHEMA), helper, code],
        );
        let mut saved = snapshot(&root);
        std::fs::write(root.join("schema.sql"), OLD_SCHEMA).expect("writes");
        let mut head = open(&root);
        let findings = compare(Direction::Backward, "v", &mut saved, &mut head, (3, 14));
        let lines: Vec<usize> = findings
            .expect("compares")
            .into_iter()
            .map(|finding| finding.diagnostic.line)
            .collect();
        assert_eq!(lines, [2, 3]);
        let files = saved.files().expect("reads");
        let file = files
            .iter()
            .find(|file| file.relative == Path::new("app/q.py"));
        let settings = &file.expect("saved").settings;
        assert_eq!(
            settings.rules.get("select-star").map(String::as_str),
            Some("ignore")
        );
        assert_eq!(settings.types, [("TEXT".to_string(), "str".to_string())]);
        std::fs::remove_dir_all(&root).expect("removes");
    }

    const USERS: &str = "CREATE TABLE users (id INT, bio TEXT);\n";
    const EVENTS: &str = "CREATE TABLE events (id INT, at TIMESTAMP);\n";
    const INDEX: &str = "SET lock_timeout = '1s';\nCREATE INDEX events_at ON events (at);\n";
    const DATABASES: &str = "[tool.sqlty]\nschema = \"migrations/\"\n\
        [tool.sqlty.databases.events]\nschema = \"events/\"\ninclude = [\"events/**\"]\n";

    fn judged(findings: Vec<Finding>) -> Vec<(PathBuf, &'static str)> {
        let rules = |finding: Finding| (finding.path, finding.diagnostic.rule.name());
        findings.into_iter().map(rules).collect()
    }

    #[test]
    fn a_snapshot_judges_the_migrations_of_databases_it_recorded() {
        let root = project(
            "recorded",
            &[
                ("pyproject.toml", DATABASES),
                ("migrations/0001.sql", USERS),
                ("events/0001.sql", EVENTS),
                ("events/q.py", "x = 1\n"),
                ("app/q.py", "x = 1\n"),
            ],
        );
        let mut saved = snapshot(&root);
        std::fs::write(root.join("events/0002.sql"), INDEX).expect("writes");
        let mut head = open(&root);
        let sets = migration_sets(&mut head).expect("reads");
        let probes: Vec<_> = sets.iter().map(|set| set.probe.as_deref()).collect();
        assert_eq!(
            probes,
            [Some(Path::new("app/q.py")), Some(Path::new("events/q.py"))]
        );
        let found = judged(migrations("s", &sets, &mut saved).expect("judges"));
        assert_eq!(
            found,
            [(PathBuf::from("events/0002.sql"), "missing-concurrently")]
        );
        let own = &sets[0].settings;
        let key = |against: Against| against.key.0;
        let default = saved.database(Path::new("."), "default", own);
        assert!(key(default.expect("reads")).is_some());
        assert!(key(saved
            .database(Path::new("."), "events", own)
            .expect("reads"))
        .is_none());
        std::fs::remove_dir_all(&root).expect("removes");
    }

    #[test]
    fn a_database_the_baseline_lacks_has_no_migrations_to_judge() {
        let root = project(
            "lacks",
            &[
                ("pyproject.toml", DATABASES),
                ("migrations/0001.sql", USERS),
                ("events/0001.sql", EVENTS),
            ],
        );
        let old = project(
            "lacks-old",
            &[
                ("pyproject.toml", "[tool.sqlty]\nschema = \"migrations/\"\n"),
                ("migrations/0001.sql", USERS),
            ],
        );
        std::fs::write(root.join("events/0002.sql"), INDEX).expect("writes");
        let mut head = open(&root);
        let mut baseline = open(&old);
        let sets = migration_sets(&mut head).expect("reads");
        assert_eq!(sets[1].probe, None);
        let own = &sets[1].settings;
        let against = baseline.database(Path::new("."), "events", own);
        assert!(against.expect("reads").key.0.is_none());
        assert!(migrations("old", &sets, &mut baseline)
            .expect("judges")
            .is_empty());
        for directory in [root, old] {
            std::fs::remove_dir_all(directory).expect("removes");
        }
    }

    #[test]
    fn migrations_are_read_when_a_baseline_first_differs_and_once() {
        let root = project(
            "once",
            &[
                ("pyproject.toml", "[tool.sqlty]\nschema = \"migrations/\"\n"),
                ("migrations/0001.sql", EVENTS),
                ("migrations/0002.sql", INDEX),
            ],
        );
        let mut head = open(&root);
        let sets = migration_sets(&mut head).expect("reads");
        let same = sets[0].settings.schema_sql.clone().expect("a schema");
        let found = |sql: &str| judged(migrations("b", &sets, &mut schema_files(sql)).expect("ok"));
        assert_eq!(found(&same), []);
        assert!(sets[0].texts.get().is_none());
        let expected = [(PathBuf::from("migrations/0002.sql"), "missing-concurrently")];
        assert_eq!(found(EVENTS), expected);
        assert!(sets[0].texts.get().is_some());
        assert_eq!(found(&format!("{EVENTS}{USERS}")), expected);
        std::fs::remove_dir_all(&root).expect("removes");
    }

    #[test]
    fn names_each_query_format() {
        use sqlty_python::queries::Format;
        let names = [
            Format::Auto,
            Format::Aiosql,
            Format::Sqlc,
            Format::Pugsql,
            Format::File,
        ]
        .map(snapshot::format_name);
        assert_eq!(names, ["auto", "aiosql", "sqlc", "pugsql", "file"]);
    }

    #[test]
    fn rejects_a_snapshot_of_another_shape() {
        let root = project("shapes", &[("s.json", "")]);
        let path = root.join("s.json");
        let file = r#""files":{"a.py":{"settings":9}}"#;
        for (body, why) in [
            (r#""schemas":{}"#, "invalid schemas"),
            (
                r#""schemas":[{"sql":"","dialect":"nope"}]"#,
                "invalid dialect",
            ),
            (r#""schemas":[],"settings":{}"#, "invalid settings"),
            (
                r#""schemas":[],"settings":[{"dialect":"postgres","schema":3}]"#,
                "invalid schema index",
            ),
            (
                r#""schemas":[],"settings":[{"dialect":"postgres","rules":{"nope":"error"}}]"#,
                "nope",
            ),
            (
                r#""schemas":[],"settings":[],"directories":[]"#,
                "invalid directories",
            ),
            (
                &format!(r#""schemas":[],"settings":[],"directories":{{}},{file}"#),
                "invalid settings index",
            ),
        ] {
            std::fs::write(&path, format!(r#"{{"sqlty":1,{body}}}"#)).expect("writes");
            let error = Snapshot::load(&path, &path, "s", &[PathBuf::from(".")], (3, 14)).err();
            assert!(
                error.as_ref().is_some_and(|error| error.contains(why)),
                "{body}: {error:?}"
            );
        }
        std::fs::remove_dir_all(&root).expect("removes");
    }

    #[test]
    fn spells_paths_relative_to_a_base() {
        let relative = |path: &str, base: &str| relative_to(Path::new(path), Path::new(base));
        assert_eq!(relative("/r/db/s.sql", "/r"), PathBuf::from("db/s.sql"));
        assert_eq!(relative("/r", "/r"), PathBuf::from("."));
        assert_eq!(relative("/x/s.sql", "/r/a"), PathBuf::from("../../x/s.sql"));
        let above: Vec<&Path> = parents(Path::new("a/b")).collect();
        assert_eq!(above, [Path::new("a"), Path::new(".")]);
    }

    #[test]
    fn restores_nothing_without_git_when_nothing_is_listed() {
        assert_eq!(restore(Path::new("/nonexistent"), &[], &[]), Ok(()));
    }

    #[test]
    fn names_why_git_did_not_start() {
        let denied = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        assert_eq!(spawn_error(&denied), "git: permission denied");
    }

    #[test]
    fn names_a_missing_revision_and_a_missing_repository() {
        let directory = std::env::temp_dir().join("sqlty-breaking-no-repository");
        std::fs::create_dir_all(&directory).expect("creates");
        assert_eq!(
            git(
                &directory,
                &["rev-parse", "--verify", "--quiet", "nope^{commit}"]
            ),
            Err("unknown revision 'nope'".to_string())
        );
        assert_eq!(Direction::parse("forward"), Direction::Forward);
        assert_eq!(Direction::parse("both"), Direction::Both);
        assert_eq!(Direction::parse("backward").name(), "backward");
    }
}
