//! The modules of the checked paths, as `sqlty._helpers.HelperIndex` holds them: the
//! helpers each defines, and the types and row types other modules import from them. A
//! module is read from its source, never imported.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, Weak};

use regex::Regex;
use ruff_python_ast::{Expr, StmtFunctionDef};

use crate::calls::{definitions, helper, index_functions, parameter_names, rank, Helper, Kind};
use crate::frozen::{Frozen, Table};
use crate::module::Import;
use crate::modules::Modules;
use crate::receivers::{Facts, Val};
use crate::types::extract::extract_type_definitions;
use crate::types::info::{Dict, TypeInfo};
use crate::types::pydantic::Known;
use crate::types::stdlib::Version;
use crate::types::symbols::{Index, ModuleTypes, RowType, Source};
use crate::types::ty::Ty;

/// Local names bound to imported names `(module, name)`.
pub type Functions = Frozen<(String, String)>;
/// What `imports_of` gives: the functions, and the modules by local name.
pub type Targets = (Functions, Table);

/// `_imports_of`: the local names `entries` bind to imported functions `(module, name)`
/// and to modules, in the module `module`.
pub fn imports_of(entries: &[Import], module: Option<&str>) -> Targets {
    let mut functions = Vec::new();
    let mut modules = Vec::new();
    let package = module.map_or("", |module| {
        module.rsplit_once('.').map_or("", |(package, _)| package)
    });
    for import in entries {
        let Some(base) = &import.module else {
            modules.push((import.local.clone(), import.original.clone()));
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
        modules.push((
            import.local.clone(),
            if base.is_empty() {
                import.original.clone()
            } else {
                format!("{base}.{}", import.original)
            },
        ));
        functions.push((import.local.clone(), (base, import.original.clone())));
    }
    (
        functions.into_iter().collect(),
        modules.into_iter().collect(),
    )
}

/// The module a dotted prefix names, through the modules `modules` binds.
pub fn module_of(prefix: &str, modules: &Table) -> String {
    let (head, rest) = prefix.split_once('.').unwrap_or((prefix, ""));
    match modules.get(head) {
        Some(module) if rest.is_empty() => module.clone(),
        Some(module) => format!("{module}.{rest}"),
        None => prefix.to_string(),
    }
}

/// `_target`: the module and name that `NAME` or `module.NAME` of `source` refers to.
fn target(source: &Source, dotted: &str) -> Option<(String, String)> {
    let (functions, modules) = source.targets();
    match dotted.rsplit_once('.') {
        None => functions.get(dotted).cloned(),
        Some((prefix, name)) => Some((module_of(prefix, modules), name.to_string())),
    }
}

/// A type, or `None` when unknown, and the re-exports to its definition.
type Typed = (Option<Ty>, usize);

/// The row types of a module, by name.
pub type Defs = Arc<Dict<Arc<TypeInfo>>>;
/// A module of the tree: its text, parsed when another module's lookup first reaches it.
pub struct Module {
    pub text: Arc<str>,
    name: String,
    version: Version,
    parsed: OnceLock<Option<Arc<Source>>>,
    facts: OnceLock<Option<Arc<Facts>>>,
    defined: OnceLock<Defined>,
}

/// The functions and methods of a module by name (`Class.method`), in the order
/// [`definitions`] gives them, each without its body: what a registered helper is read
/// from, kept so that each registration does not read the module again.
type Defined = HashMap<String, Vec<(StmtFunctionDef, Kind)>>;

impl Module {
    /// `text` as the module `name`, not parsed yet.
    pub fn new(text: Arc<str>, name: &str, version: Version) -> Self {
        Module {
            text,
            name: name.to_string(),
            version,
            parsed: OnceLock::new(),
            facts: OnceLock::new(),
            defined: OnceLock::new(),
        }
    }

    /// The module `name` whose file is not read: nothing is known of it.
    pub fn unread(name: &str, version: Version) -> Self {
        Module {
            text: Arc::from(""),
            name: name.to_string(),
            version,
            parsed: OnceLock::from(None),
            facts: OnceLock::from(None),
            defined: OnceLock::new(),
        }
    }

    /// The module of `source`, already parsed.
    pub fn parsed(source: Arc<Source>) -> Self {
        Module {
            text: Arc::clone(&source.text),
            name: source.module.clone().unwrap_or_default(),
            version: source.version,
            parsed: OnceLock::from(Some(source)),
            facts: OnceLock::new(),
            defined: OnceLock::new(),
        }
    }

    /// The module parsed and kept, `None` where Rust does not read it.
    pub fn source(&self) -> Option<&Arc<Source>> {
        self.parsed
            .get_or_init(|| self.parse().map(Arc::new))
            .as_ref()
    }

    /// What the module declares, for [`crate::receivers`]: read once and kept with the
    /// module, for every index that holds it.
    pub fn facts(&self) -> Option<&Arc<Facts>> {
        self.facts_or(|| {
            let source = self.source()?;
            Some(Facts::of(source, Some(&self.name)))
        })
    }

    /// [`Module::facts`], read by `read` when they are not kept yet: a caller that has
    /// walked the module gives what its walk found.
    pub fn facts_or(&self, read: impl FnOnce() -> Option<Facts>) -> Option<&Arc<Facts>> {
        self.facts.get_or_init(|| read().map(Arc::new)).as_ref()
    }

    /// The definitions of the module by name, read once.
    fn defined(&self) -> &Defined {
        self.defined.get_or_init(|| {
            let mut defined = Defined::new();
            for (name, function, kind) in self
                .source()
                .into_iter()
                .flat_map(|source| definitions(source))
            {
                let signature = StmtFunctionDef {
                    body: Vec::new().into(),
                    node_index: function.node_index.clone(),
                    range: function.range,
                    is_async: function.is_async,
                    decorator_list: function.decorator_list.clone(),
                    name: function.name.clone(),
                    type_params: function.type_params.clone(),
                    parameters: function.parameters.clone(),
                    returns: function.returns.clone(),
                };
                defined.entry(name).or_default().push((signature, kind));
            }
            defined
        })
    }

    /// The name the module is read as: `pkg.__init__` for a package.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The module as parsed so far, else parsed for the caller alone.
    pub fn load(&self) -> Option<Arc<Source>> {
        match self.parsed.get() {
            Some(parsed) => parsed.clone(),
            None => self.parse().map(Arc::new),
        }
    }

    fn parse(&self) -> Option<Source> {
        #[cfg(test)]
        PARSED.with(|count| count.set(count.get() + 1));
        stacker::maybe_grow(STACK_RED_ZONE, STACK_SEGMENT, || {
            Some(Source::new(Arc::clone(&self.text), self.version)?.named(Some(&self.name)))
        })
    }
}

#[cfg(test)]
thread_local! {
    /// How many modules this thread parsed, to check that a build does not parse them all.
    static PARSED: Cell<usize> = const { Cell::new(0) };
}

/// The stack a parse needs left, and the stack it grows by when less is left.
const STACK_RED_ZONE: usize = 2 << 20;
const STACK_SEGMENT: usize = 16 << 20;

/// `as`, which any import binding a name to another has.
static AS: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| sqlty_sql::regex(r"\bas\b"));

