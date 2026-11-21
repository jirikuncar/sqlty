use crate::common::*;
use sqlty_python::locate::*;
use sqlty_sql::highlight::Highlight;

/// The query at the first `|` of `source` (removed), with `helpers` the starts of
/// strings helper calls pass, as `needle` positions in the source without the `|`.
fn at(source: &str, helpers: &[&str]) -> Option<(String, String, Route)> {
    let cursor = source.find('|').expect("a cursor");
    let source = source.replacen('|', "", 1);
    let starts: Vec<usize> = helpers.iter().map(|h| source.find(h).unwrap()).collect();
    let found = Queries::new(&source, &ImportStyle::parse(None).unwrap(), &starts).at(cursor)?;
    assert_eq!(found.source_offset(found.offset), Some(cursor), "{source}");
    let marked = format!(
        "{}|{}",
        &found.text[..found.offset],
        &found.text[found.offset..]
    );
    Some((marked, found.text, found.route))
}

fn marked(source: &str, helpers: &[&str]) -> Option<(String, Route)> {
    at(source, helpers).map(|(marked, _, route)| (marked, route))
}

#[test]
fn each_route() {
    assert_eq!(
        marked(r#"q = "--sql\nSELECT | FROM users""#, &[]),
        Some(("--sql\nSELECT | FROM users".into(), Route::Marker))
    );
    assert_eq!(
        marked(
            r#"q = SQL[tuple[int]]("--sql\nSELECT count(*) FROM |")"#,
            &[]
        ),
        Some(("--sql\nSELECT count(*) FROM |".into(), Route::Wrapper))
    );
    assert_eq!(
        marked(
            r#"fetch_all("SELECT | FROM users", tuple[int])"#,
            &["\"SELECT"]
        ),
        Some(("SELECT | FROM users".into(), Route::Helper))
    );
    assert_eq!(
        marked(
            r#"db.fetch_one("--sql\nSELECT u.| FROM users u", str)"#,
            &["\"--sql"]
        ),
        Some(("--sql\nSELECT u.| FROM users u".into(), Route::Marker))
    );
}

#[test]
fn string_forms() {
    assert_eq!(
        marked(
            "q = (\n  \"--sql\\n\"\n  \"SELECT id \"\n  'FROM |users'\n)",
            &[]
        ),
        Some(("--sql\nSELECT id FROM |users".into(), Route::Marker))
    );
    assert_eq!(
        marked("q = \"\"\"--sql\nSELECT id\nFROM u|\"\"\"", &[]),
        Some(("--sql\nSELECT id\nFROM u|".into(), Route::Marker))
    );
    assert_eq!(
        marked("q = \"  -- sql\\nSELECT |\"", &[]),
        Some(("--sql\nSELECT |".into(), Route::Marker))
    );
    assert_eq!(
        marked(r#"run(r"SELECT '\d' AS d, |")"#, &["r\""]),
        Some((r"SELECT '\d' AS d, |".into(), Route::Helper))
    );
    assert_eq!(
        marked(
            r#"q = "--sql\nSELECT \"Name\", \x41|, 'é', \u00e9 FROM t""#,
            &[]
        ),
        Some((
            "--sql\nSELECT \"Name\", A|, 'é', é FROM t".into(),
            Route::Marker
        ))
    );
    assert_eq!(
        marked(r#"q = "--sql\nSELECT 'é', 'ü', i| FROM t""#, &[]),
        Some(("--sql\nSELECT 'é', 'ü', i| FROM t".into(), Route::Marker))
    );
}

#[test]
fn cursor_at_either_end_of_the_content() {
    let source = r#"q = "--sql\nSELECT 1""#;
    let queries = Queries::new(source, &ImportStyle::parse(None).unwrap(), &[]);
    let end = queries.at(source.len() - 1).unwrap();
    assert_eq!(end.offset, end.text.len());
    assert_eq!(end.span, 4..source.len());
    assert!(queries.at(source.len()).is_none());
    assert!(queries.at(5).is_none(), "in the marker");
    let at = queries.at(source.find("SELECT").unwrap()).unwrap();
    assert_eq!(at.source_offset(0), None);
    assert_eq!(at.source_offset(at.text.len() + 1), None);
    assert_eq!(at.source_offset(5), Some(source.find("\\n").unwrap()));
}

#[test]
fn does_not_complete() {
    for source in [
        r#"q = f"--sql\nSELECT {x}, | FROM t""#,
        r#"q = t"--sql\nSELECT {x}, | FROM t""#,
        r#"q = rb"--sql\nSELECT | FROM t""#,
        r#"run("SELECT | FROM users", int)"#,
        r#"q = "SELECT | FROM users""#,
        r#"q = query  # "--sql\nSELECT |"#,
        "q = (\"--sql\\nSELECT 1\" |  \"FROM t\")",
        r#"q = "--sql\nSELECT 1" + |x"#,
    ] {
        assert_eq!(marked(source, &[]), None, "{source}");
    }
    assert_eq!(marked(r#"run(f"SELECT {x} |", int)"#, &["f\""]), None);
}

/// The source text and kind of each colored range of `source`.
fn colored(source: &str, helpers: &[&str]) -> Vec<(String, Highlight)> {
    let starts: Vec<usize> = helpers.iter().map(|h| source.find(h).unwrap()).collect();
    Queries::new(source, &ImportStyle::parse(None).unwrap(), &starts)
        .highlights("postgres")
        .into_iter()
        .map(|(range, kind)| (source[range].to_string(), kind))
        .collect()
}

#[test]
fn highlights_each_checked_string() {
    use Highlight::{Comment, Keyword, Number, Operator, String as Str};
    assert_eq!(
        colored(r#"q = "--sql\nSELECT 1 FROM t""#, &[]),
        [
            ("--sql".into(), Comment),
            ("SELECT".into(), Keyword),
            ("1".into(), Number),
            ("FROM".into(), Keyword),
        ]
    );
    assert_eq!(
        colored(
            "q = (\n  \"--sql\\n\"\n  \"SELECT 'a\\tb' \"\n  'FR' 'OM t')",
            &[]
        ),
        [
            ("--sql".into(), Comment),
            ("SELECT".into(), Keyword),
            (r"'a\tb'".into(), Str),
            ("FR".into(), Keyword),
            ("OM".into(), Keyword),
        ],
        "a token across literals is split, an escape sequence is whole"
    );
    assert_eq!(
        colored(r#"run("SELECT * FROM t", int)"#, &["\"SELECT"]),
        [
            ("SELECT".into(), Keyword),
            ("*".into(), Operator),
            ("FROM".into(), Keyword),
        ]
    );
    for unchecked in [
        r#"q = "SELECT 1""#,
        r#"q = f"--sql\nSELECT {x}""#,
        r#"q = SQL[int]("SELECT 1")"#,
    ] {
        assert_eq!(colored(unchecked, &[]), [], "{unchecked}");
    }
}

#[test]
fn locates_among_many_queries() {
    let source: String = (0..50)
        .map(|i| format!("q{i} = \"--sql\\nSELECT {i} FROM t\"\nx = 'plain'\n"))
        .collect();
    let queries = Queries::new(&source, &ImportStyle::parse(None).unwrap(), &[]);
    let at = queries.at(source.find("SELECT 37").unwrap() + 7).unwrap();
    assert_eq!(
        (at.text.as_str(), at.offset),
        ("--sql\nSELECT 37 FROM t", 13)
    );
    assert!(queries.at(source.find("plain").unwrap()).is_none());
}

#[test]
fn rarer_sites_and_offsets() {
    assert_eq!(
        [Route::Marker, Route::Wrapper, Route::Helper].map(Route::name),
        ["marker", "wrapper", "helper"]
    );
    assert_eq!(colored(r#"run("SELECT 1", int)"#, &["ELECT"]), []);
    assert_eq!(colored(r#"run(f"SELECT {x}")"#, &["f\""]), []);
    assert_eq!(colored(r#"q = r"\x2d-sql\nSELECT 1""#, &[]), []);
    assert_eq!(
        colored(
            "from sqlty import SQL\nq = SQL[int](r\"\\x2d-sql\\nSELECT 1\")",
            &[]
        ),
        []
    );
    for unchecked in [
        "from sqlty import SQL\nq = SQL[int](\"SELECT 1\")",
        "from sqlty import SQL\nq = SQL[int](f\"--sql\\nSELECT {x}\")",
    ] {
        assert_eq!(colored(unchecked, &[]), [], "{unchecked}");
    }
    let source = r#"q = "--sql\nSELECT '\u2014\t' FROM t""#;
    let queries = Queries::new(source, &ImportStyle::parse(None).unwrap(), &[]);
    let escape = source.find(r"\t").unwrap();
    let at = queries.at(escape + 1).unwrap();
    assert_eq!(at.text[at.offset..].chars().next(), Some('\t'));
    let dash = at.text.find('\u{2014}').unwrap();
    let named = source.find(r"\u").unwrap();
    assert_eq!(at.source_offset(dash + 1), Some(escape));
    assert_eq!(at.source_offset(dash), Some(named));
}
