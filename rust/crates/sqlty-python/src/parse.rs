//! `ruff_python_parser`, held to what Python's `ast.parse` accepts.
//!
//! Each function returns `None` where `ast.parse` rejects the source or the two parsers
//! may read it differently, so the caller falls back to Python:
//!
//! - a byte order mark, a NUL character, or a `\ud800`-`\udfff` escape (ruff decodes a
//!   surrogate to U+FFFD, Python keeps it);
//! - a `from __future__` import Python does not know (`braces`) or one that changes the
//!   grammar (`barry_as_FLUFL`), in the leading block where `ast.parse` checks them;
//! - more than 200 open brackets or 100 indentation levels (CPython's tokenizer limits);
//! - 150 nested f- or t-strings, or a format spec nested three deep;
//! - an expression or statement nested more than [`MAX_DEPTH`] deep, an `elif` counting as
//!   one level as in `ast`: CPython gives up somewhere between 1,000 and 10,000 levels.
//!
//! The parser recurses once per nesting level and needs up to about 1 KiB of stack per
//! byte of a deeply nested source, so it runs on a stack of [`STACK_PER_BYTE`] per byte;
//! a source longer than [`MAX_LEN`] is left to Python rather than risk overflowing it.

use ruff_python_ast::token::TokenKind;
use ruff_python_ast::visitor::{walk_expr, walk_interpolated_string_element, walk_stmt, Visitor};
use ruff_python_ast::{Expr, InterpolatedStringElement, ModExpression, ModModule, Stmt};
use ruff_python_parser::{parse_expression, parse_module, Parsed};

const STACK_PER_BYTE: usize = if cfg!(debug_assertions) {
    16 << 10
} else {
    2 << 10
};
const STACK_BASE: usize = 1 << 20;
/// The longest source parsed in Rust.
pub const MAX_LEN: usize = 256 << 10;
/// The deepest nesting of expressions and statements read in Rust.
pub const MAX_DEPTH: usize = 1000;
const MAX_BRACKETS: usize = 200;
const MAX_INDENT: usize = 100;
const MAX_INTERPOLATED: usize = 150;
const MAX_FORMAT_SPEC: usize = 2;
const FUTURES: [&str; 9] = [
    "nested_scopes",
    "generators",
    "division",
    "absolute_import",
    "with_statement",
    "print_function",
    "unicode_literals",
    "generator_stop",
    "annotations",
];

/// The module `source`, when `ast.parse` reads it the same.
pub fn module(source: &str) -> Option<Parsed<ModModule>> {
    let parsed = guarded(source, parse_module)?;
    if !within_limits(&parsed) || !known_futures(&parsed.syntax().body) {
        return None;
    }
    let mut depth = Depth::default();
    depth.visit_body(&parsed.syntax().body);
    (!depth.exceeded).then_some(parsed)
}

/// The expression `source`, when `ast.parse(source, mode="eval")` reads it the same.
pub fn expression(source: &str) -> Option<Parsed<ModExpression>> {
    let parsed = guarded(source, parse_expression)?;
    if !within_limits(&parsed) {
        return None;
    }
    let mut depth = Depth::default();
    depth.visit_expr(&parsed.syntax().body);
    (!depth.exceeded).then_some(parsed)
}

fn guarded<T: Send, E: Send>(
    source: &str,
    parse: impl FnOnce(&str) -> Result<T, E> + Send,
) -> Option<T> {
    if source.len() > MAX_LEN
        || source.starts_with('\u{feff}')
        || source.contains('\0')
        || has_surrogate_escape(source)
    {
        return None;
    }
    let stack = STACK_BASE + source.len() * STACK_PER_BYTE;
    stacker::maybe_grow(stack, stack, || parse(source)).ok()
}