/// The sources of the modules of a checked tree and what is read of them, shared by an
/// index and its forks.
pub struct Tree {
    this: Weak<Tree>,
    sources: HashMap<String, Arc<Module>>,
    exports: Mutex<HashMap<String, Arc<Functions>>>,
    row_types: Mutex<HashMap<String, Defs>>,
    types: Mutex<HashMap<(String, String), Typed>>,
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
    /// The stubs the host gave the text of, read before any other module of their name.
    stubs: Arc<HashMap<String, Arc<Module>>>,
    /// The stubs of `stub-paths` and the libraries under the roots.
    outside: Option<Arc<Modules>>,
    names: HashSet<String>,
    pattern: Option<Regex>,
    /// The classes `[[tool.sqlty.functions]]` registers a method of, by path, and what
    /// each path starts with (`lib`, `lib.Outer`, `lib.Outer.Inner`).
    registered: HashSet<String>,
}

/// Whether a module of `text` may define helpers; no other is read for them.
pub fn defines_helpers(text: &str) -> bool {
    text.contains("Annotated") && text.contains("sqlty") && text.contains("SQL")
}

/// The helpers `source` defines, as `Builder::add` indexes them; read only where
/// [`defines_helpers`].
pub fn helpers_of(source: &Source) -> Dict<Helper> {
    index_functions(source)
}

