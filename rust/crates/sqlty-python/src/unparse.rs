//! `ast.unparse` of an expression: the text the Python passes compare annotations by and
//! quote in messages. A port of CPython's `_ast_unparse.Unparser`, over the ruff tree read
//! as `ast` reads it (adjacent literal text of an f-string merged, `{x=}` spelled out).

use std::fmt::Write;
use std::sync::LazyLock;

use regex::Regex;
use ruff_python_ast::{
    BoolOp, CmpOp, Comprehension, ConversionFlag, Expr, FStringPartRef, InterpolatedElement,
    InterpolatedStringElement, InterpolatedStringElements, Keyword, Number, Operator, Parameter,
    Parameters, UnaryOp,
};
use ruff_text_size::Ranged;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Prec {
    NamedExpr,
    Tuple,
    Yield,
    Test,
    Or,
    And,
    Not,
    Cmp,
    Bor,
    Bxor,
    Band,
    Shift,
    Arith,
    Term,
    Factor,
    Power,
    Await,
    Atom,
}

impl Prec {
    fn next(self) -> Self {
        use Prec::{
            And, Arith, Atom, Await, Band, Bor, Bxor, Cmp, Factor, NamedExpr, Not, Or, Power,
            Shift, Term, Test, Tuple, Yield,
        };
        match self {
            NamedExpr => Tuple,
            Tuple => Yield,
            Yield => Test,
            Test => Or,
            Or => And,
            And => Not,
            Not => Cmp,
            Cmp => Bor,
            Bor => Bxor,
            Bxor => Band,
            Band => Shift,
            Shift => Arith,
            Arith => Term,
            Term => Factor,
            Factor => Power,
            Power => Await,
            Await | Atom => Atom,
        }
    }
}

/// `ast.unparse(expr)`, for an expression of `source`.
pub fn unparse(expr: &Expr, source: &str) -> String {
    let mut unparser = Unparser {
        out: String::new(),
        source,
        names: None,
    };
    unparser.expr(expr, Prec::Test);
    unparser.out
}

/// The expressions names are replaced with, each an expression of its own source.
pub type Substitutions<'s> = std::collections::HashMap<&'s str, (&'s Expr, &'s str)>;

/// [`unparse`] of `expr` with each name of `names` replaced by its expression, as an
/// `ast.NodeTransformer` that swaps `Name` nodes would have it.
pub fn unparse_with(expr: &Expr, source: &str, names: &Substitutions<'_>) -> String {
    let mut unparser = Unparser {
        out: String::new(),
        source,
        names: Some(names),
    };
    unparser.expr(expr, Prec::Test);
    unparser.out
}

struct Unparser<'s> {
    out: String,
    source: &'s str,
    names: Option<&'s Substitutions<'s>>,
}

/// A value of an f- or t-string as `ast` holds it.
enum Part<'a> {
    Text(String),
    Field(&'a InterpolatedElement, ConversionFlag),
}

const QUOTES: [&str; 4] = ["'", "\"", "\"\"\"", "'''"];

