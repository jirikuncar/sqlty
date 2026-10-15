//! Structural expression keys for the analyzer.
//!
//! Expressions are hashed structurally to find what a condition proved about them (see
//! [`Facts`](super::nullability::Facts)) and to key the subquery memo. Hash flooding does not
//! apply to a query being analyzed, so [`rustc_hash::FxHasher`] stands in for SipHash.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use rustc_hash::FxHasher;

use sqlparser::ast::Expr;

/// Identifies an expression by its structure (spans aside), without keeping a copy of it.
///
/// Two independent 64-bit hashes make the key 128 bits wide, so two different expressions
/// of one query share a key with a negligible probability. Facts hold keys, not expressions,
/// because cloning every sub-expression of a condition costs more than the lookups save.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) struct ExprKey(u64, u64);

impl ExprKey {
    pub(super) fn of(expr: &Expr) -> Self {
        let mut fast = FxHasher::default();
        expr.hash(&mut fast);
        let mut strong = DefaultHasher::new();
        expr.hash(&mut strong);
        ExprKey(fast.finish(), strong.finish())
    }
}
