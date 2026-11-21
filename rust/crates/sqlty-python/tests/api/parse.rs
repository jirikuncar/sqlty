use ruff_python_ast::{Expr, Stmt};
use sqlty_python::parse::*;

#[test]
fn rejects_what_ast_rejects() {
    for source in [
        "\u{feff}x = 1\n",
        "x = 'a\0'\n",
        "x = '\\ud800'\n",
        "x = '\\U0000DFFF'\n",
        "'doc'\nfrom __future__ import braces\n",
        "from __future__ import barry_as_FLUFL\n",
        &format!("x = {}1{}", "(".repeat(201), ")".repeat(201)),
        &format!("x = {}1{}", "f'{".repeat(150), "}'".repeat(150)),
        "x = f'{x:{y:{z:{w}}}}'\n",
        &format!("x = {}1", "-".repeat(MAX_DEPTH + 1)),
        &format!("if x: pass\n{}", "elif x: pass\n".repeat(MAX_DEPTH)),
    ] {
        assert!(module(source).is_none(), "{source:.60}");
    }
}

#[test]
fn accepts_what_ast_accepts() {
    for source in [
        "x = 1\nfrom __future__ import braces\n",
        "from __future__ import annotations, division\n",
        "x = '\\\\ud800'[0:0] or '\\u00e9'\n",
        &format!("x = {}1{}", "(".repeat(200), ")".repeat(200)),
        &format!("x = {}1{}", "f'{".repeat(149), "}'".repeat(149)),
        "x = f'{x:{y:{z}}}'\n",
    ] {
        assert!(module(source).is_some(), "{source:.60}");
    }
}

#[test]
fn newlines_in_strings_read_as_ast_reads_them() {
    let source = "x = (\"\"\"a\r\nb\rc\\r\"\"\"\r\n  'd')\r\n";
    let parsed = module(source).unwrap();
    let Stmt::Assign(assign) = &parsed.syntax().body[0] else {
        panic!()
    };
    let Expr::StringLiteral(literal) = assign.value.as_ref() else {
        panic!()
    };
    assert_eq!(text(literal, source), "a\nb\nc\rd");
}

#[test]
fn deep_nesting_does_not_overflow() {
    for source in [
        format!("x = {}1{}", "(".repeat(5_000), ")".repeat(5_000)),
        format!("x = {}1", "-".repeat(20_000)),
        format!("x = {}1", "lambda: ".repeat(3_000)),
    ] {
        assert!(module(&source).is_none());
    }
}