impl Unparser<'_> {
    fn sub(&self, expr: &Expr, prec: Prec) -> String {
        let mut unparser = Unparser {
            out: String::new(),
            source: self.source,
            names: self.names,
        };
        unparser.expr(expr, prec);
        unparser.out
    }

    fn parens(&mut self, needed: bool, body: impl FnOnce(&mut Self)) {
        if needed {
            self.out.push('(');
        }
        body(self);
        if needed {
            self.out.push(')');
        }
    }

    fn list(&mut self, items: &[Expr]) {
        for (i, item) in items.iter().enumerate() {
            if i > 0 {
                self.out.push_str(", ");
            }
            self.expr(item, Prec::Test);
        }
    }

    /// `items_view`: a lone item keeps a trailing comma.
    fn items(&mut self, items: &[Expr]) {
        self.list(items);
        if items.len() == 1 {
            self.out.push(',');
        }
    }

    fn comprehensions(&mut self, generators: &[Comprehension]) {
        for generator in generators {
            self.out.push_str(if generator.is_async {
                " async for "
            } else {
                " for "
            });
            self.expr(&generator.target, Prec::Tuple);
            self.out.push_str(" in ");
            self.expr(&generator.iter, Prec::Test.next());
            for test in &generator.ifs {
                self.out.push_str(" if ");
                self.expr(test, Prec::Test.next());
            }
        }
    }

    #[recursive::recursive]
    #[allow(clippy::too_many_lines)]
    fn expr(&mut self, expr: &Expr, prec: Prec) {
        match expr {
            Expr::BoolOp(e) => {
                let (word, mut level) = match e.op {
                    BoolOp::And => (" and ", Prec::And),
                    BoolOp::Or => (" or ", Prec::Or),
                };
                self.parens(prec > level, |this| {
                    for (i, value) in e.values.iter().enumerate() {
                        if i > 0 {
                            this.out.push_str(word);
                        }
                        level = level.next();
                        this.expr(value, level);
                    }
                });
            }
            Expr::Named(e) => self.parens(prec > Prec::NamedExpr, |this| {
                this.expr(&e.target, Prec::Atom);
                this.out.push_str(" := ");
                this.expr(&e.value, Prec::Atom);
            }),
            Expr::BinOp(e) => {
                let (word, level) = match e.op {
                    Operator::Add => ("+", Prec::Arith),
                    Operator::Sub => ("-", Prec::Arith),
                    Operator::Mult => ("*", Prec::Term),
                    Operator::MatMult => ("@", Prec::Term),
                    Operator::Div => ("/", Prec::Term),
                    Operator::Mod => ("%", Prec::Term),
                    Operator::FloorDiv => ("//", Prec::Term),
                    Operator::LShift => ("<<", Prec::Shift),
                    Operator::RShift => (">>", Prec::Shift),
                    Operator::BitOr => ("|", Prec::Bor),
                    Operator::BitXor => ("^", Prec::Bxor),
                    Operator::BitAnd => ("&", Prec::Band),
                    Operator::Pow => ("**", Prec::Power),
                };
                let (left, right) = if e.op == Operator::Pow {
                    (level.next(), level)
                } else {
                    (level, level.next())
                };
                self.parens(prec > level, |this| {
                    this.expr(&e.left, left);
                    let _ = write!(this.out, " {word} ");
                    this.expr(&e.right, right);
                });
            }
            Expr::UnaryOp(e) => {
                let (word, level) = match e.op {
                    UnaryOp::Invert => ("~", Prec::Factor),
                    UnaryOp::Not => ("not ", Prec::Not),
                    UnaryOp::UAdd => ("+", Prec::Factor),
                    UnaryOp::USub => ("-", Prec::Factor),
                };
                self.parens(prec > level, |this| {
                    this.out.push_str(word);
                    this.expr(&e.operand, level);
                });
            }
            Expr::Lambda(e) => self.parens(prec > Prec::Test, |this| {
                this.out.push_str("lambda");
                let parameters = e
                    .parameters
                    .as_deref()
                    .map(|p| this.parameters(p))
                    .unwrap_or_default();
                if !parameters.is_empty() {
                    this.out.push(' ');
                    this.out.push_str(&parameters);
                }
                this.out.push_str(": ");
                this.expr(&e.body, Prec::Test);
            }),
            Expr::If(e) => self.parens(prec > Prec::Test, |this| {
                this.expr(&e.body, Prec::Test.next());
                this.out.push_str(" if ");
                this.expr(&e.test, Prec::Test.next());
                this.out.push_str(" else ");
                this.expr(&e.orelse, Prec::Test);
            }),
            Expr::Dict(e) => {
                self.out.push('{');
                for (i, item) in e.items.iter().enumerate() {
                    if i > 0 {
                        self.out.push_str(", ");
                    }
                    match &item.key {
                        Some(key) => {
                            self.expr(key, Prec::Test);
                            self.out.push_str(": ");
                            self.expr(&item.value, Prec::Test);
                        }
                        None => {
                            self.out.push_str("**");
                            self.expr(&item.value, Prec::Bor);
                        }
                    }
                }
                self.out.push('}');
            }
            Expr::Set(e) => {
                if e.elts.is_empty() {
                    self.out.push_str("{*()}");
                } else {
                    self.out.push('{');
                    self.list(&e.elts);
                    self.out.push('}');
                }
            }
            Expr::ListComp(e) => {
                self.out.push('[');
                self.expr(&e.elt, Prec::Test);
                self.comprehensions(&e.generators);
                self.out.push(']');
            }
            Expr::SetComp(e) => {
                self.out.push('{');
                self.expr(&e.elt, Prec::Test);
                self.comprehensions(&e.generators);
                self.out.push('}');
            }
            Expr::DictComp(e) => {
                self.out.push('{');
                match e.key.as_deref() {
                    Some(key) => {
                        self.expr(key, Prec::Test);
                        self.out.push_str(": ");
                        self.expr(&e.value, Prec::Test);
                    }
                    None => {
                        self.out.push_str("**");
                        self.expr(&e.value, Prec::Test);
                    }
                }
                self.comprehensions(&e.generators);
                self.out.push('}');
            }
            Expr::Generator(e) => {
                self.out.push('(');
                self.expr(&e.elt, Prec::Test);
                self.comprehensions(&e.generators);
                self.out.push(')');
            }
            Expr::Await(e) => self.parens(prec > Prec::Await, |this| {
                this.out.push_str("await ");
                this.expr(&e.value, Prec::Atom);
            }),
            Expr::Yield(e) => self.parens(prec > Prec::Yield, |this| {
                this.out.push_str("yield");
                if let Some(value) = &e.value {
                    this.out.push(' ');
                    this.expr(value, Prec::Atom);
                }
            }),
            Expr::YieldFrom(e) => self.parens(prec > Prec::Yield, |this| {
                this.out.push_str("yield from ");
                this.expr(&e.value, Prec::Atom);
            }),
            Expr::Compare(e) => self.parens(prec > Prec::Cmp, |this| {
                let mut operands = e.operands.iter();
                if let Some(left) = operands.next() {
                    this.expr(left, Prec::Cmp.next());
                }
                for (op, operand) in e.ops.iter().zip(operands) {
                    let word = match op {
                        CmpOp::Eq => "==",
                        CmpOp::NotEq => "!=",
                        CmpOp::Lt => "<",
                        CmpOp::LtE => "<=",
                        CmpOp::Gt => ">",
                        CmpOp::GtE => ">=",
                        CmpOp::Is => "is",
                        CmpOp::IsNot => "is not",
                        CmpOp::In => "in",
                        CmpOp::NotIn => "not in",
                    };
                    let _ = write!(this.out, " {word} ");
                    this.expr(operand, Prec::Cmp.next());
                }
            }),
            Expr::Call(e) => {
                self.expr(&e.func, Prec::Atom);
                self.out.push('(');
                self.list(&e.arguments.args);
                for (i, keyword) in e.arguments.keywords.iter().enumerate() {
                    if i > 0 || !e.arguments.args.is_empty() {
                        self.out.push_str(", ");
                    }
                    self.keyword(keyword);
                }
                self.out.push(')');
            }
            Expr::FString(e) => {
                let mut parts = Vec::new();
                for part in &e.value {
                    match part {
                        FStringPartRef::Literal(literal) => push_text(&mut parts, &literal.value),
                        FStringPartRef::FString(fstring) => fields(&fstring.elements, &mut parts),
                    }
                }
                self.out.push('f');
                self.interpolated(&parts, false);
            }
            Expr::TString(e) => {
                let mut parts = Vec::new();
                for part in e.value.as_slice() {
                    fields(&part.elements, &mut parts);
                }
                self.out.push('t');
                self.interpolated(&parts, true);
            }
            Expr::StringLiteral(e) => {
                if e.value.is_unicode() {
                    self.out.push('u');
                }
                self.out.push_str(&repr_str(e.value.to_str()));
            }
            Expr::BytesLiteral(e) => {
                let bytes: Vec<u8> = e.value.bytes().collect();
                self.out.push_str(&repr_bytes(&bytes));
            }
            Expr::NumberLiteral(e) => match &e.value {
                Number::Int(int) => match int.as_u64() {
                    Some(small) => {
                        let _ = write!(self.out, "{small}");
                    }
                    None => self.out.push_str(&decimal(&self.source[e.range()])),
                },
                Number::Float(value) => self.out.push_str(&repr_float(*value)),
                Number::Complex { imag, .. } => {
                    let text = repr_float(*imag);
                    self.out.push_str(text.strip_suffix(".0").unwrap_or(&text));
                    self.out.push('j');
                }
            },
            Expr::BooleanLiteral(e) => self.out.push_str(if e.value { "True" } else { "False" }),
            Expr::NoneLiteral(_) => self.out.push_str("None"),
            Expr::EllipsisLiteral(_) => self.out.push_str("..."),
            Expr::Attribute(e) => {
                self.expr(&e.value, Prec::Atom);
                if matches!(e.value.as_ref(), Expr::NumberLiteral(n) if n.value.is_int())
                    || e.value.is_boolean_literal_expr()
                {
                    self.out.push(' ');
                }
                self.out.push('.');
                self.out.push_str(e.attr.as_str());
            }
            Expr::Subscript(e) => {
                self.expr(&e.value, Prec::Atom);
                self.out.push('[');
                match e.slice.as_ref() {
                    Expr::Tuple(tuple) if !tuple.elts.is_empty() => self.items(&tuple.elts),
                    slice => self.expr(slice, Prec::Test),
                }
                self.out.push(']');
            }
            Expr::Starred(e) => {
                self.out.push('*');
                self.expr(&e.value, Prec::Bor);
            }
            Expr::Name(e) => match self.names.and_then(|names| names.get(e.id.as_str())) {
                Some((replacement, source)) => {
                    let mut unparser = Unparser {
                        out: String::new(),
                        source,
                        names: None,
                    };
                    unparser.expr(replacement, prec);
                    self.out.push_str(&unparser.out);
                }
                None => self.out.push_str(e.id.as_str()),
            },
            Expr::List(e) => {
                self.out.push('[');
                self.list(&e.elts);
                self.out.push(']');
            }
            Expr::Tuple(e) => self.parens(e.elts.is_empty() || prec > Prec::Tuple, |this| {
                this.items(&e.elts);
            }),
            Expr::Slice(e) => {
                if let Some(lower) = &e.lower {
                    self.expr(lower, Prec::Test);
                }
                self.out.push(':');
                if let Some(upper) = &e.upper {
                    self.expr(upper, Prec::Test);
                }
                if let Some(step) = &e.step {
                    self.out.push(':');
                    self.expr(step, Prec::Test);
                }
            }
            Expr::IpyEscapeCommand(_) => {}
        }
    }

    fn keyword(&mut self, keyword: &Keyword) {
        match &keyword.arg {
            Some(name) => {
                self.out.push_str(name.as_str());
                self.out.push('=');
            }
            None => self.out.push_str("**"),
        }
        self.expr(&keyword.value, Prec::Test);
    }

    fn parameter(&self, parameter: &Parameter) -> String {
        match &parameter.annotation {
            Some(annotation) => format!(
                "{}: {}",
                parameter.name.as_str(),
                self.sub(annotation, Prec::Test)
            ),
            None => parameter.name.to_string(),
        }
    }

    fn parameters(&self, parameters: &Parameters) -> String {
        let mut out: Vec<String> = Vec::new();
        let positional = parameters.posonlyargs.len();
        for (index, p) in parameters
            .posonlyargs
            .iter()
            .chain(&parameters.args)
            .enumerate()
        {
            let mut text = self.parameter(&p.parameter);
            if let Some(default) = &p.default {
                text.push('=');
                text.push_str(&self.sub(default, Prec::Test));
            }
            out.push(text);
            if index + 1 == positional {
                out.push("/".to_string());
            }
        }
        if parameters.vararg.is_some() || !parameters.kwonlyargs.is_empty() {
            let name = parameters
                .vararg
                .as_deref()
                .map(|p| self.parameter(p))
                .unwrap_or_default();
            out.push(format!("*{name}"));
        }
        for p in &parameters.kwonlyargs {
            let mut text = self.parameter(&p.parameter);
            if let Some(default) = &p.default {
                text.push('=');
                text.push_str(&self.sub(default, Prec::Test));
            }
            out.push(text);
        }
        if let Some(kwarg) = &parameters.kwarg {
            out.push(format!("**{}", self.parameter(kwarg)));
        }
        out.join(", ")
    }

    /// `_write_ftstring`: each value written apart, then quoted together.
    fn interpolated(&mut self, parts: &[Part<'_>], template: bool) {
        let written: Vec<(String, bool)> = parts
            .iter()
            .map(|part| match part {
                Part::Text(text) => (text.replace('{', "{{").replace('}', "}}"), true),
                Part::Field(field, conversion) => (self.field(field, *conversion, template), false),
            })
            .collect();
        let mut quotes: Vec<&str> = QUOTES.to_vec();
        let mut out = Vec::new();
        let mut fallback = false;
        for (value, constant) in &written {
            if *constant {
                let (value, possible) = str_literal(value, &quotes, true);
                if !possible.iter().any(|q| quotes.contains(q)) {
                    fallback = true;
                    break;
                }
                quotes = possible;
                out.push(value);
            } else {
                if value.contains('\n') {
                    quotes.retain(|q| q.len() == 3);
                }
                let fitting: Vec<&str> = quotes
                    .iter()
                    .copied()
                    .filter(|q| !value.contains(q))
                    .collect();
                if !fitting.is_empty() {
                    quotes = fitting;
                }
                out.push(value.clone());
            }
        }
        if fallback {
            quotes = vec!["'''"];
            out = written
                .iter()
                .map(|(value, constant)| {
                    if *constant {
                        let quoted = repr_str(&format!("\"{value}"));
                        quoted[2..quoted.len() - 1].to_string()
                    } else {
                        value.clone()
                    }
                })
                .collect();
        }
        let quote = quotes[0];
        self.out.push_str(quote);
        for value in out {
            self.out.push_str(&value);
        }
        self.out.push_str(quote);
    }

    fn field(
        &self,
        field: &InterpolatedElement,
        conversion: ConversionFlag,
        template: bool,
    ) -> String {
        let mut out = String::from("{");
        let text = if template {
            interpolation_text(field, self.source)
        } else {
            self.sub(&field.expression, Prec::Test.next())
        };
        if text.starts_with('{') {
            out.push(' ');
        }
        out.push_str(&text);
        match conversion {
            ConversionFlag::None => {}
            ConversionFlag::Str => out.push_str("!s"),
            ConversionFlag::Ascii => out.push_str("!a"),
            ConversionFlag::Repr => out.push_str("!r"),
        }
        if let Some(spec) = &field.format_spec {
            out.push(':');
            let mut parts = Vec::new();
            fields(&spec.elements, &mut parts);
            for part in &parts {
                match part {
                    Part::Text(text) => out.push_str(
                        &text
                            .replace('{', "{{")
                            .replace('}', "}}")
                            .replace('\\', "\\\\")
                            .replace('\'', "\\'")
                            .replace('"', "\\\"")
                            .replace('\n', "\\n"),
                    ),
                    Part::Field(inner, conversion) => {
                        out.push_str(&self.field(inner, *conversion, false));
                    }
                }
            }
        }
        out.push('}');
        out
    }
}

