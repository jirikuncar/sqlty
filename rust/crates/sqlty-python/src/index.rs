//! The modules of the checked paths, as `sqlty._helpers.HelperIndex` holds them: the
//! helpers each defines, and the types and row types other modules import from them. A
//! module is read from its source, never imported.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};

use regex::Regex;
use ruff_python_ast::Expr;

use crate::calls::{definitions, helper, index_functions, parameter_names, rank, Helper, Kind};
use crate::module::Import;
use crate::types::extract::extract_type_definitions;
use crate::types::info::{Dict, TypeInfo};
use crate::types::pydantic::Known;
use crate::types::stdlib::Version;
use crate::types::symbols::{Index, ModuleTypes, RowType, Source};
use crate::types::ty::Ty;

/// Local names bound to imported names `(module, name)`.
pub type Functions = HashMap<String, (String, String)>;
/// What `imports_of` gives: the functions, and the modules by local name.
pub type Targets = (Functions, HashMap<String, String>);

/// `_imports_of`: the local names `entries` bind to imported functions `(module, name)`
/// and to modules, in the module `module`.
pub fn imports_of(
    entries: &[Import],
    module: Option<&str>,
) -> (Functions, HashMap<String, String>) {
    let mut functions = Functions::new();
    let mut modules = HashMap::new();
    let package = module.map_or("", |module| {
        module.rsplit_once('.').map_or("", |(package, _)| package)
    });
    for import in entries {
        let Some(base) = &import.module else {
            modules.insert(import.local.clone(), import.original.clone());
            continue;
        };
        let mut base = base.clone();
        if import.level != 0 {
            if module.is_none() {
                continue;
            }
            let parts: Vec<&str> = if package.is_empty() {
                Vec::new()
            } else {
                package.split('.').collect()
            };
            let up = import.level as usize - 1;
            if up > parts.len() {
                continue;
            }
            let mut parts: Vec<&str> = parts[..parts.len() - up].to_vec();
            if !base.is_empty() {
                parts.push(&base);
            }
            base = parts.join(".");
        }
        modules.insert(
            import.local.clone(),
            if base.is_empty() {
                import.original.clone()
            } else {
                format!("{base}.{}", import.original)
            },
        );
        functions.insert(import.local.clone(), (base, import.original.clone()));
    }
    (functions, modules)
}

/// The module a dotted prefix names, through the modules `modules` binds.
pub fn module_of(prefix: &str, modules: &HashMap<String, String>) -> String {
    let (head, rest) = prefix.split_once('.').unwrap_or((prefix, ""));
    match modules.get(head) {
        Some(module) if rest.is_empty() => module.clone(),
        Some(module) => format!("{module}.{rest}"),
        None => prefix.to_string(),
    }
}

/// `_target`: the module and name that `NAME` or `module.NAME` of `source` refers to.
fn target(source: &Source, dotted: &str) -> Option<(String, String)> {
    let (functions, modules) = source
        .targets
        .get_or_init(|| imports_of(&source.entries, source.module.as_deref()));
    match dotted.rsplit_once('.') {
        None => functions.get(dotted).cloned(),
        Some((prefix, name)) => Some((module_of(prefix, modules), name.to_string())),
    }
}

/// The row types of a module, by name.
pub type Defs = Arc<Dict<Arc<TypeInfo>>>;
/// The module files read for `register`, by path and module name.
type Read = HashMap<(PathBuf, String), Option<Arc<Source>>>;

/// Where `register` reads the modules that are not indexed: the roots to look under and
/// how to read the text of a file, as the engine does no I/O.
#[derive(Clone, Copy)]
pub struct Roots<'a> {
    pub paths: &'a [PathBuf],
    pub read: &'a dyn Fn(&Path) -> Option<String>,
}

/// The sources of the modules of a checked tree and what is read of them, shared by an
/// index and its forks.
pub struct Tree {
    this: Weak<Tree>,
    sources: HashMap<String, Arc<Source>>,
    exports: Mutex<HashMap<String, Arc<Functions>>>,
    row_types: Mutex<HashMap<String, Defs>>,
    types: Mutex<HashMap<(String, String), Option<Ty>>>,
    names_of: Mutex<HashMap<String, Arc<ModuleTypes>>>,
    /// Held by the thread that reads types, as `HelperIndex._lock` is: another thread
    /// never sees a module half read.
    reading: Mutex<()>,
}

