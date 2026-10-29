//! The SQL dialects the analyzer reads, and the sqlparser dialect each maps to.

use sqlparser::ast::{Expr, Statement};
use sqlparser::dialect::{
    dialect_from_str, ClickHouseDialect, Dialect, GenericDialect, MySqlDialect, PostgreSqlDialect,
    Precedence, SQLiteDialect,
};
use sqlparser::keywords::Keyword;
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::Token;

/// A SQL dialect: PostgreSQL, MySQL and SQLite are specified, the others follow sqlparser.
///
/// Parse a user's name with [`str::parse`], which rejects names it does not know
/// ([`UnknownDialect`]); [`SqlDialect::name`] is the canonical spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SqlDialect {
    /// PostgreSQL.
    Postgres,
    /// MySQL and MariaDB.
    MySql,
    /// SQLite.
    Sqlite,
    /// ClickHouse.
    ClickHouse,
    /// Another sqlparser dialect, by its name (`generic` when sqlparser has none).
    Other(&'static str),
}

/// sqlparser dialects beyond the special-cased ones; `tsql` is read as `mssql`.
pub(super) const OTHER_DIALECTS: [&str; 12] = [
    "ansi",
    "bigquery",
    "databricks",
    "duckdb",
    "hive",
    "mssql",
    "oracle",
    "redshift",
    "snowflake",
    "spark",
    "sparksql",
    "teradata",
];

/// A dialect name sqlty does not know.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("unknown dialect {name:?}; expected one of: {}", SqlDialect::NAMES.join(", "))]
pub struct UnknownDialect {
    /// The name as given.
    pub name: String,
}

impl std::str::FromStr for SqlDialect {
    type Err = UnknownDialect;

    /// A dialect from its name, case-insensitively (`postgresql` and `tsql` are aliases).
    fn from_str(name: &str) -> Result<Self, UnknownDialect> {
        let lower = name.to_ascii_lowercase();
        if Self::NAMES.contains(&lower.as_str()) || matches!(lower.as_str(), "postgresql" | "tsql")
        {
            Ok(Self::from_name(&lower))
        } else {
            Err(UnknownDialect {
                name: name.to_string(),
            })
        }
    }
}

impl SqlDialect {
    /// Every dialect name [`str::parse`] accepts, besides the aliases `postgresql` and `tsql`.
    pub const NAMES: [&'static str; 17] = [
        "postgres",
        "mysql",
        "sqlite",
        "clickhouse",
        "generic",
        "ansi",
        "bigquery",
        "databricks",
        "duckdb",
        "hive",
        "mssql",
        "oracle",
        "redshift",
        "snowflake",
        "spark",
        "sparksql",
        "teradata",
    ];

    /// The canonical name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Postgres => "postgres",
            Self::MySql => "mysql",
            Self::Sqlite => "sqlite",
            Self::ClickHouse => "clickhouse",
            Self::Other(name) => name,
        }
    }

    pub(crate) fn from_name(name: &str) -> Self {
        match name.to_ascii_lowercase().as_str() {
            "postgres" | "postgresql" => Self::Postgres,
            "mysql" => Self::MySql,
            "sqlite" => Self::Sqlite,
            "clickhouse" => Self::ClickHouse,
            "tsql" => Self::Other("mssql"),
            other => Self::Other(
                OTHER_DIALECTS
                    .into_iter()
                    .find(|known| *known == other)
                    .unwrap_or("generic"),
            ),
        }
    }

    /// Whether the dialect reads PostgreSQL's `x::type` cast.
    pub(crate) fn has_cast_operator(self) -> bool {
        match self {
            Self::Postgres | Self::ClickHouse => true,
            Self::MySql | Self::Sqlite => false,
            Self::Other(name) => matches!(
                name,
                "databricks"
                    | "duckdb"
                    | "generic"
                    | "redshift"
                    | "snowflake"
                    | "spark"
                    | "sparksql"
            ),
        }
    }

    pub(crate) fn parser_dialect(self) -> Box<dyn Dialect> {
        match self {
            Self::Postgres => Box::new(PostgreSqlDialect {}),
            Self::MySql => Box::new(MySql(MySqlDialect {})),
            Self::Sqlite => Box::new(SQLiteDialect {}),
            Self::ClickHouse => Box::new(ClickHouseDialect {}),
            Self::Other(name) => dialect_from_str(name).unwrap_or_else(|| Box::new(GenericDialect)),
        }
    }
}

