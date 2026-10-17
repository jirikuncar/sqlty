//! Inference rules: the result type and nullability of operators, declared in
//! `rules/**/*.inference.toml` and compiled into static data by `build.rs`.
//!
//! A rule applies to an operator in some dialects (all when it lists none) and to operands
//! whose SQL types it lists (any when it lists none). Dialect rules are tried before the
//! ones in `rules/common/`, each in file order, and the first that applies is used. An
//! operator no rule covers is typed by the analyzer's code, as before rules existed.

use sqlparser::ast::BinaryOperator;

use super::dialect::SqlDialect;

/// The result type of a rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuleType {
    /// A SQL type, such as `TEXT`.
    Sql(&'static str),
    /// Any value: `typing.Any`, with no SQL type.
    Any,
    /// Not known: the expression stays untyped.
    Unknown,
    /// Typed by the analyzer's code: the rule gives the nullability only, for operands
    /// whose type depends on more than their SQL types (an interval's unit, a date).
    Code,
    /// The type of the left operand.
    Left,
    /// The type of the right operand.
    Right,
    /// The common type of both operands, as `CASE` and `UNION` unify them.
    Common,
    /// The wider of two numbers (`SMALLINT` < `INT` < `BIGINT` < `DECIMAL` < `REAL` <
    /// `DOUBLE`).
    Promote,
    /// [`RuleType::Promote`], and at least `DECIMAL`: MySQL's `/`.
    PromoteToDecimal,
    /// The canonical type of the left number's rank: a shift keeps its left operand's type.
    PromoteLeft,
}

/// How a rule's result can be NULL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuleNullability {
    /// NULL when an operand is.
    Operands,
    /// Can be NULL whatever the operands are, such as a JSON member that is missing.
    Nullable,
    /// Never NULL.
    NonNull,
    /// `AND` and `OR`: [`RuleNullability::Operands`], the right side read under what the
    /// left one proves (true for `AND`, not true for `OR`).
    ShortCircuit,
    /// A division: NULL for a zero divisor, unless the divisor is a number other than zero;
    /// else [`RuleNullability::Operands`].
    NonzeroDivisor,
}

/// An inference rule for a binary operator.
#[derive(Debug)]
#[allow(
    dead_code,
    reason = "`id`, `reference` and `reasoning` are for `--explain`"
)]
pub(crate) struct BinaryRule {
    /// Unique, such as `postgres.json.long-arrow`.
    pub(crate) id: &'static str,
    /// The documentation the rule follows: name, version and section.
    pub(crate) reference: &'static str,
    /// Why the rule types the operator so, in one sentence.
    pub(crate) reasoning: &'static str,
    /// The dialects the rule applies to; empty for all.
    dialects: &'static [SqlDialect],
    /// The SQL types the left operand may have; `None` for any.
    left: Option<&'static [&'static str]>,
    /// The SQL types the right operand may have; `None` for any.
    right: Option<&'static [&'static str]>,
    pub(crate) result: RuleType,
    pub(crate) nullability: RuleNullability,
}

impl BinaryRule {
    /// Whether the rule needs the operand types to decide whether it applies.
    pub(crate) fn reads_operands(&self) -> bool {
        self.left.is_some() || self.right.is_some()
    }

    /// Whether the rule applies in `dialect` to operands of these SQL types.
    pub(crate) fn applies(
        &self,
        dialect: SqlDialect,
        left: Option<&str>,
        right: Option<&str>,
    ) -> bool {
        let admits = |types: Option<&[&str]>, operand: Option<&str>| {
            types.is_none_or(|types| operand.is_some_and(|operand| types.contains(&operand)))
        };
        (self.dialects.is_empty() || self.dialects.contains(&dialect))
            && admits(self.left, left)
            && admits(self.right, right)
    }
}

include!(concat!(env!("OUT_DIR"), "/inference_rules.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_rule_is_documented() {
        for rule in BINARY_RULES {
            assert!(!rule.reference.is_empty(), "{} has no reference", rule.id);
            assert!(!rule.reasoning.is_empty(), "{} has no reasoning", rule.id);
        }
    }

    #[test]
    fn dialect_rules_come_first() {
        let first = |dialect: SqlDialect| {
            binary_rules(&BinaryOperator::LongArrow)
                .iter()
                .find(|rule| rule.applies(dialect, None, None))
                .map(|rule| rule.id)
        };
        assert_eq!(first(SqlDialect::Sqlite), Some("sqlite.json.long-arrow"));
        assert_eq!(first(SqlDialect::Postgres), Some("common.json.long-arrow"));
    }
}
