//! Operator typing: numeric promotion, arithmetic and common types.

use super::*;

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
        let sql = |name: &str| Ok(from_sql_type(name.to_string(), self.dialect));
        match op {
            Gt | Lt | GtEq | LtEq | Spaceship | Eq | NotEq | And | Or | Xor | Match | Regexp
            | Glob | PGOverlap | PGRegexMatch | PGRegexIMatch | PGRegexNotMatch
            | PGRegexNotIMatch | PGLikeMatch | PGILikeMatch | PGNotLikeMatch | PGNotILikeMatch
            | PGStartsWith | AtAt | AtArrow | ArrowAt | AtQuestion | Question | QuestionAnd
            | QuestionPipe | Overlaps => sql("BOOLEAN"),
            LongArrow | HashLongArrow => sql("TEXT"),
            Arrow | HashArrow | HashMinus if self.dialect == SqlDialect::Postgres => {
                let json = self.expr(left, scope)?.1.is_some_and(|ty| ty == "JSON");
                sql(if json && *op != HashMinus {
                    "JSON"
                } else {
                    "JSONB"
                })
            }
            StringConcat if self.dialect != SqlDialect::MySql => {
                let (l, r) = (self.expr(left, scope)?, self.expr(right, scope)?);
                if l.1.as_deref() == Some("JSONB") || r.1.as_deref() == Some("JSONB") {
                    sql("JSONB")
                } else if is_text(&l) || is_text(&r) {
                    sql("TEXT")
                } else {
                    Ok((None, None))
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
                    .map_or((None, None), |ty| from_sql_type(ty, self.dialect)))
            }
            _ => Ok((None, None)),
        }
    }
}

pub(super) fn type_base(sql_type: &str) -> &str {
    sql_type.split(['(', '<']).next().unwrap_or_default().trim()
}

/// The SQL type of an operand; literals know only their Python type.
pub(super) fn operand_type(typed: &Typed) -> Option<&str> {
    match (typed.1.as_deref(), typed.0.as_deref()) {
        (Some(sql_type), _) => Some(type_base(sql_type)),
        (None, Some("int")) => Some("INT"),
        (None, Some("float")) => Some("DOUBLE"),
        (None, Some(DECIMAL)) => Some("DECIMAL"),
        _ => None,
    }
}

pub(super) fn is_text(typed: &Typed) -> bool {
    match &typed.1 {
        Some(sql_type) => matches!(type_base(sql_type), "TEXT" | "VARCHAR" | "CHAR"),
        None => typed.0.as_deref() == Some("str"),
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
    if right == (None, None) || left == right {
        return Some(left);
    }
    if left == (None, None) || (left.1.is_none() && left.0 == right.0) {
        return Some(right);
    }
    if right.1.is_none() && left.0 == right.0 {
        return Some(left);
    }
    if let (Some(x), Some(y)) = (
        left.1.as_deref().and_then(array_element),
        right.1.as_deref().and_then(array_element),
    ) {
        let elem = |ty: &str| from_sql_type(ty.to_string(), dialect);
        let elem = common_type(elem(x), elem(y), dialect)?.1?;
        return Some(from_sql_type(format!("ARRAY<{elem}>"), dialect));
    }
    let (a, b) = (category(&left)?, category(&right)?);
    const TEMPORAL: [&str; 3] = ["DATE", "TIMESTAMP", "TIMESTAMPTZ"];
    let common = if let (Some(x), Some(y)) = (numeric_rank(a), numeric_rank(b)) {
        NUMERIC_RANKS[x.max(y)][0]
    } else if is_text(&left) && is_text(&right) {
        if a == b {
            a
        } else {
            "TEXT"
        }
    } else if let (Some(x), Some(y)) = (
        TEMPORAL.iter().position(|ty| *ty == a),
        TEMPORAL.iter().position(|ty| *ty == b),
    ) {
        TEMPORAL[x.max(y)]
    } else {
        return None;
    };
    Some(from_sql_type(common.to_string(), dialect))
}

/// The type category of a set operation's input; literals know only their Python type.
pub(super) fn category(typed: &Typed) -> Option<&str> {
    match (typed.1.as_deref(), typed.0.as_deref()) {
        (None, Some("str")) => Some("TEXT"),
        _ => operand_type(typed),
    }
}
