//! [`Lookup`]: a [`Catalog`] that finds each relation name once per analysis.

use std::cell::RefCell;
use std::ops::Deref;

use rustc_hash::FxHashMap;

use super::{Catalog, Relation, SchemaColumn};
use crate::analyzer::Column;

/// The [`Catalog`] one analysis reads, with each relation name looked up along the search
/// path once: an analysis asks for the same few tables for every column it resolves. Holds
/// one entry per distinct name of the query.
pub(crate) struct Lookup<'a> {
    catalog: &'a Catalog,
    found: RefCell<FxHashMap<Box<str>, Option<&'a Relation>>>,
}

impl<'a> Lookup<'a> {
    pub(crate) fn new(catalog: &'a Catalog) -> Self {
        Self {
            catalog,
            found: RefCell::default(),
        }
    }

    /// The catalog itself, for what lives as long as it.
    pub(crate) fn catalog(&self) -> &'a Catalog {
        self.catalog
    }

    fn find(&self, name: &str) -> Option<&'a Relation> {
        if let Some(&found) = self.found.borrow().get(name) {
            return found;
        }
        let found = self.catalog.find(name);
        self.found.borrow_mut().insert(name.into(), found);
        found
    }

    /// Columns of the table `name`, or `None` for a view, an unknown name or a table of
    /// unknown shape.
    pub(crate) fn table(&self, name: &str) -> Option<&'a [SchemaColumn]> {
        match self.find(name)? {
            Relation::Table(table) => Some(&table.columns),
            _ => None,
        }
    }

    /// The column `column` of the table `name`: `None` when `table` would be, `Some(None)`
    /// when the table has no such column.
    pub(crate) fn column(&self, name: &str, column: &str) -> Option<Option<&'a SchemaColumn>> {
        match self.find(name)? {
            Relation::Table(table) => Some(table.column(column)),
            _ => None,
        }
    }

    /// Groups of columns of the table `name` that a `CHECK` keeps at least one of
    /// non-null, such as `CHECK (num_nonnulls(a, b) = 1)`.
    pub(crate) fn nonnull_groups(&self, name: &str) -> &'a [Vec<String>] {
        match self.find(name) {
            Some(Relation::Table(table)) => &table.nonnull_groups,
            _ => &[],
        }
    }

    /// Output columns of the view `name`.
    pub(crate) fn view(&self, name: &str) -> Option<&'a [Column]> {
        match self.find(name)? {
            Relation::View(columns) => Some(columns),
            _ => None,
        }
    }

    /// Whether the schema defines a table or view `name`, whatever is known of its shape.
    pub(crate) fn defines(&self, name: &str) -> bool {
        self.find(name).is_some()
    }

    /// [`Catalog::is_unmodelled_system_relation`], with the name looked up once.
    pub(crate) fn is_unmodelled_system_relation(&self, name: &str) -> bool {
        self.find(name).is_none() && self.catalog.is_unmodelled_system_relation(name)
    }
}

impl Deref for Lookup<'_> {
    type Target = Catalog;

    fn deref(&self) -> &Catalog {
        self.catalog
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use crate::analyzer::{analyze_select, load_schema};
    use crate::catalog::visited;

    #[test]
    fn an_analysis_finds_each_table_once_whatever_it_reads_of_it() {
        let columns: Vec<String> = (0..32).map(|i| format!("c{i}")).collect();
        let schema = load_schema(
            &format!("CREATE TABLE t ({} INTEGER);", columns.join(" INTEGER, ")),
            "postgres",
        );
        let lookups = |n: usize| {
            let sql = format!("SELECT {} FROM t WHERE c0 = 1", columns[..n].join(", "));
            visited(|| {
                analyze_select(&sql, "postgres", Some(&schema)).unwrap();
            })
        };
        assert_eq!(lookups(1), lookups(32));
    }
}
