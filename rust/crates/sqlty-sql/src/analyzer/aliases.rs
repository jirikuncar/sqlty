//! `column-alias-count`: a column alias list that does not fit the relation it renames.

use sqlparser::ast::TableAlias;
use sqlparser::tokenizer::Span;

use super::dialect::SqlDialect;
use super::query::{Analyzer, Column};
use super::scope::Source;
use crate::diagnostics::Rule;

/// What a column alias list renames.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Renamed {
    Cte,
    Derived,
    Function,
    Table,
}

impl Analyzer<'_> {
    /// Records `column-alias-count` when `alias` names more columns than `columns`, or
    /// fewer where the database also rejects that; skipped for a width not known.
    pub(super) fn check_alias_count(
        &self,
        alias: &TableAlias,
        columns: &[Column],
        renamed: Renamed,
    ) {
        if alias.columns.is_empty() || columns.iter().any(Column::is_unknown_star) {
            return;
        }
        self.check_alias_width(alias, columns.len(), renamed);
    }

    /// [`Self::check_alias_count`] of a relation `width` columns wide.
    pub(super) fn check_alias_width(&self, alias: &TableAlias, width: usize, renamed: Renamed) {
        let names = alias.columns.len();
        if names == 0 || names == width || !self.diag.enabled() {
            return;
        }
        // PostgreSQL keeps the remaining names of a shorter list; MySQL and SQLite do not.
        let judged = match self.dialect {
            SqlDialect::Postgres => names > width,
            SqlDialect::Other("duckdb") => {
                names > width && matches!(renamed, Renamed::Derived | Renamed::Table)
            }
            SqlDialect::MySql => matches!(renamed, Renamed::Cte | Renamed::Derived),
            SqlDialect::Sqlite => renamed == Renamed::Cte,
            _ => false,
        };
        if !judged {
            return;
        }
        let _at = self.diag.at(Span::union_iter(
            alias.columns.iter().map(|column| column.name.span),
        ));
        self.diag.record(
            Rule::ColumnAliasCount,
            format!(
                "`{}` has {} but {}",
                alias.name.value,
                count(width, "column"),
                count(names, "alias"),
            ),
        );
    }

    /// [`Self::check_alias_width`] of a schema table, view or CTE read under `alias`.
    pub(super) fn check_table_aliases(&self, alias: &TableAlias, entries: &[Source]) {
        let [source] = entries else {
            return;
        };
        let Some(table) = source.table.named() else {
            return;
        };
        let width = match self.virtual_columns(table) {
            Some(columns) if columns.iter().any(Column::is_unknown_star) => return,
            Some(columns) => columns.len(),
            None => match self.schema.table(table) {
                Some(columns) => columns.len(),
                None => return,
            },
        };
        self.check_alias_width(alias, width, Renamed::Table);
    }
}

fn count(n: usize, noun: &str) -> String {
    match (n, noun) {
        (1, _) => format!("1 {noun}"),
        (_, "alias") => format!("{n} aliases"),
        _ => format!("{n} {noun}s"),
    }
}
