use crate::common::*;
use sqlty_python::project::*;
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
    let pending = Tree::new(&[("/v/sqlty.toml", Some("<unread>"))]);
    let discovery = Discovery::new(Box::new(pending), Path::new("/v"));
    for _ in 0..2 {
        assert!(matches!(
            discovery.find(Path::new("/v")),
            Err(FindError::Need(path)) if path == Path::new("/v/sqlty.toml")
        ));
    }
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
