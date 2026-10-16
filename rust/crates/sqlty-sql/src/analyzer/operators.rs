//! Operator typing: numeric promotion, arithmetic and common types.

use sqlparser::ast::{BinaryOperator, Expr, Value};

use super::dialect::SqlDialect;
use super::expr::{array_element, from_sql_type};
use super::literals::{merge, python_kind};
use super::query::Analyzer;
use super::scope::Scope;
use super::typed::Typed;
use super::{Inferred, DECIMAL};

impl<'a> Analyzer<'a> {
    /// Result type of `left op right`: predicates are `BOOLEAN`, text and JSON operators
    /// give `TEXT` or `JSONB`, arithmetic promotes its operands.
    pub(super) fn binary_op(
        &self,
        left: &Expr,
        op: &BinaryOperator,
        right: &Expr,
        scope: &Scope,
    ) -> Inferred<Typed> {
        use BinaryOperator::*;
        let sql = |name: &str| Ok(from_sql_type(name, self.dialect));
        match op {
            Gt | Lt | GtEq | LtEq | Spaceship | Eq | NotEq | And | Or | Xor | Match | Regexp
            | Glob | PGOverlap | PGRegexMatch | PGRegexIMatch | PGRegexNotMatch
            | PGRegexNotIMatch | PGLikeMatch | PGILikeMatch | PGNotLikeMatch | PGNotILikeMatch
            | PGStartsWith | AtAt | AtArrow | ArrowAt | AtQuestion | Question | QuestionAnd
            | QuestionPipe | Overlaps => sql("BOOLEAN"),
            LongArrow | HashLongArrow => sql("TEXT"),
            Arrow | HashArrow | HashMinus if self.dialect == SqlDialect::Postgres => {
                let json = self.expr(left, scope)?.sql() == Some("JSON");
                sql(if json && *op != HashMinus {
                    "JSON"
                } else {
                    "JSONB"
                })
            }
            StringConcat if self.dialect != SqlDialect::MySql => {
                let (l, r) = (self.expr(left, scope)?, self.expr(right, scope)?);
                let array = |t: &Typed| t.sql().is_some_and(|ty| array_element(ty).is_some());
                if l.sql() == Some("JSONB") || r.sql() == Some("JSONB") {
                    sql("JSONB")
                } else if self.dialect == SqlDialect::Postgres && (array(&l) || array(&r)) {
                    // `anyarray || anyarray`, `anyarray || elem` and `elem || anyarray`.
                    Ok(match (array(&l), array(&r)) {
                        (true, true) => common_type(l, r, self.dialect).unwrap_or(Typed::UNKNOWN),
                        (true, false) => l,
                        _ => r,
                    })
                } else if is_text(&l) || is_text(&r) {
                    sql("TEXT")
                } else {
                    Ok(Typed::UNKNOWN)
                }
            }
            PGExp => {
                let (l, r) = (self.expr(left, scope)?, self.expr(right, scope)?);
                let numeric = |t: &Typed| matches!(operand_type(t), Some("DECIMAL" | "NUMERIC"));
                sql(if numeric(&l) && numeric(&r) {
                    "DECIMAL"
                } else {
                    "DOUBLE"
                })
            }
            BitwiseAnd | BitwiseOr | PGBitwiseXor | PGBitwiseShiftLeft | PGBitwiseShiftRight
                if self.dialect == SqlDialect::Postgres =>
            {
                // Integer operands: the wider type; a shift keeps its left operand's type.
                let (l, r) = (self.expr(left, scope)?, self.expr(right, scope)?);
                let integer = |t: &Typed| {
                    operand_type(t)
                        .and_then(numeric_rank)
                        .filter(|rank| *rank <= 2)
                };
                let shift = matches!(op, PGBitwiseShiftLeft | PGBitwiseShiftRight);
                Ok(match (integer(&l), integer(&r)) {
                    (Some(a), Some(b)) => {
                        let rank = if shift { a } else { a.max(b) };
                        from_sql_type(NUMERIC_RANKS[rank][0], self.dialect)
                    }
                    _ => Typed::UNKNOWN,
                })
            }
            Plus | Minus | Multiply | Divide | Modulo => {
                let (l, r) = (self.expr(left, scope)?, self.expr(right, scope)?);
                if self.dialect == SqlDialect::MySql {
                    if let Some(typed) = mysql_date_add(op, (left, &l), (right, &r)) {
                        return Ok(from_sql_type(typed, self.dialect));
                    }
                }
                let wild = |expr: &Expr| matches!(expr, Expr::Value(v) if matches!(v.value, Value::Null | Value::Placeholder(_)));
                let (l, r) = match (wild(left), wild(right)) {
                    (true, false) => (r.clone(), r),
                    (false, true) => (l.clone(), l),
                    _ => (l, r),
                };
                Ok(arithmetic(op, &l, &r, self.dialect)
                    .map_or(Typed::UNKNOWN, |ty| from_sql_type(&ty, self.dialect)))
            }
            _ => Ok(Typed::UNKNOWN),
        }
    }
}