/// sqlparser's MySQL dialect, with the operator precedence of MySQL 8.4
/// (<https://dev.mysql.com/doc/refman/8.4/en/operator-precedence.html>): `^` binds tighter
/// than `*`, `<<` and `>>` looser than `+`, and `XOR` sits between `AND` and `OR`.
/// sqlparser ranks all three as for PostgreSQL.
#[derive(Debug)]
struct MySql(MySqlDialect);

/// Forwards each method that [`MySqlDialect`] overrides, and names them in `FORWARDED`.
macro_rules! forward {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        #[cfg(test)]
        const FORWARDED: &[&str] = &[$(stringify!($name)),*];

        impl Dialect for MySql {
            fn dialect(&self) -> std::any::TypeId {
                self.0.dialect()
            }

            fn get_next_precedence(&self, parser: &Parser) -> Option<Result<u8, ParserError>> {
                let precedence = match &parser.peek_token_ref().token {
                    Token::Caret => MY_CARET,
                    Token::Mul | Token::Div | Token::DuckIntDiv | Token::Mod | Token::StringConcat => {
                        MY_MUL
                    }
                    Token::Word(w) if w.keyword == Keyword::DIV => MY_MUL,
                    Token::ShiftLeft | Token::ShiftRight => MY_SHIFT,
                    Token::Word(w) if w.keyword == Keyword::XOR => MY_XOR,
                    _ => return None,
                };
                Some(Ok(precedence))
            }

            fn prec_value(&self, prec: Precedence) -> u8 {
                match prec {
                    // The operand of a unary `-`: MySQL reads `-2 ^ 3` as `(-2) ^ 3`.
                    Precedence::MulDivModOp => MY_UNARY_MINUS,
                    _ => self.0.prec_value(prec),
                }
            }

            $(fn $name(&self, $($arg: $ty),*) -> $ret {
                self.0.$name($($arg),*)
            })*
        }
    };
}

/// sqlparser's default ranks: `*` is 40, `+` 30, `&` 23, `AND` 10, `OR` 5.
const MY_UNARY_MINUS: u8 = 46;
const MY_CARET: u8 = 45;
const MY_MUL: u8 = 40;
const MY_SHIFT: u8 = 25;
const MY_XOR: u8 = 7;

forward! {
    is_identifier_start(ch: char) -> bool;
    is_identifier_part(ch: char) -> bool;
    is_delimited_identifier_start(ch: char) -> bool;
    identifier_quote_style(identifier: &str) -> Option<char>;
    supports_string_literal_backslash_escape() -> bool;
    supports_string_literal_concatenation() -> bool;
    ignores_wildcard_escapes() -> bool;
    supports_numeric_prefix() -> bool;
    supports_bitwise_shift_operators() -> bool;
    supports_multiline_comment_hints() -> bool;
    parse_infix(parser: &mut Parser, expr: &Expr, precedence: u8) -> Option<Result<Expr, ParserError>>;
    parse_statement(parser: &mut Parser) -> Option<Result<Statement, ParserError>>;
    require_interval_qualifier() -> bool;
    supports_limit_comma() -> bool;
    supports_create_table_select() -> bool;
    supports_insert_set() -> bool;
    supports_user_host_grantee() -> bool;
    is_table_factor_alias(explicit: bool, kw: &Keyword, parser: &mut Parser) -> bool;
    supports_table_hints() -> bool;
    requires_single_line_comment_whitespace() -> bool;
    supports_match_against() -> bool;
    supports_select_modifiers() -> bool;
    supports_set_names() -> bool;
    supports_comma_separated_set_assignments() -> bool;
    supports_update_order_by() -> bool;
    supports_data_type_signed_suffix() -> bool;
    supports_cross_join_constraint() -> bool;
    supports_double_ampersand_operator() -> bool;
    supports_binary_kw_as_cast() -> bool;
    supports_comment_optimizer_hint() -> bool;
    supports_constraint_keyword_without_name() -> bool;
    supports_key_column_option() -> bool;
    supports_group_by_with_modifier() -> bool;
    supports_left_associative_joins_without_parens() -> bool;
}

