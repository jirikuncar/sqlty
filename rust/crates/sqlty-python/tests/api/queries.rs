use crate::common::*;
use sqlty_python::queries::*;

fn names(source: &str, format: Format) -> Vec<(String, String)> {
    split(source, format, "stem")
        .into_iter()
        .map(|query| (query.name, source[query.body].trim().to_string()))
        .collect()
}

#[test]
fn headers_of_each_format() {
    let pair = |name: &str, sql: &str| (name.to_string(), sql.to_string());
    let aiosql = "-- preamble\n-- name: get-user^\n-- doc\nSELECT 1;\n\n-- name: add!\nINSERT INTO t VALUES (1)";
    assert_eq!(
        names(aiosql, Format::Auto),
        [
            pair("get_user", "-- doc\nSELECT 1;"),
            pair("add", "INSERT INTO t VALUES (1)")
        ]
    );
    let sqlc = "-- name: GetUser :one\nSELECT 1;\n-- name: ListUsers :many\nSELECT 2;";
    assert_eq!(
        names(sqlc, Format::Auto),
        [pair("GetUser", "SELECT 1;"), pair("ListUsers", "SELECT 2;")]
    );
    let pugsql = "-- :name find_user :one\nSELECT 1";
    assert_eq!(names(pugsql, Format::Auto), [pair("find_user", "SELECT 1")]);
    assert_eq!(names("SELECT 1", Format::Auto), [pair("stem", "SELECT 1")]);
    assert_eq!(names(aiosql, Format::File)[0].0, "stem");
    assert!(
        names(sqlc, Format::Aiosql).is_empty(),
        "sqlc headers are not aiosql's"
    );
}

#[test]
fn placeholders_keep_their_length_and_skip_strings_and_casts() {
    let sql = "SELECT '@a', :u.x, x::pg.t, @@v -- @b\nWHERE sqlc.narg( 'n' ) = @name";
    assert_eq!(
        placeholders(sql, Format::Sqlc).0,
        "SELECT '@a', :u.x, x::pg.t, @@v -- @b\nWHERE :n               = :name"
    );
    assert_eq!(
        placeholders("SELECT sqlc.embed( u ), u.id FROM users u", Format::Sqlc),
        (
            "SELECT u.*            , u.id FROM users u".to_string(),
            Vec::new(),
            vec![(7..10, 19..20)]
        )
    );
    assert_eq!(
        placeholders(sql, Format::Aiosql).0,
        "SELECT '@a', :u_x, x::pg.t, @@v -- @b\nWHERE sqlc.narg( 'n' ) = @name"
    );
    assert_eq!(placeholders(sql, Format::Pugsql).0, sql);
}

#[test]
fn a_header_on_the_last_line_has_an_empty_body() {
    let source = "SELECT 0;\n-- name: last";
    let [query] = split(source, Format::Aiosql, "stem").try_into().unwrap();
    assert!(source[query.body].is_empty());
}

#[test]
fn rarer_formats_and_checks() {
    for name in Format::NAMES {
        assert!(name.parse::<Format>().is_ok(), "{name}");
    }
    assert!("nope".parse::<Format>().is_err());
    assert_eq!(
        placeholders("SELECT :u.id, :u.id", Format::Aiosql).1,
        [("u_id".to_string(), "u.id".to_string())]
    );
    let (schema, _) =
        sqlty_sql::analyzer::load_schema_with_skipped("CREATE TABLE t (a INT)", "postgres");
    let options = Options::new("postgres", Some(&schema), None, false).unwrap();
    let rules = only(&[Rule::UnparsedQuery]);
    let found = check_queries(
        "SELECT nope FROM t;\nSELECT * FROM t",
        Format::File,
        "q",
        &options,
        &rules,
    );
    assert!(found.is_empty(), "{found:?}");
    let options = Options::new("postgres", None, None, false).unwrap();
    let found = check_queries("SELEC", Format::File, "q", &options, &rules);
    assert_eq!(found.len(), 1, "{found:?}");
}
