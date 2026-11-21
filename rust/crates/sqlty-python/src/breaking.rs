//! What a schema change breaks: the diagnostics a module gets against another schema but
//! not against its own, as `sqlty breaking` reports them.

use std::collections::HashSet;
use std::hash::Hash;

/// Rules that judge the code rather than the schema.
pub const CODE_RULES: [&str; 2] = ["stale-rewrite", "unused-ignore"];

/// Rules of an annotation wider than the rows, which still reads them.
const WIDER: [&str; 3] = ["stale-type", "redundant-optional", "redundant-literal"];

/// Whether a diagnostic of `rule` can report a breaking change.
pub fn reports(rule: &str) -> bool {
    !CODE_RULES.contains(&rule) && !WIDER.contains(&rule)
}

/// The diagnostics of `against` whose `key` (line, rule and message) none of `own` has,
/// and whose `rule` can report a breaking change.
pub fn breaking<T, K: Eq + Hash>(
    own: &[T],
    against: Vec<T>,
    key: impl Fn(&T) -> K,
    rule: impl Fn(&T) -> &str,
) -> Vec<T> {
    let own: HashSet<K> = own.iter().map(&key).collect();
    against
        .into_iter()
        .filter(|found| reports(rule(found)) && !own.contains(&key(found)))
        .collect()
}
