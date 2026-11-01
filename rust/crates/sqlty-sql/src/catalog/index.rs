//! Prefix lookups: the relations, functions and types whose name starts with what was
//! typed, for completion.
//!
//! A hash map answers "is there a `users`" but not "what starts with `us`" without
//! scanning every key. [`NameIndex`] keeps each kind of name sorted twice, by bare name and
//! by `schema.name`, so a prefix is a binary search and a walk over the matches:
//! O(log N + k). It holds the catalog's own `Arc<str>` keys, so it adds pointers, not
//! copies of the names. `load` builds it once.

use std::sync::Arc;

use super::{Catalog, Relation, SchemaColumn};
use crate::analyzer::Column;

#[cfg(test)]
thread_local! {
    /// How many catalog entries lookups reached on this thread: sorted entries a prefix
    /// lookup walked, and relations looked up by name.
    static VISITED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Count one catalog entry reached.
#[cfg(test)]
pub(super) fn visit() {
    VISITED.with(|count| count.set(count.get() + 1));
}

/// How many catalog entries the lookups of `run` reach.
#[cfg(test)]
pub(crate) fn visited(run: impl FnOnce()) -> usize {
    VISITED.with(|count| count.set(0));
    run();
    VISITED.with(std::cell::Cell::get)
}

/// A name the catalog defines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NameRef<'a> {
    /// The catalog key, `schema.name`, which [`Catalog::columns_of`] takes.
    pub key: &'a str,
    /// The schema; `None` outside PostgreSQL, where names have none.
    pub schema: Option<&'a str>,
    /// The name, folded as the catalog folds it.
    pub name: &'a str,
}

/// What a relation is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelationKind {
    /// A table, with known columns.
    Table,
    /// A view, with known columns.
    View,
    /// Defined, but its shape is not known.
    Unknown,
}

/// A relation the catalog defines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelationRef<'a> {
    /// Its name.
    pub name: NameRef<'a>,
    /// Table, view, or unknown shape.
    pub kind: RelationKind,
}

