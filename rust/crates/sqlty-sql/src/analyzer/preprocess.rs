//! Turning a query string into statements: the `--sql` marker, dialect fixes sqlparser
//! needs, and the checks for syntax the analyzer does not read.

use std::ops::ControlFlow;
use std::sync::LazyLock;

use regex::{Captures, Regex};
use sqlparser::ast::{
    visit_expressions, visit_expressions_mut, DataType, Expr, Function, FunctionArguments,
    ObjectName, Statement, Visit, Visitor,
};
use sqlparser::keywords::Keyword;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Token, Tokenizer};

use super::dialect::SqlDialect;
use super::dml::selects_into;
use super::input::{iso_types, parser_input};

/// A `?` placeholder in group 1, or a literal, quoted name, comment or `?|`/`?&` operator to skip.
pub(super) static QMARK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"'(?:[^']|'')*'|"(?:[^"]|"")*"|--[^\n]*|(?s:/\*.*?\*/)|\?[|&]|(\?)"#).unwrap()
});

/// MySQL column types sqlparser rejects: `ZEROFILL` (which implies `UNSIGNED`),
/// `INT3`/`MIDDLEINT` before `UNSIGNED`, and the `NATIONAL` spellings of `NCHAR` and
/// `NVARCHAR`.
pub(super) static MYSQL_TYPE_FIXES: LazyLock<[(Regex, &str); 4]> = LazyLock::new(|| {
    [
        (
            Regex::new(r"(?i)\bNATIONAL\s+(?:VARCHAR|(?:CHAR|CHARACTER)\s+VARYING)\b").unwrap(),
            "NVARCHAR",
        ),
        (
            Regex::new(r"(?i)\bNATIONAL\s+(?:CHAR|CHARACTER)\b").unwrap(),
            "NCHAR",
        ),
        (
            Regex::new(r"(?i)(?:\s+(?:UN)?SIGNED)?\s+ZEROFILL\b(?:\s+(?:UN)?SIGNED\b)?").unwrap(),
            " UNSIGNED",
        ),
        (
            Regex::new(r"(?i)\b(?:INT3|MIDDLEINT)\b(\s*\(\s*\d+\s*\))?(\s+UNSIGNED\b)").unwrap(),
            "MEDIUMINT$1$2",
        ),
    ]
});

