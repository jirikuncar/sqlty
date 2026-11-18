//! The dotted names a checked file is imported by, from its packages (the directories up
//! through those holding an `__init__.py`) and the checked directories.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{PoisonError, RwLock};

use super::{normalized, Files, Listing};

/// What the host's files tell of packages, each directory asked once.
pub struct Packages {
    files: Box<dyn Files + Send + Sync>,
    known: RwLock<HashMap<PathBuf, bool>>,
}

impl Packages {
    /// Packages as `files` lists them.
    pub fn new(files: Box<dyn Files + Send + Sync>) -> Self {
        Self {
            files,
            known: RwLock::default(),
        }
    }

    fn is_package(&self, directory: &Path) -> bool {
        let known = self.known.read().unwrap_or_else(PoisonError::into_inner);
        if let Some(&known) = known.get(directory) {
            return known;
        }
        drop(known);
        let found = match self.files.list(directory) {
            Listing::Entries(entries) => entries.iter().any(|(name, _)| name == "__init__.py"),
            Listing::Missing | Listing::Unread => false,
        };
        let mut known = self.known.write().unwrap_or_else(PoisonError::into_inner);
        known.insert(directory.to_path_buf(), found);
        found
    }

    /// Each of `roots` against `cwd`, resolved, that is a directory.
    pub fn roots(&self, roots: &[PathBuf], cwd: &Path) -> Vec<PathBuf> {
        roots
            .iter()
            .map(|root| self.files.canonical(&normalized(&cwd.join(root))))
            .filter(|root| matches!(self.files.list(root), Listing::Entries(_)))
            .collect()
    }

    /// The names of `directory` and its parents up through the packages, outermost first.
    pub fn package_path(&self, directory: &Path) -> Vec<String> {
        let mut names = Vec::new();
        let mut current = directory;
        while let (Some(parent), Some(name)) = (current.parent(), current.file_name()) {
            if !self.is_package(current) {
                break;
            }
            names.push(name.to_string_lossy().into_owned());
            current = parent;
        }
        names.reverse();
        names
    }

    /// The dotted names the resolved `path` is imported by, the likeliest first: its package
    /// path, then its path from each of the [`roots`](Self::roots), without and with the
    /// root's name. `pkg/__init__.py` is `pkg`.
    pub fn names(&self, path: &Path, roots: &[PathBuf]) -> Vec<String> {
        let directory = path.parent().unwrap_or(Path::new(""));
        let stem = stem(path);
        let mut parts = self.package_path(directory);
        if stem != "__init__" {
            parts.push(stem.clone());
        }
        let mut names = vec![parts.join(".")];
        for root in roots {
            let Some(relative) = strip(path, root) else {
                continue;
            };
            let mut relative: Vec<String> = relative
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();
            let last = relative.pop().map(|last| stem_of(&last));
            relative.extend(last.filter(|last| last != "__init__"));
            let name = root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
            names.push(relative.join("."));
            names.push(
                [name.unwrap_or_default()]
                    .into_iter()
                    .chain(relative)
                    .collect::<Vec<_>>()
                    .join("."),
            );
        }
        let mut found: Vec<String> = Vec::new();
        for name in names {
            if !name.is_empty() && !found.contains(&name) {
                found.push(name);
            }
        }
        match found.is_empty() {
            true => vec![stem],
            false => found,
        }
    }

    /// The checked directories (relative to `cwd`), the parents of the checked files and
    /// the package roots of `files`, resolved, each once.
    pub fn checked_roots(&self, paths: &[PathBuf], files: &[PathBuf], cwd: &Path) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = paths
            .iter()
            .map(|path| match self.files.list(&normalized(&cwd.join(path))) {
                Listing::Entries(_) => path.clone(),
                _ => path.parent().map(Path::to_path_buf).unwrap_or_default(),
            })
            .collect();
        let mut seen: Vec<&Path> = Vec::new();
        for directory in files.iter().filter_map(|file| file.parent()) {
            if seen.contains(&directory) {
                continue;
            }
            seen.push(directory);
            let mut root = directory;
            for _ in self.package_path(directory) {
                root = root.parent().unwrap_or(root);
            }
            roots.push(root.to_path_buf());
        }
        let mut found: Vec<PathBuf> = Vec::new();
        for root in roots {
            let resolved = self.files.canonical(&normalized(&cwd.join(root)));
            if !found.contains(&resolved) {
                found.push(resolved);
            }
        }
        found
    }
}

/// Where `[[tool.sqlty.functions]]` modules are read from, the likeliest first:
/// `config_dir/src`, `config_dir`, the `checked` roots inside it, then `extra` (such as the
/// host's `sys.path`).
pub fn helper_roots(
    config_dir: Option<&Path>,
    checked: &[PathBuf],
    extra: &[PathBuf],
) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(project) = config_dir {
        roots.extend([project.join("src"), project.to_path_buf()]);
    }
    let inside = checked
        .iter()
        .filter(|root| config_dir.is_none_or(|project| root.starts_with(project)));
    roots.extend(inside.cloned());
    roots.extend(extra.iter().cloned());
    let mut found: Vec<PathBuf> = Vec::new();
    for root in roots {
        if !found.contains(&root) {
            found.push(root);
        }
    }
    found
}