/// The columns of a relation, as the catalog stores them.
#[derive(Clone, Copy, Debug)]
pub enum Columns<'a> {
    /// A table's columns.
    Table(&'a [SchemaColumn]),
    /// A view's output columns.
    View(&'a [Column]),
}

impl Catalog {
    /// Relations whose name starts with `prefix`, at most `limit`, by name. With a
    /// `schema` (PostgreSQL only), those of that schema; without, those a bare name finds:
    /// `pg_catalog`, then the `search_path`, the first schema defining a name hiding the
    /// others. `"Us` keeps its case in PostgreSQL; an unquoted prefix folds to lower case.
    ///
    /// O(log N + k), where k counts the entries walked: the matches, and in the
    /// unqualified case the same names in schemas a bare name does not reach.
    pub fn relations_with_prefix(
        &self,
        prefix: &str,
        schema: Option<&str>,
        limit: usize,
    ) -> Vec<RelationRef<'_>> {
        self.with_prefix(&self.index.relations, prefix, schema, limit)
            .map(|name| RelationRef {
                kind: match self.relations[name.key] {
                    Relation::Table(_) => RelationKind::Table,
                    Relation::View(_) => RelationKind::View,
                    Relation::Unknown => RelationKind::Unknown,
                },
                name,
            })
            .collect()
    }

    /// Functions whose name starts with `prefix`, those of unknown shape included; see
    /// [`Catalog::relations_with_prefix`].
    pub fn functions_with_prefix(
        &self,
        prefix: &str,
        schema: Option<&str>,
        limit: usize,
    ) -> Vec<NameRef<'_>> {
        self.with_prefix(&self.index.functions, prefix, schema, limit)
            .collect()
    }

    /// Domains, enums and composite types whose name starts with `prefix`; see
    /// [`Catalog::relations_with_prefix`].
    pub fn types_with_prefix(
        &self,
        prefix: &str,
        schema: Option<&str>,
        limit: usize,
    ) -> Vec<NameRef<'_>> {
        self.with_prefix(&self.index.types, prefix, schema, limit)
            .collect()
    }

    /// The columns of the table or view `name` (a [`NameRef::key`], or a name as a query
    /// writes it); `None` for an unknown name or a relation of unknown shape. O(1).
    pub fn columns_of(&self, name: &str) -> Option<Columns<'_>> {
        match self.find(name)? {
            Relation::Table(table) => Some(Columns::Table(&table.columns)),
            Relation::View(columns) => Some(Columns::View(columns)),
            Relation::Unknown => None,
        }
    }

    fn with_prefix<'a>(
        &'a self,
        sorted: &'a Sorted,
        prefix: &str,
        schema: Option<&str>,
        limit: usize,
    ) -> impl Iterator<Item = NameRef<'a>> {
        let prefix = self.fold_typed(prefix);
        let schema = schema
            .filter(|_| self.postgres)
            .map(|schema| self.fold_typed(schema));
        sorted
            .matching(&prefix, schema.as_deref(), &self.search_path, limit)
            .into_iter()
            .map(move |key| {
                let (schema, name) = split(key);
                NameRef {
                    key,
                    schema: self.postgres.then_some(schema),
                    name,
                }
            })
    }

    /// Typed text as the catalog keys it: PostgreSQL keeps the case of a quoted name and
    /// folds an unquoted one; other dialects compare names case-insensitively.
    pub(crate) fn fold_typed(&self, text: &str) -> String {
        if !self.postgres {
            return text
                .trim_start_matches(['"', '`', '['])
                .trim_end_matches(['"', '`', ']'])
                .to_lowercase();
        }
        match text.strip_prefix('"') {
            Some(quoted) => quoted.strip_suffix('"').unwrap_or(quoted).to_string(),
            None => text.to_lowercase(),
        }
    }
}

/// The names of each kind, sorted for prefix lookups.
#[derive(Default)]
pub(super) struct NameIndex {
    relations: Sorted,
    functions: Sorted,
    types: Sorted,
}

impl NameIndex {
    /// The index of `catalog`'s names. O(N log N).
    pub(super) fn new(catalog: &Catalog) -> Self {
        NameIndex {
            relations: Sorted::new(catalog.relations.keys()),
            functions: Sorted::new(
                catalog
                    .functions
                    .keys()
                    .chain(catalog.unknown_functions.keys()),
            ),
            types: Sorted::new(catalog.type_names.keys()),
        }
    }
}

/// `schema.name` keys, sorted by `(schema, name)` and by `(name, key)`.
#[derive(Default)]
struct Sorted {
    by_key: Vec<Arc<str>>,
    by_name: Vec<Arc<str>>,
}

impl Sorted {
    fn new<'a>(keys: impl Iterator<Item = &'a Arc<str>>) -> Self {
        let mut by_key: Vec<Arc<str>> = keys.cloned().collect();
        by_key.sort_unstable_by(|a, b| split(a).cmp(&split(b)));
        by_key.dedup();
        let mut by_name = by_key.clone();
        by_name.sort_unstable_by(|a, b| split(a).1.cmp(split(b).1).then_with(|| a.cmp(b)));
        Sorted { by_key, by_name }
    }

    /// Keys whose name starts with `prefix`: in `schema`, or with `None` those a bare name
    /// finds along `path`. At most `limit`.
    fn matching(
        &self,
        prefix: &str,
        schema: Option<&str>,
        path: &[String],
        limit: usize,
    ) -> Vec<&str> {
        let (sorted, start) = match schema {
            Some(schema) => (
                &self.by_key,
                self.by_key
                    .partition_point(|key| split(key) < (schema, prefix)),
            ),
            None => (
                &self.by_name,
                self.by_name.partition_point(|key| split(key).1 < prefix),
            ),
        };
        let mut found = Vec::new();
        for key in &sorted[start..] {
            if found.len() == limit {
                break;
            }
            #[cfg(test)]
            visit();
            let (key_schema, name) = split(key);
            if !name.starts_with(prefix) || schema.is_some_and(|schema| schema != key_schema) {
                break;
            }
            if schema.is_some() || self.visible(key_schema, name, path) {
                found.push(&**key);
            }
        }
        found
    }

    /// Whether a bare `name` finds the one in `schema`: no schema before it on `path`
    /// defines the name. O(path length × log N).
    fn visible(&self, schema: &str, name: &str, path: &[String]) -> bool {
        std::iter::once("pg_catalog")
            .chain(path.iter().map(String::as_str))
            .find(|on_path| *on_path == schema || self.contains(on_path, name))
            == Some(schema)
    }

    fn contains(&self, schema: &str, name: &str) -> bool {
        self.by_key
            .binary_search_by(|key| split(key).cmp(&(schema, name)))
            .is_ok()
    }
}

