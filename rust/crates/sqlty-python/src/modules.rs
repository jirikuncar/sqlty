//! The modules outside the checked tree: the stubs of `stub-paths` and the libraries under
//! the roots, found by name and read when a lookup first reaches them.
//!
//! One [`Modules`] answers every such lookup: a helper `[[tool.sqlty.functions]]` names,
//! the class of a receiver, the helpers a stub marks. The engine does no I/O: the host
//! lists the stubs and gives the function that reads a file. Each file is read once,
//! whatever the number of threads and indexes that ask, and what is read of it (its tree,
//! its declarations) is kept with it. A host keeps a `Modules` for as long as the files
//! it read are unchanged.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use regex::Regex;

use crate::calls::Helper;
use crate::index::{defines_helpers, helpers_of, Module};
use crate::types::info::Dict;
use crate::types::stdlib::Version;

/// What a host finds at a path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum File {
    /// No file.
    Missing,
    /// A file that is not read: not text, or larger than a module is parsed
    /// ([`crate::parse::MAX_LEN`]), which the host asks before it reads it. The module is
    /// there, so no other root is searched for it, and nothing is known of it.
    Unread,
    /// Its text.
    Text(String),
}

/// How a host reads a file.
pub type ReadFile = Arc<dyn Fn(&Path) -> File + Send + Sync>;

/// A stub the host found under `stub-paths`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stub {
    /// The module it describes: `lib` for `lib/__init__.pyi` and `lib-stubs/__init__.pyi`.
    pub module: String,
    /// The name it is read as, which its relative imports start from: `lib.__init__`.
    pub read_as: String,
    /// The file.
    pub path: PathBuf,
}

type Slot = Arc<OnceLock<Option<Arc<Module>>>>;

/// The modules outside the checked tree, read on demand.
pub struct Modules {
    version: Version,
    stubs: HashMap<String, Stub>,
    /// The first part of the names of the stubs, and a pattern matching any of them.
    packages: Vec<String>,
    mention: Option<Regex>,
    roots: Vec<PathBuf>,
    read: ReadFile,
    stub: Mutex<HashMap<String, Slot>>,
    library: Mutex<HashMap<String, Slot>>,
    marked: Mutex<HashMap<String, Arc<Dict<Helper>>>>,
}

fn slot(slots: &Mutex<HashMap<String, Slot>>, module: &str) -> Slot {
    let mut slots = slots.lock().unwrap_or_else(PoisonError::into_inner);
    match slots.get(module) {
        Some(slot) => Arc::clone(slot),
        None => {
            let slot = Slot::default();
            slots.insert(module.to_string(), Arc::clone(&slot));
            slot
        }
    }
}

/// `path` with `.suffix` after its last part.
fn with_suffix(path: &Path, suffix: &str) -> Option<PathBuf> {
    let mut name = path.file_name()?.to_os_string();
    name.push(".");
    name.push(suffix);
    Some(path.with_file_name(name))
}

impl Modules {
    /// The modules `stubs` describe, the first of a name taken, and those under `roots`,
    /// read with `read`.
    pub fn new(version: Version, stubs: Vec<Stub>, roots: Vec<PathBuf>, read: ReadFile) -> Self {
        let mut by_module: HashMap<String, Stub> = HashMap::new();
        for stub in stubs {
            by_module.entry(stub.module.clone()).or_insert(stub);
        }
        let mut packages: Vec<String> = by_module
            .keys()
            .map(|module| module.split('.').next().unwrap_or(module).to_string())
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        packages.sort();
        let escaped: Vec<String> = packages.iter().map(|name| regex::escape(name)).collect();
        let mention = (!escaped.is_empty())
            .then(|| Regex::new(&format!(r"\b(?:{})\b", escaped.join("|"))).ok())
            .flatten();
        Modules {
            version,
            stubs: by_module,
            packages,
            mention,
            roots,
            read,
            stub: Mutex::default(),
            library: Mutex::default(),
            marked: Mutex::default(),
        }
    }

