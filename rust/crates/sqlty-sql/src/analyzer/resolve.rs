//! Column lookup, and the reasons a name stays unresolved.
//!
//! Every lookup that fails or yields an untyped column records why into the analyzer's
//! [`Diagnostics`](crate::diagnostics::Diagnostics): an unknown table, an unknown or
//! ambiguous column, a source the analyzer does not read. Names are only judged when a
//! schema is configured; without one every table and column is unknown.

use std::rc::Rc;

use sqlparser::ast::{Expr, Ident};

use crate::catalog::fold;
use crate::diagnostics::NameRule;

use super::dialect::SqlDialect;
use super::literals::value_set;
use super::nullability::Nullability;
use super::query::{Analyzer, Column};
use super::scope::{resolve, Scope, Source, Table};
use super::typed::Typed;
use super::SchemaColumn;

/// Where a resolved column comes from, for explaining why it is untyped.
pub(super) enum Origin {
    /// A schema table's column, as `table.column`.
    Schema(String),
    /// A schema view's column, as `view.column`.
    View(String),
    /// A CTE or derived table of the query, whose own query explains it.
    Local,
}

/// Sources a name can resolve to: the FROM clause, then enclosing queries (correlation).
fn sources<'s>(scope: &'s Scope, outer: &'s Scope) -> impl Iterator<Item = &'s Source> {
    scope.iter().chain(outer.iter())
}

/// A column found in one source, borrowed from where it is declared.
enum ColumnRef<'s> {
    /// A column of a CTE, derived table or table function (`local`), or of a schema view.
    Virtual { column: &'s Column, local: bool },
    /// A column of a schema table.
    Schema(&'s SchemaColumn),
    /// A column every table has without declaring it.
    System(Typed),
}

impl ColumnRef<'_> {
    fn typed(&self, dialect: SqlDialect) -> Typed {
        match self {
            ColumnRef::Virtual { column, .. } => column.typed(),
            ColumnRef::Schema(column) => column.typed(dialect),
            ColumnRef::System(typed) => typed.clone(),
        }
    }

    /// Whether the source says the column can be NULL: a schema column lacking `NOT NULL` /
    /// `PRIMARY KEY`, or a column its CTE, view or derived table marks nullable.
    fn nullability(&self) -> Nullability {
        match self {
            ColumnRef::Virtual { column, .. } => column.nullability,
            ColumnRef::Schema(column) => Nullability::declared(column.not_null),
            ColumnRef::System(_) => Nullability::NonNull,
        }
    }
}

/// A column reference resolved to the source it reads.
struct Resolved<'s> {
    /// The alias or table name the source is read by.
    reference: String,
    /// The key of the source's table.
    table: String,
    /// The column, as the resolver names it (case folded).
    column: String,
    found: ColumnRef<'s>,
}

impl<'a> Analyzer<'a> {
    /// Whether table, column and ambiguity rules apply: a schema is configured and the
    /// dialect's name rules (case folding, quoting) are the ones modelled.
    pub(super) fn judges_names(&self) -> bool {
        !self.schema.is_empty()
            && matches!(
                self.dialect,
                SqlDialect::Postgres | SqlDialect::MySql | SqlDialect::Sqlite
            )
    }

    /// Whether a CTE, view or subquery column called `name` is the one `column` (folded)
    /// names: PostgreSQL compares exactly, the other dialects ignore case.
    pub(super) fn same_column(&self, name: &str, column: &str) -> bool {
        if self.dialect == SqlDialect::Postgres {
            name == column
        } else {
            name.to_lowercase() == column
        }
    }

    /// Whether `table` is a schema table of known columns, a view, CTE or derived table.
    fn knows_table(&self, table: &str) -> bool {
        self.virtual_columns(table).is_some() || self.schema.table(table).is_some()
    }

    /// Records `unknown-table` when a FROM item names no schema table, view or CTE.
    pub(super) fn check_table(&self, table: &Table) {
        if !self.diag.enabled() {
            return;
        }
        let Some(table) = table.named() else {
            return;
        };
        if self.knows_table(table) || self.schema.defines(table) {
            return;
        }
        if self.judges_names() {
            self.diag.record(
                NameRule::UnknownTable,
                format!("table `{table}` is not defined in the schema"),
            );
        } else {
            self.diag.explain_silently();
        }
    }

    /// `table.column`, built only when diagnostics are collected.
    fn label(&self, table: &str, column: &str) -> String {
        if self.diag.enabled() {
            format!("{table}.{column}")
        } else {
            String::new()
        }
    }

