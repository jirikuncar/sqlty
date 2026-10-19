//! Turning a query string into statements: the `--sql` marker, dialect fixes sqlparser
//! needs, and the checks for syntax the analyzer does not read.

use std::borrow::Cow;
use std::ops::ControlFlow;
use std::sync::LazyLock;

use regex::{Captures, Regex};
use sqlparser::ast::{
    visit_expressions, visit_expressions_mut, DataType, Expr, Function, FunctionArguments,
    ObjectName, Statement,
};
use sqlparser::parser::Parser;

use super::dialect::SqlDialect;
use super::star_expand::is_word;

/// A leading `--sql` marker line: MySQL only reads `--` followed by whitespace as a comment.
pub(super) static SQL_MARKER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A\s*--sql[^\n]*\n?").unwrap());

/// PostgreSQL syntax sqlparser cannot read, respelled as the closest it can. Of row-lock
/// clauses, `NO KEY UPDATE` becomes `UPDATE`, `KEY SHARE` becomes `SHARE`, and `OF` keeps only
/// its first table. `SET CONSTRAINTS ... DEFERRED`, which returns no rows, becomes a `SET`.
pub(super) static POSTGRES_FIXES: LazyLock<[(Regex, &str); 4]> = LazyLock::new(|| {
    const NAME: &str = r#"(?:"[^"]*"|\w+)(?:\s*\.\s*(?:"[^"]*"|\w+))*"#;
    [
        (
            Regex::new(r"(?i)\bFOR\s+NO\s+KEY\s+UPDATE\b").unwrap(),
            "FOR UPDATE",
        ),
        (
            Regex::new(r"(?i)\bFOR\s+KEY\s+SHARE\b").unwrap(),
            "FOR SHARE",
        ),
        (
            Regex::new(&format!(
                r"(?i)\b(FOR\s+(?:UPDATE|SHARE))\s+OF\s+({NAME})(?:\s*,\s*{NAME})*"
            ))
            .unwrap(),
            "$1 OF $2",
        ),
        (
            Regex::new(&format!(
                r"(?i)^(\s*)SET\s+CONSTRAINTS\s+(?:ALL|{NAME}(?:\s*,\s*{NAME})*)\s+(?:DEFERRED|IMMEDIATE)\b"
            ))
            .unwrap(),
            "${1}SET CONSTRAINTS TO DEFAULT",
        ),
    ]
});

/// `TABLE name`, which PostgreSQL and MySQL read as `SELECT * FROM name`, where it starts
/// a query: sqlparser reads it only after a set operator or a `WITH`.
static TABLE_COMMAND: LazyLock<Regex> = LazyLock::new(|| {
    const NAME: &str = r#"(?:"[^"]*"|`[^`]*`|\w+)(?:\s*\.\s*(?:"[^"]*"|`[^`]*`|\w+))*"#;
    Regex::new(&format!(
        r"(?i)(\A\s*|[()]\s*|\b(?:UNION|INTERSECT|EXCEPT)(?:\s+(?:ALL|DISTINCT))?\s+)TABLE\s+({NAME})"
    ))
    .unwrap()
});

/// PostgreSQL `type 'string'` literals sqlparser only reads for its built-in types, in
/// groups 1 and 2; literals, quoted names and comments are matched to be skipped.
pub(super) static POSTGRES_TYPED_STRING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"'(?:[^']|'')*'|"(?:[^"]|"")*"|--[^\n]*|(?s:/\*.*?\*/)|(?i:\b(bpchar|cidr|citext|daterange|hstore|inet|int4range|int8range|jsonpath|macaddr8?|money|name|numrange|oid|pg_lsn|tsrange|tstzrange|txid_snapshot))\s+('(?:[^']|'')*')"#,
    )
    .unwrap()
});

/// psycopg placeholders (`%s`, `%b`, `%t`, `%(name)s`, ...) and the `%%` escape.
pub(super) static PLACEHOLDER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"%%|%(?:\([^)]+\))?[sbt]").unwrap());

/// A `?` placeholder in group 1, or a literal, quoted name, comment or `?|`/`?&` operator to skip.
pub(super) static QMARK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"'(?:[^']|'')*'|"(?:[^"]|"")*"|--[^\n]*|(?s:/\*.*?\*/)|\?[|&]|(\?)"#).unwrap()
});

/// ISO types sqlparser does not read, in groups 1 and 2, respelled as ones it does:
/// `BINARY LARGE OBJECT` as `BLOB`, and `T MULTISET` as the array `T[]`. Literals, quoted
/// names and comments are matched to be skipped.
static ISO_TYPES: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"'(?:[^']|'')*'|"(?:[^"]|"")*"|--[^\n]*|(?s:/\*.*?\*/)|(?i:\b(BINARY\s+LARGE\s+OBJECT)\b)|(?i:\s+(MULTISET)\b)"#,
    )
    .unwrap()
});

/// `sql` with [`ISO_TYPES`] respelled; borrowed when it names none.
fn iso_types(sql: &str) -> Cow<'_, str> {
    let names = |word: &[u8]| {
        sql.as_bytes()
            .windows(word.len())
            .any(|w| w.eq_ignore_ascii_case(word))
    };
    if !names(b"LARGE") && !names(b"MULTISET") {
        return Cow::Borrowed(sql);
    }
    ISO_TYPES.replace_all(sql, |caps: &Captures| match (caps.get(1), caps.get(2)) {
        (Some(_), _) => "BLOB".to_string(),
        (_, Some(_)) => "[]".to_string(),
        _ => caps[0].to_string(),
    })
}

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