/// Whether `source` may spell a surrogate: `\ud800` to `\udfff`, or `\U0000d800` to `\U0000dfff`.
fn has_surrogate_escape(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut i = 0;
    while let Some(found) = bytes[i..].iter().position(|&b| b == b'\\') {
        let at = i + found;
        let hex = match bytes.get(at + 1) {
            Some(b'u') => bytes.get(at + 2..),
            Some(b'U') => bytes
                .get(at + 2..at + 6)
                .filter(|z| *z == b"0000")
                .and(bytes.get(at + 6..)),
            _ => None,
        };
        if hex.is_some_and(surrogate) {
            return true;
        }
        i = at + 2;
        if i >= bytes.len() {
            break;
        }
    }
    false
}

fn surrogate(hex: &[u8]) -> bool {
    matches!(
        hex,
        [b'd' | b'D', b'8'..=b'9' | b'a'..=b'f' | b'A'..=b'F', ..]
    )
}

fn within_limits<T>(parsed: &Parsed<T>) -> bool {
    let (mut brackets, mut indent) = (0usize, 0usize);
    for token in parsed.tokens() {
        match token.kind() {
            TokenKind::Lpar | TokenKind::Lsqb | TokenKind::Lbrace => {
                brackets += 1;
                if brackets > MAX_BRACKETS {
                    return false;
                }
            }
            TokenKind::Rpar | TokenKind::Rsqb | TokenKind::Rbrace => {
                brackets = brackets.saturating_sub(1);
            }
            TokenKind::Indent => {
                indent += 1;
                if indent >= MAX_INDENT {
                    return false;
                }
            }
            TokenKind::Dedent => indent = indent.saturating_sub(1),
            _ => {}
        }
    }
    true
}

/// Whether the leading `from __future__` imports, which `ast.parse` checks, are ones Python knows.
fn known_futures(body: &[Stmt]) -> bool {
    let mut rest = body;
    if let [Stmt::Expr(expr), tail @ ..] = rest {
        if expr.value.is_string_literal_expr() {
            rest = tail;
        }
    }
    for stmt in rest {
        let Stmt::ImportFrom(from) = stmt else {
            break;
        };
        if from.level != 0
            || from
                .module
                .as_ref()
                .is_none_or(|m| m.as_str() != "__future__")
        {
            break;
        }
        if !from
            .names
            .iter()
            .all(|alias| FUTURES.contains(&alias.name.as_str()))
        {
            return false;
        }
    }
    true
}

#[derive(Default)]
struct Depth {
    depth: usize,
    interpolated: usize,
    spec: usize,
    exceeded: bool,
}

impl Depth {
    fn enter(&mut self) -> bool {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            self.exceeded = true;
        }
        !self.exceeded
    }
}

impl<'a> Visitor<'a> for Depth {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        // `ast` nests each `elif` in the `orelse` of the one before it.
        let elifs = match stmt {
            Stmt::If(node) => node.elif_else_clauses.len(),
            _ => 0,
        };
        self.depth += elifs;
        if self.enter() {
            walk_stmt(self, stmt);
        }
        self.depth -= 1 + elifs;
    }

    #[recursive::recursive]
    fn visit_expr(&mut self, expr: &'a Expr) {
        let interpolated = matches!(expr, Expr::FString(_) | Expr::TString(_));
        if interpolated {
            self.interpolated += 1;
            if self.interpolated >= MAX_INTERPOLATED {
                self.exceeded = true;
            }
        }
        if self.enter() {
            walk_expr(self, expr);
        }
        self.depth -= 1;
        if interpolated {
            self.interpolated -= 1;
        }
    }

    fn visit_interpolated_string_element(&mut self, element: &'a InterpolatedStringElement) {
        let spec = matches!(element, InterpolatedStringElement::Interpolation(field) if field.format_spec.is_some());
        if spec {
            self.spec += 1;
            if self.spec > MAX_FORMAT_SPEC {
                self.exceeded = true;
            }
        }
        if !self.exceeded {
            walk_interpolated_string_element(self, element);
        }
        if spec {
            self.spec -= 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn deep_nesting_does_not_overflow() {
        for source in [
            format!("x = {}1{}", "(".repeat(5_000), ")".repeat(5_000)),
            format!("x = {}1", "-".repeat(20_000)),
            format!("x = {}1", "lambda: ".repeat(3_000)),
        ] {
            assert!(module(&source).is_none());
        }
    }
}
