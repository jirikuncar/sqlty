//! Prototype of option C of `plans/2026-11-20-typed-receivers.md`: the method a helper call
//! resolves to, asked of ty's semantic model (`ty_python_semantic`) instead of a resolver
//! of our own. Files are read, never imported or executed.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use ruff_db::diagnostic::Diagnostic;
use ruff_db::files::{system_path_to_file, File, Files};
use ruff_db::parsed::parsed_module;
use ruff_db::source::source_text;
use ruff_db::system::{OsSystem, System, SystemPath, SystemPathBuf};
use ruff_db::vendored::VendoredFileSystem;
use ruff_db::Db as SourceDb;
use ruff_python_ast::visitor::source_order::{walk_expr, walk_stmt, SourceOrderVisitor};
use ruff_python_ast::{Expr, ExprAttribute, Stmt};
use ruff_text_size::{Ranged, TextRange};
use sqlty_python::helpers::typed::Resolved;
use ty_module_resolver::{file_to_module, SearchPathSettings, UseDefaultStrategy};
use ty_python_core::program::ProgramSettings;
use ty_python_core::ProgramFile;
use ty_python_semantic::dependency::DependencyMetadata;
use ty_python_semantic::lint::{LintRegistry, RuleSelection};
use ty_python_semantic::{
    default_lint_registry, definitions_for_attribute, AnalysisSettings, HasType, Program,
    PythonVersionWithSource, SemanticModel,
};

/// Where ty finds modules, as its `environment.root`, `extra-paths` and site-packages.
#[derive(Clone, Debug, Default)]
pub struct Paths {
    /// First-party roots: the project, and `src/` in it.
    pub roots: Vec<PathBuf>,
    /// Searched first; where `.pyi` stubs of untyped libraries live (`typings/`).
    pub extra: Vec<PathBuf>,
    /// The `site-packages` directories of the environment.
    pub site_packages: Vec<PathBuf>,
}

/// The database ty's queries run on: the files of the disk, typeshed from the binary.
#[salsa::db]
#[derive(Clone)]
struct Database {
    storage: salsa::Storage<Self>,
    files: Files,
    system: OsSystem,
    vendored: VendoredFileSystem,
    rules: Arc<RuleSelection>,
    analysis: Arc<AnalysisSettings>,
    settings: Arc<ProgramSettings>,
}

impl Database {
    fn new(cwd: &SystemPath, paths: &Paths) -> Self {
        let utf8 = |paths: &[PathBuf]| -> Vec<SystemPathBuf> {
            paths
                .iter()
                .filter(|path| path.is_dir())
                .filter_map(|path| SystemPathBuf::from_path_buf(path.clone()).ok())
                .collect()
        };
        let vendored = ty_vendored::file_system().clone();
        let mut db = Self {
            storage: salsa::Storage::new(None),
            files: Files::default(),
            system: OsSystem::new(cwd),
            settings: Arc::new(ProgramSettings::empty(&vendored)),
            vendored,
            rules: Arc::new(RuleSelection::from_registry(default_lint_registry())),
            analysis: Arc::new(AnalysisSettings::default()),
        };
        let search = SearchPathSettings {
            src_roots: utf8(&paths.roots),
            extra_paths: utf8(&paths.extra),
            site_packages_paths: utf8(&paths.site_packages),
            ..SearchPathSettings::empty()
        };
        let Ok(search_paths) =
            search.to_search_paths(db.system(), db.vendored(), &UseDefaultStrategy);
        search_paths.try_register_static_roots(&db);
        db.settings = Arc::new(ProgramSettings {
            search_paths,
            ..ProgramSettings::empty(&db.vendored)
        });
        db
    }

    fn program(&self) -> Program<'_> {
        Program::from_settings(self, &self.settings)
    }
}

#[salsa::db]
impl salsa::Database for Database {}

#[salsa::db]
impl SourceDb for Database {
    fn vendored(&self) -> &VendoredFileSystem {
        &self.vendored
    }

    fn system(&self) -> &dyn System {
        &self.system
    }

    fn files(&self) -> &Files {
        &self.files
    }
}

#[salsa::db]
impl ty_module_resolver::Db for Database {}