/// `path` under `root`, compared as the platform compares paths.
fn strip(path: &Path, root: &Path) -> Option<PathBuf> {
    let depth = fold(path)
        .strip_prefix(fold(root))
        .ok()?
        .components()
        .count();
    let parts: Vec<_> = path.components().collect();
    (depth > 0).then(|| parts[parts.len() - depth..].iter().collect())
}

#[cfg(windows)]
fn fold(path: &Path) -> PathBuf {
    PathBuf::from(path.to_string_lossy().to_lowercase())
}

#[cfg(not(windows))]
fn fold(path: &Path) -> PathBuf {
    path.to_path_buf()
}

fn stem(path: &Path) -> String {
    path.file_name()
        .map(|name| stem_of(&name.to_string_lossy()))
        .unwrap_or_default()
}

/// `os.path.splitext(name)[0]`: a leading dot is not an extension.
fn stem_of(name: &str) -> String {
    let start = name.len() - name.trim_start_matches('.').len();
    match name[start..].rfind('.') {
        Some(dot) => name[..start + dot].to_string(),
        None => name.to_string(),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::project::{Kind, Read};

    struct Tree(Vec<&'static str>);

    impl Files for Tree {
        fn read(&self, _: &Path) -> Read {
            Read::Missing
        }

        fn list(&self, path: &Path) -> Listing {
            let mut entries = Vec::new();
            for file in &self.0 {
                if let Ok(rest) = Path::new(file).strip_prefix(path) {
                    if let Some(name) = rest.components().next() {
                        let name = name.as_os_str().to_string_lossy().into_owned();
                        entries.push((name, Kind::File));
                    }
                }
            }
            match entries.is_empty() {
                true => Listing::Missing,
                false => Listing::Entries(entries),
            }
        }
    }

    fn packages() -> Packages {
        Packages::new(Box::new(Tree(vec![
            "/w/src/pkg/__init__.py",
            "/w/src/pkg/sub/__init__.py",
            "/w/src/pkg/sub/mod.py",
            "/w/src/pkg/data.v2.py",
            "/w/scripts/run.py",
        ])))
    }

    #[test]
    fn a_module_is_named_by_its_packages_then_by_each_root() {
        let packages = packages();
        let names = |path: &str, roots: &[&str]| {
            let roots: Vec<PathBuf> = roots.iter().map(PathBuf::from).collect();
            packages.names(Path::new(path), &packages.roots(&roots, Path::new("/w")))
        };
        assert_eq!(names("/w/src/pkg/sub/mod.py", &[]), ["pkg.sub.mod"]);
        assert_eq!(
            names("/w/src/pkg/sub/__init__.py", &["src"]),
            ["pkg.sub", "src.pkg.sub"]
        );
        assert_eq!(names("/w/src/pkg/data.v2.py", &[]), ["pkg.data.v2"]);
        assert_eq!(
            names("/w/scripts/run.py", &[".", "scripts", "missing"]),
            ["run", "scripts.run", "w.scripts.run"]
        );
        assert_eq!(names("/w/scripts/__init__.py", &["scripts"]), ["scripts"]);
        assert_eq!(names("/elsewhere/.hidden.py", &["."]), [".hidden"]);
        assert_eq!(names("/elsewhere/run", &[]), ["run"]);
        assert_eq!(names("/__init__.py", &[]), ["__init__"]);
    }

    #[test]
    fn roots_are_the_checked_directories_and_package_roots() {
        let packages = packages();
        let paths = [PathBuf::from("src/pkg"), PathBuf::from("scripts/run.py")];
        let files = [
            PathBuf::from("/w/src/pkg/sub/mod.py"),
            PathBuf::from("/w/src/pkg/__init__.py"),
            PathBuf::from("/w/scripts/run.py"),
        ];
        assert_eq!(
            packages.checked_roots(&paths, &files, Path::new("/w")),
            [
                PathBuf::from("/w/src/pkg"),
                PathBuf::from("/w/scripts"),
                PathBuf::from("/w/src")
            ]
        );
        assert_eq!(
            helper_roots(
                Some(Path::new("/p")),
                &[PathBuf::from("/p/app"), PathBuf::from("/q")],
                &[PathBuf::from("/p")]
            ),
            [
                PathBuf::from("/p/src"),
                PathBuf::from("/p"),
                PathBuf::from("/p/app")
            ]
        );
        assert_eq!(
            helper_roots(None, &[PathBuf::from("/q")], &[]),
            [PathBuf::from("/q")]
        );
    }
}