pub(super) fn type_base(sql_type: &str) -> &str {
    sql_type.split(['(', '<']).next().unwrap_or_default().trim()
}

/// The SQL type of an operand; literals know only their Python type.
pub(super) fn operand_type(typed: &Typed) -> Option<&str> {
    match (typed.sql(), python_kind(typed)) {
        (Some(sql_type), _) => Some(type_base(sql_type)),
        (None, Some("int")) => Some("INT"),
        (None, Some("float")) => Some("DOUBLE"),
        (None, Some(DECIMAL)) => Some("DECIMAL"),
        _ => None,
    }
}

pub(super) fn is_text(typed: &Typed) -> bool {
    match &typed.sql {
        Some(sql_type) => matches!(type_base(sql_type), "TEXT" | "VARCHAR" | "CHAR"),
        None => python_kind(typed) == Some("str"),
    }
}

/// Numeric types in promotion order, with the canonical name of each rank.
pub(super) const NUMERIC_RANKS: [&[&str]; 6] = [
    &["SMALLINT", "TINYINT"],
    &["INT", "MEDIUMINT"],
    &["BIGINT"],
    &["DECIMAL", "NUMERIC"],
    &["FLOAT", "REAL"],
    &["DOUBLE"],
];

/// The rank of `FLOAT` (`real`) in [`NUMERIC_RANKS`].
const FLOAT_RANK: usize = 4;

pub(super) fn numeric_rank(sql_type: &str) -> Option<usize> {
    NUMERIC_RANKS
        .iter()
        .position(|names| names.contains(&sql_type))
}

/// MySQL `date ± INTERVAL n unit` (or `INTERVAL n unit + date`): a `DATE` stays a date for
/// day units and coarser, anything else is a `DATETIME`.
pub(super) fn mysql_date_add(
    op: &BinaryOperator,
    (left, left_typed): (&Expr, &Typed),
    (right, right_typed): (&Expr, &Typed),
) -> Option<&'static str> {
    use sqlparser::ast::DateTimeField::{
        Day, Days, Month, Months, Quarter, Week, Weeks, Year, Years,
    };
    let (date, interval) = match (left, right) {
        (_, Expr::Interval(interval)) => (left_typed, interval),
        (Expr::Interval(interval), _) if *op == BinaryOperator::Plus => (right_typed, interval),
        _ => return None,
    };
    if !matches!(op, BinaryOperator::Plus | BinaryOperator::Minus) {
        return None;
    }
    let whole_days = interval.last_field.is_none()
        && matches!(
            interval.leading_field,
            Some(Year | Years | Quarter | Month | Months | Week(_) | Weeks | Day | Days)
        );
    match operand_type(date)? {
        "DATE" if whole_days => Some("DATE"),
        "DATE" | "DATETIME" | "TIMESTAMP" => Some("DATETIME"),
        _ => None,
    }
}