#[salsa::db]
impl ty_python_core::Db for Database {
    fn should_check_file(&self, file: File) -> bool {
        !file.path(self).is_vendored_path()
    }
}

#[salsa::db]
impl ty_python_semantic::Db for Database {
    fn check_file(&self, _file: File) -> Vec<Diagnostic> {
        Vec::new()
    }

    fn program_file(&self, file: File) -> ProgramFile<'_> {
        self.program().program_file(self, file)
    }

    fn python_version_with_source(&self, _file: File) -> &PythonVersionWithSource {
        &self.settings.python_version
    }

    fn rule_selection(&self, _file: File) -> &RuleSelection {
        &self.rules
    }

    fn lint_registry(&self) -> &LintRegistry {
        default_lint_registry()
    }

    fn analysis_settings(&self, _file: File) -> &AnalysisSettings {
        &self.analysis
    }

    fn dependency_metadata(&self, _file: File) -> Option<&DependencyMetadata> {
        None
    }

    fn verbose(&self) -> bool {
        false
    }

    fn is_open_file(&self, _file: File) -> bool {
        false
    }

    fn dyn_clone(&self) -> Box<dyn ty_python_semantic::Db> {
        Box::new(self.clone())
    }
}

/// The callees of the method calls of a module whose attribute is wanted.
struct Callees<'a, 'w> {
    wanted: &'w dyn Fn(&str) -> bool,
    found: Vec<&'a ExprAttribute>,
}

impl<'a> SourceOrderVisitor<'a> for Callees<'a, '_> {
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Call(call) = expr {
            if let Expr::Attribute(attribute) = call.func.as_ref() {
                if (self.wanted)(attribute.attr.as_str()) {
                    self.found.push(attribute);
                }
            }
        }
        walk_expr(self, expr);
    }
}

/// The qualified name (`Class.method`) of the function or class whose name is at `focus`.
fn qualified(body: &[Stmt], focus: TextRange, prefix: &str) -> Option<String> {
    for stmt in body {
        if !stmt.range().contains_range(focus) {
            continue;
        }
        return match stmt {
            Stmt::FunctionDef(function) if function.name.range() == focus => {
                Some(format!("{prefix}{}", function.name))
            }
            Stmt::ClassDef(class) if class.name.range() == focus => {
                Some(format!("{prefix}{}", class.name))
            }
            Stmt::ClassDef(class) => {
                qualified(&class.body, focus, &format!("{prefix}{}.", class.name))
            }
            // An overload under `if TYPE_CHECKING:` or a version check.
            Stmt::If(_) | Stmt::Try(_) | Stmt::With(_) => {
                let mut nested = Nested {
                    focus,
                    prefix,
                    found: None,
                };
                walk_stmt(&mut nested, stmt);
                nested.found
            }
            _ => None,
        };
    }
    None
}

/// The definitions in the bodies of a compound statement.
struct Nested<'p> {
    focus: TextRange,
    prefix: &'p str,
    found: Option<String>,
}

impl<'a> SourceOrderVisitor<'a> for Nested<'_> {
    fn visit_body(&mut self, body: &'a [Stmt]) {
        if self.found.is_none() {
            self.found = qualified(body, self.focus, self.prefix);
        }
    }
}

thread_local! {
    /// The file the thread is checking.
    static CURRENT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    /// The thread's handle of the database.
    static HANDLE: RefCell<Option<Database>> = const { RefCell::new(None) };
}

static CONFIGURED: OnceLock<(PathBuf, Paths)> = OnceLock::new();
static DATABASE: OnceLock<Option<Mutex<Database>>> = OnceLock::new();

/// The types ty gives the receivers asked, with whether the method was found, counted;
/// kept once [`trace_receivers`] is called.
static RECEIVERS: OnceLock<Mutex<std::collections::BTreeMap<(String, bool), usize>>> =
    OnceLock::new();

/// Makes the resolver count the receiver types it sees, for [`report`].
pub fn trace_receivers() {
    let _ = RECEIVERS.set(Mutex::default());
}