    /// The roots the libraries are looked for under.
    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// The stub of `module`, read when first asked for.
    pub fn stub(&self, module: &str) -> Option<Arc<Module>> {
        let stub = self.stubs.get(module)?;
        slot(&self.stub, module)
            .get_or_init(|| self.module(&stub.path, &stub.read_as))
            .clone()
    }

    /// The module `module` of the first root that holds it: its stub package
    /// (`lib-stubs`), then its `.pyi`, then its `.py`; a package before a module of the
    /// same name. Read when first asked for.
    pub fn library(&self, module: &str) -> Option<Arc<Module>> {
        if self.roots.is_empty() {
            return None;
        }
        slot(&self.library, module)
            .get_or_init(|| self.find(module, false).into_iter().next())
            .clone()
    }

    /// Every file that holds `module` in the first root that has it, in the order
    /// [`Modules::library`] prefers them: where a helper is looked for when the stub that
    /// is read first does not define it, as a partial stub leaves the rest to the
    /// library. Read each time: it is asked once per registered helper.
    pub fn libraries(&self, module: &str) -> Vec<Arc<Module>> {
        self.find(module, true)
    }

    fn module(&self, path: &Path, read_as: &str) -> Option<Arc<Module>> {
        match (self.read)(path) {
            File::Missing => None,
            File::Unread => Some(Arc::new(Module::unread(read_as, self.version))),
            File::Text(text) => {
                let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
                Some(Arc::new(Module::new(
                    Arc::from(text),
                    read_as,
                    self.version,
                )))
            }
        }
    }

    /// The files of the first root that holds `module`: the first of them, or `all`.
    fn find(&self, module: &str, all: bool) -> Vec<Arc<Module>> {
        let parts: Vec<&str> = module.split('.').collect();
        let Some((top, rest)) = parts.split_first() else {
            return Vec::new();
        };
        if parts
            .iter()
            .any(|part| part.is_empty() || !part.chars().all(crate::types::generic::is_word))
        {
            return Vec::new();
        }
        let mut held = Vec::new();
        for root in &self.roots {
            let places = [
                (format!("{top}-stubs"), "pyi"),
                (top.to_string(), "pyi"),
                (top.to_string(), "py"),
            ];
            for (package, suffix) in places {
                let mut base = root.join(&package);
                base.extend(rest);
                let init = base.join(format!("__init__.{suffix}"));
                // `lib-stubs` is a package: no file is named `lib-stubs.pyi`.
                let file = (package == *top || !rest.is_empty())
                    .then(|| with_suffix(&base, suffix))
                    .flatten();
                let found = self
                    .module(&init, &format!("{module}.__init__"))
                    .or_else(|| self.module(&file?, module));
                held.extend(found);
                if !all && !held.is_empty() {
                    return held;
                }
            }
            // The first root holding the module ends the search.
            if !held.is_empty() {
                return held;
            }
        }
        held
    }