#[cfg(test)]
mod tests {
    use super::SqlDialect;

    /// `expr` with every operation parenthesized, to show how it groups.
    fn grouped(expr: &sqlparser::ast::Expr) -> String {
        use sqlparser::ast::Expr;
        match expr {
            Expr::BinaryOp { left, op, right } => {
                format!("({} {op} {})", grouped(left), grouped(right))
            }
            Expr::UnaryOp { op, expr } => format!("({op}{})", grouped(expr)),
            Expr::IsNull(expr) => format!("({} IS NULL)", grouped(expr)),
            other => other.to_string(),
        }
    }

    #[test]
    fn mysql_groups_by_mysql_precedence() {
        use sqlparser::ast::{SelectItem, SetExpr, Statement};
        let cases = [
            ("a XOR n IS NULL", "(a XOR (n IS NULL))"),
            ("a XOR n <=> NULL", "(a XOR (n <=> NULL))"),
            ("a OR b XOR c AND d", "(a OR (b XOR (c AND d)))"),
            ("2 ^ 3 * 1.5", "((2 ^ 3) * 1.5)"),
            ("2 ^ 3 ^ 4", "((2 ^ 3) ^ 4)"),
            ("-2 ^ 3", "((-2) ^ 3)"),
            ("n DIV 2 ^ 1", "(n DIV (2 ^ 1))"),
            ("1 & 2 << 1", "(1 & (2 << 1))"),
            ("1 << 2 + 1", "(1 << (2 + 1))"),
            ("1 + 2 * 3 - 4", "((1 + (2 * 3)) - 4)"),
        ];
        let dialect = SqlDialect::MySql.parser_dialect();
        for (sql, expected) in cases {
            let statements =
                sqlparser::parser::Parser::parse_sql(&*dialect, &format!("SELECT {sql}")).unwrap();
            let Statement::Query(query) = &statements[0] else {
                unreachable!()
            };
            let SetExpr::Select(select) = &*query.body else {
                unreachable!()
            };
            let SelectItem::UnnamedExpr(expr) = &select.projection[0] else {
                unreachable!()
            };
            assert_eq!(grouped(expr), expected, "{sql}");
        }
    }

    /// A sqlparser upgrade that overrides another [`MySqlDialect`] method fails here until
    /// `forward!` lists it.
    #[test]
    fn mysql_forwards_every_override() {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
        let metadata = std::process::Command::new(cargo)
            .args([
                "metadata",
                "--format-version",
                "1",
                "--offline",
                "--manifest-path",
            ])
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
            .output()
            .unwrap();
        let metadata = String::from_utf8(metadata.stdout).unwrap();
        let manifest =
            regex::Regex::new(r#""manifest_path":"([^"]*[/\\]sqlparser-[0-9.]+[/\\]Cargo\.toml)""#)
                .unwrap()
                .captures(&metadata)
                .unwrap()[1]
                .replace("\\\\", "\\");
        let source = std::fs::read_to_string(
            std::path::Path::new(&manifest).with_file_name("src/dialect/mysql.rs"),
        )
        .unwrap();
        let start = source.find("impl Dialect for MySqlDialect {").unwrap();
        let block = &source[start..start + source[start..].find("\n}\n").unwrap()];
        let overridden: Vec<&str> = regex::Regex::new(r"(?m)^    fn (\w+)")
            .unwrap()
            .captures_iter(block)
            .map(|c| c.get(1).unwrap().as_str())
            .collect();
        assert_eq!(overridden, super::FORWARDED);
    }

    #[test]
    fn every_name_round_trips() {
        for name in SqlDialect::NAMES {
            let dialect: SqlDialect = name.parse().unwrap();
            assert_eq!(dialect.name(), name);
        }
    }

    #[test]
    fn names_are_case_insensitive_with_aliases() {
        assert_eq!("PostgreSQL".parse(), Ok(SqlDialect::Postgres));
        assert_eq!("TSQL".parse(), Ok(SqlDialect::Other("mssql")));
    }

    #[test]
    fn an_unknown_name_lists_the_known_ones() {
        let error = "postgress".parse::<SqlDialect>().unwrap_err();
        assert_eq!(error.name, "postgress");
        assert!(error
            .to_string()
            .starts_with("unknown dialect \"postgress\"; expected one of: postgres, mysql,"));
    }
}
