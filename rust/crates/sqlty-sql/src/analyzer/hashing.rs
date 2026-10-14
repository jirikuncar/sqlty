//! A fast, non-cryptographic hasher for keys the analyzer builds itself.
//!
//! Expressions are hashed structurally to find what a condition proved about them (see
//! [`Facts`](super::nullability::Facts)) and to key the subquery memo. The standard library's
//! SipHash defends against hash flooding, which does not apply to a query being analyzed,
//! and costs several times more than this multiply-and-rotate (the "Fx" hash of rustc).

use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{BuildHasherDefault, Hash, Hasher};

use sqlparser::ast::Expr;

const SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

#[derive(Default, Clone, Copy)]
pub(super) struct FxHasher {
    hash: u64,
}

impl FxHasher {
    fn add(&mut self, word: u64) {
        self.hash = (self.hash.rotate_left(5) ^ word).wrapping_mul(SEED);
    }
}

impl Hasher for FxHasher {
    fn write(&mut self, bytes: &[u8]) {
        let (chunks, rest) = bytes.as_chunks::<8>();
        for chunk in chunks {
            self.add(u64::from_le_bytes(*chunk));
        }
        if !rest.is_empty() {
            let mut word = [0u8; 8];
            word[..rest.len()].copy_from_slice(rest);
            self.add(u64::from_le_bytes(word));
        }
    }

    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }

    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }

    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    fn finish(&self) -> u64 {
        self.hash
    }
}

/// A `HashSet` using [`FxHasher`].
pub(super) type FxHashSet<T> = HashSet<T, BuildHasherDefault<FxHasher>>;

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