thread_local! {
    /// The modules and names this thread is reading: a cycle sees them as unknown.
    static BUSY: RefCell<HashSet<(String, Option<String>)>> = RefCell::new(HashSet::new());
}

thread_local! {
    /// How many times this thread holds `Tree::reading`.
    static HELD: Cell<usize> = const { Cell::new(0) };
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// `Tree::reading`, taken once per thread however deep the reads nest.
struct Reading<'t> {
    _guard: Option<MutexGuard<'t, ()>>,
}

impl Drop for Reading<'_> {
    fn drop(&mut self) {
        HELD.set(HELD.get() - 1);
    }
}

/// A module or name marked as being read until dropped.
struct Busy((String, Option<String>));

impl Busy {
    /// `None` when this thread is already reading it: a cycle.
    fn mark(module: &str, name: Option<&str>) -> Option<Busy> {
        let key = (module.to_string(), name.map(str::to_string));
        BUSY.with_borrow_mut(|all| all.insert(key.clone()))
            .then_some(Busy(key))
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        BUSY.with_borrow_mut(|all| all.remove(&self.0));
    }
}

/// The helpers and sources of the modules of a checked tree, by module name.
pub struct HelperIndex {
    pub modules: HashMap<String, Dict<Helper>>,
    tree: Arc<Tree>,
    version: Version,
    read: Arc<Mutex<Read>>,
    unread: Arc<AtomicBool>,
    names: HashSet<String>,
    pattern: Option<Regex>,
}

/// An index being filled.
pub struct Builder {
    modules: HashMap<String, Dict<Helper>>,
    /// The tree forked from, until a module is added.
    tree: Option<Arc<Tree>>,
    added: HashMap<String, Arc<Source>>,
    version: Version,
    read: Arc<Mutex<Read>>,
    /// Whether `register` met a file Rust does not read.
    unread: Arc<AtomicBool>,
}

impl Builder {
    pub fn new(version: Version) -> Self {
        Builder {
            modules: HashMap::new(),
            tree: None,
            added: HashMap::new(),
            version,
            read: Arc::default(),
            unread: Arc::default(),
        }
    }

    fn sources(&self) -> &HashMap<String, Arc<Source>> {
        self.tree.as_ref().map_or(&self.added, |tree| &tree.sources)
    }

    /// `HelperIndex.add`: index the helpers of `source` as module `module`; whether it has
    pub fn add(&mut self, module: &str, source: Source) -> bool {
        let source = Arc::new(match source.module {
            Some(_) => source,
            None => source.named(Some(module)),
        });
        let text = &*source.text;
        let found = if text.contains("Annotated") && text.contains("sqlty") && text.contains("SQL")
        {
            index_functions(&source)
        } else {
            Dict::new()
        };
        if let Some(tree) = self.tree.take() {
            self.added.clone_from(&tree.sources);
        }
        self.added.insert(module.to_string(), source);
        if found.is_empty() {
            return false;
        }
        let known = self.modules.entry(module.to_string()).or_default();
        for (name, helper) in found.iter() {
            known.insert(name, helper.clone());
        }
        true
    }

    /// `HelperIndex.register`: `import_path` (`pkg.mod.func` or `pkg.mod.Class.method`) as
    /// a helper taking its query as `query` and its row type as `row`, see
    /// `[[tool.sqlty.functions]]`. Its definition is read from an indexed module, or from
    /// the first of `roots` holding it.
    pub fn register(
        &mut self,
        import_path: &str,
        query: &str,
        row: Option<&str>,
        roots: Roots<'_>,
        database: Option<&str>,
    ) {
        let parts: Vec<&str> = import_path.split('.').collect();
        for i in (1..parts.len()).rev() {
            let name = parts[i..].join(".");
            let Some(mut found) = self.definition(&parts[..i], &name, query, row, roots, 0) else {
                continue;
            };
            found.configured = true;
            found.database = database.map(str::to_string);
            self.modules
                .entry(parts[..i].join("."))
                .or_default()
                .insert(&name, found);
            return;
        }
        let keywords: HashSet<String> = [Some(query), row]
            .into_iter()
            .flatten()
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect();
        for back in [1, 2] {
            if parts.len() > back {
                let i = parts.len() - back;
                let name = parts[i..].join(".");
                let found = Helper {
                    name: name.clone(),
                    positional: Vec::new(),
                    keywords: keywords.clone(),
                    query: query.to_string(),
                    row: row.map(str::to_string),
                    kind: Kind::Function,
                    params: None,
                    rest: None,
                    named: None,
                    configured: true,
                    database: database.map(str::to_string),
                };
                self.modules
                    .entry(parts[..i].join("."))
                    .or_default()
                    .insert(&name, found);
            }
        }
    }