/// `Interpolation.str`: the source of the expression of a t-string field, from the `{` to
/// the `=`, `!`, `:` or `}` after it, without trailing whitespace.
pub fn interpolation_text(field: &InterpolatedElement, source: &str) -> String {
    if let Some(debug) = &field.debug_text {
        return format!("{}{}", debug.leading(), debug.expression());
    }
    let start = field.start().to_usize() + 1;
    let stop = match field.format_spec.as_deref() {
        Some(spec) => spec.start().to_usize() - 1,
        None => field.end().to_usize() - 1,
    };
    let mut text = source[start..stop].trim_end();
    if field.conversion != ConversionFlag::None {
        text = text[..text.len().saturating_sub(2)].trim_end();
    }
    text.to_string()
}

fn push_text(parts: &mut Vec<Part<'_>>, text: &str) {
    if text.is_empty() {
        return;
    }
    match parts.last_mut() {
        Some(Part::Text(last)) => last.push_str(text),
        _ => parts.push(Part::Text(text.to_string())),
    }
}

/// The values of an f- or t-string as `ast` holds them: adjacent text merged, `{x=}` as the
/// text `x=` and a field converted with `!r` unless it has a conversion or a spec.
fn fields<'a>(elements: &'a InterpolatedStringElements, parts: &mut Vec<Part<'a>>) {
    for element in elements {
        match element {
            InterpolatedStringElement::Literal(literal) => push_text(parts, &literal.value),
            InterpolatedStringElement::Interpolation(field) => {
                let mut conversion = field.conversion;
                if let Some(debug) = &field.debug_text {
                    push_text(parts, debug.as_str());
                    if conversion == ConversionFlag::None && field.format_spec.is_none() {
                        conversion = ConversionFlag::Repr;
                    }
                }
                parts.push(Part::Field(field, conversion));
            }
        }
    }
}

static UNPRINTABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[\p{Cc}\p{Cf}\p{Co}\p{Cn}\p{Zl}\p{Zp}\p{Zs}]").expect("a valid pattern")
});

/// `str.isprintable` of one character.
fn printable(c: char) -> bool {
    c == ' ' || (c.is_ascii() && !c.is_ascii_control()) || {
        let mut buffer = [0u8; 4];
        !c.is_ascii() && !UNPRINTABLE.is_match(c.encode_utf8(&mut buffer))
    }
}

/// `c.encode("unicode_escape")`.
fn unicode_escape(c: char, out: &mut String) {
    match c {
        '\\' => out.push_str("\\\\"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        c if (c as u32) < 0x100 => {
            let _ = write!(out, "\\x{:02x}", c as u32);
        }
        c if (c as u32) < 0x10000 => {
            let _ = write!(out, "\\u{:04x}", c as u32);
        }
        c => {
            let _ = write!(out, "\\U{:08x}", c as u32);
        }
    }
}

/// `_str_literal_helper`: the escaped text and the quotes that can hold it.
fn str_literal<'q>(text: &str, quotes: &[&'q str], whitespace: bool) -> (String, Vec<&'q str>) {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        if !whitespace && (c == '\n' || c == '\t') {
            escaped.push(c);
        } else if c == '\\' || !printable(c) {
            unicode_escape(c, &mut escaped);
        } else {
            escaped.push(c);
        }
    }
    let mut possible: Vec<&str> = quotes.to_vec();
    if escaped.contains('\n') {
        possible.retain(|q| q.len() == 3);
    }
    possible.retain(|q| !escaped.contains(q));
    if possible.is_empty() {
        let quoted = repr_str(text);
        let first = &quoted[..1];
        let quote = quotes
            .iter()
            .copied()
            .find(|q| q.contains(first))
            .unwrap_or(if first == "'" { "'" } else { "\"" });
        return (quoted[1..quoted.len() - 1].to_string(), vec![quote]);
    }
    if let Some(last) = escaped.chars().last() {
        possible.sort_by_key(|q| q.starts_with(last));
        if possible[0].starts_with(last) {
            escaped.pop();
            escaped.push('\\');
            escaped.push(last);
        }
    }
    (escaped, possible)
}