/// Result type of `left op right` for numbers (promoted to the wider type), and in
/// PostgreSQL for dates, times, timestamps and intervals; a placeholder or `NULL` operand
/// takes the other's type.
pub(super) fn arithmetic(
    op: &BinaryOperator,
    left: &Typed,
    right: &Typed,
    dialect: SqlDialect,
) -> Option<String> {
    use BinaryOperator::*;
    let (l, r) = (operand_type(left), operand_type(right));
    if let (Some(a), Some(b)) = (l, r) {
        if let (Some(a), Some(b)) = (numeric_rank(a), numeric_rank(b)) {
            let mut rank = a.max(b);
            // PostgreSQL has `real` operators for two `real`s only: anything else with a
            // `real` resolves to `double precision`.
            if dialect == SqlDialect::Postgres && rank == FLOAT_RANK && a != b {
                rank = FLOAT_RANK + 1;
            }
            let mysql_division = dialect == SqlDialect::MySql && *op == Divide;
            let rank = if mysql_division { rank.max(3) } else { rank };
            return Some(NUMERIC_RANKS[rank][0].to_string());
        }
    }
    // MySQL reads a date as a number in arithmetic, and SQLite stores it as text.
    if dialect != SqlDialect::Postgres {
        return None;
    }
    let temporal = |ty: Option<&str>| matches!(ty, Some("DATE" | "TIMESTAMP" | "TIMESTAMPTZ"));
    match (l, r) {
        (Some(a), Some("INTERVAL")) if temporal(Some(a)) && matches!(op, Plus | Minus) => {
            Some(if a == "DATE" { "TIMESTAMP" } else { a }.to_string())
        }
        (Some("INTERVAL"), Some(b)) if temporal(Some(b)) && *op == Plus => {
            Some(if b == "DATE" { "TIMESTAMP" } else { b }.to_string())
        }
        (Some("DATE"), Some("INT" | "SMALLINT")) if matches!(op, Plus | Minus) => {
            Some("DATE".to_string())
        }
        (Some("INT" | "SMALLINT"), Some("DATE")) if *op == Plus => Some("DATE".to_string()),
        (Some("DATE"), Some("TIME")) | (Some("TIME"), Some("DATE")) if *op == Plus => {
            Some("TIMESTAMP".to_string())
        }
        (Some("DATE"), Some("TIMETZ")) | (Some("TIMETZ"), Some("DATE")) if *op == Plus => {
            Some("TIMESTAMPTZ".to_string())
        }
        (Some(time @ ("TIME" | "TIMETZ")), Some("INTERVAL")) if matches!(op, Plus | Minus) => {
            Some(time.to_string())
        }
        (Some("INTERVAL"), Some(time @ ("TIME" | "TIMETZ"))) if *op == Plus => {
            Some(time.to_string())
        }
        (Some("TIME"), Some("TIME")) if *op == Minus => Some("INTERVAL".to_string()),
        (Some("INTERVAL"), Some(n)) if *op == Divide && numeric_rank(n).is_some() => {
            Some("INTERVAL".to_string())
        }
        (Some(a), Some(b))
            if *op == Minus && a == b && matches!(a, "TIMESTAMP" | "TIMESTAMPTZ") =>
        {
            Some("INTERVAL".to_string())
        }
        (Some("DATE"), Some("DATE")) if *op == Minus => Some("INT".to_string()),
        (Some("INTERVAL"), Some("INTERVAL")) if matches!(op, Plus | Minus) => {
            Some("INTERVAL".to_string())
        }
        (Some("INTERVAL"), Some(n)) | (Some(n), Some("INTERVAL"))
            if *op == Multiply && numeric_rank(n).is_some() =>
        {
            Some("INTERVAL".to_string())
        }
        _ => None,
    }
}