    /// `_definition`: the helper `name` of module `parts` defines, following the names it
    /// re-exports.
    fn definition(
        &self,
        parts: &[&str],
        name: &str,
        query: &str,
        row: Option<&str>,
        roots: Roots<'_>,
        depth: usize,
    ) -> Option<Helper> {
        let module = parts.join(".");
        let source = match self.sources().get(&module) {
            Some(source) => source.clone(),
            None => self.module_source(parts, roots)?,
        };
        let wanted: Vec<&str> = [Some(query), row].into_iter().flatten().collect();
        let found = definitions(&source)
            .into_iter()
            .filter(|(known, _, _)| known == name)
            .min_by_key(|(_, function, _)| {
                let names = parameter_names(function);
                rank(function, wanted.iter().all(|name| names.contains(*name)))
            });
        if let Some((_, function, kind)) = found {
            return Some(helper(
                name.to_string(),
                function,
                kind,
                query.to_string(),
                row.map(str::to_string),
            ));
        }
        if depth > 8 {
            return None;
        }
        let (head, rest) = match name.split_once('.') {
            Some((head, rest)) => (head, format!(".{rest}")),
            None => (name, String::new()),
        };
        let (exports, _) = imports_of(&source.entries, source.module.as_deref().or(Some(&module)));
        let (target, original) = exports.get(head)?;
        let parts: Vec<&str> = target.split('.').collect();
        self.definition(
            &parts,
            &format!("{original}{rest}"),
            query,
            row,
            roots,
            depth + 1,
        )
    }

    /// `_module_source`: module `parts` read from the first of `roots` holding it.
    fn module_source(&self, parts: &[&str], roots: Roots<'_>) -> Option<Arc<Source>> {
        let module = parts.join(".");
        let (last, package) = parts.split_last()?;
        for root in roots.paths {
            let mut file: PathBuf = root.clone();
            file.extend(package);
            file.push(format!("{last}.py"));
            let mut init: PathBuf = root.clone();
            init.extend(parts);
            init.push("__init__.py");
            for (path, name) in [(file, module.clone()), (init, format!("{module}.__init__"))] {
                let key = (path, name);
                let known = lock(&self.read).get(&key).cloned();
                let source = match known {
                    Some(source) => source,
                    None => {
                        let source = match (roots.read)(&key.0) {
                            None => None,
                            Some(text) => {
                                let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
                                let Some(source) = Source::new(text, self.version) else {
                                    // The first root holding the file ends the search.
                                    self.unread.store(true, Ordering::Relaxed);
                                    return None;
                                };
                                Some(Arc::new(source.named(Some(&key.1))))
                            }
                        };
                        lock(&self.read).insert(key, source.clone());
                        source
                    }
                };
                if source.is_some() {
                    return source;
                }
            }
        }
        None
    }

