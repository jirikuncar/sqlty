//! Operator typing: numeric promotion, arithmetic and common types.

use sqlparser::ast::{BinaryOperator, Expr, Value};

use super::dialect::SqlDialect;
use super::expr::{array_element, from_sql_type};
use super::inference::{binary_rules, BinaryRule, RuleType};
use super::literals::{merge, python_kind};
use super::query::Analyzer;
use super::scope::Scope;
use super::typed::Typed;
use super::{Inferred, DECIMAL};

impl Analyzer<'_> {
    /// Result type of `left op right`: an inference rule's (`rules/`), except for MySQL's
    /// `date ± INTERVAL n unit`, whose unit decides the type.
    pub(super) fn binary_op(
        &self,
        left: &Expr,
        op: &BinaryOperator,
        right: &Expr,
        scope: &Scope,
    ) -> Inferred<Typed> {
        use BinaryOperator::*;
        if self.dialect == SqlDialect::MySql && matches!(op, Plus | Minus) {
            let (l, r) = (self.expr(left, scope)?, self.expr(right, scope)?);
            if let Some(typed) = mysql_date_add(op, (left, &l), (right, &r)) {
                return Ok(from_sql_type(typed, self.dialect));
            }
        }
        let Some((rule, operands)) = self.binary_rule(left, op, right, scope)? else {
            return Ok(Typed::UNKNOWN);
        };
        let reads = !matches!(
            rule.result,
            RuleType::Sql(_) | RuleType::Any | RuleType::Unknown
        );
        let (l, r) = match operands {
            Some(operands) => operands,
            None if reads => self.rule_operands(left, op, right, scope)?,
            None => (Typed::UNKNOWN, Typed::UNKNOWN),
        };
        let rank = |t: &Typed| operand_type(t).and_then(numeric_rank);
        let promoted = |least: usize| match (rank(&l), rank(&r)) {
            (Some(a), Some(b)) => {
                from_sql_type(NUMERIC_RANKS[a.max(b).max(least)][0], self.dialect)
            }
            _ => Typed::UNKNOWN,
        };
        Ok(match rule.result {
            RuleType::Sql(name) => from_sql_type(name, self.dialect),
            RuleType::Any => Typed::python_only("typing.Any"),
            RuleType::Unknown => Typed::UNKNOWN,
            RuleType::Promote => promoted(0),
            RuleType::PromoteToDecimal => promoted(3),
            RuleType::PromoteLeft => rank(&l).map_or(Typed::UNKNOWN, |a| {
                from_sql_type(NUMERIC_RANKS[a][0], self.dialect)
            }),
            RuleType::Left => l,
            RuleType::Right => r,
            RuleType::Common => common_type(l, r, self.dialect).unwrap_or(Typed::UNKNOWN),
        })
    }

    /// The operand types an inference rule reads. In arithmetic a placeholder or `NULL`
    /// takes the other operand's type.
    fn rule_operands(
        &self,
        left: &Expr,
        op: &BinaryOperator,
        right: &Expr,
        scope: &Scope,
    ) -> Inferred<(Typed, Typed)> {
        let (l, r) = (self.expr(left, scope)?, self.expr(right, scope)?);
        if !is_arithmetic(op) {
            return Ok((l, r));
        }
        let wild = |expr: &Expr| matches!(expr, Expr::Value(v) if matches!(v.value, Value::Null | Value::Placeholder(_)));
        Ok(match (wild(left), wild(right)) {
            (true, false) => (r.clone(), r),
            (false, true) => (l.clone(), l),
            _ => (l, r),
        })
    }

    /// The inference rule for `left op right`, with the operand types when it read them.
    pub(super) fn binary_rule(
        &self,
        left: &Expr,
        op: &BinaryOperator,
        right: &Expr,
        scope: &Scope,
    ) -> Inferred<Option<RuleMatch>> {
        let rules = binary_rules(op);
        if rules.is_empty() {
            return Ok(None);
        }
        let reads_left = rules.iter().any(BinaryRule::reads_left);
        let reads_right = rules.iter().any(BinaryRule::reads_right);
        // Arithmetic reads both sides: a placeholder takes the other operand's type.
        if reads_left && reads_right || (reads_left || reads_right) && is_arithmetic(op) {
            let typed = self.rule_operands(left, op, right, scope)?;
            let (l, r) = (rule_type(&typed.0), rule_type(&typed.1));
            let found = rules.iter().find(|rule| rule.applies(self.dialect, l, r));
            return Ok(found.map(|rule| (rule, Some(typed.clone()))));
        }
        let side = |read: bool, expr: &Expr| -> Inferred<Option<Typed>> {
            read.then(|| self.expr(expr, scope)).transpose()
        };
        let (l, r) = (side(reads_left, left)?, side(reads_right, right)?);
        let (l, r) = (
            l.as_ref().and_then(rule_type),
            r.as_ref().and_then(rule_type),
        );
        let found = rules.iter().find(|rule| rule.applies(self.dialect, l, r));
        Ok(found.map(|rule| (rule, None)))
    }
}