#[cfg(test)]
thread_local! {
    /// How many times [`parse`] ran sqlparser on this thread.
    pub(super) static PARSES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Parse `sql`; `None` when sqlparser fails or [`reject_unsupported`] rejects it.
pub(super) fn parse(sql: &str, dialect: SqlDialect) -> Option<Vec<Statement>> {
    #[cfg(test)]
    PARSES.with(|n| n.set(n.get() + 1));
    let stmts = Parser::parse_sql(dialect.parser_dialect().as_ref(), sql).ok()?;
    checked(stmts, sql, dialect)
}

/// `stmts` parsed from `sql`, unless [`reject_unsupported`] rejects them.
fn checked(mut stmts: Vec<Statement>, sql: &str, dialect: SqlDialect) -> Option<Vec<Statement>> {
    if reject_unsupported(&stmts, dialect) || too_deep(&stmts) {
        return None;
    }
    if dialect == SqlDialect::Postgres && names_bare_function(sql) {
        let _ = visit_expressions_mut(&mut stmts, |expr| {
            bare_function(expr);
            ControlFlow::<()>::Continue(())
        });
    }
    Some(stmts)
}

/// Whether `sql` may name one of the functions [`bare_function`] reads, so that the walk
/// is skipped for the queries that do not.
fn names_bare_function(sql: &str) -> bool {
    sql.as_bytes()
        .windows(7)
        .any(|w| w.eq_ignore_ascii_case(b"current") || w.eq_ignore_ascii_case(b"system_"))
}

/// PostgreSQL's `CURRENT_ROLE`, `CURRENT_SCHEMA`, `CURRENT_CATALOG` and `SYSTEM_USER`, reserved
/// words sqlparser reads as names, as the calls without parentheses they are.
fn bare_function(expr: &mut Expr) {
    let Expr::Identifier(ident) = expr else {
        return;
    };
    let function = [
        "current_role",
        "current_schema",
        "current_catalog",
        "system_user",
    ]
    .iter()
    .any(|name| ident.value.eq_ignore_ascii_case(name));
    if function && ident.quote_style.is_none() {
        *expr = Expr::Function(Function {
            name: ObjectName::from(vec![ident.clone()]),
            uses_odbc_syntax: false,
            parameters: FunctionArguments::None,
            args: FunctionArguments::None,
            filter: None,
            null_treatment: None,
            over: None,
            within_group: Vec::new(),
        });
    }
}

/// Parse schema DDL, with the MySQL column types sqlparser rejects respelled, and in
/// SQLite multi-word type names (`UNSIGNED BIG INT`) joined into one word.
pub(crate) fn parse_ddl(sql: &str, dialect: SqlDialect) -> Option<Vec<Statement>> {
    let sql = &iso_types(sql, dialect);
    if dialect == SqlDialect::Sqlite {
        return parse(sql, dialect).or_else(|| parse(&sqlite_type_names(sql)?, dialect));
    }
    if dialect != SqlDialect::MySql {
        return parse(sql, dialect);
    }
    let sql = MYSQL_TYPE_FIXES
        .iter()
        .fold(sql.to_string(), |sql, (pattern, replacement)| {
            pattern.replace_all(&sql, *replacement).into_owned()
        });
    parse(&sql, dialect)
}

/// A PostgreSQL `CREATE FUNCTION` without the planner hints sqlparser does not read:
/// `ROWS n`, `COST n`, `[NOT] LEAKPROOF` and `SUPPORT name`, which never change the result
/// type. sqlparser stops at an option it does not know, so a hint is only dropped there,
/// never from a type, a setting value or the body.
pub(crate) fn parse_function_without_hints(sql: &str) -> Option<Vec<Statement>> {
    let sql = iso_types(sql, SqlDialect::Postgres);
    let dialect = SqlDialect::Postgres.parser_dialect();
    let mut tokens = Tokenizer::new(dialect.as_ref(), &sql)
        .tokenize_with_location()
        .ok()?;
    loop {
        let mut parser = Parser::new(dialect.as_ref()).with_tokens_with_locations(tokens.clone());
        let stmt = parser.parse_statement().ok()?;
        if matches!(parser.peek_token_ref().token, Token::EOF | Token::SemiColon) {
            return checked(vec![stmt], &sql, SqlDialect::Postgres);
        }
        let at = parser.index();
        let mut hint =
            Parser::new(dialect.as_ref()).with_tokens_with_locations(tokens[at..].to_vec());
        let read = if hint.parse_keyword(Keyword::ROWS) || hint.parse_keyword(Keyword::COST) {
            hint.parse_number().is_ok()
        } else if hint.parse_keyword(Keyword::SUPPORT) {
            hint.parse_object_name(false).is_ok()
        } else {
            hint.parse_keywords(&[Keyword::NOT, Keyword::LEAKPROOF])
                || hint.parse_keyword(Keyword::LEAKPROOF)
        };
        if !read {
            return None;
        }
        tokens.drain(at..at + hint.index());
    }
}

/// Words that end a column's type name in SQLite: the constraints that may follow it.
const SQLITE_CONSTRAINTS: [&str; 12] = [
    "AS",
    "CHECK",
    "COLLATE",
    "CONSTRAINT",
    "DEFAULT",
    "GENERATED",
    "NOT",
    "NULL",
    "PRIMARY",
    "REFERENCES",
    "UNIQUE",
    "AUTOINCREMENT",
];

/// A `CREATE TABLE` whose multi-word column type names, which SQLite accepts and sqlparser
/// does not, are joined with `_` (`UNSIGNED BIG INT` -> `UNSIGNED_BIG_INT`); `None` when
/// nothing changes.
pub(super) fn sqlite_type_names(sql: &str) -> Option<String> {
    static COLUMN: LazyLock<Regex> = LazyLock::new(|| {
        let name = r#"(?:"[^"]*"|`[^`]*`|\[[^\]]*\]|\w+)"#;
        Regex::new(&format!(
            r"(?i)([(,]\s*{name}\s+)([A-Za-z_]\w*(?:\s+[A-Za-z_]\w*)+)"
        ))
        .unwrap()
    });
    let open = sql.find('(')?;
    let mut changed = false;
    let columns = COLUMN.replace_all(&sql[open..], |caps: &Captures| {
        let words: Vec<&str> = caps[2].split_whitespace().collect();
        let end = words
            .iter()
            .position(|word| SQLITE_CONSTRAINTS.contains(&word.to_ascii_uppercase().as_str()))
            .unwrap_or(words.len());
        let head = &caps[1];
        let column = head
            .trim_start_matches([',', '('])
            .trim()
            .to_ascii_uppercase();
        if end < 2
            || matches!(
                column.as_str(),
                "CONSTRAINT" | "FOREIGN" | "PRIMARY" | "UNIQUE"
            )
        {
            return caps[0].to_string();
        }
        changed = true;
        let rest = words[end..].join(" ");
        format!(
            "{head}{}{}{rest}",
            words[..end].join("_"),
            if rest.is_empty() { "" } else { " " }
        )
    });
    changed.then(|| format!("{}{columns}", &sql[..open]))
}

/// Parse a query (not schema DDL) through [`parser_input`]. In PostgreSQL, where
/// `?` is also a JSONB operator, `?` placeholders become `$1`, `$2`, ... only when
/// the query doesn't parse as written.
pub(super) fn parse_query(sql: &str, dialect: SqlDialect) -> Option<Vec<Statement>> {
    parse_input(parser_input(sql, dialect).text(), dialect)
}

/// [`parse_query`] of what [`parser_input`] made of a query.
pub(super) fn parse_input(sql: &str, dialect: SqlDialect) -> Option<Vec<Statement>> {
    parse(sql, dialect).or_else(|| {
        (dialect == SqlDialect::Postgres && sql.contains('?'))
            .then(|| parse(&numbered_placeholders(sql), dialect))
            .flatten()
    })
}

/// Index just past the quoted string or identifier that opens at `start`; a doubled quote
/// stays inside it.
pub(super) fn quoted_end(bytes: &[u8], start: usize) -> usize {
    let quote = bytes[start];
    let mut i = start + 1;
    while i < bytes.len() {
        if bytes[i] == quote {
            if bytes.get(i + 1) != Some(&quote) {
                return i + 1;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    bytes.len()
}

pub(super) fn numbered_placeholders(sql: &str) -> String {
    let mut n = 0;
    QMARK
        .replace_all(sql, |caps: &Captures| match caps.get(1) {
            Some(_) => {
                n += 1;
                format!("${n}")
            }
            None => caps[0].to_string(),
        })
        .into_owned()
}

/// How deep expressions may nest. sqlparser reads a chain such as `a OR b OR ...` without
/// limit, and its depth bounds the stack and time the analysis takes.
pub(super) const MAX_DEPTH: usize = 1000;

/// Whether an expression of `stmts` nests deeper than [`MAX_DEPTH`].
pub(super) fn too_deep(stmts: &[Statement]) -> bool {
    struct Depth(usize);
    impl Visitor for Depth {
        type Break = ();

        fn pre_visit_expr(&mut self, _expr: &Expr) -> ControlFlow<()> {
            self.0 += 1;
            if self.0 > MAX_DEPTH {
                ControlFlow::Break(())
            } else {
                ControlFlow::Continue(())
            }
        }

        fn post_visit_expr(&mut self, _expr: &Expr) -> ControlFlow<()> {
            self.0 -= 1;
            ControlFlow::Continue(())
        }
    }
    let mut depth = Depth(0);
    stmts.iter().any(|stmt| stmt.visit(&mut depth).is_break())
}

/// Whether `stmts` use what sqlparser accepts but the dialect does not:
/// `CAST(x AS <not a type>)`, which sqlparser re-reads as a call to a function
/// named `CAST`, and MySQL-only types such as `SET('a')` in other dialects.
pub(super) fn reject_unsupported(stmts: &[Statement], dialect: SqlDialect) -> bool {
    let foreign = |dt: &DataType| {
        dialect != SqlDialect::MySql
            && matches!(
                dt,
                DataType::Set(_) | DataType::SignedInteger | DataType::UnsignedInteger
            )
    };
    let bad_expr = |expr: &Expr| match expr {
        Expr::Function(func)
            if func.name.0.len() == 1
                && func.name.0[0]
                    .as_ident()
                    .is_some_and(|ident| ident.quote_style.is_none())
                && func.name.to_string().eq_ignore_ascii_case("CAST") =>
        {
            ControlFlow::Break(())
        }
        Expr::Cast { data_type, .. } if foreign(data_type) => ControlFlow::Break(()),
        _ => ControlFlow::Continue(()),
    };
    stmts.iter().any(|stmt| {
        visit_expressions(stmt, bad_expr).is_break()
            || matches!(stmt, Statement::CreateTable(create)
                if create.columns.iter().any(|col| foreign(&col.data_type)))
            || matches!(stmt, Statement::Query(query)
                if dialect == SqlDialect::Sqlite && selects_into(&query.body))
    })
}

pub(super) fn parse_first(sql: &str, dialect: SqlDialect) -> Option<Statement> {
    parse_query(sql, dialect)?.into_iter().next()
}