    /// The index, to check modules against.
    pub fn build(self) -> Arc<HelperIndex> {
        let names: HashSet<String> = self
            .modules
            .values()
            .flat_map(Dict::iter)
            .map(|(name, _)| name.rsplit('.').next().unwrap_or(name).to_string())
            .collect();
        // A module can re-export a helper under another name, which a call then spells.
        let exports: Vec<Functions> = self
            .sources()
            .iter()
            .map(|(module, source)| {
                let name = source.module.as_deref().unwrap_or(module);
                imports_of(&source.entries, Some(name)).0
            })
            .collect();
        let mut names = names;
        for _ in 0..8 {
            let renamed: Vec<String> = exports
                .iter()
                .flatten()
                .filter(|(local, (_, original))| {
                    names.contains(original) && !names.contains(*local)
                })
                .map(|(local, _)| local.clone())
                .collect();
            if renamed.is_empty() {
                break;
            }
            names.extend(renamed);
        }
        let mut sorted: Vec<&String> = names.iter().collect();
        sorted.sort();
        let pattern = (!sorted.is_empty())
            .then(|| {
                let names: Vec<String> = sorted.iter().map(|name| regex::escape(name)).collect();
                Regex::new(&names.join("|")).ok()
            })
            .flatten();
        let tree = self.tree.unwrap_or_else(|| {
            Arc::new_cyclic(|this| Tree {
                this: this.clone(),
                sources: self.added,
                exports: Mutex::default(),
                row_types: Mutex::default(),
                types: Mutex::default(),
                names_of: Mutex::default(),
                reading: Mutex::default(),
            })
        });
        Arc::new(HelperIndex {
            modules: self.modules,
            tree,
            version: self.version,
            read: self.read,
            unread: self.unread,
            names,
            pattern,
        })
    }
}

impl HelperIndex {
    /// `HelperIndex.fork`: a copy to `register` more helpers in; the sources are shared.
    pub fn fork(&self) -> Builder {
        Builder {
            modules: self.modules.clone(),
            tree: Some(self.tree.clone()),
            added: HashMap::new(),
            version: self.version,
            read: self.read.clone(),
            unread: self.unread.clone(),
        }
    }

    /// The module `module` as it was added.
    pub fn source(&self, module: &str) -> Option<&Arc<Source>> {
        self.tree.sources.get(module)
    }

    /// The Python version the modules are read as.
    pub fn version(&self) -> Version {
        self.version
    }

    /// Whether `register` met a file Rust does not read, which Python may read.
    pub fn unread(&self) -> bool {
        self.unread.load(Ordering::Relaxed)
    }

    /// The unqualified names of the indexed helpers, methods included.
    pub fn names(&self) -> &HashSet<String> {
        &self.names
    }

    /// Whether `text` contains the name of some helper, so it may call one.
    pub fn mentions(&self, text: &str) -> bool {
        // No pattern holds too many names: any text may call one.
        match &self.pattern {
            Some(pattern) => pattern.is_match(text),
            None => !self.names.is_empty(),
        }
    }

    /// The types and row types of the modules.
    pub fn types(&self) -> Arc<dyn Index> {
        self.tree.clone()
    }

    /// `HelperIndex.get`: the helper `name` (maybe `Class.method`) of `module`, following
    /// the names the module re-exports.
    pub fn get(&self, module: &str, name: &str) -> Option<&Helper> {
        self.get_at(module, name, 0)
    }

    /// `HelperIndex.fragment`: the value of `NAME` or `module.NAME` that `source` imports
    /// from a checked module, with the source it is an expression of. Only a module-level
    /// name assigned once counts; `module` names `source` when it has no name of its own.
    pub fn fragment<'a>(
        &'a self,
        source: &Source,
        dotted: &str,
        module: Option<&str>,
    ) -> Option<(&'a Source, &'a Expr)> {
        let name = source.module.as_deref().or(module);
        let (functions, modules) = imports_of(&source.entries, name);
        match dotted.rsplit_once('.') {
            None => {
                let (module, original) = functions.get(dotted)?;
                self.tree.constant(module, original, 0)
            }
            Some((prefix, name)) => self.tree.constant(&module_of(prefix, &modules), name, 0),
        }
    }

    /// The row types of `source`, a module of the index, as `extract_type_definitions`
    /// reads them.
    pub fn definitions(&self, source: &Source) -> Option<Defs> {
        let module = source.module.as_deref()?;
        let known = self.tree.sources.get(module)?;
        std::ptr::eq(&**known, source).then(|| self.tree.definitions(module, known))
    }

    fn get_at(&self, module: &str, name: &str, depth: usize) -> Option<&Helper> {
        if let Some(found) = self
            .modules
            .get(module)
            .and_then(|helpers| helpers.get(name))
        {
            return Some(found);
        }
        if depth > 8 {
            return None;
        }
        let source = self.tree.sources.get(module)?;
        let exports = self.tree.reexports(module, source);
        let (head, rest) = match name.split_once('.') {
            Some((head, rest)) => (head, format!(".{rest}")),
            None => (name, String::new()),
        };
        let (target, original) = exports.get(head)?;
        self.get_at(target, &format!("{original}{rest}"), depth + 1)
    }
}

