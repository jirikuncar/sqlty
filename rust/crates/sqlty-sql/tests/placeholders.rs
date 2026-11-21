//! `rewrite`: placeholders respelled `$N` by style.

use sqlty_sql::placeholders::{rewrite, Styles};

const ALL: Styles = Styles {
    question_marks: true,
    colon_names: true,
    sigil_names: false,
    dollar_numbers: true,
    backslash_escapes: false,
};
const PLAIN: Styles = Styles {
    question_marks: false,
    colon_names: false,
    sigil_names: false,
    dollar_numbers: true,
    backslash_escapes: false,
};

fn text(sql: &str, styles: Styles) -> String {
    rewrite(sql, styles).text
}

#[test]
fn psycopg_placeholders() {
    let found = rewrite("SELECT %s, %(a)s, %b, %(a)t, 5 %% 2, 5 % 2", PLAIN);
    assert_eq!(found.text, "SELECT $1, $2, $3, $2, 5 % 2, 5 % 2");
    assert_eq!(found.names, [None, Some("a".into()), None]);
}

#[test]
fn native_placeholders_stay() {
    let found = rewrite("SELECT $2, $1", ALL);
    assert_eq!(found.text, "SELECT $2, $1");
    assert!(found.names.is_empty());
}

#[test]
fn question_marks() {
    let numbered = Styles {
        dollar_numbers: false,
        ..ALL
    };
    assert_eq!(text("a = ? AND b = %s", numbered), "a = ?1 AND b = ?2");
    assert_eq!(text("a = ? AND b = ?2", ALL), "a = $1 AND b = $2");
    assert_eq!(text("a ? 'k'", PLAIN), "a ? 'k'");
    assert_eq!(text("a ?| b, c ?& d", ALL), "a ?| b, c ?& d");
}

#[test]
fn colon_names() {
    let found = rewrite("x = :id AND y::int = :id OR z = :other", ALL);
    assert_eq!(found.text, "x = $1 AND y::int = $1 OR z = $2");
    assert_eq!(found.names, [Some("id".into()), Some("other".into())]);
    assert_eq!(text("a[1:2], t:x, x[y:z]", ALL), "a[1:2], t:x, x[y:z]");
    assert_eq!(text(":a::int, \\:b, :c", ALL), ":a::int, \\:b, $1");
    assert_eq!(text("x = :id", PLAIN), "x = :id");
}

#[test]
fn sigil_names() {
    let sqlite = Styles {
        sigil_names: true,
        dollar_numbers: false,
        ..ALL
    };
    let found = rewrite("x = @id AND y = $id OR z = :other AND w = a$b", sqlite);
    assert_eq!(found.text, "x = ?1 AND y = ?1 OR z = ?2 AND w = a$b");
    assert_eq!(found.names, [Some("id".into()), Some("other".into())]);
    assert_eq!(text("x = @id", ALL), "x = @id");
}

#[test]
fn literals_and_comments_are_text() {
    let sql = "SELECT '%s ? :a', \"?\", `:b` -- ? %s\n, /* ? */ ?";
    assert_eq!(
        text(sql, ALL),
        "SELECT '%s ? :a', \"?\", `:b` -- ? %s\n, /* ? */ $1"
    );
    assert_eq!(text("SELECT 'it''s ?', ?", ALL), "SELECT 'it''s ?', $1");
}

#[test]
fn backslash_escapes() {
    let mysql = Styles {
        backslash_escapes: true,
        ..ALL
    };
    assert_eq!(text(r"SELECT 'it\'s ?', ?", mysql), r"SELECT 'it\'s ?', $1");
    assert_eq!(text(r"SELECT 'a\\', ?", mysql), r"SELECT 'a\\', $1");
    // Standard strings end at the quote; `E` strings take the backslash.
    assert_eq!(text(r"SELECT 'a\', ?", ALL), r"SELECT 'a\', $1");
    assert_eq!(text(r"SELECT E'it\'s ?', ?", ALL), r"SELECT E'it\'s ?', $1");
    assert_eq!(text(r"SELECT bare'x', ?", ALL), r"SELECT bare'x', $1");
}

#[test]
fn stray_percent_is_text() {
    assert_eq!(
        text("a % b, %x, %(a b)s, %(", ALL),
        "a % b, %x, %(a b)s, %("
    );
}