/// `repr` of a `str`.
pub fn repr_str(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for c in text.chars() {
        if c == quote {
            out.push('\\');
            out.push(c);
        } else if c == '\\' || !printable(c) {
            unicode_escape(c, &mut out);
        } else {
            out.push(c);
        }
    }
    out.push(quote);
    out
}

/// `repr` of a `bytes`.
pub fn repr_bytes(bytes: &[u8]) -> String {
    let quote = if bytes.contains(&b'\'') && !bytes.contains(&b'"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from("b");
    out.push(quote);
    for &byte in bytes {
        match byte {
            b'\\' => out.push_str("\\\\"),
            b'\t' => out.push_str("\\t"),
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            byte if byte as char == quote => {
                out.push('\\');
                out.push(quote);
            }
            0x20..=0x7e => out.push(byte as char),
            byte => {
                let _ = write!(out, "\\x{byte:02x}");
            }
        }
    }
    out.push(quote);
    out
}

/// `repr` of a `float`, an infinity as the literal `ast.unparse` writes for it.
pub fn repr_float(value: f64) -> String {
    if value.is_infinite() {
        return if value > 0.0 { "1e309" } else { "-1e309" }.to_string();
    }
    if value.is_nan() {
        return "(1e309-1e309)".to_string();
    }
    let scientific = format!("{value:e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if (-4..16).contains(&exponent) {
        let text = format!("{value}");
        return if text.contains('.') {
            text
        } else {
            format!("{text}.0")
        };
    }
    let sign = if exponent < 0 { '-' } else { '+' };
    format!("{mantissa}e{sign}{:02}", exponent.abs())
}

/// The decimal spelling of an integer literal of any size and base.
fn decimal(literal: &str) -> String {
    let text: String = literal.chars().filter(|c| *c != '_').collect();
    let lower = text.to_ascii_lowercase();
    let (radix, digits) = match lower.get(..2) {
        Some("0x") => (16, &lower[2..]),
        Some("0o") => (8, &lower[2..]),
        Some("0b") => (2, &lower[2..]),
        _ => (10, lower.as_str()),
    };
    // Base 10^9 limbs, least significant first.
    let mut limbs: Vec<u64> = vec![0];
    for digit in digits.chars().filter_map(|c| c.to_digit(radix)) {
        let mut carry = u64::from(digit);
        for limb in &mut limbs {
            let value = *limb * u64::from(radix) + carry;
            *limb = value % 1_000_000_000;
            carry = value / 1_000_000_000;
        }
        if carry > 0 {
            limbs.push(carry);
        }
    }
    let mut out = String::new();
    for (i, limb) in limbs.iter().rev().enumerate() {
        if i == 0 {
            let _ = write!(out, "{limb}");
        } else {
            let _ = write!(out, "{limb:09}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round(source: &str) -> String {
        let parsed = crate::parse::expression(source).unwrap();
        unparse(&parsed.syntax().body, source)
    }

    #[test]
    fn expressions() {
        for (source, expected) in [
            ("dict[str,list[int|None]]", "dict[str, list[int | None]]"),
            ("Literal[\"a\",'b\"',1]", "Literal['a', 'b\"', 1]"),
            ("(a,)", "(a,)"),
            ("x[(a,b)]", "x[a, b]"),
            ("x[()]", "x[()]"),
            ("-(-a)**-b", "-(-a) ** (-b)"),
            ("(a+b)*c-d", "(a + b) * c - d"),
            ("not a and (b or c)", "not a and (b or c)"),
            ("a if b else(c if d else e)", "a if b else c if d else e"),
            (
                "lambda a,/,b=1,*c,d,**e:0",
                "lambda a, /, b=1, *c, d, **e: 0",
            ),
            ("f(x for x in y)", "f((x for x in y))"),
            ("{**a,'b':1}", "{**a, 'b': 1}"),
            (
                "1e100+1.5+1e16+1e-5+2j+0x10",
                "1e+100 + 1.5 + 1e+16 + 1e-05 + 2j + 16",
            ),
            ("0xffffffffffffffffffff", "1208925819614629174706175"),
            ("3 .real", "3 .real"),
            ("f'{a!r:>{w}} {b=}'", "f'{a!r:>{w}} b={b!r}'"),
            ("b'a\\x00\\''", "b\"a\\x00'\""),
            ("'a\\n\\u00e9\\x7f'", "'a\\né\\x7f'"),
        ] {
            assert_eq!(round(source), expected, "{source}");
        }
    }
}
