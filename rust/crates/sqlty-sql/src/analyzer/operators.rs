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
                let mysql_division = self.dialect == SqlDialect::MySql && *op == Divide;
                let wild = |expr: &Expr| matches!(expr, Expr::Value(v) if matches!(v.value, Value::Null | Value::Placeholder(_)));
                let (l, r) = match (wild(left), wild(right)) {
                    (true, false) => (r.clone(), r),
                    (false, true) => (l.clone(), l),
                    _ => (l, r),
                };
                Ok(arithmetic(op, &l, &r, mysql_division)
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

pub(super) fn numeric_rank(sql_type: &str) -> Option<usize> {
    NUMERIC_RANKS
        .iter()
        .position(|names| names.contains(&sql_type))
}

/// Result type of `left op right` for numbers (promoted to the wider type), and for dates,
/// timestamps and intervals; a placeholder or `NULL` operand takes the other's type.
pub(super) fn arithmetic(
    op: &BinaryOperator,
    left: &Typed,
    right: &Typed,
    mysql_division: bool,
) -> Option<String> {
    use BinaryOperator::*;
    let (l, r) = (operand_type(left), operand_type(right));
    let temporal = |ty: Option<&str>| matches!(ty, Some("DATE" | "TIMESTAMP" | "TIMESTAMPTZ"));
    match (l, r) {
        (Some(a), Some(b)) if numeric_rank(a).is_some() && numeric_rank(b).is_some() => {
            let rank = numeric_rank(a)?.max(numeric_rank(b)?);
            let rank = if mysql_division { rank.max(3) } else { rank };
            Some(NUMERIC_RANKS[rank][0].to_string())
        }
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
        (Some("DATE"), Some("TIME")) if *op == Plus => Some("TIMESTAMP".to_string()),
        (Some("TIME"), Some("INTERVAL")) if matches!(op, Plus | Minus) => Some("TIME".to_string()),
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