/// A pattern matching any of `names`, `None` for none.
fn alternation(names: &HashSet<String>) -> Option<Regex> {
    let mut sorted: Vec<&String> = names.iter().collect();
    sorted.sort();
    let escaped: Vec<String> = sorted.iter().map(|name| regex::escape(name)).collect();
    (!escaped.is_empty())
        .then(|| Regex::new(&escaped.join("|")).ok())
        .flatten()
}

/// A pattern matching the text that renames any of `names` in an import, `name as`;
/// `None` for no name. Between the two, an import in parentheses may hold line ends
/// and comments, and any import a backslash before a line end.
fn renaming(names: &HashSet<String>) -> Option<Regex> {
    let mut sorted: Vec<&String> = names.iter().collect();
    sorted.sort();
    let escaped: Vec<String> = sorted.iter().map(|name| regex::escape(name)).collect();
    let pattern = format!(r"\b(?:{})(?:\s|\\|#[^\n]*)+as\b", escaped.join("|"));
    (!escaped.is_empty())
        .then(|| Regex::new(&pattern).ok())
        .flatten()
}

/// An index being filled.
pub struct Builder {
    modules: HashMap<String, Dict<Helper>>,
    /// The tree forked from, until a module is added.
    tree: Option<Arc<Tree>>,
    added: HashMap<String, Arc<Module>>,
    version: Version,
    stubs: Arc<HashMap<String, Arc<Module>>>,
    outside: Option<Arc<Modules>>,
}

/// The module `module`: its stub, else the checked module `sources` holds, else the
/// library under a root. Every lookup of a module by name goes through here, so a helper
/// that is registered, the class of a receiver and a helper a stub marks are read from
/// the same file.
fn resolve(
    stubs: &HashMap<String, Arc<Module>>,
    outside: Option<&Modules>,
    sources: &HashMap<String, Arc<Module>>,
    module: &str,
) -> Option<Arc<Module>> {
    stubs
        .get(module)
        .cloned()
        .or_else(|| outside?.stub(module))
        .or_else(|| sources.get(module).cloned())
        .or_else(|| outside?.library(module))
}

impl Builder {
    pub fn new(version: Version) -> Self {
        Builder {
            modules: HashMap::new(),
            tree: None,
            added: HashMap::new(),
            version,
            stubs: Arc::default(),
            outside: None,
        }
    }

    /// Index the stub `source` (a `.pyi`) as module `module`: its classes are read before
    /// those of any other module of that name, and the methods and functions it marks
    /// `Annotated[LiteralString, SQL]` are helpers, as if `[[tool.sqlty.functions]]`
    /// named them.
    pub fn add_stub(&mut self, module: &str, source: Arc<Module>) {
        if defines_helpers(&source.text) {
            if let Some(parsed) = source.source() {
                let known = self.modules.entry(module.to_string()).or_default();
                for (name, helper) in helpers_of(parsed).iter() {
                    let mut helper = helper.clone();
                    helper.configured = true;
                    known.insert(name, helper);
                }
            }
        }
        Arc::make_mut(&mut self.stubs).insert(module.to_string(), source);
    }

    /// Read the modules that are not indexed from `outside`: a stub of its `stub-paths`
    /// before the indexed module of that name, a library under its roots after. The
    /// helpers its stubs mark are indexed, for the packages some indexed module names.
    pub fn outside(&mut self, outside: Arc<Modules>) {
        let marked = outside.marked(self.sources().values().map(|module| &*module.text));
        for (module, helpers) in marked {
            let known = self.modules.entry(module).or_default();
            for (name, helper) in helpers.iter() {
                known.insert(name, helper.clone());
            }
        }
        self.outside = Some(outside);
    }

    /// The roots the modules that are not indexed are read from.
    pub fn roots(&self) -> Option<&[std::path::PathBuf]> {
        self.outside.as_deref().map(Modules::roots)
    }

    fn sources(&self) -> &HashMap<String, Arc<Module>> {
        self.tree.as_ref().map_or(&self.added, |tree| &tree.sources)
    }