/// The rule that applies to an operator, with the operand types when it read them.
type RuleMatch = (&'static BinaryRule, Option<(Typed, Typed)>);

/// The SQL type an inference rule sees for an operand: its type's base name (`ARRAY` for
/// any array), or that of a literal (`TEXT` for a string).
fn rule_type(typed: &Typed) -> Option<&str> {
    operand_type(typed).or_else(|| (python_kind(typed) == Some("str")).then_some("TEXT"))
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

fn is_arithmetic(op: &BinaryOperator) -> bool {
    use BinaryOperator::*;
    matches!(op, Plus | Minus | Multiply | Divide | Modulo)
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
    if dialect == SqlDialect::Sqlite {
        if let Some(union) = sqlite_union(&left, &right) {
            return Some(union);
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
    if dialect == SqlDialect::MySql {
        return mysql_common_type(a, b).map(|common| from_sql_type(common, dialect));
    }
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

/// SQLite has no type for an expression: a `CASE` or `UNION` of an integer and a text
/// gives either value, so its Python type is their union. Numbers still widen.
fn sqlite_union(left: &Typed, right: &Typed) -> Option<Typed> {
    const NUMBERS: [&str; 4] = ["int", "float", "decimal.Decimal", "bool"];
    let (x, y) = (left.python()?, right.python()?);
    let numeric = |python: &str| python.split(" | ").all(|kind| NUMBERS.contains(&kind));
    if x == y || (numeric(x) && numeric(y)) {
        return None;
    }
    let mut union: Vec<&str> = x.split(" | ").collect();
    for kind in y.split(" | ") {
        if !union.contains(&kind) {
            union.push(kind);
        }
    }
    Some(Typed::python_only(&union.join(" | ")))
}

/// MySQL's aggregation of the types of `CASE`, `IF`, `COALESCE` or `UNION` inputs: numbers
/// widen, temporal types meet at `DATETIME`, a binary string wins and other mixes are text.
fn mysql_common_type<'a>(a: &'a str, b: &'a str) -> Option<&'a str> {
    const TEMPORAL: [&str; 4] = ["DATE", "TIME", "DATETIME", "TIMESTAMP"];
    const BINARY: [&str; 6] = [
        "VARBINARY",
        "BINARY",
        "BLOB",
        "TINYBLOB",
        "MEDIUMBLOB",
        "LONGBLOB",
    ];
    let rank = |ty: &str| match ty {
        "BOOLEAN" | "YEAR" => Some(0),
        _ => numeric_rank(
            ty.strip_prefix('U')
                .filter(|signed| numeric_rank(signed).is_some())
                .unwrap_or(ty),
        ),
    };
    let known = |ty: &str| {
        rank(ty).is_some()
            || TEMPORAL.contains(&ty)
            || BINARY.contains(&ty)
            || matches!(ty, "TEXT" | "VARCHAR" | "CHAR" | "JSON")
    };
    if !known(a) || !known(b) {
        return None;
    }
    Some(match (rank(a), rank(b)) {
        (Some(x), Some(y)) => NUMERIC_RANKS[x.max(y)][0],
        _ if a == b => a,
        _ if TEMPORAL.contains(&a) && TEMPORAL.contains(&b) => "DATETIME",
        _ if BINARY.contains(&a) || BINARY.contains(&b) => "VARBINARY",
        _ => "TEXT",
    })
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

    /// The type of `left op right` over columns of the two types, as a query infers it.
    fn arithmetic_type(left: &str, symbol: &str, right: &str, dialect: &str) -> Option<String> {
        thread_local! {
            static SCHEMAS: std::cell::RefCell<std::collections::HashMap<String, crate::analyzer::Schema>> =
                std::cell::RefCell::default();
        }
        const TYPES: [&str; 11] = [
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
        let column = |ty: &str| format!("c_{}", ty.to_lowercase());
        let ddl = TYPES
            .iter()
            .map(|ty| {
                let declared = if *ty == "DOUBLE" {
                    "DOUBLE PRECISION"
                } else {
                    ty
                };
                format!("{} {declared}", column(ty))
            })
            .collect::<Vec<_>>()
            .join(", ");
        let schema = SCHEMAS.with_borrow_mut(|schemas| {
            schemas
                .entry(dialect.to_string())
                .or_insert_with(|| {
                    crate::analyzer::load_schema(&format!("CREATE TABLE t ({ddl})"), dialect)
                })
                .clone()
        });
        crate::analyzer::analyze_select(
            &format!(
                "SELECT {} {symbol} {} AS x FROM t",
                column(left),
                column(right)
            ),
            dialect,
            Some(&schema),
        )
        .ok()
        .flatten()
        .and_then(|columns| columns[0].sql_type.clone())
    }

    #[test]
    fn arithmetic_promotes_numbers_and_types_date_math() {
        let operators = ["+", "-", "*", "/"];
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
        for symbol in operators {
            for left in types {
                for right in types {
                    if let Some(result) = arithmetic_type(left, symbol, right, "postgres") {
                        lines.push(format!("{left} {symbol} {right} = {result}"));
                    }
                }
            }
        }
        insta::assert_snapshot!(lines.join("\n"));
    }

    #[test]
    fn mysql_division_is_at_least_decimal() {
        assert_eq!(
            arithmetic_type("INT", "/", "INT", "mysql").as_deref(),
            Some("DECIMAL")
        );
        assert_eq!(
            arithmetic_type("INT", "/", "INT", "postgres").as_deref(),
            Some("INT")
        );
        assert_eq!(
            arithmetic_type("INT", "/", "DOUBLE", "mysql").as_deref(),
            Some("DOUBLE")
        );
    }

    #[test]
    fn arithmetic_with_an_unknown_operand_has_no_type() {
        assert_eq!(arithmetic_type("INT", "+", "TEXT", "postgres"), None);
        assert_eq!(arithmetic_type("DATE", "*", "INT", "postgres"), None);
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
