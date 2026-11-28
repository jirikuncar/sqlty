//! Columns by name across every relation, for completing a column before any `FROM`.
//!
//! [`ColumnNames`] lists each column of each table and view once, as two `u32` positions
//! (the relation in the name index's sorted keys, the column in the relation), sorted by
//! column name then relation: a prefix is a binary search and a walk over the matches,
//! O(log C + k) for C columns, built at load in O(C log C). The schema's own relations and
//! PostgreSQL's system catalogs are kept apart, so the schema's columns come first without
//! walking the others.

use std::borrow::Cow;
use std::sync::Arc;

use super::index::{shape, split, Columns, NameRef, RelationRef};
use super::{Catalog, SchemaColumn};
use crate::analyzer::Column;

/// `(relation, column)` positions sorted by column name, then relation: the schema's own
/// relations, then the system catalogs.
#[derive(Clone, Default)]
pub(super) struct ColumnNames {
    schema: Vec<(u32, u32)>,
    system: Vec<(u32, u32)>,
}

impl ColumnNames {
    /// The columns of the relations of `keys`, sorted. O(C log C).
    pub(super) fn new(catalog: &Catalog, keys: &[Arc<str>]) -> Self {
        let mut schema = Vec::new();
        let mut system = Vec::new();
        for (r, key) in keys.iter().enumerate() {
            let names: Vec<Cow<'_, str>> = match shape(&catalog.relations[key]) {
                Some(Columns::Table(columns)) => columns
                    .iter()
                    .map(|c| Cow::Borrowed(c.name.as_str()))
                    .collect(),
                Some(Columns::View(columns)) => {
                    columns.iter().map(|c| catalog.folded(&c.name)).collect()
                }
                None => continue,
            };
            let list = if catalog.postgres && is_system(split(key).0) {
                &mut system
            } else {
                &mut schema
            };
            list.extend(
                names
                    .into_iter()
                    .enumerate()
                    .map(|(c, name)| (name, r as u32, c as u32)),
            );
        }
        let sorted = |mut list: Vec<(Cow<'_, str>, u32, u32)>| {
            list.sort_unstable();
            list.into_iter().map(|(_, r, c)| (r, c)).collect()
        };
        ColumnNames {
            schema: sorted(schema),
            system: sorted(system),
        }
    }
}

/// Whether a PostgreSQL schema holds system catalogs.
fn is_system(schema: &str) -> bool {
    matches!(schema, "pg_catalog" | "information_schema")
}

/// A column of a table or a view.
#[derive(Clone, Copy, Debug)]
pub enum ColumnOf<'a> {
    /// A table's column.
    Table(&'a SchemaColumn),
    /// A view's output column.
    View(&'a Column),
}

/// A column and the relation that has it.
#[derive(Clone, Copy, Debug)]
pub struct NamedColumn<'a> {
    /// The relation.
    pub relation: NameRef<'a>,
    /// How a query names the relation: bare when a bare name finds it and it is no system
    /// catalog, else `schema.name`.
    pub qualified: bool,
    /// The column.
    pub column: ColumnOf<'a>,
}