/// The type of a set operation's column whose branches have types `left` and `right`,
/// following PostgreSQL: an unknown type yields to the other, numbers promote to the
/// wider type, text and timestamps to the preferred one, arrays by their elements;
/// other mixes conflict (`None`).
pub(super) fn common_type(left: Typed, right: Typed, dialect: SqlDialect) -> Option<Typed> {
    if right == Typed::UNKNOWN || left == right {
        return Some(left);
    }
    if left == Typed::UNKNOWN {
        return Some(right);
    }
    if let Some(merged) = merge(&left, &right, dialect) {
        return merged;
    }
    let postgres = dialect == SqlDialect::Postgres;
    if postgres {
        // A string literal has the type `unknown`, which takes the other input's type.
        match (is_unknown_literal(&left), is_unknown_literal(&right)) {
            (true, false) => return Some(right),
            (false, true) => return Some(left),
            _ => {}
        }
    }
    // An integer literal is an `integer`, so it widens a `smallint`.
    let widens = |literal: &Typed, other: &Typed| {
        postgres
            && literal.sql.is_none()
            && python_kind(literal) == Some("int")
            && operand_type(other) == Some("SMALLINT")
    };
    if !widens(&left, &right) && !widens(&right, &left) {
        if left.sql.is_none() && left.python == right.python {
            return Some(right);
        }
        if right.sql.is_none() && left.python == right.python {
            return Some(left);
        }
    }
    if let (Some(x), Some(y)) = (
        left.sql().and_then(array_element),
        right.sql().and_then(array_element),
    ) {
        let elem = |ty: &str| from_sql_type(ty, dialect);
        let elem = common_type(elem(x), elem(y), dialect)?.sql?;
        return Some(from_sql_type(&format!("ARRAY<{elem}>"), dialect));
    }
    let (a, b) = (category(&left)?, category(&right)?);
    const TEMPORAL: [&str; 3] = ["DATE", "TIMESTAMP", "TIMESTAMPTZ"];
    let common = if let (Some(x), Some(y)) = (numeric_rank(a), numeric_rank(b)) {
        NUMERIC_RANKS[x.max(y)][0]
    } else if is_text(&left) && is_text(&right) {
        // `text`, `varchar` and `bpchar` coerce to each other implicitly, so the first wins
        // (`select_common_type` only switches to a type the current one casts to one-way).
        a
    } else if let (Some(x), Some(y)) = (
        TEMPORAL.iter().position(|ty| *ty == a),
        TEMPORAL.iter().position(|ty| *ty == b),
    ) {
        TEMPORAL[x.max(y)]
    } else {
        return None;
    };
    Some(from_sql_type(common, dialect))
}

/// A string literal: PostgreSQL types it `unknown` until the other inputs decide.
pub(super) fn is_unknown_literal(typed: &Typed) -> bool {
    typed.sql.is_none() && python_kind(typed) == Some("str")
}

/// The common type of the branches of a `CASE`, the arguments of `COALESCE`, `GREATEST`
/// or `LEAST`, or the elements of a `VALUES` column, following PostgreSQL: untyped inputs
/// yield to the others, numbers promote and `COALESCE(int_col, 1.5)` is a `NUMERIC`.
/// Inputs that cannot be unified (PostgreSQL rejects them) give `None`.
pub(super) fn common_of(
    types: impl IntoIterator<Item = Typed>,
    dialect: SqlDialect,
) -> Option<Typed> {
    types.into_iter().try_fold(Typed::UNKNOWN, |acc, typed| {
        common_type(acc, typed, dialect)
    })
}

/// The type category of a set operation's input; literals know only their Python type.
pub(super) fn category(typed: &Typed) -> Option<&str> {
    match (typed.sql(), python_kind(typed)) {
        (None, Some("str")) => Some("TEXT"),
        _ => operand_type(typed),
    }
}

#[cfg(test)]
mod tests {
    use sqlparser::ast::BinaryOperator;

    use super::super::test_support::reports;
    use super::*;

    fn sql(ty: &str) -> Typed {
        Typed::of_sql(ty, SqlDialect::Postgres)
    }

    fn literal(python: &str) -> Typed {
        Typed::python_only(python)
    }

    fn common(left: Typed, right: Typed) -> String {
        match common_type(left, right, SqlDialect::Postgres) {
            Some(typed) => format!(
                "{} | {}",
                typed.python().unwrap_or("?"),
                typed.sql().unwrap_or("?")
            ),
            None => "conflict".to_string(),
        }
    }

    #[test]
    fn type_base_drops_parameters() {
        assert_eq!(type_base("DECIMAL(10, 2)"), "DECIMAL");
        assert_eq!(type_base("ARRAY<INT>"), "ARRAY");
        assert_eq!(type_base("TEXT"), "TEXT");
        assert_eq!(type_base(""), "");
    }

