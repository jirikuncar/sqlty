//! The tables a foreign key links to a table, for join completion.
//!
//! A table keeps its own foreign keys, child to parent. [`ReferencedBy`] is the reverse,
//! built once by `load`: the keys pointing at each table, sorted by parent and child name,
//! so the children of a table whose name starts with a prefix are a binary search and a
//! walk over the matches.

use std::sync::Arc;

use super::index::split;
use super::{Catalog, NameRef, Relation};

/// A table a foreign key links to another: joined, `pairs` of `(its column, the other
/// table's column)` are equal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Join<'a> {
    /// The linked table.
    pub table: NameRef<'a>,
    /// Its columns, each with the column of the other table it equals.
    pub pairs: Vec<(&'a str, &'a str)>,
}

/// Foreign keys by the table they reference: `(parent key, child key, index of the key in
/// the child's list)`, sorted by parent, then child name.
#[derive(Default)]
pub(super) struct ReferencedBy(Vec<(Arc<str>, Arc<str>, usize)>);

impl ReferencedBy {
    /// The reverse of `catalog`'s foreign keys. O(F log F) for F keys.
    pub(super) fn new(catalog: &Catalog) -> Self {
        let mut found = Vec::new();
        for (child, relation) in &catalog.relations {
            let Relation::Table(table) = relation else {
                continue;
            };
            for (i, fk) in table.foreign_keys.iter().enumerate() {
                if let Some((parent, _)) = catalog.relations.get_key_value(fk.table.as_str()) {
                    found.push((Arc::clone(parent), Arc::clone(child), i));
                }
            }
        }
        found.sort_unstable_by(|a, b| {
            (&*a.0, split(&a.1).1, &*a.1, a.2).cmp(&(&*b.0, split(&b.1).1, &*b.1, b.2))
        });
        ReferencedBy(found)
    }
}

impl Catalog {
    /// The tables a foreign key links to the table `name`, either way, whose name starts with
    /// `prefix`: the tables it references, then those referencing it, at most `limit`.
    ///
    /// O(K + log F + k): K foreign keys of the table, a binary search over the F of the
    /// schema, and k children walked; never a function of how many tables there are.
    pub fn joins_with_prefix(&self, name: &str, prefix: &str, limit: usize) -> Vec<Join<'_>> {
        let prefix = self.fold_typed(prefix);
        let Some(key) = self.table_id(name) else {
            return Vec::new();
        };
        let Some(Relation::Table(table)) = self.relations.get(key) else {
            return Vec::new();
        };
        let mut found: Vec<Join<'_>> = table
            .foreign_keys
            .iter()
            .filter_map(|fk| {
                let (parent, _) = self.relations.get_key_value(fk.table.as_str())?;
                Some(Join {
                    table: self.name_ref(parent),
                    pairs: fk
                        .referred
                        .iter()
                        .map(String::as_str)
                        .zip(fk.columns.iter().map(String::as_str))
                        .collect(),
                })
            })
            .filter(|join| join.table.name.starts_with(&prefix))
            .take(limit)
            .collect();
        let children = &self.index.referenced_by.0;
        let start = children.partition_point(|(parent, child, _)| {
            (&**parent, split(child).1) < (key, prefix.as_str())
        });
        for (parent, child, i) in &children[start..] {
            if found.len() == limit || &**parent != key || !split(child).1.starts_with(&prefix) {
                break;
            }
            #[cfg(test)]
            super::index::visit();
            if let Some(Relation::Table(table)) = self.relations.get(&**child) {
                let fk = &table.foreign_keys[*i];
                found.push(Join {
                    table: self.name_ref(child),
                    pairs: fk
                        .columns
                        .iter()
                        .map(String::as_str)
                        .zip(fk.referred.iter().map(String::as_str))
                        .collect(),
                });
            }
        }
        found
    }
}
