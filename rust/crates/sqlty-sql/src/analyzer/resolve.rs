//! Column lookup, and the reasons a name stays unresolved.
//!
//! Every lookup that fails or yields an untyped column records why into the analyzer's
//! [`Diagnostics`](crate::diagnostics::Diagnostics): an unknown table, an unknown or
//! ambiguous column, a source the analyzer does not read. Names are only judged when a
//! schema is configured; without one every table and column is unknown.

use super::*;

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
fn sources<'s>(scope: &'s Scope, outer: &'s Scope) -> impl Iterator<Item = &'s (String, String)> {
    scope.iter().chain(outer.iter())
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
    pub(super) fn check_table(&self, table: &str) {
        if !self.diag.enabled() {
            return;
        }
        if is_opaque(table) || self.knows_table(table) || self.schema.defines(table) {
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

    /// A column of one source: `(type, schema column)`, the latter set when a schema table
    /// holds it (`table.column`), for type diagnostics.
    fn lookup_column(&self, table: &str, column: &str) -> Option<(Typed, Origin)> {
        if let Some(columns) = self.virtual_columns(table) {
            let origin = if self.virtuals.contains_key(table) {
                Origin::Local
            } else {
                Origin::View(self.label(table, column))
            };
            return columns
                .iter()
                .find(|col| self.same_column(&col.name, column))
                .map(|col| ((col.python_type.clone(), col.sql_type.clone()), origin));
        }
        let columns = self.schema.table(table)?;
        match columns.iter().find(|col| col.name == column) {
            Some(col) => Some((
                col.typed(self.dialect),
                Origin::Schema(self.label(table, column)),
            )),
            None => self
                .system_column(column)
                .map(|typed| (typed, Origin::Local)),
        }
    }

    /// A column every table has without declaring it: SQLite's `rowid`, PostgreSQL's
    /// `ctid`, `xmin`, `xmax`, `cmin`, `cmax` and `tableoid`. PostgreSQL 12 dropped `oid`.
    /// PostgreSQL drivers load `tid`, `xid` and `cid` as text.
    fn system_column(&self, column: &str) -> Option<Typed> {
        let typed = |python: &str, sql: &str| (Some(python.to_string()), Some(sql.to_string()));
        match (self.dialect, column) {
            (SqlDialect::Sqlite, "rowid" | "oid" | "_rowid_") => Some(typed("int", "INT")),
            (SqlDialect::Postgres, "tableoid") => Some(typed("int", "OID")),
            (SqlDialect::Postgres, "ctid") => Some(typed("str", "TID")),
            (SqlDialect::Postgres, "xmin" | "xmax") => Some(typed("str", "XID")),
            (SqlDialect::Postgres, "cmin" | "cmax") => Some(typed("str", "CID")),
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
        match expr {
            Expr::Identifier(ident) => {
                let column = fold(ident, self.dialect);
                sources(scope, &self.outer).find_map(|(reference, table)| {
                    self.lookup_column(table, &column)?;
                    Some((reference.clone(), table.clone(), column.clone()))
                })
            }
            Expr::CompoundIdentifier(parts) if parts.len() >= 2 => {
                let qualifier = parts[parts.len() - 2].value.to_lowercase();
                let column = fold(&parts[parts.len() - 1], self.dialect);
                for sources in [scope, &self.outer] {
                    if sources.iter().any(|(reference, _)| *reference == qualifier) {
                        let table = resolve(&qualifier, sources);
                        self.lookup_column(&table, &column)?;
                        return Some((qualifier, table, column));
                    }
                }
                self.lookup_column(&qualifier, &column)?;
                Some((qualifier.clone(), qualifier, column))
            }
            _ => None,
        }
    }

    /// Whether the source says `column` of `table` can be NULL: a schema column lacking
    /// `NOT NULL` / `PRIMARY KEY`, or a column its CTE, view or derived table marks nullable.
    pub(super) fn declared_nullability(&self, table: &str, column: &str) -> Nullability {
        if let Some(columns) = self.virtual_columns(table) {
            return columns
                .iter()
                .find(|col| self.same_column(&col.name, column))
                .map_or(Nullability::Unknown, |col| col.nullability);
        }
        let Some(columns) = self.schema.table(table) else {
            return Nullability::Unknown;
        };
        match columns.iter().find(|col| col.name == column) {
            Some(col) => Nullability::declared(col.not_null),
            None if self.system_column(column).is_some() => Nullability::NonNull,
            None => Nullability::Unknown,
        }
    }

    /// Type of a column: its qualifier's source, else the first source having it, the
    /// FROM clause before enclosing queries.
    pub(super) fn resolve_column(&self, expr: &Expr, scope: &Scope) -> Option<(Typed, Origin)> {
        match expr {
            Expr::Identifier(ident) => {
                let column = fold(ident, self.dialect);
                sources(scope, &self.outer)
                    .find_map(|(_, table)| self.lookup_column(table, &column))
            }
            Expr::CompoundIdentifier(parts) if parts.len() >= 2 => {
                let qualifier = parts[parts.len() - 2].value.to_lowercase();
                let column = fold(&parts[parts.len() - 1], self.dialect);
                for sources in [scope, &self.outer] {
                    if sources.iter().any(|(reference, _)| *reference == qualifier) {
                        return self.lookup_column(&resolve(&qualifier, sources), &column);
                    }
                }
                self.lookup_column(&qualifier, &column)
            }
            _ => None,
        }
    }

    /// Typed column reference, recording why when it is untyped.
    pub(super) fn column_expr(&self, expr: &Expr, scope: &Scope) -> Typed {
        if let Expr::Identifier(ident) = expr {
            self.check_ambiguous(ident, scope);
        }
        match self.resolve_column(expr, scope) {
            Some((typed, origin)) => {
                if typed.0.is_none() && self.diag.enabled() {
                    match (origin, &typed.1) {
                        (Origin::Schema(column), sql_type) => self.diag.record(
                            NameRule::UnknownType,
                            format!(
                                "column `{column}` has type {}, which has no Python type",
                                sql_type
                                    .as_deref()
                                    .map_or("unknown".to_string(), |ty| format!("`{ty}`"))
                            ),
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
            None => {
                self.explain_unresolved(expr, scope);
                (None, None)
            }
        }
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
            .filter(|(_, table)| self.lookup_column(table, &column).is_some())
            .map(|(reference, _)| reference.as_str())
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
        let all: Vec<&(String, String)> = sources(scope, &self.outer).collect();
        // SQLite and MySQL read an unresolved "double-quoted" name as a string literal.
        if unqualified && column.quote_style == Some('"') && self.dialect != SqlDialect::Postgres {
            return self.diag.explain_silently();
        }
        // A whole-row reference: `SELECT t FROM users t`.
        if unqualified && all.iter().any(|(reference, _)| *reference == name) {
            return self.diag.explain_silently();
        }
        let candidates: Vec<&&(String, String)> = match &qualifier {
            Some(qualifier) => all.iter().filter(|(r, _)| r == qualifier).collect(),
            None => all.iter().collect(),
        };
        // A qualifier no FROM item names may still be an alias inside an opaque source.
        let pool: &[&(String, String)] = if candidates.is_empty() { &all } else { &[] };
        let opaque = candidates
            .iter()
            .map(|source| **source)
            .chain(pool.iter().copied())
            .map(|(_, table)| table)
            .find(|table| is_opaque(table));
        if let Some(table) = opaque {
            return self.diag.record(
                NameRule::UnsupportedSyntax,
                format!(
                    "column `{}` may come from {}, which sqlty does not read the columns of",
                    column.value,
                    opaque_description(table)
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
        let unreadable = candidates.iter().any(|(_, table)| {
            !self.knows_table(table)
                || self
                    .virtual_columns(table)
                    .is_some_and(|columns| columns.iter().any(Column::is_unknown_star))
        });
        if unreadable {
            return self.diag.explain_silently();
        }
        let tables: Vec<&str> = candidates
            .iter()
            .map(|(reference, _)| reference.as_str())
            .collect();
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
            if self.virtuals.contains_key(&table) {
                continue;
            }
            if let Some(columns) = self.virtual_columns(&table) {
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
            } else if let Some(columns) = self.schema.table(&table) {
                for col in columns {
                    if col.typed(self.dialect).0.is_none() {
                        self.diag.record(
                            NameRule::UnknownType,
                            format!(
                                "column `{table}.{}` has type `{}`, which has no Python type",
                                col.name, col.sql_type
                            ),
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
            .into_iter()
            .filter(|table| {
                !self.virtuals.contains_key(table) && self.virtual_columns(table).is_some()
            })
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
    pub(super) fn explain_star(
        &self,
        reference: Option<&str>,
        scope: &Scope,
        from: Option<&[TableWithJoins]>,
    ) {
        if !self.diag.enabled() {
            return;
        }
        let reference = reference.map(str::to_lowercase);
        let tables = Self::star_tables(reference.as_deref(), scope);
        if let Some(table) = tables.iter().find(|table| is_opaque(table)) {
            return self.diag.record(
                NameRule::UnsupportedSyntax,
                format!(
                    "`*` reads {}, which sqlty does not read the columns of",
                    opaque_description(table)
                ),
            );
        }
        if !self.judges_names() {
            return self.diag.explain_silently();
        }
        if let Some(reference) = &reference {
            if !scope.iter().any(|(alias, _)| alias == reference) {
                return self.diag.record(
                    NameRule::UnknownTable,
                    format!("`{reference}` in `{reference}.*` is not a table or alias in the FROM clause"),
                );
            }
        } else if from.is_some_and(any_merging_join) {
            return self.diag.record(
                NameRule::UnsupportedSyntax,
                "`*` over a USING or NATURAL join merges columns, so it is not expanded",
            );
        }
        if tables.iter().any(|table| !self.knows_table(table)) || scope.is_empty() {
            return self.diag.explain_silently();
        }
        if tables.iter().any(|table| {
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
