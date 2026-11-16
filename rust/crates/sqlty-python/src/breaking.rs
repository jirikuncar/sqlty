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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn only_what_the_other_schema_adds_breaks() {
        let own = [(1, "unknown-column", "a")];
        let against = vec![
            (1, "unknown-column", "a"),
            (2, "unknown-column", "a"),
            (3, "stale-type", "b"),
            (4, "unused-ignore", "c"),
        ];
        let found = breaking(
            &own,
            against,
            |&(line, rule, message)| (line, rule, message),
            |found| found.1,
        );
        assert_eq!(found, [(2, "unknown-column", "a")]);
    }
}
