//! Column lookup, and the reasons a name stays unresolved.
//!
//! Every lookup that fails or yields an untyped column records why into the analyzer's
//! [`Diagnostics`](crate::diagnostics::Diagnostics): an unknown table, an unknown or
//! ambiguous column, a source the analyzer does not read. Names are only judged when a
//! schema is configured; without one every table and column is unknown.

use std::borrow::Cow;
use std::rc::Rc;

use rustc_hash::FxHashMap;

use sqlparser::ast::{Expr, Ident, TableFactor, TableWithJoins};
use sqlparser::tokenizer::Span;

use crate::catalog::{fold, folded, lowercase};
use crate::diagnostics::{NameDiagnostic, Related, Rule};

use super::dialect::SqlDialect;
use super::literals::value_set;
use super::names::table_key;
use super::nullability::{Facts, Nullability};
use super::query::{Analyzer, Column};
use super::scope::{relations, Scope, Source, Table};
use super::suggest;
use super::typed::Typed;
use super::Candidate;
use super::SchemaColumn;

/// Where a name of a FROM clause is taken: by an alias or a derived table, its first use,
/// and each table read by it.
type Taken<'a> = (Option<Span>, Span, FxHashMap<Cow<'a, str>, Span>);

/// The first characters a misspelled table shares with the names [`Analyzer::meant_table`]
/// considers.
const SHARED_PREFIX: usize = 2;

/// The most names [`Analyzer::meant_table`] compares.
const MEANT_CANDIDATES: usize = 64;

/// Whether `name` can replace an unquoted table name as it is: plain, and no keyword.
fn unquoted(analyzer: &Analyzer<'_>, name: &str) -> bool {
    analyzer.plain(name)
        && sqlparser::keywords::ALL_KEYWORDS
            .binary_search(&name.to_ascii_uppercase().as_str())
            .is_err()
}