    #[test]
    fn operand_type_reads_the_sql_type_then_the_literal() {
        assert_eq!(operand_type(&sql("DECIMAL(10, 2)")), Some("DECIMAL"));
        assert_eq!(operand_type(&literal("int")), Some("INT"));
        assert_eq!(operand_type(&literal("float")), Some("DOUBLE"));
        assert_eq!(operand_type(&literal(DECIMAL)), Some("DECIMAL"));
        assert_eq!(operand_type(&literal("str")), None);
        assert_eq!(operand_type(&Typed::UNKNOWN), None);
    }

    #[test]
    fn text_is_text_varchar_char_or_a_string_literal() {
        assert!(is_text(&sql("TEXT")));
        assert!(is_text(&sql("VARCHAR(20)")));
        assert!(is_text(&sql("CHAR(2)")));
        assert!(is_text(&literal("str")));
        assert!(!is_text(&sql("INT")));
        assert!(!is_text(&Typed::UNKNOWN));
    }

    #[test]
    fn numeric_rank_orders_the_promotion_chain() {
        let ranks: Vec<_> = ["SMALLINT", "INT", "BIGINT", "DECIMAL", "REAL", "DOUBLE"]
            .into_iter()
            .map(|ty| numeric_rank(ty).unwrap())
            .collect();
        assert_eq!(ranks, [0, 1, 2, 3, 4, 5]);
        assert_eq!(numeric_rank("TEXT"), None);
        assert_eq!(numeric_rank("TINYINT"), numeric_rank("SMALLINT"));
    }

    #[test]
    fn arithmetic_promotes_numbers_and_types_date_math() {
        let operators = [
            ("+", BinaryOperator::Plus),
            ("-", BinaryOperator::Minus),
            ("*", BinaryOperator::Multiply),
            ("/", BinaryOperator::Divide),
        ];
        let types = [
            "SMALLINT",
            "INT",
            "BIGINT",
            "DECIMAL",
            "DOUBLE",
            "DATE",
            "TIME",
            "TIMESTAMP",
            "TIMESTAMPTZ",
            "INTERVAL",
            "TEXT",
        ];
        let mut lines = Vec::new();
        for (symbol, op) in &operators {
            for left in types {
                for right in types {
                    if let Some(result) =
                        arithmetic(op, &sql(left), &sql(right), SqlDialect::Postgres)
                    {
                        lines.push(format!("{left} {symbol} {right} = {result}"));
                    }
                }
            }
        }
        insta::assert_snapshot!(lines.join("\n"));
    }

    #[test]
    fn mysql_division_is_at_least_decimal() {
        let int = sql("INT");
        assert_eq!(
            arithmetic(&BinaryOperator::Divide, &int, &int, SqlDialect::MySql).as_deref(),
            Some("DECIMAL")
        );
        assert_eq!(
            arithmetic(&BinaryOperator::Divide, &int, &int, SqlDialect::Postgres).as_deref(),
            Some("INT")
        );
        assert_eq!(
            arithmetic(
                &BinaryOperator::Divide,
                &int,
                &sql("DOUBLE"),
                SqlDialect::MySql
            )
            .as_deref(),
            Some("DOUBLE")
        );
    }

    #[test]
    fn arithmetic_with_an_unknown_operand_has_no_type() {
        let int = sql("INT");
        assert_eq!(
            arithmetic(
                &BinaryOperator::Plus,
                &int,
                &Typed::UNKNOWN,
                SqlDialect::Postgres
            ),
            None
        );
        assert_eq!(
            arithmetic(
                &BinaryOperator::Plus,
                &int,
                &literal("str"),
                SqlDialect::Postgres
            ),
            None
        );
    }

