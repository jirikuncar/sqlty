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

use super::complete::lex;
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
/// `INSERT INTO t (a)`, `WITH t (a) AS`, `AS t (a)`.
const NOT_A_CALL: [&str; 5] = ["INTO", "TABLE", "REFERENCES", "AS", "WITH"];

/// The built-in function whose argument list holds byte `cursor` of `query`, in the
/// dialect `schema` routes it to; the innermost call; `None` outside one, in grouping parentheses, or in a string or comment. O(length
/// of the query) to read the context, then O(log n) in the size of the catalog.
pub fn signature_help(
    query: &str,
    cursor: usize,
    dialect: &str,
    schema: Option<&SchemaData>,
) -> Option<SignatureHelp> {
    let dialect = SqlDialect::from_name(super::route(query, dialect, schema).0);
    let tokens = lex(query.get(..cursor)?)?;
    let mut open: Vec<Option<(usize, usize)>> = Vec::new();
    for (at, tok) in tokens.iter().enumerate() {
        match tok.text.as_str() {
            "(" => open.push(called(&tokens, at)),
            ")" => {
                open.pop();
            }
            "," => {
                if let Some(Some((_, commas))) = open.last_mut() {
                    *commas += 1;
                }
            }
            _ => {}
        }
    }
    let (name, commas) = (*open.last()?)?;
    let word = &tokens[name];
    let found = signatures(dialect, unquoted(&word.text)?);
    if found.is_empty() {
        return None;
    }
    let count = commas + 1;
    let active = found
        .iter()
        .position(|s| s.accepts(count))
        .or_else(|| {
            found
                .iter()
                .position(|s| s.variadic || s.parameters.len() >= count)
        })
        .unwrap_or(0);
    Some(SignatureHelp {
        signatures: found,
        active_signature: active,
        active_parameter: found[active].parameter(commas),
        name: word.span.clone(),
    })
}

/// The name a `(` at `at` calls, with no comma read yet; `None` for grouping parentheses.
fn called(tokens: &[super::complete::Tok], at: usize) -> Option<(usize, usize)> {
    let name = at.checked_sub(1)?;
    let word = &tokens[name];
    let before = name.checked_sub(1).map(|i| &tokens[i]);
    let relation =
        before.is_some_and(|t| t.word && NOT_A_CALL.iter().any(|k| t.text.eq_ignore_ascii_case(k)));
    (word.word && !relation).then_some((name, 0))
}

/// The name a word spells: a quoted one only in lower case, as the catalog lists it.
fn unquoted(word: &str) -> Option<&str> {
    match word.strip_prefix(['"', '`']) {
        Some(rest) => {
            let name = rest.trim_end_matches(['"', '`']);
            (!name.bytes().any(|b| b.is_ascii_uppercase())).then_some(name)
        }
        None => Some(word),
    }
}