/// Parse `sql`; `None` when sqlparser fails or [`reject_unsupported`] rejects it.
pub(super) fn parse(sql: &str, dialect: SqlDialect) -> Option<Vec<Statement>> {
    let mut stmts = Parser::parse_sql(dialect.parser_dialect().as_ref(), sql).ok()?;
    if reject_unsupported(&stmts, dialect) {
        return None;
    }
    if dialect == SqlDialect::Postgres {
        let _ = visit_expressions_mut(&mut stmts, |expr| {
            bare_function(expr);
            ControlFlow::<()>::Continue(())
        });
    }
    Some(stmts)
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
    let sql = &iso_types(sql);
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
    let sql = parser_input(sql, dialect);
    parse(&sql, dialect).or_else(|| {
        (dialect == SqlDialect::Postgres && sql.contains('?'))
            .then(|| parse(&numbered_placeholders(&sql), dialect))
            .flatten()
    })
}

/// `sql` as sqlparser should read it, in every dialect: the `--sql` marker line is
/// dropped, PostgreSQL's `cidr '...'` and similar literals become casts (a leading space form like `--sql SELECT 1` leaves nothing), placeholders
/// become `NULL`, a value of unknown type, and `%%` becomes `%`. Like psycopg,
/// literals and comments are not skipped; any query this mangles fails to parse
/// and stays uninferred rather than mistyped.
pub(super) fn parser_input(sql: &str, dialect: SqlDialect) -> String {
    let sql = SQL_MARKER.replace(sql, "");
    let sql = match iso_types(&sql) {
        Cow::Borrowed(_) => sql,
        Cow::Owned(respelled) => Cow::Owned(respelled),
    };
    let sql = if dialect == SqlDialect::Postgres {
        let sql = drop_conflict_predicate(&sql);
        let sql = POSTGRES_FIXES
            .iter()
            .fold(sql, |sql, (pattern, replacement)| {
                pattern.replace_all(&sql, *replacement).into_owned()
            });
        POSTGRES_TYPED_STRING
            .replace_all(&sql, |caps: &Captures| match (caps.get(1), caps.get(2)) {
                (Some(ty), Some(value)) => format!("CAST({} AS {})", value.as_str(), ty.as_str()),
                _ => caps[0].to_string(),
            })
            .into_owned()
    } else {
        sql.into_owned()
    };
    let sql = match dialect {
        SqlDialect::Postgres | SqlDialect::MySql => TABLE_COMMAND
            .replace_all(&sql, "${1}SELECT * FROM $2")
            .into_owned(),
        _ => sql,
    };
    PLACEHOLDER
        .replace_all(&sql, |caps: &Captures| match &caps[0] {
            "%%" => "%",
            _ => "NULL",
        })
        .into_owned()
}

/// Drops the `WHERE <predicate>` of `ON CONFLICT (cols) WHERE <predicate> DO ...`, the
/// partial unique index that arbitrates the conflict; it does not change the result columns.
pub(super) fn drop_conflict_predicate(sql: &str) -> String {
    static CONFLICT_TARGET: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\bON\s+CONFLICT\s*\(").unwrap());
    static WHERE_CLAUSE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)\A\s*WHERE\b").unwrap());
    let mut out = String::with_capacity(sql.len());
    let mut last = 0;
    for target in CONFLICT_TARGET.find_iter(sql) {
        if target.start() < last {
            continue;
        }
        let Some(close) = closing_paren(sql, target.end()) else {
            break;
        };
        let Some(clause) = WHERE_CLAUSE.find(&sql[close + 1..]) else {
            continue;
        };
        let Some(end) = do_keyword(sql, close + 1 + clause.end()) else {
            continue;
        };
        out.push_str(&sql[last..=close]);
        out.push(' ');
        last = end;
    }
    out.push_str(&sql[last..]);
    out
}

/// Byte index of the `)` closing the parenthesis that ends just before `start`.
pub(super) fn closing_paren(sql: &str, start: usize) -> Option<usize> {
    let bytes = sql.as_bytes();
    let (mut depth, mut i) = (1, start);
    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' => i = quoted_end(bytes, i),
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

/// Byte index of the first `DO` word at or after `start` outside parentheses and quotes.
pub(super) fn do_keyword(sql: &str, start: usize) -> Option<usize> {
    let bytes = sql.as_bytes();
    let (mut depth, mut i) = (0, start);
    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' => {
                i = quoted_end(bytes, i);
                continue;
            }
            b'(' => depth += 1,
            b')' if depth == 0 => return None,
            b')' => depth -= 1,
            _ if depth == 0
                && bytes
                    .get(i..i + 2)
                    .is_some_and(|w| w.eq_ignore_ascii_case(b"DO"))
                && (i == 0 || !is_word(bytes[i - 1]))
                && bytes.get(i + 2).is_none_or(|&b| !is_word(b)) =>
            {
                return Some(i)
            }
            _ => {}
        }
        i += 1;
    }
    None
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
    })
}

pub(super) fn parse_first(sql: &str, dialect: SqlDialect) -> Option<Statement> {
    parse_query(sql, dialect)?.into_iter().next()
}