impl Catalog {
    /// The relations a bare name finds whose name starts with `prefix` and that have the
    /// most of `columns` (names as a query writes them), at most `limit`: those with every
    /// one first, by schema and name, then by how many they have and by name, the system
    /// catalogs last in each group. A name no
    /// relation has counts for none.
    ///
    /// Relations with every column come from a walk of the shortest list of relations
    /// having one, each checked in the others by binary search, until `limit` are found;
    /// the others from the first `limit` of each list. So the cost grows with the relations
    /// having a column, never with how many tables there are.
    pub fn relations_with_columns(
        &self,
        columns: &[&str],
        prefix: &str,
        limit: usize,
    ) -> Vec<(RelationRef<'_>, usize)> {
        let index = &self.index.columns;
        let prefix = self.fold_typed(prefix);
        let mut names: Vec<String> = columns.iter().map(|c| self.fold_typed(c)).collect();
        names.sort_unstable();
        names.dedup();
        let mut having: Vec<[&[(u32, u32)]; 2]> = names
            .iter()
            .map(|name| [&index.schema, &index.system].map(|list| self.having(list, name)))
            .filter(|lists| lists[0].len() + lists[1].len() > 0)
            .collect();
        having.sort_by_key(|lists| lists[0].len() + lists[1].len());
        let count = |r: u32| {
            having
                .iter()
                .filter(|lists| {
                    lists
                        .iter()
                        .any(|list| list.binary_search_by_key(&r, |&(at, _)| at).is_ok())
                })
                .count()
        };
        let mut found: Vec<(u32, usize)> = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();
        let mut walk = |list: &[(u32, u32)], all: bool, found: &mut Vec<(u32, usize)>| {
            for (examined, &(r, _)) in list.iter().enumerate() {
                if (!all && examined == limit) || (all && found.len() == limit) {
                    break;
                }
                #[cfg(test)]
                super::index::visit();
                let (schema, name) = split(&self.index.relations.by_key[r as usize]);
                if seen.contains(&r)
                    || !name.starts_with(prefix.as_str())
                    || !self
                        .index
                        .relations
                        .visible(schema, name, &self.search_path)
                {
                    continue;
                }
                let n = count(r);
                if !all || n == having.len() {
                    seen.insert(r);
                    found.push((r, n));
                }
            }
        };
        if let Some(shortest) = having.first() {
            for list in shortest {
                walk(list, true, &mut found);
            }
        }
        let full = found.len();
        if having.len() > 1 {
            for lists in &having {
                for list in lists {
                    walk(list, false, &mut found);
                }
            }
        }
        found[full..].sort_unstable_by_key(|&(r, n)| {
            let (schema, name) = split(&self.index.relations.by_key[r as usize]);
            (
                std::cmp::Reverse(n),
                self.postgres && is_system(schema),
                name,
            )
        });
        found.truncate(limit);
        found
            .into_iter()
            .map(|(r, n)| {
                (
                    self.relation_ref(&self.index.relations.by_key[r as usize]),
                    n,
                )
            })
            .collect()
    }

    /// The entries of `list` whose column is `name`, sorted by relation.
    fn having<'a>(&self, list: &'a [(u32, u32)], name: &str) -> &'a [(u32, u32)] {
        let start = list.partition_point(|&at| self.column_at(at).is_none_or(|c| *c.0 < *name));
        let end = start
            + list[start..]
                .partition_point(|&at| self.column_at(at).is_some_and(|c| *c.0 == *name));
        &list[start..end]
    }

    /// The columns of any table or view whose name starts with `prefix`, at most `limit`:
    /// the schema's own relations first, then PostgreSQL's system catalogs, each by column
    /// name then relation.
    ///
    /// O(log C + k) for the C columns of the schema and the k walked; never a function of
    /// how many tables there are.
    pub fn columns_with_prefix(&self, prefix: &str, limit: usize) -> Vec<NamedColumn<'_>> {
        let prefix = self.fold_typed(prefix);
        let names = &self.index.columns;
        let mut found = Vec::new();
        for list in [&names.schema, &names.system] {
            let start =
                list.partition_point(|&at| self.column_at(at).is_none_or(|c| *c.0 < *prefix));
            for &at in &list[start..] {
                if found.len() == limit {
                    return found;
                }
                #[cfg(test)]
                super::index::visit();
                let Some((name, column)) = self.column_at(at) else {
                    break;
                };
                if !name.starts_with(prefix.as_str()) {
                    break;
                }
                let relation = self.name_ref(&self.index.relations.by_key[at.0 as usize]);
                let qualified = relation.schema.is_some_and(|schema| {
                    is_system(schema)
                        || !self
                            .index
                            .relations
                            .visible(schema, relation.name, &self.search_path)
                });
                found.push(NamedColumn {
                    relation,
                    qualified,
                    column,
                });
            }
        }
        found
    }

    /// The name and the column at `(relation, column)` of the column index, which holds
    /// no relation of unknown shape.
    fn column_at(&self, (r, c): (u32, u32)) -> Option<(Cow<'_, str>, ColumnOf<'_>)> {
        let key = &self.index.relations.by_key[r as usize];
        Some(match shape(&self.relations[key])? {
            Columns::Table(columns) => (
                Cow::Borrowed(columns[c as usize].name.as_str()),
                ColumnOf::Table(&columns[c as usize]),
            ),
            Columns::View(columns) => (
                self.folded(&columns[c as usize].name),
                ColumnOf::View(&columns[c as usize]),
            ),
        })
    }

    /// A view column's name as the index sorts it: lower case outside PostgreSQL, where
    /// typed names compare without case and a view keeps the case of its aliases.
    fn folded<'a>(&self, name: &'a str) -> Cow<'a, str> {
        if self.postgres || !name.contains(|c: char| c.is_uppercase()) {
            Cow::Borrowed(name)
        } else {
            Cow::Owned(name.to_lowercase())
        }
    }
}
