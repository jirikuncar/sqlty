//! Columns by name across every relation, for completing a column before any `FROM`.
//!
//! [`ColumnNames`] lists each column of each table and view once, as two `u32` positions
//! (the relation in the name index's sorted keys, the column in the relation), sorted by
//! column name then relation: a prefix is a binary search and a walk over the matches,
//! O(log C + k) for C columns, built on the first lookup. The schema's own relations and
//! PostgreSQL's system catalogs are kept apart, so the schema's columns come first without
//! walking the others.

use std::sync::Arc;

use super::index::{shape, split, Columns, NameRef};
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
            let names: Vec<&str> = match shape(&catalog.relations[key]) {
                Some(Columns::Table(columns)) => columns.iter().map(|c| c.name.as_str()).collect(),
                Some(Columns::View(columns)) => columns.iter().map(|c| c.name.as_str()).collect(),
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
        let sorted = |mut list: Vec<(&str, u32, u32)>| {
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
    /// The columns of any table or view whose name starts with `prefix`, at most `limit`:
    /// the schema's own relations first, then PostgreSQL's system catalogs, each by column
    /// name then relation.
    ///
    /// O(log C + k) for the C columns of the schema and the k walked; never a function of
    /// how many tables there are. The first call builds the index, O(C log C).
    pub fn columns_with_prefix(&self, prefix: &str, limit: usize) -> Vec<NamedColumn<'_>> {
        let prefix = self.fold_typed(prefix);
        let names = self
            .index
            .columns
            .get_or_init(|| ColumnNames::new(self, &self.index.relations.by_key));
        let mut found = Vec::new();
        for list in [&names.schema, &names.system] {
            let start = list
                .partition_point(|&at| self.column_at(at).map_or("", |c| c.0) < prefix.as_str());
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
    fn column_at(&self, (r, c): (u32, u32)) -> Option<(&str, ColumnOf<'_>)> {
        let key = &self.index.relations.by_key[r as usize];
        Some(match shape(&self.relations[key])? {
            Columns::Table(columns) => (
                &columns[c as usize].name,
                ColumnOf::Table(&columns[c as usize]),
            ),
            Columns::View(columns) => (
                &columns[c as usize].name,
                ColumnOf::View(&columns[c as usize]),
            ),
        })
    }
}
