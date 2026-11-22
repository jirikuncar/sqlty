//! `Interpolation.str` as CPython 3.14.6 (legacy) and 3.14.10 read each field.
//!
//! Its own test binary: `LEGACY_FIELD_TEXT` is a process-wide switch, and no other test
//! may run while it is flipped.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a failed setup fails the test"
)]

use std::sync::atomic::Ordering;

use ruff_python_ast::Expr;
use sqlty_python::unparse::{interpolation_text, LEGACY_FIELD_TEXT};

fn texts(source: &str) -> String {
    let parsed = sqlty_python::parse::expression(source).expect("parses");
    let Expr::TString(tstring) = &*parsed.syntax().body else {
        panic!("a t-string: {source}");
    };
    let field = tstring.value.as_slice()[0]
        .elements
        .interpolations()
        .next()
        .expect("a field");
    interpolation_text(field, source)
}

#[test]
fn interpolation_texts() {
    for (source, legacy, current) in [
        (
            "t'{f'{a!r:>{\"'\"}} {{'!s}'",
            "f'{a!r:>{\"'\"}} {{'",
            "f'{a!r:>{\"'\"}} {{'",
        ),
        ("t'{ x != y  !r }'", " x != y", " x != y  "),
        ("t'{a!r:{b!s}}'", "a", "a"),
        ("t'''{a +\r\n b}'''", "a +\n b", "a +\n b"),
        ("t'''{a # c\n}'''", "a", "a \n"),
        ("t'''{a +# c\n b}'''", "a +\n b", "a +\n b"),
        ("t'''{a + '#' # c\n}'''", "a + '#'", "a + '#' \n"),
        ("t'{x!=1:y}'", "x", "x!=1"),
        ("t'{a!=b!=c:y}'", "a!=b", "a!=b!=c"),
        ("t'{a if b!=c else d:>3}'", "a if b", "a if b!=c else d"),
        ("t'{x!=1}'", "x!=1", "x!=1"),
        ("t'{x!=1!r}'", "x!=1", "x!=1"),
        ("t'{a[b!=c]:y}'", "a[b!=c]", "a[b!=c]"),
        ("t'{\"!\":y}'", "\"!\"", "\"!\""),
        ("t'{ x = }'", " x", " x "),
        ("t'{x = !r}'", "x", "x "),
        ("t'{x=:y}'", "x", "x"),
        ("t'{x=!a}'", "x", "x"),
        ("t'''{x = \\\n}'''", "x = \\", "x "),
        ("t'{a + \"x!\" !r}'", "a + \"x!\"", "a + \"x!\" "),
        ("t'{(a!=b)!=c:y}'", "(a!=b)", "(a!=b)!=c"),
        ("t'{f'{'!'}'!=b:y}'", "f'{'!'}'", "f'{'!'}'!=b"),
        (
            "t'{f'{a:{b!r}>3}'!=c:y}'",
            "f'{a:{b!r}>3}'",
            "f'{a:{b!r}>3}'!=c",
        ),
        ("t'{f'{{!'!=b:y}'", "f'{{!'", "f'{{!'!=b"),
        (
            "t'{f'{(a)!r}\\'!'!=b:y}'",
            "f'{(a)!r}\\'!'",
            "f'{(a)!r}\\'!'!=b",
        ),
        (
            "t'''{a + '''\"'''!=b:y}'''",
            "a + '''\"'''",
            "a + '''\"'''!=b",
        ),
        (
            "t'{a + \"\\\\\" + b!=c:y}'",
            "a + \"\\\\\" + b",
            "a + \"\\\\\" + b!=c",
        ),
        (
            "t'''{a + \"\\\"#\" # c\n}'''",
            "a + \"\\\"",
            "a + \"\\\"#\" \n",
        ),
        ("t'''{a # c'\n + b}'''", "a \n + b", "a \n + b"),
        ("t'''{a # c\n=}'''", "a", "a \n"),
        ("t'''{a!=b # c!\n:y}'''", "a", "a!=b \n"),
        ("t'''{a # c!\n!r}'''", "a", "a \n"),
        ("t'{a != f'{'#'}' != a:y}'", "a != f'{'", "a != f'{'"),
        ("t'{a != f'{'#'}':y}'", "a", "a != f'{'"),
        ("t'''{'\\'' # !=\n!r}'''", "'\\'' # !", "'\\'' \n"),
    ] {
        LEGACY_FIELD_TEXT.store(true, Ordering::Relaxed);
        assert_eq!(texts(source), legacy, "{source}");
        LEGACY_FIELD_TEXT.store(false, Ordering::Relaxed);
        assert_eq!(texts(source), current, "{source}");
    }
}
