//! The overloads of the built-in function a cursor is in a call of, for signature help.
//!
//! ```
//! use sqlty_sql::analyzer::signature_help;
//!
//! let sql = "SELECT left(name, 3) FROM users";
//! let help = signature_help(sql, sql.find('3').unwrap(), "postgres", None).unwrap();
//! let active = &help.signatures[help.active_signature];
//! assert_eq!(active.label, "left(string: text, n: integer) -> text");
//! assert_eq!(active.parameter_label(help.active_parameter.unwrap()), Some("n: integer"));
//! assert_eq!(&sql[help.name], "left");
//! ```

use std::ops::Range;

use super::complete::{lex, Tok};
use super::{SchemaData, SqlDialect};
use crate::signatures::{signatures, Signature};

/// The call a cursor is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureHelp {
    /// The overloads of the function.
    pub signatures: &'static [Signature],
    /// The first overload that takes as many arguments as the call has up to the cursor.
    pub active_signature: usize,
    /// The parameter of that overload the cursor's argument binds to.
    pub active_parameter: Option<usize>,
    /// The bytes of the function's name.
    pub name: Range<usize>,
}

/// Words a name before `(` follows when it names a relation or its columns, not a call:
/// `INSERT INTO t (a)`, `WITH t (a) AS`, `AS t (a)`, `CREATE TABLE IF NOT EXISTS t (a)`.
const NOT_A_CALL: [&str; 8] = [
    "INTO",
    "TABLE",
    "REFERENCES",
    "AS",
    "WITH",
    "EXISTS",
    "COPY",
    "VIEW",
];

/// An open call: its name's token, the commas read and where the current argument starts.
struct Call {
    name: usize,
    commas: usize,
    argument: usize,
    /// Whether an earlier argument was passed by name (`days => 1`).
    named: bool,
}

/// The built-in function whose argument list holds byte `cursor` of `query`, in the
/// dialect `schema` routes it to; the innermost call; `None` outside one, in grouping
/// parentheses, in a `[...]` or `{...}` literal, or in a `'...'` string or a comment. An argument
/// passed by name (`days => 1`) makes that parameter active, and no parameter is active after one. O(length
/// of the query) to read the context, then O(log n) in the size of the catalog.
pub fn signature_help(
    query: &str,
    cursor: usize,
    dialect: &str,
    schema: Option<&SchemaData>,
) -> Option<SignatureHelp> {
    let dialect = SqlDialect::from_name(super::route(query, dialect, schema).0);
    let tokens = lex(query.get(..cursor)?)?;
    let mut open: Vec<Option<Call>> = Vec::new();
    for (at, tok) in tokens.iter().enumerate() {
        match tok.text.as_str() {
            "(" => open.push(called(&tokens, at)),
            "[" | "{" => open.push(None),
            ")" | "]" | "}" => {
                open.pop();
            }
            "," => {
                if let Some(Some(call)) = open.last_mut() {
                    call.named |= named(&tokens[call.argument..at]).is_some();
                    call.commas += 1;
                    call.argument = at + 1;
                }
            }
            _ => {}
        }
    }
    let call = open.pop()??;
    let word = &tokens[call.name];
    let found = signatures(dialect, unquoted(&word.text)?);
    if found.is_empty() {
        return None;
    }
    let count = call.commas + 1;
    let active = found
        .iter()
        .position(|s| s.accepts(count))
        .or_else(|| {
            found
                .iter()
                .position(|s| s.variadic || s.parameters.len() >= count)
        })
        .unwrap_or(0);
    let signature = &found[active];
    let active_parameter = match named(&tokens[call.argument..]) {
        Some(name) => signature
            .parameters
            .iter()
            .position(|p| p.name.is_some_and(|n| n.eq_ignore_ascii_case(name))),
        None if call.named => None,
        None => signature.parameter(call.commas),
    };
    Some(SignatureHelp {
        signatures: found,
        active_signature: active,
        active_parameter,
        name: word.span.clone(),
    })
}

/// The call a `(` at `at` opens; `None` for grouping parentheses. A name after one of
/// [`NOT_A_CALL`], qualified or not, is a relation's.
fn called(tokens: &[Tok], at: usize) -> Option<Call> {
    let name = at.checked_sub(1)?;
    if !tokens[name].word {
        return None;
    }
    let mut first = name;
    while first >= 2 && tokens[first - 1].text == "." && tokens[first - 2].word {
        first -= 2;
    }
    let relation = first.checked_sub(1).is_some_and(|i| {
        let t = &tokens[i];
        t.word && NOT_A_CALL.iter().any(|k| t.text.eq_ignore_ascii_case(k))
    });
    (!relation).then_some(Call {
        name,
        commas: 0,
        argument: at + 1,
        named: false,
    })
}

/// The parameter name an argument's tokens pass it by: `name => value` or `name := value`.
fn named(argument: &[Tok]) -> Option<&str> {
    match argument {
        [name, a, b, ..]
            if name.word
                && matches!((a.text.as_str(), b.text.as_str()), ("=", ">") | (":", "=")) =>
        {
            Some(name.text.as_str())
        }
        _ => None,
    }
}

/// The name a word spells: a quoted one only in lower case, as the catalog lists it.
pub(super) fn unquoted(word: &str) -> Option<&str> {
    match word.strip_prefix(['"', '`']) {
        Some(rest) => {
            let name = rest.trim_end_matches(['"', '`']);
            (!name.bytes().any(|b| b.is_ascii_uppercase())).then_some(name)
        }
        None => Some(word),
    }
}