/// `schema.name` as `(schema, name)`.
fn split(key: &str) -> (&str, &str) {
    key.split_once('.').unwrap_or(("", key))
}

#[cfg(test)]
mod tests {
    use super::super::load;
    use super::*;

    fn names(found: &[RelationRef<'_>]) -> Vec<String> {
        found
            .iter()
            .map(|r| match r.name.schema {
                Some(schema) => format!("{schema}.{}:{:?}", r.name.name, r.kind),
                None => format!("{}:{:?}", r.name.name, r.kind),
            })
            .collect()
    }

    fn pg(sql: &str) -> Catalog {
        load(sql, "postgres").0
    }

    #[test]
    fn prefix_folds_like_the_catalog() {
        let c = pg(
            r#"CREATE TABLE Users (id int); CREATE TABLE "UserLog" (id int);
            CREATE TABLE usage (id int);"#,
        );
        let found = c.relations_with_prefix("US", None, 10);
        assert_eq!(names(&found), ["public.usage:Table", "public.users:Table"]);
        let found = c.relations_with_prefix("\"User", None, 10);
        assert_eq!(names(&found), ["public.UserLog:Table"]);
        assert!(c.relations_with_prefix("\"userl", None, 10).is_empty());
    }

    #[test]
    fn bare_prefix_follows_the_search_path() {
        let c = pg("CREATE SCHEMA app; SET search_path TO app, public;
            CREATE TABLE users (id int); CREATE TABLE public.users (id text);
            CREATE TABLE public.user_roles (id int); CREATE TABLE other.user_secrets (id int);");
        let found = c.relations_with_prefix("user", None, 10);
        assert_eq!(
            names(&found),
            ["public.user_roles:Table", "app.users:Table"]
        );
        let found = c.relations_with_prefix("pg_clas", None, 10);
        assert_eq!(names(&found), ["pg_catalog.pg_class:Table"]);
    }

    #[test]
    fn schema_narrows_to_one_schema() {
        let c = pg(
            "CREATE TABLE users (id int); CREATE TABLE other.users (id int);
            CREATE TABLE other.user_secrets (id int); CREATE TABLE \"Other\".users (id int);",
        );
        let found = c.relations_with_prefix("us", Some("other"), 10);
        assert_eq!(
            names(&found),
            ["other.user_secrets:Table", "other.users:Table"]
        );
        let found = c.relations_with_prefix("", Some("OTHER"), 10);
        assert_eq!(found.len(), 2);
        let found = c.relations_with_prefix("", Some("\"Other\""), 10);
        assert_eq!(names(&found), ["Other.users:Table"]);
        let found = c.relations_with_prefix("col", Some("information_schema"), 10);
        assert_eq!(names(&found), ["information_schema.columns:Table"]);
        assert!(c.relations_with_prefix("col", None, 10).is_empty());
    }

    #[test]
    fn views_and_unknown_shapes() {
        let c = pg(
            "CREATE TABLE ta (id int, name text); CREATE VIEW tv AS SELECT id FROM ta;
            CREATE TABLE tu (LIKE missing);",
        );
        let found = c.relations_with_prefix("t", None, 10);
        assert_eq!(
            names(&found),
            ["public.ta:Table", "public.tu:Unknown", "public.tv:View"]
        );
        assert!(
            matches!(c.columns_of(found[0].name.key), Some(Columns::Table(cols)) if cols.len() == 2)
        );
        assert!(matches!(c.columns_of("tv"), Some(Columns::View(cols)) if cols.len() == 1));
        assert!(c.columns_of(found[1].name.key).is_none());
        assert!(c.columns_of("missing").is_none());
    }

    #[test]
    fn limit_caps_the_result() {
        let c = pg("CREATE TABLE a1 (id int); CREATE TABLE a2 (id int); CREATE TABLE a3 (id int);");
        assert_eq!(c.relations_with_prefix("a", None, 2).len(), 2);
        assert!(c.relations_with_prefix("a", None, 0).is_empty());
    }

    #[test]
    fn other_dialects_have_no_schemas() {
        for dialect in ["mysql", "sqlite"] {
            let (c, _) = load(
                "CREATE TABLE Users (id INT); CREATE TABLE db.usage (id INT);",
                dialect,
            );
            let found = c.relations_with_prefix("`US", Some("db"), 10);
            assert_eq!(names(&found), ["usage:Table", "users:Table"], "{dialect}");
            assert_eq!(found[1].name.key, "public.users");
            assert!(c.columns_of(found[1].name.key).is_some(), "{dialect}");
        }
    }

    #[test]
    fn functions_and_types() {
        let c = pg(
            "CREATE FUNCTION total() RETURNS int LANGUAGE sql AS 'select 1';
            CREATE FUNCTION tally() LANGUAGE sql AS 'select 1';
            CREATE TYPE tone AS ENUM ('a'); CREATE TYPE tpoint AS (x int);
            CREATE DOMAIN tpct AS int;",
        );
        let functions: Vec<_> = c
            .functions_with_prefix("t", Some("public"), 10)
            .iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(functions, ["tally", "total"]);
        assert_eq!(
            c.functions_with_prefix("tot", None, 10)[0].key,
            "public.total"
        );
        let types: Vec<_> = c
            .types_with_prefix("t", None, 10)
            .iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(types, ["tone", "tpct", "tpoint"]);
    }

    /// A lookup walks the matches it returns (and the entry that ends them), never the
    /// rest of the catalog: the counts are the same at 50, 540 and 5000 tables.
    #[test]
    fn lookups_do_not_depend_on_the_number_of_tables() {
        let counts: Vec<_> = [50, 540, 5000]
            .into_iter()
            .map(|tables| {
                let mut sql = String::from("CREATE TABLE other.users (id int);\n");
                for i in 0..tables {
                    sql += &format!(
                        "CREATE TABLE t{i} (id int);\nCREATE TABLE other.t{i} (id int);\n"
                    );
                }
                sql += "CREATE TABLE users (id int);\n";
                let c = pg(&sql);
                let user =
                    visited(|| assert_eq!(c.relations_with_prefix("user", None, 10).len(), 1));
                let capped =
                    visited(|| assert_eq!(c.relations_with_prefix("t1", None, 5).len(), 5));
                let qualified = visited(|| {
                    assert_eq!(c.relations_with_prefix("t1", Some("other"), 5).len(), 5);
                });
                let none = visited(|| assert!(c.relations_with_prefix("zz", None, 10).is_empty()));
                (user, capped, qualified, none)
            })
            .collect();
        assert_eq!(counts[0], counts[1]);
        assert_eq!(counts[1], counts[2]);
        assert!(
            counts[2].0 <= 3 && counts[2].1 <= 10 && counts[2].2 <= 5,
            "{counts:?}"
        );
    }
}