    /// A column of one source, when it has it.
    fn find_column(&self, table: &str, column: &str) -> Option<ColumnRef<'_>> {
        if let Some(columns) = self.virtual_columns(table) {
            return columns
                .iter()
                .find(|col| self.same_column(&col.name, column))
                .map(|column| ColumnRef::Virtual {
                    column,
                    local: self.virtuals.contains_key(table),
                });
        }
        let columns = self.schema.table(table)?;
        match columns.iter().find(|col| col.name == column) {
            Some(col) => Some(ColumnRef::Schema(col)),
            None => self.system_column(column).map(ColumnRef::System),
        }
    }

    /// Where a column found in `table` comes from, for explaining why it is untyped.
    fn origin(&self, found: &ColumnRef<'_>, table: &str, column: &str) -> Origin {
        match found {
            ColumnRef::Virtual { local: false, .. } => Origin::View(self.label(table, column)),
            ColumnRef::Schema(_) => Origin::Schema(self.label(table, column)),
            ColumnRef::Virtual { .. } | ColumnRef::System(_) => Origin::Local,
        }
    }

    /// A column every table has without declaring it: SQLite's `rowid`, PostgreSQL's
    /// `ctid`, `xmin`, `xmax`, `cmin`, `cmax` and `tableoid`. PostgreSQL 12 dropped `oid`.
    /// PostgreSQL drivers load `tid`, `xid` and `cid` as text.
    fn system_column(&self, column: &str) -> Option<Typed> {
        let typed = |python: &str, sql: &str| Typed {
            python: Some(Rc::from(python)),
            sql: Some(Rc::from(sql)),
            values: None,
        };
        match (self.dialect, column) {
            (SqlDialect::Sqlite, "rowid" | "oid" | "_rowid_") => Some(typed("int", "INT")),
            (SqlDialect::Postgres, "tableoid") => Some(typed("int", "OID")),
            (SqlDialect::Postgres, "ctid") => Some(typed("str", "TID")),
            (SqlDialect::Postgres, "xmin" | "xmax") => Some(typed("str", "XID")),
            (SqlDialect::Postgres, "cmin" | "cmax") => Some(typed("str", "CID")),
            _ => None,
        }
    }

    /// The source a column reference reads and the column in it: its qualifier's source, else
    /// the first source having it, the FROM clause before enclosing queries.
    fn resolve_reference(&self, expr: &Expr, scope: &Scope) -> Option<Resolved<'_>> {
        match expr {
            Expr::Identifier(ident) => {
                let column = fold(ident, self.dialect);
                sources(scope, &self.outer).find_map(|source| {
                    let found = self.find_column(source.table.named()?, &column)?;
                    Some(Resolved {
                        reference: source.reference.clone(),
                        table: source.table.named()?.to_string(),
                        column: column.clone(),
                        found,
                    })
                })
            }
            Expr::CompoundIdentifier(parts) if parts.len() >= 2 => {
                let qualifier = parts[parts.len() - 2].value.to_lowercase();
                let column = fold(&parts[parts.len() - 1], self.dialect);
                for sources in [scope, &self.outer] {
                    if sources.iter().any(|source| source.reference == qualifier) {
                        let table = resolve(&qualifier, sources);
                        let table = table.named()?;
                        let found = self.find_column(table, &column)?;
                        return Some(Resolved {
                            reference: qualifier,
                            table: table.to_string(),
                            column,
                            found,
                        });
                    }
                }
                let found = self.find_column(&qualifier, &column)?;
                Some(Resolved {
                    reference: qualifier.clone(),
                    table: qualifier,
                    column,
                    found,
                })
            }
            _ => None,
        }
    }

    /// `(reference, table, column)` of the source a column reference reads, found as
    /// [`Self::resolve_column`] finds it.
    pub(super) fn locate_column(
        &self,
        expr: &Expr,
        scope: &Scope,
    ) -> Option<(String, String, String)> {
        let found = self.resolve_reference(expr, scope)?;
        Some((found.reference, found.table, found.column))
    }

    /// Whether the source says `column` of `table` can be NULL (see [`ColumnRef`]).
    pub(super) fn declared_nullability(&self, table: &str, column: &str) -> Nullability {
        match self.find_column(table, column) {
            // `CHECK (column IS NOT NULL)` or `CHECK (num_nulls(column, ...) = 0)`.
            Some(ColumnRef::Schema(_)) if self.checked_non_null(table, &[column]) => {
                Nullability::NonNull
            }
            found => found.map_or(Nullability::Unknown, |found| found.nullability()),
        }
    }

    /// Whether `columns` of the schema table `table` include a group a `CHECK` keeps at
    /// least one of non-null.
    pub(super) fn checked_non_null(&self, table: &str, columns: &[&str]) -> bool {
        self.virtual_columns(table).is_none()
            && self
                .schema
                .nonnull_groups(table)
                .iter()
                .any(|group| group.iter().all(|name| columns.contains(&name.as_str())))
    }

    /// Type of a column: its qualifier's source, else the first source having it, the
    /// FROM clause before enclosing queries.
    pub(super) fn resolve_column(&self, expr: &Expr, scope: &Scope) -> Option<(Typed, Origin)> {
        let resolved = self.resolve_reference(expr, scope)?;
        let origin = self.origin(&resolved.found, &resolved.table, &resolved.column);
        Some((resolved.found.typed(self.dialect), origin))
    }

    /// Typed column reference, recording why when it is untyped.
    pub(super) fn column_expr(&self, expr: &Expr, scope: &Scope) -> Typed {
        if let Expr::Identifier(ident) = expr {
            self.check_ambiguous(ident, scope);
        }
        match self.resolve_column(expr, scope) {
            Some((typed, origin)) => {
                let typed = self.narrowed_values(expr, scope, typed);
                if typed.python.is_none() && self.diag.enabled() {
                    match (origin, &typed.sql) {
                        (Origin::Schema(column), sql_type) => self.diag.record(
                            NameRule::UnknownType,
                            untyped_column_message(&column, sql_type.as_deref().unwrap_or("")),
                        ),
                        (Origin::View(column), _) => self.diag.record(
                            NameRule::UnsupportedSyntax,
                            format!("view column `{column}` is not typed by the view's query"),
                        ),
                        // Typed in the CTE or subquery that defines it; reported there.
                        (Origin::Local, _) => self.diag.explain_silently(),
                    }
                }
                typed
            }
            None => match self.whole_row_table(expr, scope) {
                Some(table) => {
                    self.diag.explain_silently();
                    Typed {
                        sql: Some(table.to_uppercase().into()),
                        ..Typed::UNKNOWN
                    }
                }
                None => {
                    self.explain_unresolved(expr, scope);
                    Typed::UNKNOWN
                }
            },
        }
    }

    /// The schema table of a PostgreSQL whole-row reference (`SELECT u FROM users u`),
    /// whose type is the table's row type.
    pub(super) fn whole_row_table<'s>(&self, expr: &Expr, scope: &'s Scope) -> Option<&'s str> {
        let Expr::Identifier(ident) = expr else {
            return None;
        };
        if self.dialect != SqlDialect::Postgres || ident.quote_style.is_some() {
            return None;
        }
        let name = fold(ident, self.dialect);
        let key = scope
            .iter()
            .find(|source| source.reference == name)?
            .table
            .named()?;
        self.schema.table(key).map(|_| key)
    }

    /// `typed` limited to the values the conditions in force leave the column `expr`
    /// (`WHERE status = 'paid'`), when it has a value set.
    fn narrowed_values(&self, expr: &Expr, scope: &Scope, typed: Typed) -> Typed {
        if self.facts.values.is_empty() {
            return typed;
        }
        let Some(known) = value_set(&typed) else {
            return typed;
        };
        let Some((reference, _, column)) = self.locate_column(expr, scope) else {
            return typed;
        };
        let Some((_, _, allowed)) = self
            .facts
            .values
            .iter()
            .find(|(r, c, _)| *r == reference && *c == column)
        else {
            return typed;
        };
        // No value left means no row: keep the declared set rather than an empty one.
        let sql = typed.sql.clone();
        Typed::of_values(crate::values::intersect(known, allowed), sql).unwrap_or(typed)
    }

    /// `ambiguous-column`: an unqualified column in more than one FROM source. Columns
    /// merged by `USING` (and every column of a `NATURAL` join) are single.
    fn check_ambiguous(&self, ident: &Ident, scope: &Scope) {
        if !self.diag.enabled() || !self.judges_names() {
            return;
        }
        let column = fold(ident, self.dialect);
        if self
            .merged
            .iter()
            .any(|merged| *merged == "*" || *merged == column)
        {
            return;
        }
        let mut references: Vec<&str> = scope
            .iter()
            .filter(|source| {
                source
                    .table
                    .named()
                    .is_some_and(|table| self.find_column(table, &column).is_some())
            })
            .map(|source| source.reference.as_str())
            .collect();
        references.dedup();
        if references.len() > 1 {
            self.diag.record(
                NameRule::AmbiguousColumn,
                format!(
                    "column reference `{}` is ambiguous: {} all have it",
                    ident.value,
                    list(&references)
                ),
            );
        }
    }

    /// Records the reason a column reference found no column.
    fn explain_unresolved(&self, expr: &Expr, scope: &Scope) {
        if !self.diag.enabled() {
            return;
        }
        let (qualifier, column) = match expr {
            Expr::Identifier(ident) => (None, ident),
            Expr::CompoundIdentifier(parts) if parts.len() >= 2 => (
                Some(parts[parts.len() - 2].value.to_lowercase()),
                &parts[parts.len() - 1],
            ),
            _ => return self.diag.explain_silently(),
        };
        let name = fold(column, self.dialect);
        let unqualified = qualifier.is_none();
        let all: Vec<&Source> = sources(scope, &self.outer).collect();
        // SQLite and MySQL read an unresolved "double-quoted" name as a string literal.
        if unqualified && column.quote_style == Some('"') && self.dialect != SqlDialect::Postgres {
            return self.diag.explain_silently();
        }
        // A whole-row reference: `SELECT t FROM users t`.
        if unqualified && all.iter().any(|source| source.reference == name) {
            return self.diag.explain_silently();
        }
        // MySQL's `@name` is a user variable, `@@name` a system one.
        if unqualified && column.quote_style.is_none() && column.value.starts_with('@') {
            return self.diag.explain_silently();
        }
        // `VALUES (DEFAULT)` and `SET c = DEFAULT`: the keyword, not a column.
        if unqualified
            && column.quote_style.is_none()
            && column.value.eq_ignore_ascii_case("DEFAULT")
        {
            return self.diag.explain_silently();
        }
        let candidates: Vec<&&Source> = match &qualifier {
            Some(qualifier) => all
                .iter()
                .filter(|source| source.reference == *qualifier)
                .collect(),
            None => all.iter().collect(),
        };
        // A qualifier no FROM item names may still be an alias inside an opaque source.
        let pool: &[&Source] = if candidates.is_empty() { &all } else { &[] };
        let opaque = candidates
            .iter()
            .map(|source| **source)
            .chain(pool.iter().copied())
            .find_map(|source| source.table.opaque_description());
        if let Some(description) = opaque {
            return self.diag.record(
                NameRule::UnsupportedSyntax,
                format!(
                    "column `{}` may come from {description}, which sqlty does not read the columns of",
                    column.value,
                ),
            );
        }
        if !self.judges_names() {
            return self.diag.explain_silently();
        }
        if let Some(qualifier) = &qualifier {
            if candidates.is_empty() {
                // `old`/`new` in RETURNING and `excluded` in ON CONFLICT are not FROM items.
                if matches!(qualifier.as_str(), "old" | "new" | "excluded") {
                    return self.diag.explain_silently();
                }
                return self.diag.record(
                    NameRule::UnknownTable,
                    format!("`{qualifier}` is not a table or alias in the FROM clause"),
                );
            }
        }
        // Columns of a source that is itself unknown are explained where it is declared.
        let unreadable = candidates.iter().any(|source| {
            source.table.named().is_none_or(|table| {
                !self.knows_table(table)
                    || self
                        .virtual_columns(table)
                        .is_some_and(|columns| columns.iter().any(Column::is_unknown_star))
            })
        });
        if unreadable {
            return self.diag.explain_silently();
        }
        let mut tables: Vec<&str> = Vec::new();
        for source in &candidates {
            if !tables.contains(&source.reference.as_str()) {
                tables.push(&source.reference);
            }
        }
        let message = match (&qualifier, tables.is_empty()) {
            (Some(qualifier), _) => {
                format!("column `{}` does not exist in `{qualifier}`", column.value)
            }
            (None, true) => format!(
                "column `{}` does not exist: the query has no FROM",
                column.value
            ),
            (None, false) => format!(
                "column `{}` does not exist in {}",
                column.value,
                list(&tables)
            ),
        };
        self.diag.record(NameRule::UnknownColumn, message);
    }

    /// The columns a star lists that have no Python type: a schema column of an unmapped
    /// type, or a view column its own query left untyped.
    pub(super) fn explain_untyped_star_columns(&self, reference: Option<&str>, scope: &Scope) {
        if !self.diag.enabled() {
            return;
        }
        let reference = reference.map(str::to_lowercase);
        for table in Self::star_tables(reference.as_deref(), scope) {
            let Some(table) = table.named() else {
                continue;
            };
            if self.virtuals.contains_key(table) {
                continue;
            }
            if let Some(columns) = self.virtual_columns(table) {
                for col in columns
                    .iter()
                    .filter(|c| c.python_type.is_none() && !c.is_unknown_star())
                {
                    self.diag.record(
                        NameRule::UnsupportedSyntax,
                        format!(
                            "view column `{table}.{}` is not typed by the view's query",
                            col.name
                        ),
                    );
                }
            } else if let Some(columns) = self.schema.table(table) {
                for col in columns {
                    if col.typed(self.dialect).python.is_none() {
                        self.diag.record(
                            NameRule::UnknownType,
                            untyped_column_message(&format!("{table}.{}", col.name), &col.sql_type),
                        );
                    }
                }
            }
        }
    }

    /// A star that expanded to an unknown `*`: a view whose own query could not be typed
    /// says so; for a CTE or derived table that query explains it.
    pub(super) fn explain_view_star(&self, reference: Option<&str>, scope: &Scope) {
        if !self.diag.enabled() {
            return;
        }
        let reference = reference.map(str::to_lowercase);
        let views: Vec<String> = Self::star_tables(reference.as_deref(), scope)
            .iter()
            .filter_map(Table::named)
            .filter(|table| {
                !self.virtuals.contains_key(table) && self.virtual_columns(table).is_some()
            })
            .map(str::to_string)
            .collect();
        if views.is_empty() {
            return self.diag.explain_silently();
        }
        self.diag.record(
            NameRule::UnsupportedSyntax,
            format!(
                "`*` reads view `{}`, whose columns are not typed",
                views.join("`, `")
            ),
        );
    }

    /// Records why a `*` or `t.*` stayed unresolved.
    pub(super) fn explain_star(&self, reference: Option<&str>, scope: &Scope) {
        if !self.diag.enabled() {
            return;
        }
        let reference = reference.map(str::to_lowercase);
        let tables = Self::star_tables(reference.as_deref(), scope);
        if let Some(description) = tables.iter().find_map(Table::opaque_description) {
            return self.diag.record(
                NameRule::UnsupportedSyntax,
                format!("`*` reads {description}, which sqlty does not read the columns of"),
            );
        }
        if !self.judges_names() {
            return self.diag.explain_silently();
        }
        if let Some(reference) = &reference {
            if !scope.iter().any(|source| source.reference == *reference) {
                return self.diag.record(
                    NameRule::UnknownTable,
                    format!("`{reference}` in `{reference}.*` is not a table or alias in the FROM clause"),
                );
            }
        }
        if tables
            .iter()
            .any(|table| table.named().is_none_or(|table| !self.knows_table(table)))
            || scope.is_empty()
        {
            return self.diag.explain_silently();
        }
        if tables.iter().filter_map(Table::named).any(|table| {
            self.virtual_columns(table)
                .is_some_and(|columns| columns.iter().any(Column::is_unknown_star))
        }) {
            return self.explain_view_star(reference.as_deref(), scope);
        }
        self.diag.record(
            NameRule::UnsupportedSyntax,
            "`*` could not be expanded into columns",
        );
    }
}