/// The counters, and the receiver types when traced, as lines.
pub fn report() -> String {
    use std::sync::atomic::Ordering::Relaxed;
    let mut lines = vec![format!(
        "ty: {} files, {} method calls asked, {} resolved, {:.3} s of thread time",
        STATS.files.load(Relaxed),
        STATS.callees.load(Relaxed),
        STATS.resolved.load(Relaxed),
        STATS.nanos.load(Relaxed) as f64 / 1e9,
    )];
    if let Some(types) = RECEIVERS.get() {
        let types = types
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut rows: Vec<_> = types.iter().collect();
        rows.sort_by(|a, b| b.1.cmp(a.1));
        for ((ty, found), count) in rows {
            let mark = if *found { "resolved" } else { "unresolved" };
            lines.push(format!("{count:6} {mark:10} {ty}"));
        }
    }
    lines.join("\n")
}

/// Counters of the prototype, printed by `SQLTY_TY_STATS=1` (see [`report`]).
pub static STATS: Stats = Stats {
    files: std::sync::atomic::AtomicUsize::new(0),
    callees: std::sync::atomic::AtomicUsize::new(0),
    resolved: std::sync::atomic::AtomicUsize::new(0),
    nanos: std::sync::atomic::AtomicU64::new(0),
};

/// How much ty was asked, summed over the threads.
pub struct Stats {
    /// Files a resolution ran for.
    pub files: std::sync::atomic::AtomicUsize,
    /// Method calls asked.
    pub callees: std::sync::atomic::AtomicUsize,
    /// Method calls with a definition.
    pub resolved: std::sync::atomic::AtomicUsize,
    /// Time spent in ty, in nanoseconds of thread time.
    pub nanos: std::sync::atomic::AtomicU64,
}

/// Runs `check` as the check of the file at `path`, for the resolver to know which file
/// the text it is given is of.
pub fn with_file<T>(path: &Path, check: impl FnOnce() -> T) -> T {
    let previous = CURRENT.with(|current| current.replace(Some(path.to_path_buf())));
    let result = check();
    CURRENT.with(|current| *current.borrow_mut() = previous);
    result
}

/// Makes ty resolve the receivers the checks cannot type, over the modules of `paths`,
/// with relative paths read from `cwd`. The database is built when a check first asks.
/// Returns whether this call installed it.
pub fn install(cwd: &Path, paths: &Paths) -> bool {
    if CONFIGURED.set((cwd.to_path_buf(), paths.clone())).is_err() {
        return false;
    }
    sqlty_python::helpers::typed::install(Box::new(resolve));
    true
}

/// The database of the installed paths.
fn database() -> Option<&'static Mutex<Database>> {
    DATABASE
        .get_or_init(|| {
            let (cwd, paths) = CONFIGURED.get()?;
            let cwd = SystemPath::from_std_path(cwd)?;
            Some(Mutex::new(Database::new(cwd, paths)))
        })
        .as_ref()
}

/// The definitions of the wanted method calls of the file being checked, when its text on
/// disk is `text`.
fn resolve(text: &str, wanted: &dyn Fn(&str) -> bool) -> Resolved {
    let Some(path) = CURRENT.with(|current| current.borrow().clone()) else {
        return Resolved::new();
    };
    let Some(shared) = database() else {
        return Resolved::new();
    };
    let started = std::time::Instant::now();
    let found = HANDLE.with(|handle| {
        let mut handle = handle.borrow_mut();
        let db = handle.get_or_insert_with(|| {
            shared
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        });
        resolve_in(db, &path, text, wanted).unwrap_or_default()
    });
    let count = |counter: &std::sync::atomic::AtomicUsize, n: usize| {
        counter.fetch_add(n, std::sync::atomic::Ordering::Relaxed);
    };
    count(&STATS.files, 1);
    count(&STATS.resolved, found.len());
    STATS.nanos.fetch_add(
        u64::try_from(started.elapsed().as_nanos()).unwrap_or(u64::MAX),
        std::sync::atomic::Ordering::Relaxed,
    );
    found
}