impl Tree {
    fn reexports(&self, module: &str, source: &Source) -> Arc<Functions> {
        if let Some(found) = lock(&self.exports).get(module) {
            return found.clone();
        }
        let name = source.module.as_deref().unwrap_or(module);
        let exports = Arc::new(imports_of(&source.entries, Some(name)).0);
        lock(&self.exports).insert(module.to_string(), exports.clone());
        exports
    }

    fn type_at(&self, module: &str, name: &str, depth: usize) -> Option<Ty> {
        if depth > 8 {
            return None;
        }
        let source = self.sources.get(module)?;
        let key = (module.to_string(), name.to_string());
        if let Some(found) = lock(&self.types).get(&key) {
            return found.clone();
        }
        let _reading = self.read();
        if let Some(found) = lock(&self.types).get(&key) {
            return found.clone();
        }
        // An import cycle sees the name as unknown while it is resolved.
        let busy = Busy::mark(module, Some(name))?;
        let names = {
            let known = lock(&self.names_of).get(module).cloned();
            known.unwrap_or_else(|| {
                let index: Option<Arc<dyn Index>> = self.this.upgrade().map(|this| this as _);
                let names = Arc::new(ModuleTypes::new(
                    source.clone(),
                    index.as_ref(),
                    Some(module),
                ));
                lock(&self.names_of).insert(module.to_string(), names.clone());
                names
            })
        };
        let found = if names.is_local(name) {
            crate::parse::expression(name).and_then(|parsed| {
                source
                    .resolver(Known::Imported)
                    .resolve(&parsed.syntax().body, name, &*names)
            })
        } else {
            self.reexports(module, source)
                .get(name)
                .and_then(|(target, original)| self.type_at(target, original, depth + 1))
        };
        drop(busy);
        lock(&self.types).insert(key, found.clone());
        found
    }

    fn constant(&self, module: &str, name: &str, depth: usize) -> Option<(&Source, &Expr)> {
        if depth > 8 {
            return None;
        }
        let source = self.sources.get(module)?;
        if let Some(Some(value)) = crate::compose::constants(source).get(name) {
            return Some((source, value));
        }
        let exports = self.reexports(module, source);
        let (target, original) = exports.get(name)?;
        self.constant(target, original, depth + 1)
    }

    fn definitions(&self, module: &str, source: &Arc<Source>) -> Defs {
        let known = || lock(&self.row_types).get(module).cloned();
        if let Some(defs) = known() {
            return defs;
        }
        let _reading = self.read();
        if let Some(defs) = known() {
            return defs;
        }
        // Modules that import each other's row types see none while either is read.
        let Some(busy) = Busy::mark(module, None) else {
            return Arc::default();
        };
        let index: Option<Arc<dyn Index>> = self.this.upgrade().map(|this| this as _);
        let defs = Arc::new(extract_type_definitions(
            source,
            index.as_ref(),
            Some(module),
        ));
        drop(busy);
        lock(&self.row_types).insert(module.to_string(), defs.clone());
        defs
    }

    fn read(&self) -> Reading<'_> {
        let guard = (HELD.get() == 0).then(|| lock(&self.reading));
        HELD.set(HELD.get() + 1);
        Reading { _guard: guard }
    }

    fn row_type_at(&self, module: &str, name: &str, depth: usize) -> Option<RowType> {
        if depth > 8 {
            return None;
        }
        let source = self.sources.get(module)?;
        let defs = self.definitions(module, source);
        if let Some(info) = defs.get(name) {
            return Some(RowType {
                info: info.clone(),
                defs,
            });
        }
        let exports = self.reexports(module, source);
        let (target, original) = exports.get(name)?;
        self.row_type_at(target, original, depth + 1)
    }
}

impl Index for Tree {
    fn type_of(&self, source: &Source, dotted: &str) -> Option<Ty> {
        let (module, name) = target(source, dotted)?;
        self.type_at(&module, &name, 0)
    }

    fn row_type(&self, source: &Source, dotted: &str) -> Option<RowType> {
        let (module, name) = target(source, dotted)?;
        self.row_type_at(&module, &name, 0)
    }
}
