use crate::common::*;
use sqlty_python::project::*;
use sqlty_python::project::{Kind, Read};

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