    /// Index `source` as module `module` with its helpers `found` by `helpers_of`, so that
    /// sources can be read in parallel and one source shared by the names of its module.
    pub fn add_found(&mut self, module: &str, source: Arc<Module>, found: &Dict<Helper>) -> bool {
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
    /// `[[tool.sqlty.functions]]`. Its definition is read from its stub, an indexed module
    /// or a library ([`Builder::outside`]).
    pub fn register(
        &mut self,
        import_path: &str,
        query: &str,
        row: Option<&str>,
        database: Option<&str>,
    ) {
        let parts: Vec<&str> = import_path.split('.').collect();
        for i in (1..parts.len()).rev() {
            let name = parts[i..].join(".");
            let Some((mut found, defined_in)) = self.definition(&parts[..i], &name, query, row, 0)
            else {
                continue;
            };
            found.configured = true;
            found.database = database.map(str::to_string);
            // A receiver's class is known by the module that defines it.
            if let Some((module, defined)) = defined_in.filter(|_| name.contains('.')) {
                let mut found = found.clone();
                found.name.clone_from(&defined);
                self.modules
                    .entry(module)
                    .or_default()
                    .insert(&defined, found);
            }
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

    /// The helper `name` of module `parts` defines, following the names it re-exports.
    /// A place that holds the module without binding the name at all (a stub that
    /// describes part of a library, one that does not parse) leaves it to the next: the
    /// stub, the indexed module, then the files of the library.
    fn definition(
        &self,
        parts: &[&str],
        name: &str,
        query: &str,
        row: Option<&str>,
        depth: usize,
    ) -> Option<(Helper, Option<(String, String)>)> {
        let module = parts.join(".");
        let outside = self.outside.as_deref();
        let first = [
            self.stubs.get(&module).cloned(),
            outside.and_then(|outside| outside.stub(&module)),
            self.sources().get(&module).cloned(),
        ];
        // A place that binds the head of the name without the helper (a class whose
        // method is inherited, say) ends the search: another file's parameters could
        // contradict the class the receivers are typed by.
        let head = name.split('.').next().unwrap_or(name);
        let ended = Cell::new(false);
        let defined = |known: &Arc<Module>| {
            if ended.get() {
                return None;
            }
            let found = self.defined_in(known, &module, name, query, row, depth);
            let bound = known.facts().is_some_and(|facts| facts.binds(head));
            ended.set(found.is_none() && bound);
            found
        };
        if let Some(found) = first.iter().flatten().find_map(defined) {
            return Some(found);
        }
        // The library file every lookup reads is kept; the others are read for this.
        let outside = outside?;
        if let Some(found) = outside.library(&module).as_ref().and_then(defined) {
            return Some(found);
        }
        if ended.get() {
            return None;
        }
        let others = outside.libraries(&module);
        others.iter().skip(1).find_map(defined)
    }

    /// The helper `name` as `source`, the module `module`, defines or re-exports it.
    fn defined_in(
        &self,
        known: &Module,
        module: &str,
        name: &str,
        query: &str,
        row: Option<&str>,
        depth: usize,
    ) -> Option<(Helper, Option<(String, String)>)> {
        let source = known.source()?;
        let wanted: Vec<&str> = [Some(query), row].into_iter().flatten().collect();
        let found = known
            .defined()
            .get(name)
            .into_iter()
            .flatten()
            .min_by_key(|(function, _)| {
                let names = parameter_names(function);
                rank(function, wanted.iter().all(|name| names.contains(*name)))
            });
        if let Some((function, kind)) = found {
            let found = helper(
                name.to_string(),
                function,
                *kind,
                query.to_string(),
                row.map(str::to_string),
            );
            let defined = (depth > 0).then(|| (module.to_string(), name.to_string()));
            return Some((found, defined));
        }
        if depth > 8 {
            return None;
        }
        let (head, rest) = match name.split_once('.') {
            Some((head, rest)) => (head, format!(".{rest}")),
            None => (name, String::new()),
        };
        let targets = source.targets_in(source.module.as_deref().or(Some(module)));
        let (exports, _) = &*targets;
        let Some((target, original)) = exports.get(head) else {
            // `Db = Other`: the class is read where it is defined, and the helper is
            // attached to it, so that its parameters are known.
            let facts = known.facts()?;
            let bound = facts.bound(head)?;
            let [Val::Name(alias)] = &*bound else {
                return None;
            };
            // A name bound more than once may not be the class the assignment read.
            let once = facts.bound(alias).is_some_and(|bound| bound.len() == 1);
            let aliased = format!("{alias}{rest}");
            let (found, defined) = (once && alias != head)
                .then(|| self.defined_in(known, module, &aliased, query, row, depth + 1))
                .flatten()?;
            return Some((found, defined.or(Some((module.to_string(), aliased)))));
        };
        let parts: Vec<&str> = target.split('.').collect();
        self.definition(&parts, &format!("{original}{rest}"), query, row, depth + 1)
    }

    /// The index, to check modules against.
    pub fn build(self) -> Arc<HelperIndex> {
        let names: HashSet<String> = self
            .modules
            .values()
            .flat_map(Dict::iter)
            .map(|(name, _)| name.rsplit('.').next().unwrap_or(name).to_string())
            .collect();
        // A module can re-export a helper under another name, which a call then spells; its
        // text then holds the name, so only such modules have their imports read.
        let mut names = names;
        for _ in 0..8 {
            if names.is_empty() {
                break;
            }
            // Only `import ... name as ...` binds a helper to another name: a module
            // whose text does not hold that is not parsed for its imports.
            let named = renaming(&names);
            let mut renamed = Vec::new();
            let aliasing = self.sources().values().filter(|known| match &named {
                Some(named) => named.is_match(&known.text),
                None => AS.is_match(&known.text),
            });
            for source in aliasing.filter_map(|known| known.load()) {
                renamed.extend(
                    source
                        .targets()
                        .0
                        .iter()
                        .filter(|(local, (_, original))| {
                            names.contains(original) && !names.contains(*local)
                        })
                        .map(|(local, _)| local.clone()),
                );
            }
            if renamed.is_empty() {
                break;
            }
            names.extend(renamed);
        }
        let pattern = alternation(&names);
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
        let mut registered = HashSet::new();
        for (module, helpers) in &self.modules {
            for (name, _) in helpers.iter().filter(|(_, helper)| helper.configured) {
                let Some((class, _)) = name.rsplit_once('.') else {
                    continue;
                };
                let path = format!("{module}.{class}");
                let ends = path.match_indices('.').map(|(at, _)| at);
                registered.extend(ends.map(|at| path[..at].to_string()));
                registered.insert(path);
            }
        }
        Arc::new(HelperIndex {
            registered,
            modules: self.modules,
            tree,
            version: self.version,
            stubs: self.stubs,
            outside: self.outside,
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
            stubs: self.stubs.clone(),
            outside: self.outside.clone(),
        }
    }

    /// The module `module`, wherever it is read from: its stub, the checked tree, a
    /// library. `None` for a module that is not found.
    pub fn resolve(&self, module: &str) -> Option<Arc<Module>> {
        resolve(
            &self.stubs,
            self.outside.as_deref(),
            &self.tree.sources,
            module,
        )
    }

    /// Whether the index reads modules outside the checked tree.
    pub fn reads_outside(&self) -> bool {
        self.outside.is_some()
    }

    /// The stubs the index reads helpers from, each as the module it describes, the name
    /// it is read as and its text: what [`Builder::add_stub`] takes to index them again,
    /// as a snapshot of the checked files does.
    pub fn stubs(&self) -> Vec<(String, String, Arc<str>)> {
        let entry = |module: &str, stub: &Module| {
            (
                module.to_string(),
                stub.name().to_string(),
                Arc::clone(&stub.text),
            )
        };
        let mut found: Vec<_> = self
            .stubs
            .iter()
            .map(|(module, stub)| entry(module, stub))
            .collect();
        if let Some(outside) = &self.outside {
            let texts = self.tree.sources.values().map(|module| &*module.text);
            for module in outside.named(texts) {
                if let Some(stub) = outside
                    .stub(module)
                    .filter(|_| !self.stubs.contains_key(module))
                {
                    found.push(entry(module, &stub));
                }
            }
        }
        found.sort();
        found
    }

    /// What the module `module` declares, for [`crate::receivers`]. `None` for a module
    /// that is not found or not read.
    pub fn facts(&self, module: &str) -> Option<Arc<Facts>> {
        self.resolve(module)?.facts().cloned()
    }

    /// The module `module` as it was added.
    pub fn module(&self, module: &str) -> Option<&Arc<Module>> {
        self.tree.sources.get(module)
    }

    /// The module `module` parsed and kept.
    pub fn source(&self, module: &str) -> Option<&Arc<Source>> {
        self.module(module)?.source()
    }

    /// The Python version the modules are read as.
    pub fn version(&self) -> Version {
        self.version
    }

    /// The unqualified names of the indexed helpers, methods included.
    pub fn names(&self) -> &HashSet<String> {
        &self.names
    }

    /// Whether `text` contains the name of some helper, so it may call one.
    pub fn mentions(&self, text: &str) -> bool {
        // No pattern holds too many names: any text may call one.
        self.pattern
            .as_ref()
            .map_or(!self.names.is_empty(), |pattern| pattern.is_match(text))
    }

    /// The types and row types of the modules.
    pub fn types(&self) -> Arc<dyn Index> {
        self.tree.clone()
    }

    /// Whether `[[tool.sqlty.functions]]` registers a method of `module.class`, or of a
    /// class in it (`module.class.Inner`).
    pub fn registers(&self, module: &str, class: &str) -> bool {
        !self.registered.is_empty() && self.registered.contains(&format!("{module}.{class}"))
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
        let (functions, modules) = &*source.targets_in(source.module.as_deref().or(module));
        self.constant(functions, modules, dotted)
    }

    fn constant(
        &self,
        functions: &Functions,
        modules: &Table,
        dotted: &str,
    ) -> Option<(&Source, &Expr)> {
        match dotted.rsplit_once('.') {
            None => {
                let (module, original) = functions.get(dotted)?;
                self.tree.constant(module, original, 0)
            }
            Some((prefix, name)) => self.tree.constant(&module_of(prefix, modules), name, 0),
        }
    }

    /// The row types of `source`, a module of the index, as `extract_type_definitions`
    /// reads them.
    pub fn definitions(&self, source: &Source) -> Option<Defs> {
        let module = source.module.as_deref()?;
        let known = self.tree.sources.get(module)?.source()?;
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
        let source = self.tree.sources.get(module)?.source()?;
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

    /// The type `name` of `module` and the re-exports to it, or `None` past `hops`. A result
    /// past `hops` is not cached, and a cached one is `None` past them, so it does not depend
    /// on which module asked first.
    fn typed(&self, module: &str, name: &str, hops: usize) -> Option<Typed> {
        let Some(source) = self.sources.get(module).and_then(|known| known.source()) else {
            return Some((None, 0));
        };
        let key = (module.to_string(), name.to_string());
        let known = || lock(&self.types).get(&key).cloned();
        let mut reading = None;
        if let Some(found) = known().or_else(|| {
            reading = Some(self.read());
            known()
        }) {
            return (found.1 <= hops).then_some(found);
        }
        // An import cycle sees the name as unknown while it is resolved.
        let Some(busy) = Busy::mark(module, Some(name)) else {
            return Some((None, 0));
        };
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
            let ty = crate::parse::expression(name).and_then(|parsed| {
                source
                    .resolver(Known::Imported)
                    .resolve(&parsed.syntax().body, name, &*names)
            });
            (ty, 0)
        } else {
            match self.reexports(module, source).get(name) {
                Some((target, original)) => {
                    let (ty, behind) = self.typed(target, original, hops.checked_sub(1)?)?;
                    (ty, behind + 1)
                }
                None => (None, 0),
            }
        };
        drop(busy);
        lock(&self.types).insert(key, found.clone());
        Some(found)
    }

    fn constant(&self, module: &str, name: &str, depth: usize) -> Option<(&Source, &Expr)> {
        if depth > 8 {
            return None;
        }
        let source = self.sources.get(module)?.source()?;
        if let Some(value) = crate::compose::constant(source, name) {
            return Some((source, value));
        }
        let exports = self.reexports(module, source);
        let (target, original) = exports.get(name)?;
        self.constant(target, original, depth + 1)
    }

    fn definitions(&self, module: &str, source: &Arc<Source>) -> Defs {
        let known = || lock(&self.row_types).get(module).cloned();
        let mut reading = None;
        if let Some(defs) = known().or_else(|| {
            reading = Some(self.read());
            known()
        }) {
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
        let source = self.sources.get(module)?.source()?;
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
        self.typed(&module, &name, 8)?.0
    }

    fn row_type(&self, source: &Source, dotted: &str) -> Option<RowType> {
        let (module, name) = target(source, dotted)?;
        self.row_type_at(&module, &name, 0)
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn registering_reads_the_definitions_of_a_module_once() {
        // Twice the registered methods of twice the classes: twice the definitions read.
        let read = |count: usize| {
            let text: String = (0..count)
                .map(|at| format!("class C{at}:\n    def get(self, sql): ...\n"))
                .collect();
            let module = Arc::new(Module::new(Arc::from(text), "lib", (3, 14)));
            let mut builder = Builder::new((3, 14));
            builder.add_found("lib", module, &Dict::new());
            crate::calls::READ.with(|read| read.set(0));
            for at in 0..count {
                builder.register(&format!("lib.C{at}.get"), "sql", None, None);
            }
            let index = builder.build();
            let found = index
                .get("lib", &format!("C{}.get", count - 1))
                .expect("read");
            assert_eq!(found.positional, ["sql"]);
            assert!(index.registers("lib", "C0") && !index.registers("lib", "C"));
            crate::calls::READ.with(Cell::get)
        };
        let (small, large) = (read(100), read(200));
        assert!(small <= 2 * 100, "{small}");
        assert!(large <= 2 * small, "{small} then {large}");
    }

    #[test]
    fn only_a_module_that_renames_a_helper_is_parsed_for_its_imports() {
        // `as` is everywhere (`with ... as`, `except ... as`, `import numpy as np`):
        // a build that parsed every such module parsed each file of a project again.
        let texts = [
            ("lib", "def get(sql): ...\n"),
            ("a", "import os as o\nwith open(o.devnull) as f: pass\n"),
            ("b", "from lib import get as fetch\n"),
            (
                "c",
                "from lib import (\n    get  # the helper\n    as read,\n)\n",
            ),
            ("d", "from lib import get \\\n    as load\n"),
            ("e", "from b import fetch as again\n"),
            ("f", "get_as = 1\nfrom lib import other as get\n"),
        ];
        let mut builder = Builder::new((3, 14));
        for (name, text) in texts {
            let module = Arc::new(Module::new(Arc::from(text), name, (3, 14)));
            builder.add_found(name, module, &Dict::new());
        }
        builder.register("lib.get", "sql", None, None);
        PARSED.with(|count| count.set(0));
        let index = builder.build();
        // Three rounds: `b`, `c` and `d` rename `get`, then `e` renames `fetch`, then
        // nothing new. `a` and `f` are never parsed, nor `lib` again.
        assert_eq!(PARSED.with(Cell::get), 3 + 4 + 4);
        for name in ["fetch", "read", "load", "again"] {
            assert!(index.mentions(&format!("{name}('SELECT 1')")), "{name}");
        }
        assert!(!index.mentions("o.devnull") && !index.mentions("other()"));
    }

    #[test]
    fn a_module_without_a_name_has_no_relative_imports() {
        let source = Source::new(
            "from . import a\nfrom .b import c\nfrom d import e\n",
            (3, 14),
        )
        .unwrap();
        let (functions, modules) = imports_of(&source.entries, None);
        assert_eq!(functions.keys().collect::<Vec<_>>(), ["e"]);
        assert_eq!(modules.keys().collect::<Vec<_>>(), ["e"]);
    }

    #[test]
    fn a_source_imports_relative_to_the_module_it_is_read_as() {
        let source = Source::new("from .b import c\n", (3, 14))
            .unwrap()
            .named(Some("app.a"));
        let own = source.targets_in(None);
        let other = source.targets_in(Some("lib.a"));
        assert_eq!(
            own.0.get("c").map(|(module, _)| module.as_str()),
            Some("app.b")
        );
        assert_eq!(
            other.0.get("c").map(|(module, _)| module.as_str()),
            Some("lib.b")
        );
    }

    #[test]
    fn a_top_level_module_imports_relative_to_the_root() {
        let source = Source::new(
            "from . import a\nfrom .. import b\nfrom .c import d\n",
            (3, 14),
        )
        .unwrap();
        let (_, modules) = imports_of(&source.entries, Some("app"));
        let modules: Vec<_> = modules.iter().collect();
        assert_eq!(
            modules,
            [(&"a".into(), &"a".into()), (&"d".into(), &"c.d".into())]
        );
    }

    #[test]
    fn a_module_keeps_its_tree_once_a_lookup_parses_it() {
        let module = Module::new(Arc::from("x = 1\n"), "app.a", (3, 14));
        let loaded = module.load().unwrap();
        assert_eq!(loaded.module.as_deref(), Some("app.a"));
        let kept = Arc::clone(module.source().unwrap());
        assert!(!Arc::ptr_eq(&loaded, &kept));
        assert!(Arc::ptr_eq(&module.load().unwrap(), &kept));
    }

    #[test]
    fn a_fork_takes_the_sources_it_was_built_with() {
        let a = Arc::new(Module::new(Arc::from("x = 1\n"), "a", (3, 14)));
        let mut builder = Builder::new((3, 14));
        builder.add_found("a", a, &Dict::new());
        let mut fork = builder.build().fork();
        let b = Arc::new(Module::parsed(Arc::new(
            Source::new("y = 1\n", (3, 14)).unwrap().named(Some("b")),
        )));
        assert!(!fork.add_found("b", b, &Dict::new()));
        let index = fork.build();
        assert!(index.source("a").is_some() && index.source("b").is_some());
    }

    const MARKED: &str = "from sqlty import SQL\nfrom typing import Annotated, LiteralString\ndef run(sql: Annotated[LiteralString, SQL]): ...\n";

    fn module(name: &str, text: &str) -> Arc<Module> {
        Arc::new(Module::new(Arc::from(text), name, (3, 14)))
    }

    #[test]
    fn a_stub_that_does_not_parse_marks_no_helper() {
        let broken = format!("{MARKED}def\n");
        let mut builder = Builder::new((3, 14));
        builder.add_stub("lib", module("lib", MARKED));
        builder.add_stub("broken", module("broken", &broken));
        let index = builder.build();
        assert!(index.get("lib", "run").is_some());
        assert!(index.get("broken", "run").is_none());
        // Both are still the modules they describe.
        let stubs: Vec<String> = index.stubs().into_iter().map(|stub| stub.0).collect();
        assert_eq!(stubs, ["broken", "lib"]);
    }

    #[test]
    fn a_stub_given_hides_the_one_read_on_demand() {
        use crate::modules::{File, ReadFile, Stub};
        use std::path::{Path, PathBuf};

        let stub = |name: &str| Stub {
            module: name.to_string(),
            read_as: name.to_string(),
            path: PathBuf::from(format!("/s/{name}.pyi")),
        };
        // `gone` was listed and then removed.
        let reader: ReadFile = Arc::new(|path: &Path| match path.ends_with("gone.pyi") {
            true => File::Missing,
            false => File::Text(MARKED.to_string()),
        });
        let stubs = vec![stub("lib"), stub("gone"), stub("other")];
        let outside = Arc::new(Modules::new((3, 14), stubs, Vec::new(), reader));
        let mut builder = Builder::new((3, 14));
        builder.add_found(
            "app",
            module("app", "import lib, gone, other\n"),
            &Dict::new(),
        );
        builder.add_stub("lib", module("lib", "class Given: ...\n"));
        builder.outside(outside);
        let index = builder.build();
        let stubs: Vec<(String, String)> = index
            .stubs()
            .into_iter()
            .map(|(module, _, text)| (module, text.to_string()))
            .collect();
        let expected = [("lib", "class Given: ...\n"), ("other", MARKED)]
            .map(|(module, text)| (module.to_string(), text.to_string()));
        assert_eq!(stubs, expected);
    }

    #[test]
    fn every_module_with_an_import_as_is_parsed_when_no_pattern_holds_the_helpers() {
        // The names of the helpers are more than a pattern compiles.
        let long = "f".repeat(4000);
        let names: Vec<String> = (0..400).map(|at| format!("{long}{at:03}")).collect();
        let text: String = names
            .iter()
            .map(|name| format!("def {name}(sql): ...\n"))
            .collect();
        let mut builder = Builder::new((3, 14));
        builder.add_found("lib", module("lib", &text), &Dict::new());
        for (name, text) in [
            ("a", "import os as o\n"),
            ("b", "from lib import other as fetch\n"),
            ("c", "from lib import other\n"),
        ] {
            builder.add_found(name, module(name, text), &Dict::new());
        }
        for name in &names {
            builder.register(&format!("lib.{name}"), "sql", None, None);
        }
        PARSED.with(|count| count.set(0));
        let index = builder.build();
        assert_eq!(PARSED.with(Cell::get), 2, "`a` and `b`, not `c`");
        assert!(index.mentions("anything()"), "any text may call a helper");
    }
}