    #[test]
    fn common_type_follows_postgres_union_rules() {
        let cases = [
            ("INT", "INT"),
            ("INT", "BIGINT"),
            ("SMALLINT", "DECIMAL"),
            ("BIGINT", "DOUBLE"),
            ("TEXT", "VARCHAR"),
            ("VARCHAR", "TEXT"),
            ("DATE", "TIMESTAMP"),
            ("TIMESTAMP", "TIMESTAMPTZ"),
            ("INT", "TEXT"),
            ("BOOLEAN", "INT"),
            ("DATE", "TIME"),
            ("ARRAY<INT>", "ARRAY<BIGINT>"),
            ("ARRAY<INT>", "ARRAY<TEXT>"),
            ("ARRAY<INT>", "INT"),
        ];
        let mut lines: Vec<String> = cases
            .iter()
            .map(|(left, right)| format!("{left} + {right} -> {}", common(sql(left), sql(right))))
            .collect();
        lines.push(format!(
            "unknown + INT -> {}",
            common(Typed::UNKNOWN, sql("INT"))
        ));
        lines.push(format!(
            "INT + unknown -> {}",
            common(sql("INT"), Typed::UNKNOWN)
        ));
        lines.push(format!(
            "'str' + TEXT -> {}",
            common(literal("str"), sql("TEXT"))
        ));
        lines.push(format!(
            "'str' + INT -> {}",
            common(literal("str"), sql("INT"))
        ));
        lines.push(format!(
            "1 + SMALLINT -> {}",
            common(literal("int"), sql("SMALLINT"))
        ));
        lines.push(format!(
            "1 + BIGINT -> {}",
            common(literal("int"), sql("BIGINT"))
        ));
        insta::assert_snapshot!(lines.join("\n"));
    }

    #[test]
    fn common_of_folds_every_input_and_fails_on_a_conflict() {
        let ints = [sql("INT"), literal("int"), Typed::UNKNOWN];
        assert_eq!(
            common_of(ints, SqlDialect::Postgres).and_then(|typed| typed.sql),
            Some("INT".into())
        );
        let mixed = [sql("INT"), sql("TEXT")];
        assert_eq!(common_of(mixed, SqlDialect::Postgres), None);
        assert_eq!(
            common_of([], SqlDialect::Postgres),
            Some(Typed::UNKNOWN),
            "no inputs give an unknown type"
        );
    }

    #[test]
    fn category_treats_string_literals_as_text() {
        assert_eq!(category(&literal("str")), Some("TEXT"));
        assert_eq!(category(&sql("BIGINT")), Some("BIGINT"));
        assert_eq!(category(&Typed::UNKNOWN), None);
    }

    #[test]
    fn unknown_literals_are_untyped_strings() {
        assert!(is_unknown_literal(&literal("str")));
        assert!(!is_unknown_literal(&sql("TEXT")));
        assert!(!is_unknown_literal(&literal("int")));
    }

    #[test]
    fn binary_operators_in_queries() {
        let ddl = "CREATE TABLE t (i INT NOT NULL, s SMALLINT, b BIGINT, n NUMERIC(8, 2), \
                   d DATE, ts TIMESTAMP, tz TIMESTAMPTZ, iv INTERVAL, txt TEXT, flag BOOLEAN)";
        let queries = [
            "SELECT i + s AS a, i * b AS b, s - n AS c, i / i AS d, i % s AS e FROM t",
            "SELECT i + 1.5 AS a, i + 1 AS b, n * 2 AS c FROM t",
            "SELECT d + 1 AS a, d - d AS b, d + iv AS c, ts - ts AS d, tz - iv AS e, iv * 2 AS f FROM t",
            "SELECT txt || 'x' AS a, txt || i AS b, 'a' || 'b' AS c FROM t",
            "SELECT i = 1 AS a, txt LIKE 'a%' AS b, i IS NULL AS c, NOT flag AS d, i BETWEEN 1 AND 2 AS e FROM t",
            "SELECT i IN (1, 2) AS a, i = ANY(ARRAY[1, 2]) AS b, flag AND true AS c, flag OR NULL AS d FROM t",
            "SELECT NULL + 1 AS a, 1 + NULL AS b, NULL AS c FROM t",
            "SELECT i + txt AS a FROM t",
            "SELECT 2 ^ 3 AS a, 7 & 3 AS b, 1 << 2 AS c, -i AS d, +n AS e, ~i AS f FROM t",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(ddl)));
    }

    #[test]
    fn division_differs_between_postgres_and_mysql() {
        let ddl = "CREATE TABLE t (a INT NOT NULL, b INT NOT NULL)";
        let query = ["SELECT a / b AS q, a DIV b AS d, a % b AS m FROM t"];
        let postgres = reports(&query, "postgres", Some(ddl));
        let mysql = reports(&query, "mysql", Some(ddl));
        insta::assert_snapshot!(format!("postgres:\n{postgres}\nmysql:\n{mysql}"));
    }
}