fn list(names: &[&str]) -> String {
    let names: Vec<String> = names.iter().map(|name| format!("`{name}`")).collect();
    names.join(", ")
}

/// Why the schema column `column` of type `sql_type` has no Python type.
fn untyped_column_message(column: &str, sql_type: &str) -> String {
    if sql_type.is_empty() {
        format!("column `{column}` has no declared type, so it has no Python type")
    } else {
        format!("column `{column}` has type `{sql_type}`, which has no Python type")
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::reports;
    use super::*;

    #[test]
    fn list_quotes_and_joins_names() {
        assert_eq!(list(&["a", "b"]), "`a`, `b`");
        assert_eq!(list(&[]), "");
    }

    const DDL: &str = "CREATE TABLE users (id INT NOT NULL, name TEXT NOT NULL, note TEXT); \
                       CREATE TABLE orders (id INT NOT NULL, user_id INT NOT NULL, \
                       total NUMERIC(10, 2), note TEXT NOT NULL); \
                       CREATE VIEW user_names AS SELECT id, name FROM users; \
                       CREATE TABLE \"Case\" (\"Mixed\" INT NOT NULL, plain INT)";

    #[test]
    fn unqualified_names_resolve_to_the_first_source_that_has_them() {
        let queries = [
            "SELECT name FROM users",
            "SELECT name, total FROM users JOIN orders ON users.id = orders.user_id",
            "SELECT users.id, orders.id AS oid FROM users, orders",
            "SELECT u.name, o.note FROM users u JOIN orders o ON true",
            "SELECT NAME FROM users",
            "SELECT users.name FROM users AS u",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn ambiguous_names_are_reported_once_per_query() {
        let queries = [
            "SELECT id FROM users, orders",
            "SELECT note FROM users JOIN orders ON true",
            "SELECT id, note FROM users, orders",
            "SELECT id FROM users u1, users u2",
            "SELECT id FROM users JOIN orders USING (id)",
            "SELECT note FROM users NATURAL JOIN orders",
            "SELECT users.id FROM users, orders",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn unknown_names_say_what_is_missing() {
        let queries = [
            "SELECT nope FROM users",
            "SELECT users.nope FROM users",
            "SELECT x.name FROM users",
            "SELECT nope FROM missing",
            "SELECT missing.col FROM missing",
            "SELECT nope FROM users JOIN orders ON true",
            "SELECT (SELECT nope FROM users) FROM users",
            "SELECT name, nope FROM user_names",
            "SELECT nope",
            "SELECT users.* , nope.* FROM users",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn quoted_names_keep_their_case_in_postgres_only() {
        let queries = [
            "SELECT \"Mixed\" FROM \"Case\"",
            "SELECT Mixed FROM \"Case\"",
            "SELECT PLAIN FROM \"Case\"",
            "SELECT \"PLAIN\" FROM \"Case\"",
        ];
        let postgres = reports(&queries, "postgres", Some(DDL));
        let mysql_ddl = "CREATE TABLE `Case` (`Mixed` INT NOT NULL, plain INT)";
        let mysql = reports(
            &[
                "SELECT mixed FROM `Case`",
                "SELECT PLAIN FROM case_",
                "SELECT `MIXED` FROM `case`",
            ],
            "mysql",
            Some(mysql_ddl),
        );
        insta::assert_snapshot!(format!("postgres:\n{postgres}\nmysql:\n{mysql}"));
    }

    #[test]
    fn system_columns_exist_without_being_declared() {
        let queries = [
            "SELECT ctid, xmin, xmax, cmin, cmax, tableoid FROM users",
            "SELECT users.ctid FROM users",
            "SELECT oid FROM users",
        ];
        let postgres = reports(&queries, "postgres", Some(DDL));
        let sqlite_ddl = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)";
        let sqlite = reports(
            &[
                "SELECT rowid, _rowid_, oid FROM users",
                "SELECT ctid FROM users",
            ],
            "sqlite",
            Some(sqlite_ddl),
        );
        insta::assert_snapshot!(format!("postgres:\n{postgres}\nsqlite:\n{sqlite}"));
    }

    #[test]
    fn nullability_comes_from_the_declaration() {
        let queries = [
            "SELECT id, note, total FROM orders",
            "SELECT o.total, u.note FROM orders o, users u",
            "SELECT name FROM user_names",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn star_over_unreadable_sources_names_the_reason() {
        let queries = [
            "SELECT * FROM missing",
            "SELECT users.* FROM users, missing",
            "SELECT * FROM users, generate_series(1, 2) AS g",
            "SELECT * FROM users, ST_Dump(NULL) AS d",
            "SELECT nope.* FROM users",
            "SELECT * FROM users JOIN orders USING (id)",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn names_are_not_judged_without_a_schema() {
        let queries = [
            "SELECT name FROM users",
            "SELECT id FROM users, orders",
            "SELECT users.* FROM users",
            "SELECT name::TEXT FROM users",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", None));
    }

    #[test]
    fn cte_columns_are_resolved_like_tables() {
        let queries = [
            "WITH c AS (SELECT id, name FROM users) SELECT c.name, name FROM c",
            "WITH c AS (SELECT id FROM users) SELECT name FROM c",
            "WITH c AS (SELECT id FROM users), d AS (SELECT id FROM c) SELECT d.id FROM d",
            "WITH c AS (SELECT nope FROM users) SELECT * FROM c",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }
}
