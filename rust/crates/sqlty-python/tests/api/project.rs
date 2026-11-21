use crate::common::*;
use sqlty_python::project::*;

fn tree(files: &[(&str, &str)]) -> impl Fn(&Path) -> Read {
    let files: HashMap<PathBuf, String> = files
        .iter()
        .map(|(path, text)| (PathBuf::from(path), text.to_string()))
        .collect();
    move |path: &Path| match files.get(path) {
        Some(text) if text == "<unread>" => Read::Unread,
        Some(text) => Read::Text(text.as_bytes().to_vec()),
        None => Read::Missing,
    }
}

fn found(directory: &str, files: &[(&str, &str)]) -> Result<Option<Found>, FindError> {
    find(Path::new(directory), &tree(files))
}

#[test]
fn a_file_the_host_reads_may_be_a_link() {
    assert!(tree(&[]).is_link(Path::new("/app/a.py")));
}

#[test]
fn the_nearest_file_with_a_table_wins() {
    let files = [
        ("/a/sqlty.toml", "dialect = \"mysql\""),
        ("/a/b/pyproject.toml", "[project]\nname = \"b\""),
        ("/a/b/c/Cargo.toml", "[package]\nname = \"c\""),
    ];
    let config = found("/a/b/c/../c/./", &files).unwrap().unwrap();
    assert_eq!(config.file, Path::new("/a/sqlty.toml"));
    assert_eq!(found("/elsewhere", &files), Ok(None));
    let root = found("/a", &[("/sqlty.toml", "dialect = \"mysql\"")]);
    assert_eq!(root.unwrap().unwrap().file, Path::new("/sqlty.toml"));
}

#[test]
fn a_host_reads_what_it_is_asked_for() {
    let files = [("/a/sqlty.toml", "<unread>")];
    assert_eq!(
        found("/a", &files),
        Err(FindError::Need(PathBuf::from("/a/sqlty.toml")))
    );
    let files = [
        ("/a/sqlty.toml", "extend = \"base.toml\""),
        ("/a/base.toml", "<unread>"),
    ];
    assert_eq!(
        found("/a", &files),
        Err(FindError::Need(PathBuf::from("/a/base.toml")))
    );
    assert!(matches!(
        tree(&files).list(Path::new("/a")),
        Listing::Missing
    ));
}

#[test]
fn errors_name_the_file() {
    let syntax = found("/a", &[("/a/pyproject.toml", "[tool")]).unwrap_err();
    assert!(
        matches!(syntax, FindError::Syntax(ref message) if message.starts_with("/a/pyproject.toml: "))
    );
    let invalid = found("/a", &[("/a/sqlty.toml", "[tool")]).unwrap_err();
    assert!(
        matches!(invalid, FindError::Invalid(ref message) if message.starts_with("/a/sqlty.toml: "))
    );
    let unknown = found("/a", &[("/a/pyproject.toml", "[tool.sqlty]\nnope = 1")]).unwrap_err();
    assert!(unknown.to_string().contains("unknown key 'nope'"));
    let missing = found("/a", &[("/a/sqlty.toml", "extend = \"../x.toml\"")]).unwrap_err();
    assert_eq!(
        missing.to_string(),
        "/a/sqlty.toml: extend = '../x.toml': config file does not exist: /x.toml"
    );
    let cycle = found(
        "/a",
        &[
            ("/a/sqlty.toml", "extend = \"b.toml\""),
            ("/a/b.toml", "extend = \"sqlty.toml\""),
        ],
    )
    .unwrap_err();
    assert_eq!(
        cycle.to_string(),
        "/a/b.toml: extend = 'sqlty.toml' extends itself in a cycle"
    );
    let tableless = found(
        "/a",
        &[
            ("/a/sqlty.toml", "extend = \"pyproject.toml\""),
            ("/a/pyproject.toml", ""),
        ],
    )
    .unwrap_err();
    assert!(tableless.to_string().contains("no [tool.sqlty] table"));
}

#[test]
fn extend_merges_tables_and_rebases_paths() {
    let files = [
        (
            "/w/app/sqlty.toml",
            "extend = \"../base/sqlty.toml\"\ndialect = \"sqlite\"\nrules = { select-star = \"warn\" }\n\
                 [databases.main]\nschema = \"own.sql\"\n",
        ),
        (
            "/w/base/sqlty.toml",
            "dialect = \"mysql\"\nschema = [\"s/*.sql\"]\nqueries = \"q\"\n\
                 rules = { extra-param = \"error\" }\n\
                 [databases.main]\nschema = \"main.sql\"\n[databases.logs]\nschema = \"logs.sql\"\n",
        ),
    ];
    let read = tree(&files);
    let config = find(Path::new("/w/app"), &read)
        .unwrap()
        .unwrap()
        .config
        .table;
    let shown: Table = toml::from_str(
        "dialect = \"sqlite\"\nschema = [\"../base/s/*.sql\"]\nqueries = [\"../base/q\"]\n\
             rules = { extra-param = \"error\", select-star = \"warn\" }\n\
             [databases.main]\nschema = \"own.sql\"\ninclude = []\nexclude = []\n\
             [databases.logs]\nschema = [\"../base/logs.sql\"]\ninclude = []\nexclude = []\n",
    )
    .unwrap();
    assert_eq!(config, shown);
}

#[test]
fn a_named_file_must_have_its_table() {
    let files = [("/a/pyproject.toml", "[project]")];
    let error = load_file(
        Path::new("/a/pyproject.toml"),
        Path::new("/"),
        &tree(&files),
    );
    assert!(error
        .unwrap_err()
        .to_string()
        .contains("no [tool.sqlty] table"));
    let error = load_file(Path::new("/a/x.toml"), Path::new("/"), &tree(&files));
    assert_eq!(
        error.unwrap_err().to_string(),
        "config file does not exist: /a/x.toml"
    );
    let files = [
        ("/a/x.toml", "extend = \"b/y.toml\""),
        ("/a/b/y.toml", "schema = \"s.sql\""),
    ];
    let config = load_file(Path::new("/a/x.toml"), Path::new("/c"), &tree(&files)).unwrap();
    assert_eq!(config.table["schema"][0].as_str(), Some("../a/b/s.sql"));
}

#[test]
fn paths_are_spelled_as_python_spells_them() {
    assert_eq!(normalized(Path::new("/a/./b/../c/")), Path::new("/a/c"));
    assert_eq!(normalized(Path::new("../a")), Path::new("../a"));
    assert_eq!(normalized(Path::new("./a")), Path::new("a"));
}