    /// The stubs of the packages some text of `texts` names, and of those the stubs so
    /// found name in turn (a stub of a framework whose attributes are of a class of a
    /// stubbed driver), by module. No other stub is read for its helpers, so a directory
    /// of stubs for libraries the project does not use costs their listing only.
    pub fn named<'t>(&self, texts: impl Iterator<Item = &'t str>) -> Vec<&str> {
        if self.stubs.is_empty() {
            return Vec::new();
        }
        let all = self.packages.len();
        let mut named: HashSet<&str> = HashSet::new();
        let Some(mention) = &self.mention else {
            // No pattern holds too many names: every stub is read.
            let mut modules: Vec<&str> = self.stubs.keys().map(String::as_str).collect();
            modules.sort_unstable();
            return modules;
        };
        for text in texts {
            if named.len() == all {
                break;
            }
            named.extend(mention.find_iter(text).map(|found| found.as_str()));
        }
        let of = |module: &str| module.split('.').next().unwrap_or(module).to_string();
        let mut read: HashSet<&str> = HashSet::new();
        // Each pass reads the stubs of the packages the pass before named.
        while named.len() < all {
            let fresh: Vec<&String> = self
                .stubs
                .keys()
                .filter(|module| named.contains(of(module).as_str()))
                .filter(|module| read.insert(module.as_str()))
                .collect();
            let known = named.len();
            for module in fresh {
                if let Some(stub) = self.stub(module) {
                    for found in mention.find_iter(&stub.text) {
                        // The match is a name of `packages`: borrow it from there.
                        let at = self
                            .packages
                            .binary_search_by(|name| name.as_str().cmp(found.as_str()));
                        named.extend(at.ok().map(|at| self.packages[at].as_str()));
                    }
                }
            }
            if named.len() == known {
                break;
            }
        }
        let mut modules: Vec<&str> = self
            .stubs
            .keys()
            .filter(|module| named.contains(of(module).as_str()))
            .map(String::as_str)
            .collect();
        modules.sort_unstable();
        modules
    }

    /// The helpers the stubs mark `Annotated[LiteralString, SQL]`, by module, for the
    /// stubs [`Modules::named`] by `texts`.
    pub fn marked<'t>(
        &self,
        texts: impl Iterator<Item = &'t str>,
    ) -> Vec<(String, Arc<Dict<Helper>>)> {
        let modules = self.named(texts);
        let mut found = Vec::new();
        for module in modules {
            let known = self
                .marked
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(module)
                .cloned();
            let helpers = known.unwrap_or_else(|| {
                let helpers = Arc::new(self.helpers_of(module));
                self.marked
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(module.to_string(), Arc::clone(&helpers));
                helpers
            });
            if !helpers.is_empty() {
                found.push((module.to_string(), helpers));
            }
        }
        found
    }

    fn helpers_of(&self, module: &str) -> Dict<Helper> {
        let mut marked = Dict::new();
        let Some(stub) = self.stub(module).filter(|stub| defines_helpers(&stub.text)) else {
            return marked;
        };
        if let Some(parsed) = stub.source() {
            for (name, helper) in helpers_of(parsed).iter() {
                let mut helper = helper.clone();
                helper.configured = true;
                marked.insert(name, helper);
            }
        }
        marked
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// The modules of `files` under the roots `/a` and `/b`, and the paths read so far.
    fn modules(
        stubs: &[(&str, &str, &str)],
        files: &[(&'static str, &'static str)],
    ) -> (Modules, Arc<Mutex<Vec<PathBuf>>>) {
        let files: HashMap<PathBuf, &'static str> = files
            .iter()
            .map(|(path, text)| (PathBuf::from(path), *text))
            .collect();
        let read = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&read);
        let reader: ReadFile = Arc::new(move |path: &Path| match files.get(path) {
            None => File::Missing,
            Some(&"<large>") => File::Unread,
            Some(text) => {
                seen.lock().unwrap().push(path.to_path_buf());
                File::Text((*text).to_string())
            }
        });
        let stubs = stubs
            .iter()
            .map(|(module, read_as, path)| Stub {
                module: (*module).to_string(),
                read_as: (*read_as).to_string(),
                path: PathBuf::from(path),
            })
            .collect();
        let roots = vec![PathBuf::from("/a"), PathBuf::from("/b")];
        (Modules::new((3, 14), stubs, roots, reader), read)
    }

    fn text(found: Option<Arc<Module>>) -> String {
        found.map_or("<none>".to_string(), |module| module.text.to_string())
    }

    #[test]
    fn a_library_is_its_stub_package_then_its_pyi_then_its_py() {
        let (found, _) = modules(
            &[],
            &[
                ("/a/lib/__init__.py", "source = 1\n"),
                ("/a/lib/__init__.pyi", "stub = 1\n"),
                ("/a/lib-stubs/__init__.pyi", "package = 1\n"),
                ("/a/lib/core.py", "core = 1\n"),
                ("/a/lib-stubs/core.pyi", "core_stub = 1\n"),
                ("/a/only.py", "only = 1\n"),
                ("/a/only.pyi", "only_stub = 1\n"),
            ],
        );
        assert_eq!(text(found.library("lib")), "package = 1\n");
        assert_eq!(text(found.library("lib.core")), "core_stub = 1\n");
        assert_eq!(text(found.library("only")), "only_stub = 1\n");
        assert_eq!(text(found.library("lib.missing")), "<none>");
        assert_eq!(found.library("lib").unwrap().name(), "lib.__init__");
    }

    #[test]
    fn a_package_is_taken_before_a_module_and_the_first_root_before_the_next() {
        let (found, _) = modules(
            &[],
            &[
                ("/a/both.py", "module = 1\n"),
                ("/a/both/__init__.py", "package = 1\n"),
                ("/b/both.pyi", "later = 1\n"),
                ("/b/second.py", "second = 1\n"),
            ],
        );
        assert_eq!(text(found.library("both")), "package = 1\n");
        assert_eq!(text(found.library("second")), "second = 1\n");
    }

    #[test]
    fn a_file_that_is_not_read_ends_the_search() {
        let (found, _) = modules(
            &[],
            &[("/a/big.py", "<large>"), ("/b/big.py", "other = 1\n")],
        );
        let big = found.library("big").expect("the module is there");
        assert!(big.source().is_none() && big.facts().is_none());
    }

    #[test]
    fn a_name_that_is_no_module_path_reads_nothing() {
        let (found, read) = modules(&[], &[("/a/x.py", "x = 1\n")]);
        for name in ["", "..x", "x/../x", "a b", "x."] {
            assert!(found.library(name).is_none(), "{name}");
        }
        assert!(read.lock().unwrap().is_empty());
    }

    #[test]
    fn a_module_is_read_once_by_every_thread_that_asks() {
        let (found, read) = modules(&[], &[("/a/lib.py", "class Db: ...\n")]);
        let asked = AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    let lib = found.library("lib").expect("found");
                    assert!(lib.facts().is_some());
                    asked.fetch_add(1, Ordering::Relaxed);
                });
            }
        });
        assert_eq!(asked.load(Ordering::Relaxed), 8);
        assert_eq!(*read.lock().unwrap(), [PathBuf::from("/a/lib.py")]);
        let first = found.library("lib").unwrap();
        let again = found.library("lib").unwrap();
        assert!(Arc::ptr_eq(first.facts().unwrap(), again.facts().unwrap()));
    }

    const MARKED: &str = "from typing import Annotated, LiteralString\nfrom sqlty import SQL\nclass Db:\n    def one(self, sql: Annotated[LiteralString, SQL]): ...\n";

    #[test]
    fn the_first_stub_of_a_module_is_taken() {
        let (found, _) = modules(
            &[
                ("lib", "lib.__init__", "/s/lib/__init__.pyi"),
                ("lib", "lib", "/s/lib.pyi"),
                ("lib", "lib.__init__", "/t/lib/__init__.pyi"),
            ],
            &[
                ("/s/lib/__init__.pyi", "first = 1\n"),
                ("/s/lib.pyi", "module = 1\n"),
                ("/t/lib/__init__.pyi", "later = 1\n"),
            ],
        );
        assert_eq!(text(found.stub("lib")), "first = 1\n");
        assert_eq!(text(found.stub("other")), "<none>");
    }

    #[test]
    fn a_stub_names_the_packages_its_classes_come_from() {
        let (found, read) = modules(
            &[
                (
                    "framework",
                    "framework.__init__",
                    "/s/framework/__init__.pyi",
                ),
                ("lib", "lib.__init__", "/s/lib/__init__.pyi"),
                ("deep", "deep.__init__", "/s/deep/__init__.pyi"),
                ("unused", "unused.__init__", "/s/unused/__init__.pyi"),
            ],
            &[
                (
                    "/s/framework/__init__.pyi",
                    "from lib import Db\nclass Site:\n    db: Db\n",
                ),
                (
                    "/s/lib/__init__.pyi",
                    "import deep\nclass Db(deep.Db): ...\n",
                ),
                ("/s/deep/__init__.pyi", MARKED),
                ("/s/unused/__init__.pyi", MARKED),
            ],
        );
        let texts = ["from framework import Site\n"];
        assert_eq!(found.named(texts.into_iter()), ["deep", "framework", "lib"]);
        let marked = found.marked(texts.into_iter());
        assert_eq!(marked.len(), 1);
        assert_eq!(marked[0].0, "deep");
        assert_eq!(read.lock().unwrap().len(), 3, "`unused` is not read");
    }

    #[test]
    fn stubs_that_name_each_other_are_each_read_once() {
        let (found, read) = modules(
            &[
                ("a", "a.__init__", "/s/a/__init__.pyi"),
                ("b", "b.__init__", "/s/b/__init__.pyi"),
                ("c", "c.__init__", "/s/c/__init__.pyi"),
            ],
            &[
                ("/s/a/__init__.pyi", "import b\n"),
                ("/s/b/__init__.pyi", "import a\n"),
                ("/s/c/__init__.pyi", "import a\n"),
            ],
        );
        assert_eq!(found.named(["import a\n"].into_iter()), ["a", "b"]);
        assert_eq!(found.named(["import b\n"].into_iter()), ["a", "b"]);
        assert_eq!(read.lock().unwrap().len(), 2);
    }

    #[test]
    fn every_place_of_the_first_root_holds_a_library() {
        let (found, _) = modules(
            &[],
            &[
                ("/a/half.pyi", "stub = 1\n"),
                ("/a/half.py", "source = 1\n"),
                ("/a/half-stubs/__init__.pyi", "package = 1\n"),
                ("/b/half.py", "later = 1\n"),
            ],
        );
        let texts: Vec<String> = found
            .libraries("half")
            .iter()
            .map(|module| module.text.to_string())
            .collect();
        assert_eq!(texts, ["package = 1\n", "stub = 1\n", "source = 1\n"]);
        assert!(found.libraries("missing").is_empty());
    }

    #[test]
    fn only_the_stubs_of_a_named_package_are_read_for_helpers() {
        let (found, read) = modules(
            &[
                ("lib", "lib.__init__", "/s/lib/__init__.pyi"),
                ("lib.plain", "lib.plain", "/s/lib/plain.pyi"),
                ("unused", "unused.__init__", "/s/unused/__init__.pyi"),
            ],
            &[
                ("/s/lib/__init__.pyi", MARKED),
                ("/s/lib/plain.pyi", "class Plain: ...\n"),
                ("/s/unused/__init__.pyi", MARKED),
            ],
        );
        assert!(read.lock().unwrap().is_empty(), "listing reads nothing");
        let texts = [
            "import os\n",
            "from lib import Db\n",
            "library = unusedly\n",
        ];
        let marked = found.marked(texts.into_iter());
        let names: Vec<_> = marked
            .iter()
            .map(|(module, helpers)| (module.as_str(), helpers.iter().count()))
            .collect();
        assert_eq!(names, [("lib", 1)]);
        assert!(marked[0]
            .1
            .get("Db.one")
            .is_some_and(|helper| helper.configured));
        let mut seen = read.lock().unwrap().clone();
        seen.sort();
        assert_eq!(
            seen,
            [
                PathBuf::from("/s/lib/__init__.pyi"),
                PathBuf::from("/s/lib/plain.pyi")
            ]
        );
        drop(found.marked(texts.into_iter()));
        assert_eq!(read.lock().unwrap().len(), 2, "kept, not read again");
    }
}