fn resolve_in(
    db: &Database,
    path: &Path,
    text: &str,
    wanted: &dyn Fn(&str) -> bool,
) -> Option<Resolved> {
    let file = system_path_to_file(db, SystemPath::from_std_path(path)?).ok()?;
    // A buffer that differs from the disk is not what ty would read.
    if source_text(db, file).as_str() != text {
        return None;
    }
    let program_file = ty_python_semantic::Db::program_file(db, file);
    let model = SemanticModel::new(db, program_file);
    let parsed = parsed_module(db, program_file.python_file(db)).load(db);
    let mut callees = Callees {
        wanted,
        found: Vec::new(),
    };
    callees.visit_body(parsed.suite());
    STATS
        .callees
        .fetch_add(callees.found.len(), std::sync::atomic::Ordering::Relaxed);
    let mut resolved = Resolved::new();
    for attribute in callees.found {
        let mut names = Vec::new();
        for definition in definitions_for_attribute(&model, attribute) {
            let Some(definition) = definition.definition() else {
                continue;
            };
            let defined_in = definition.program_file(db);
            let focus = {
                let parsed = parsed_module(db, defined_in.python_file(db)).load(db);
                let focus = definition.focus_range(db, &parsed).range();
                qualified(parsed.suite(), focus, "")
            };
            let module = file_to_module(db, defined_in.resolver_file(db));
            if let (Some(module), Some(name)) = (module, focus) {
                let pair = (module.name(db).to_string(), name);
                if !names.contains(&pair) {
                    names.push(pair);
                }
            }
        }
        if let Some(types) = RECEIVERS.get() {
            let ty = attribute.value.inferred_type(&model).map_or_else(
                || "<none>".to_string(),
                |ty| ty.display(db, &model.program_environment()).to_string(),
            );
            let key = (ty, !names.is_empty());
            *types
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .entry(key)
                .or_default() += 1;
        }
        if !names.is_empty() {
            let range = attribute.range();
            resolved.insert((range.start().to_u32(), range.end().to_u32()), names);
        }
    }
    Some(resolved)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn resolves_methods_through_stubs_bases_and_context_managers() {
        let root = std::env::temp_dir().join(format!("sqlty-ty-{}", std::process::id()));
        let root = root.canonicalize().unwrap_or(root);
        let write = |path: &str, text: &str| {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write(
            "typings/postgres/__init__.pyi",
            "from contextlib import AbstractContextManager\n\
             from .cursors import SimpleCursorBase\n\
             class Postgres:\n\
             \x20   def one(self, sql: str) -> object: ...\n\
             \x20   def get_cursor(self) -> AbstractContextManager[SimpleCursorBase]: ...\n",
        );
        write(
            "typings/postgres/cursors.pyi",
            "class SimpleCursorBase:\n    def one(self, sql: str) -> object: ...\n",
        );
        write(
            "typings/postgres/orm.pyi",
            "from typing import ClassVar\nfrom . import Postgres\n\
             class Model:\n    db: ClassVar[Postgres]\n",
        );
        let module = "from postgres import Postgres\n\
            from postgres.orm import Model\n\
            class DB(Postgres): ...\n\
            class Website:\n    db: DB\n\
            class Participant(Model):\n\
            \x20   def a(self, website: Website, other):\n\
            \x20       self.db.one('a')\n\
            \x20       website.db.one('b')\n\
            \x20       other.one('c')\n\
            \x20       with self.db.get_cursor() as cursor:\n\
            \x20           cursor.one('d')\n";
        write("app/m.py", module);
        let root = std::fs::canonicalize(&root).unwrap();
        let cwd = SystemPath::from_std_path(&root).unwrap();
        let db = Database::new(
            cwd,
            &Paths {
                roots: vec![root.clone()],
                extra: vec![root.join("typings")],
                site_packages: Vec::new(),
            },
        );
        let found = resolve_in(&db, &root.join("app/m.py"), module, &|name| name == "one").unwrap();
        let at = |call: &str| {
            let start = u32::try_from(module.find(call).unwrap()).unwrap();
            let end = start + u32::try_from(call.len()).unwrap();
            found.get(&(start, end)).cloned()
        };
        let method = |module: &str, name: &str| Some(vec![(module.to_string(), name.to_string())]);
        assert_eq!(at("self.db.one"), method("postgres", "Postgres.one"));
        assert_eq!(at("website.db.one"), method("postgres", "Postgres.one"));
        assert_eq!(at("other.one"), None);
        assert_eq!(
            at("cursor.one"),
            method("postgres.cursors", "SimpleCursorBase.one")
        );
        assert!(resolve_in(&db, &root.join("app/m.py"), "edited", &|_| true).is_none());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
