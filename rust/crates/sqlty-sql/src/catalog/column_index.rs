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
    /// The relations a bare name finds whose name starts with `prefix`, ranked by how many
    /// of `columns` (names as a query writes them) they have, at most `limit`: those with
    /// every one first, then by count, by name within a count, the system catalogs last. A
    /// name no relation has counts for none; a relation with none is left out.
    ///
    /// The candidates are the first `limit` relations of each column's list, the rarest
    /// column's included, and the first `limit` the prefix finds; each is counted by binary
    /// search in every list. O(n² × limit × log C) for n columns: never a function of how
    /// many tables there are.
    pub fn relations_with_columns(
        &self,
        columns: &[&str],
        prefix: &str,
        limit: usize,
    ) -> Vec<(RelationRef<'_>, usize)> {
        let index = &self.index.columns;
        let by_key = &self.index.relations.by_key;
        let mut names: Vec<String> = columns.iter().map(|c| self.fold_typed(c)).collect();
        names.sort_unstable();
        names.dedup();
        let having: Vec<[&[(u32, u32)]; 2]> = names
            .iter()
            .map(|name| [&index.schema, &index.system].map(|list| self.having(list, name)))
            .filter(|lists| lists[0].len() + lists[1].len() > 0)
            .collect();
        if having.is_empty() {
            return Vec::new();
        }
        let mut candidates: Vec<u32> = having
            .iter()
            .flatten()
            .flat_map(|list| list.iter().take(limit).map(|&(r, _)| r))
            .collect();
        if !prefix.is_empty() {
            candidates.extend(
                self.visible_relations_with_prefix(prefix, limit)
                    .iter()
                    .filter_map(|relation| {
                        let key = split(relation.name.key);
                        by_key.binary_search_by(|k| split(k).cmp(&key)).ok()
                    })
                    .map(|r| r as u32),
            );
        }
        candidates.sort_unstable();
        candidates.dedup();
        let prefix = self.fold_typed(prefix);
        let mut found: Vec<(u32, usize)> = candidates
            .into_iter()
            .filter_map(|r| {
                #[cfg(test)]
                super::index::visit();
                let (schema, name) = split(&by_key[r as usize]);
                if !name.starts_with(prefix.as_str())
                    || !self
                        .index
                        .relations
                        .visible(schema, name, &self.search_path)
                {
                    return None;
                }
                let count = having
                    .iter()
                    .filter(|lists| {
                        lists
                            .iter()
                            .any(|list| list.binary_search_by_key(&r, |&(at, _)| at).is_ok())
                    })
                    .count();
                (count > 0).then_some((r, count))
            })
            .collect();
        found.sort_unstable_by_key(|&(r, count)| {
            let (schema, name) = split(&by_key[r as usize]);
            (
                std::cmp::Reverse(count),
                self.postgres && is_system(schema),
                name,
            )
        });
        found.truncate(limit);
        found
            .into_iter()
            .map(|(r, count)| (self.relation_ref(&by_key[r as usize]), count))
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
            let walked = list[start..]
                .iter()
                .map_while(|&at| Some((at, self.column_at(at)?)));
            for (at, (name, column)) in walked {
                if found.len() == limit {
                    return found;
                }
                #[cfg(test)]
                super::index::visit();
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