/// The character that closes an identifier `quote` opens.
fn closing_quote(quote: char) -> char {
    match quote {
        '[' => ']',
        other => other,
    }
}

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
enum ColumnRef<'s, 'c> {
    /// A column of a CTE, derived table or table function (`local`), or of a schema view.
    Virtual { column: &'s Column, local: bool },
    /// A column of a schema table, borrowed from the catalog.
    Schema(&'c SchemaColumn),
    /// A column every table has without declaring it.
    System(Typed),
}

impl ColumnRef<'_, '_> {
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
pub(super) struct Resolved<'r, 'c> {
    /// The alias or table name the source is read by.
    pub(super) reference: Cow<'r, str>,
    /// The key of the source's table.
    pub(super) table: Cow<'r, str>,
    /// The column, as the resolver names it (case folded).
    pub(super) column: Cow<'r, str>,
    found: ColumnRef<'r, 'c>,
}

impl<'a> Analyzer<'a> {
    /// Whether table, column and ambiguity rules apply: a schema is configured and the
    /// dialect [checks names](SqlDialect::checks_names).
    pub(super) fn judges_names(&self) -> bool {
        !self.schema.is_empty() && self.dialect.checks_names()
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

    /// Records `unknown-table` when a FROM item names no schema table, view or CTE, with the
    /// closest one as a fix of its `written` name.
    pub(super) fn check_table(&self, table: &Table, written: Option<&Ident>) {
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
            let mut message = format!("table `{table}` is not defined in the schema");
            let meant = self.meant_table(table);
            if let Some(meant) = &meant {
                message.push_str(&format!("; did you mean `{meant}`?"));
            }
            let fix = meant.zip(written).filter(|(meant, ident)| {
                // A name inside another is a hint, not a fix: the table may be missing.
                suggest::is_typo(table.rsplit('.').next().unwrap_or(table), meant)
                    && (ident.quote_style.is_some() || unquoted(self, meant))
            });
            let _at = fix.as_ref().and_then(|(_, ident)| self.diag.at(ident.span));
            self.diag.push(NameDiagnostic {
                table: Some(table.to_string()),
                replacement: fix.map(|(meant, ident)| match ident.quote_style {
                    Some(quote) => format!("{quote}{meant}{}", closing_quote(quote)),
                    None => meant,
                }),
                ..NameDiagnostic::new(Rule::UnknownTable, message)
            });
        } else {
            self.diag.explain_silently();
        }
    }

    /// `duplicate-alias`: two FROM items read by the same name, which PostgreSQL and MySQL
    /// reject. Two tables without an alias may share a name when they are in different
    /// schemas.
    pub(super) fn check_duplicate_names(&self, from: &[TableWithJoins]) {
        if !self.diag.enabled() || !matches!(self.dialect, SqlDialect::Postgres | SqlDialect::MySql)
        {
            return;
        }
        // Per name: whether an alias or a derived table took it, and the tables read by it.
        let mut seen: FxHashMap<String, Taken<'_>> = FxHashMap::default();
        let names = relations(from).filter_map(|factor| match factor {
            TableFactor::Table {
                alias: None, name, ..
            } => Some((
                name.0.last()?.as_ident()?,
                Some(table_key(name, self.dialect)),
            )),
            TableFactor::Table { alias, .. }
            | TableFactor::Derived { alias, .. }
            | TableFactor::TableFunction { alias, .. }
            | TableFactor::Function { alias, .. }
            | TableFactor::UNNEST { alias, .. }
            | TableFactor::JsonTable { alias, .. }
            | TableFactor::NestedJoin { alias, .. } => {
                alias.as_ref().map(|alias| (&alias.name, None))
            }
            _ => None,
        });
        for (ident, key) in names {
            let (aliased, first, keys) = seen
                .entry(fold(ident, self.dialect))
                .or_insert_with(|| (None, ident.span, FxHashMap::default()));
            let clash = match &key {
                None => aliased.or((!keys.is_empty()).then_some(*first)),
                Some(key) => aliased.or_else(|| keys.get(key).copied()),
            };
            if let Some(earlier) = clash {
                let _at = self.diag.at(ident.span);
                self.diag.record_related(
                    Rule::DuplicateAlias,
                    format!("table name `{}` is specified more than once", ident.value),
                    None,
                    vec![Related::new(
                        earlier,
                        format!("`{}` is first used here", ident.value),
                    )],
                );
            }
            match key {
                None => {
                    aliased.get_or_insert(ident.span);
                }
                Some(key) => {
                    keys.entry(key).or_insert(ident.span);
                }
            }
        }
    }

    /// A column of one source, when it has it.
    fn find_column(&self, table: &str, column: &str) -> Option<ColumnRef<'_, 'a>> {
        if let Some(found) = self.virtual_column(table, column) {
            return found.map(|column| ColumnRef::Virtual {
                column,
                local: self.virtuals.contains_key(table),
            });
        }
        match self.schema.column(table, column)? {
            Some(col) => Some(ColumnRef::Schema(col)),
            None => self.system_column(column).map(ColumnRef::System),
        }
    }

    /// Where a column found in `table` comes from, for explaining why it is untyped.
    fn origin(found: &ColumnRef<'_, '_>, table: &str, column: &str) -> Origin {
        let label = || format!("{table}.{column}");
        match found {
            ColumnRef::Virtual { local: false, .. } => Origin::View(label()),
            ColumnRef::Schema(_) => Origin::Schema(label()),
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
    pub(super) fn resolve_reference<'r>(
        &'r self,
        expr: &'r Expr,
        scope: &'r Scope,
    ) -> Option<Resolved<'r, 'a>> {
        match expr {
            Expr::Identifier(ident) => {
                let column = folded(ident, self.dialect);
                let (source, table, found) = sources(scope, &self.outer).find_map(|source| {
                    let table = source.table.named()?;
                    Some((source, table, self.find_column(table, &column)?))
                })?;
                Some(Resolved {
                    reference: Cow::Borrowed(&source.reference),
                    table: Cow::Borrowed(table),
                    column,
                    found,
                })
            }
            Expr::CompoundIdentifier(parts) if parts.len() >= 2 => {
                let qualifier = lowercase(&parts[parts.len() - 2].value);
                let column = folded(&parts[parts.len() - 1], self.dialect);
                for sources in [scope, &*self.outer] {
                    if let Some(source) = sources.find(&qualifier) {
                        let table = source.table.named()?;
                        let found = self.find_column(table, &column)?;
                        return Some(Resolved {
                            reference: Cow::Borrowed(&source.reference),
                            table: Cow::Borrowed(table),
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

    /// The name the source a column reference reads gives that column; `None` for a
    /// system column or an unresolved reference.
    pub(super) fn source_column_name(&self, expr: &Expr, scope: &Scope) -> Option<String> {
        match self.resolve_reference(expr, scope)?.found {
            ColumnRef::Virtual { column, .. } => Some(column.name.clone()),
            ColumnRef::Schema(column) => Some(column.output_name()),
            ColumnRef::System(_) => None,
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
        Some((
            found.reference.into_owned(),
            found.table.into_owned(),
            found.column.into_owned(),
        ))
    }

    /// Whether `expr` reads a column of a source in `scope`, a schema table's or not.
    pub(super) fn resolves(&self, expr: &Expr, scope: &Scope) -> bool {
        self.resolve_reference(expr, scope).is_some()
    }

    /// [`Self::resolve_reference`] of a schema table column, and whether the column is part
    /// of a foreign key.
    pub(super) fn locate_table_column<'r>(
        &'r self,
        expr: &'r Expr,
        scope: &'r Scope,
    ) -> Option<(Resolved<'r, 'a>, &'a SchemaColumn)> {
        let found = self.resolve_reference(expr, scope)?;
        let ColumnRef::Schema(column) = found.found else {
            return None;
        };
        Some((found, column))
    }

    /// Whether the source of `resolved` says its column can be NULL (see [`ColumnRef`]).
    pub(super) fn declared_nullability(&self, resolved: &Resolved<'_, '_>) -> Nullability {
        let (table, column) = (&*resolved.table, &*resolved.column);
        match &resolved.found {
            // `CHECK (column IS NOT NULL)` or `CHECK (num_nulls(column, ...) = 0)`.
            ColumnRef::Schema(_) if self.checked_non_null(table, &[column]) => Nullability::NonNull,
            ColumnRef::Schema(SchemaColumn {
                not_null: false,
                generated: Some(expr),
                ..
            }) => {
                let scope = Scope::from(vec![Source::named(table.to_string(), table.to_string())]);
                self.nullability(expr, &scope, &Facts::default())
            }
            found => found.nullability(),
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
    pub(super) fn resolve_column(&self, expr: &Expr, scope: &Scope) -> Option<Typed> {
        let resolved = self.resolve_reference(expr, scope)?;
        Some(resolved.found.typed(self.dialect))
    }

    /// Typed column reference, recording why when it is untyped.
    pub(super) fn column_expr(&self, expr: &Expr, parts: &[Ident], scope: &Scope) -> Typed {
        if let Expr::Identifier(ident) = expr {
            self.check_ambiguous(ident, scope);
        }
        match self.resolve_reference(expr, scope) {
            Some(resolved) => {
                // `EXCLUDED` of an upsert is the row being written, not a read.
                if matches!(resolved.found, ColumnRef::Schema(_))
                    && resolved.reference != "excluded"
                {
                    self.check_deprecated(&resolved.table, &resolved.column);
                }
                let typed = resolved.found.typed(self.dialect);
                let typed = self.narrowed_values(&resolved.reference, &resolved.column, typed);
                if typed.python.is_none() && self.diag.enabled() {
                    let origin = Self::origin(&resolved.found, &resolved.table, &resolved.column);
                    match (origin, &typed.sql) {
                        (Origin::Schema(column), sql_type) => self.diag.record(
                            Rule::UnknownType,
                            untyped_column_message(&column, sql_type.as_deref().unwrap_or("")),
                        ),
                        (Origin::View(column), _) => self.diag.record(
                            Rule::UnsupportedSyntax,
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
                    self.explain_unresolved(parts, scope);
                    Typed::UNKNOWN
                }
            },
        }
    }

    /// Report a read of `column` of the schema table `table` when the project deprecates it.
    fn check_deprecated(&self, table: &str, column: &str) {
        let Some(deprecation) = self.schema.deprecation(table, column) else {
            return;
        };
        let name = table.rsplit('.').next().unwrap_or(table);
        let mut message = format!("`{name}.{column}` is deprecated");
        if let Some(replacement) = &deprecation.replacement {
            message.push_str(&format!(": read `{replacement}` instead"));
        }
        if let Some(date) = &deprecation.remove_after {
            message.push_str(&format!(", it is removed after {date}"));
        }
        self.diag.warn(NameDiagnostic {
            rule: Rule::DeprecatedColumn,
            message,
            span: None,
            replacement: None,
            table: Some(name.to_string()),
            column: Some(column.to_string()),
            shadowed_by: None,
            related: Vec::new(),
        });
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
        let key = scope.find(&name)?.table.named()?;
        self.schema.table(key).map(|_| key)
    }

    /// `typed` limited to the values the conditions in force leave `column` of `reference`
    /// (`WHERE status = 'paid'`), when it has a value set.
    fn narrowed_values(&self, reference: &str, column: &str, typed: Typed) -> Typed {
        if self.facts.values.is_empty() {
            return typed;
        }
        let Some(known) = value_set(&typed) else {
            return typed;
        };
        let Some((_, _, allowed)) = self
            .facts
            .values
            .iter()
            .find(|(r, c, _)| r == reference && c == column)
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
        if scope.len() < 2 || !self.diag.enabled() || !self.judges_names() {
            return;
        }
        let column = folded(ident, self.dialect);
        if self
            .merged
            .iter()
            .any(|merged| *merged == "*" || *merged == column)
        {
            return;
        }
        let mut having: Vec<&Source> = scope
            .iter()
            .filter(|source| {
                source
                    .table
                    .named()
                    .is_some_and(|table| self.find_column(table, &column).is_some())
            })
            .collect();
        having.dedup_by(|a, b| a.reference == b.reference);
        if having.len() > 1 {
            let references: Vec<&str> = having.iter().map(|s| s.reference.as_str()).collect();
            let here = format!("column `{}` is here", ident.value);
            self.diag.record_related(
                Rule::AmbiguousColumn,
                format!(
                    "column reference `{}` is ambiguous: {} all have it",
                    ident.value,
                    list(&references)
                ),
                Some(&ident.value),
                having
                    .iter()
                    .map(|source| Related::new(source.span, here.clone()))
                    .collect(),
            );
        }
    }

    /// Records the reason a column reference found no column.
    fn explain_unresolved(&self, parts: &[Ident], scope: &Scope) {
        let Some((column, qualifiers)) = parts.split_last().filter(|_| self.diag.enabled()) else {
            return;
        };
        let qualifier = qualifiers.last().map(|part| part.value.to_lowercase());
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
        if name == super::complete::PLACEHOLDER {
            return self.diag.probe(|| self.candidate_columns(&candidates));
        }
        // A qualifier no FROM item names may still be an alias inside an opaque source.
        let pool: &[&Source] = if candidates.is_empty() { &all } else { &[] };
        let opaque = candidates
            .iter()
            .map(|source| **source)
            .chain(pool.iter().copied())
            .find_map(|source| source.table.opaque_description());
        if let Some(description) = opaque {
            return self.diag.record(
                Rule::UnsupportedSyntax,
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
            // A qualifier of two FROM items is reported by `duplicate-alias`.
            if scope
                .iter()
                .filter(|source| source.reference == *qualifier)
                .nth(1)
                .is_some()
            {
                return self.diag.explain_silently();
            }
            if candidates.is_empty() {
                // `old`/`new` in RETURNING and `excluded` in ON CONFLICT are not FROM items.
                if matches!(qualifier.as_str(), "old" | "new" | "excluded") {
                    return self.diag.explain_silently();
                }
                return self.diag.record_about(
                    Rule::UnknownTable,
                    format!("`{qualifier}` is not a table or alias in the FROM clause"),
                    Some(qualifier),
                    Some(&column.value),
                );
            }
        }
        // Columns of a source that is itself unknown are explained where it is declared.
        let unreadable = candidates.iter().any(|source| {
            source
                .table
                .named()
                .is_none_or(|table| !self.knows_table(table) || self.has_unknown_star(table))
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
        let suggestion = self.suggestion(column, &candidates);
        let mut message = match (&qualifier, tables.is_empty()) {
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
        let replacement = suggestion.map(|(name, spelling)| {
            let shown = spelling.as_deref().unwrap_or(&name);
            message.push_str(&format!("; did you mean `{shown}`?"));
            spelling.map(|spelling| {
                let mut spelled: Vec<String> = qualifiers.iter().map(Ident::to_string).collect();
                spelled.push(spelling);
                spelled.join(".")
            })
        });
        let table = match candidates.as_slice() {
            [source] => source.table.named(),
            _ => None,
        };
        self.diag.push(NameDiagnostic {
            rule: Rule::UnknownColumn,
            message,
            span: None,
            replacement: replacement.flatten(),
            table: table.map(str::to_string),
            column: Some(column.value.clone()),
            shadowed_by: None,
            related: Vec::new(),
        });
    }

    /// Whether `name` reads as itself unquoted.
    fn plain(&self, name: &str) -> bool {
        name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && name.chars().all(|c| {
                c.is_ascii_digit()
                    || c == '_'
                    || c.is_ascii_lowercase()
                    || (c.is_ascii_uppercase() && self.dialect != SqlDialect::Postgres)
            })
    }

    /// The table, view or CTE an unknown `table` most likely means. Only names sharing its
    /// first [`SHARED_PREFIX`] characters are candidates, at most [`MEANT_CANDIDATES`], so
    /// the catalog's name index finds them in O(log N + k).
    fn meant_table(&self, table: &str) -> Option<String> {
        let (schema, name) = match table.rsplit_once('.') {
            Some((schema, name)) => (Some(schema), name),
            None => (None, table),
        };
        let prefix: String = name.chars().take(SHARED_PREFIX).collect();
        if prefix.chars().count() < SHARED_PREFIX {
            return None;
        }
        // Quoted, the folded key keeps its case.
        let relations = self.schema.catalog().relations_with_prefix(
            &format!("\"{prefix}"),
            schema.map(|schema| format!("\"{schema}\"")).as_deref(),
            MEANT_CANDIDATES,
        );
        // A tie may lie past the cap: no guess then.
        if relations.len() == MEANT_CANDIDATES {
            return None;
        }
        let ctes = self.virtuals.names().filter(|_| schema.is_none());
        let names: Vec<&str> = relations
            .iter()
            .map(|relation| relation.name.name)
            .chain(ctes.filter(|cte| cte.starts_with(&prefix)))
            .collect();
        suggest::closest(name, &names).map(str::to_string)
    }

    /// The column of `candidates` that `column` most likely misspells, and how SQL spells
    /// it when that is known.
    fn suggestion(
        &self,
        column: &Ident,
        candidates: &[&&Source],
    ) -> Option<(String, Option<String>)> {
        let plain = |name: &str| self.plain(name);
        let mut names: Vec<(&str, Option<&str>)> = Vec::new();
        for source in candidates {
            let table = source.table.named()?;
            if let Some(columns) = self.virtual_columns(table) {
                names.extend(columns.iter().map(|col| {
                    let name = col.name.as_str();
                    (name, plain(name).then_some(name))
                }));
            } else {
                let columns = self.schema.table(table)?;
                names.extend(
                    columns
                        .iter()
                        .map(|col| (col.name.as_str(), Some(col.ident.as_str()))),
                );
            }
        }
        let keys: Vec<&str> = names.iter().map(|(name, _)| *name).collect();
        let name = suggest::closest(&fold(column, self.dialect), &keys)?;
        let spelling = names.iter().find(|(key, _)| *key == name)?.1;
        Some((name.to_string(), spelling.map(str::to_string)))
    }

    /// The columns of `sources`, for completion: what a name in their scope can be.
    fn candidate_columns(&self, sources: &[&&Source]) -> Vec<Candidate> {
        let mut found = Vec::new();
        let named = sources
            .iter()
            .filter_map(|source| Some((source, source.table.named()?)));
        for (source, table) in named {
            let reference = &source.reference;
            if let Some(columns) = self.virtual_columns(table) {
                found.extend(
                    columns
                        .iter()
                        .filter(|col| !col.is_unknown_star())
                        .map(|col| {
                            Candidate::column(
                                &col.name,
                                reference,
                                col.sql_type.as_deref(),
                                col.nullability,
                            )
                        }),
                );
            } else if let Some(columns) = self.schema.table(table) {
                found.extend(columns.iter().map(|col| {
                    Candidate::column(
                        &col.ident,
                        reference,
                        Some(&col.sql_type),
                        Nullability::declared(col.not_null),
                    )
                }));
            }
        }
        found
    }

    /// The columns a star lists that have no Python type: a schema column of an unmapped
    /// type, or a view column its own query left untyped.
    pub(super) fn explain_untyped_star_columns(&self, reference: Option<&str>, scope: &Scope) {
        if !self.diag.enabled() {
            return;
        }
        let reference = reference.map(str::to_lowercase);
        let tables = Self::star_tables(reference.as_deref(), scope);
        for table in tables.iter().filter_map(Table::named) {
            if self.virtuals.contains_key(table) {
                continue;
            }
            if let Some(columns) = self.virtual_columns(table) {
                for col in columns
                    .iter()
                    .filter(|c| c.python_type.is_none() && !c.is_unknown_star())
                {
                    self.diag.record(
                        Rule::UnsupportedSyntax,
                        format!(
                            "view column `{table}.{}` is not typed by the view's query",
                            col.name
                        ),
                    );
                }
            } else {
                // `star_columns` read every table, so the schema has it.
                for col in self.schema.table(table).unwrap_or_default() {
                    if col.typed(self.dialect).python.is_none() {
                        self.diag.record(
                            Rule::UnknownType,
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
            Rule::UnsupportedSyntax,
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
                Rule::UnsupportedSyntax,
                format!("`*` reads {description}, which sqlty does not read the columns of"),
            );
        }
        if !self.judges_names() {
            return self.diag.explain_silently();
        }
        if let Some(reference) = &reference {
            if !scope.reads(reference) {
                return self.diag.record_about(
                    Rule::UnknownTable,
                    format!("`{reference}` in `{reference}.*` is not a table or alias in the FROM clause"),
                    Some(reference),
                    None,
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
        if tables
            .iter()
            .filter_map(Table::named)
            .any(|table| self.has_unknown_star(table))
        {
            return self.explain_view_star(reference.as_deref(), scope);
        }
        self.diag.record(
            Rule::UnsupportedSyntax,
            "`*` could not be expanded into columns",
        );
    }
}

/// Names quoted and joined; an unaliased subquery, whose reference is empty, by what it is.
fn list(names: &[&str]) -> String {
    let names: Vec<String> = names
        .iter()
        .map(|name| match *name {
            "" => "the subquery".to_string(),
            name => format!("`{name}`"),
        })
        .collect();
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
#[cfg_attr(coverage_nightly, coverage(off))]
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
