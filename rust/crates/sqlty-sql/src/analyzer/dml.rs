//! DML statements: whether they return rows and the scope of their target tables.

use std::collections::HashSet;

use sqlparser::ast::{
    Assignment, AssignmentTarget, Expr, FromTable, Ident, MergeAction, MergeClauseKind, ObjectName,
    OnConflict, OnConflictAction, OnInsert, OutputClause, Query, SelectItem, SetExpr, SetOperator,
    Statement, TableObject, TableWithJoins, UpdateTableFromKind,
};

use super::dialect::SqlDialect;

use super::names::{object_name, table_key};
use super::narrowing::Learned;
use super::nullability::{Facts, Nullability};
use super::query::Analyzer;
use super::scope::{table_scope, Scope, Source, Table};
use crate::catalog::fold;
use crate::ddl::RowEvents;
use crate::diagnostics::{Diagnostics, Rule};

pub(super) fn statement_returns_rows(stmt: &Statement) -> bool {
    match stmt {
        Statement::Query(query) => match query.body.as_ref() {
            SetExpr::Insert(inner)
            | SetExpr::Update(inner)
            | SetExpr::Delete(inner)
            | SetExpr::Merge(inner) => statement_returns_rows(inner),
            body => !selects_into(body),
        },
        _ => returning(stmt).is_some(),
    }
}

/// Whether a query stores its rows instead of returning them: PostgreSQL's
/// `SELECT ... INTO table`, MySQL's `SELECT ... INTO @variable`. Only the first `SELECT` of
/// a set operation may have `INTO`.
pub(super) fn selects_into(body: &SetExpr) -> bool {
    match body {
        SetExpr::Select(select) => select.into.is_some(),
        SetExpr::SetOperation { left, .. } => selects_into(left),
        SetExpr::Query(query) => selects_into(&query.body),
        _ => false,
    }
}

/// The non-empty top-level RETURNING list of INSERT/UPDATE/DELETE/MERGE.
pub(super) fn returning(stmt: &Statement) -> Option<&[SelectItem]> {
    let items = match stmt {
        Statement::Insert(insert) => &insert.returning,
        Statement::Update(update) => &update.returning,
        Statement::Delete(delete) => &delete.returning,
        Statement::Merge(merge) => {
            return match &merge.output {
                Some(OutputClause::Returning { select_items, .. }) if !select_items.is_empty() => {
                    Some(select_items)
                }
                _ => None,
            };
        }
        _ => return None,
    };
    items.as_deref().filter(|items| !items.is_empty())
}

/// A MERGE's source, which its `RETURNING` reads before the target.
pub(super) fn merge_source(stmt: &Statement) -> Option<Vec<TableWithJoins>> {
    match stmt {
        Statement::Merge(merge) => Some(vec![TableWithJoins {
            relation: merge.source.clone(),
            joins: Vec::new(),
        }]),
        _ => None,
    }
}

/// The references of a MERGE's source when a `WHEN NOT MATCHED BY SOURCE` action returns
/// target rows that no source row matched: the source's columns are NULL in them.
pub(super) fn unmatched_sources(stmt: &Statement, dialect: SqlDialect) -> Vec<String> {
    let Statement::Merge(merge) = stmt else {
        return Vec::new();
    };
    let by_source = merge.clauses.iter().any(|clause| {
        clause.clause_kind == MergeClauseKind::NotMatchedBySource
            && !matches!(clause.action, MergeAction::DoNothing { .. })
    });
    if !by_source {
        return Vec::new();
    }
    table_scope(&merge.source, dialect)
        .into_iter()
        .map(|source| source.reference)
        .collect()
}

/// The tables of UPDATE's FROM and DELETE's USING, which the statement joins to its target.
pub(super) fn joined_tables(stmt: &Statement) -> Option<&[TableWithJoins]> {
    match stmt {
        Statement::Update(update) => update.from.as_ref().map(|from| match from {
            UpdateTableFromKind::BeforeSet(tables) | UpdateTableFromKind::AfterSet(tables) => {
                tables.as_slice()
            }
        }),
        Statement::Delete(delete) => delete.using.as_deref(),
        _ => None,
    }
}

/// The tables MySQL's multi-table UPDATE and DELETE join, the targets among them.
fn multi_table(stmt: &Statement) -> Option<&[TableWithJoins]> {
    match stmt {
        Statement::Update(update) if !update.table.joins.is_empty() => {
            Some(std::slice::from_ref(&update.table))
        }
        Statement::Delete(delete) => match &delete.from {
            FromTable::WithFromKeyword(tables) | FromTable::WithoutKeyword(tables)
                if tables.len() > 1 || tables.iter().any(|table| !table.joins.is_empty()) =>
            {
                Some(tables)
            }
            _ => None,
        },
        _ => None,
    }
}

/// Tables a DML statement writes to, for resolving RETURNING columns.
pub(super) fn dml_scope(stmt: &Statement, dialect: SqlDialect) -> Scope {
    match stmt {
        Statement::Insert(insert) => match &insert.table {
            TableObject::TableName(name) => {
                let reference = insert.table_alias.as_ref().map_or_else(
                    || object_name(name),
                    |alias| alias.alias.value.to_lowercase(),
                );
                vec![Source::named(
                    reference,
                    table_key(name, dialect).into_owned(),
                )]
                .into()
            }
            _ => Scope::new(),
        },
        Statement::Update(update) => {
            let mut scope = table_scope(&update.table.relation, dialect);
            if update.from.is_some() || !update.table.joins.is_empty() {
                scope.push(other_sources());
            }
            scope
        }
        Statement::Merge(merge) => {
            let mut scope = table_scope(&merge.table, dialect);
            scope.push(other_sources());
            scope
        }
        Statement::Delete(delete) => {
            let mut scope = match &delete.from {
                FromTable::WithFromKeyword(tables) | FromTable::WithoutKeyword(tables) => tables
                    .first()
                    .map(|table| table_scope(&table.relation, dialect))
                    .unwrap_or_default(),
            };
            if delete.using.is_some() {
                scope.push(other_sources());
            }
            scope
        }
        _ => Scope::new(),
    }
}

impl Analyzer<'_> {
    /// What `RETURNING` knows about the target's columns: those the statement writes with a
    /// value that is never NULL are not NULL in the rows it returns, and so are those the
    /// `WHERE` of an UPDATE or DELETE proves, unless an UPDATE sets them.
    pub(super) fn written_facts(&self, stmt: &Statement, scope: &Scope) -> Facts {
        let mut quiet = self.clone();
        quiet.diag = Diagnostics::default();
        let Some(target) = scope.first().map(|source| source.reference.clone()) else {
            return self.facts.clone();
        };
        let key = scope.first().and_then(|source| source.table.named());
        // A `BEFORE` row trigger may change any column of the row `RETURNING` returns.
        let triggers = key.map_or_else(RowEvents::default, |key| self.schema.row_triggers(key));
        let triggered = match stmt {
            Statement::Insert(insert) => {
                triggers.insert
                    || triggers.update
                        && matches!(
                            insert.on,
                            Some(OnInsert::OnConflict(OnConflict {
                                action: OnConflictAction::DoUpdate(_),
                                ..
                            }))
                        )
            }
            Statement::Update(_) => triggers.update,
            _ => false,
        };
        if triggered {
            return self.facts.clone();
        }
        let selection = match stmt {
            Statement::Update(update) => update.selection.as_ref(),
            Statement::Delete(delete) => delete.selection.as_ref(),
            _ => None,
        };
        let learned =
            selection.map_or_else(Learned::default, |cond| quiet.learn(cond, true, scope));
        // SET reads the row WHERE let through; RETURNING returns it as SET leaves it.
        let seen = self.facts.with(learned.clone());
        let kept = match stmt {
            Statement::Update(update) => {
                let set: Vec<(String, String)> = update
                    .assignments
                    .iter()
                    .flat_map(|assignment| match &assignment.target {
                        AssignmentTarget::ColumnName(column) => vec![column],
                        AssignmentTarget::Tuple(columns) => columns.iter().collect(),
                    })
                    .filter_map(|column| column.0.last().and_then(|part| part.as_ident()))
                    .map(|ident| (target.clone(), fold(ident, self.dialect)))
                    .collect();
                self.facts.with(learned.without(&set))
            }
            _ => seen.clone(),
        };
        let non_null = |nullability| nullability == Nullability::NonNull;
        let written: Vec<String> = match stmt {
            Statement::Insert(insert) => {
                let table = key
                    .and_then(|key| self.schema.table(key))
                    .unwrap_or_default();
                let columns: Vec<String> = if insert.columns.is_empty() {
                    table
                        .iter()
                        .filter(|c| !c.invisible)
                        .map(|c| c.ident.clone())
                        .collect()
                } else {
                    insert
                        .columns
                        .iter()
                        .filter_map(|column| column.0.last().and_then(|part| part.as_ident()))
                        .map(|ident| ident.value.clone())
                        .collect()
                };
                // `None` for `DEFAULT VALUES`, which writes no column.
                let values = insert
                    .source
                    .as_deref()
                    .map(|source| quiet.query_columns(source).ok().flatten());
                let written = match &values {
                    None => Some(0),
                    Some(values) => values.as_ref().map(Vec::len),
                };
                // A column the INSERT leaves out gets its DEFAULT; `VALUES` shorter than the
                // table leaves out the last columns.
                let listed: Option<Vec<String>> = if insert.columns.is_empty() {
                    written.map(|count| {
                        table
                            .iter()
                            .filter(|c| !c.invisible)
                            .take(count)
                            .map(|c| c.name.clone())
                            .collect()
                    })
                } else {
                    Some(
                        insert
                            .columns
                            .iter()
                            .filter_map(|column| column.0.last().and_then(|part| part.as_ident()))
                            .map(|ident| fold(ident, self.dialect))
                            .collect(),
                    )
                };
                let defaulted = listed.into_iter().flat_map(|listed| {
                    table
                        .iter()
                        .filter(move |c| !listed.contains(&c.name))
                        .filter(|c| {
                            c.default.as_ref().is_some_and(|default| {
                                non_null(quiet.nullability(
                                    default,
                                    &Scope::new(),
                                    &Facts::default(),
                                ))
                            })
                        })
                        .map(|c| c.name.clone())
                });
                let inserted: Vec<String> = columns
                    .into_iter()
                    .zip(values.flatten().unwrap_or_default())
                    .filter(|(_, value)| non_null(value.nullability))
                    .map(|(column, _)| column)
                    .chain(defaulted)
                    .collect();
                match &insert.on {
                    None
                    | Some(OnInsert::OnConflict(OnConflict {
                        action: OnConflictAction::DoNothing,
                        ..
                    })) => inserted,
                    // A row that conflicts is returned as updated: a column keeps its old value
                    // unless SET gives it one, which may read the row `EXCLUDED` from `VALUES`.
                    Some(OnInsert::OnConflict(OnConflict {
                        action: OnConflictAction::DoUpdate(update),
                        ..
                    })) => {
                        let mut upsert = scope.clone();
                        let table = key.unwrap_or_default().to_string();
                        upsert.push(Source::named("excluded".to_string(), table));
                        let excluded = inserted
                            .iter()
                            .map(|column| {
                                let key = quiet.column_key(column);
                                ("excluded".to_string(), key)
                            })
                            .collect();
                        let facts = self.facts.with(Learned::columns(excluded));
                        update
                            .assignments
                            .iter()
                            .filter_map(|assignment| match &assignment.target {
                                AssignmentTarget::ColumnName(column) => column
                                    .0
                                    .last()
                                    .and_then(|part| part.as_ident())
                                    .filter(|_| {
                                        non_null(quiet.nullability(
                                            &assignment.value,
                                            &upsert,
                                            &facts,
                                        ))
                                    })
                                    .map(|ident| ident.value.clone()),
                                AssignmentTarget::Tuple(_) => None,
                            })
                            .filter(|column| inserted.contains(column))
                            .collect()
                    }
                    Some(_) => Vec::new(),
                }
            }
            Statement::Update(update) => update
                .assignments
                .iter()
                .filter_map(|assignment| match &assignment.target {
                    AssignmentTarget::ColumnName(column) => column
                        .0
                        .last()
                        .and_then(|part| part.as_ident())
                        .filter(|_| non_null(quiet.nullability(&assignment.value, scope, &seen)))
                        .map(|ident| fold(ident, self.dialect)),
                    AssignmentTarget::Tuple(_) => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let columns = written
            .iter()
            .map(|column| (target.clone(), quiet.column_key(column)))
            .collect();
        kept.with(Learned::columns(columns))
    }

    /// The names an INSERT, UPDATE or DELETE reads and writes: its target tables, the
    /// columns it sets, and those its values, conditions and source query read.
    pub(super) fn dml_names(&self, stmt: &Statement) {
        if !self.diag.enabled() {
            return;
        }
        let scope = dml_scope(stmt, self.dialect);
        for source in &scope {
            self.check_table(&source.table, None);
        }
        self.check_tenant_dml(stmt);
        // Conditions and values may also read the tables of UPDATE's FROM and DELETE's USING,
        // and those MySQL's multi-table UPDATE and DELETE join, targets included.
        let multi = multi_table(stmt);
        let (analyzer, scope) = match (multi, joined_tables(stmt)) {
            (None, None) => (self.clone(), scope),
            (multi, from) => {
                let tables: Vec<TableWithJoins> =
                    multi.into_iter().chain(from).flatten().cloned().collect();
                let Ok((analyzer, joined)) = self.scope_of(&tables);
                let targets = scope.iter().filter(|source| {
                    multi.is_none() && !matches!(source.table, Table::ExtraSources)
                });
                (analyzer, targets.cloned().chain(joined).collect())
            }
        };
        let check = |expr| analyzer.check_references(expr, &scope);
        match stmt {
            Statement::Insert(insert) => {
                // Its diagnostics are recorded; the columns give the width of a query source.
                // Nothing reads the types of a plain VALUES source, and outside PostgreSQL it
                // is not analyzed: MySQL's may name the columns set before them.
                let untyped_values = |source: &Query| {
                    self.dialect != SqlDialect::Postgres
                        && insert.returning.is_none()
                        && source.with.is_none()
                        && matches!(source.body.as_ref(), SetExpr::Values(_))
                };
                let source_columns = insert
                    .source
                    .as_deref()
                    .filter(|s| !untyped_values(s))
                    .map(|source| self.query_columns(source));
                if let TableObject::TableName(name) = &insert.table {
                    let assigned: Vec<ObjectName> = assigned_columns(&insert.assignments);
                    let columns = if insert.columns.is_empty() {
                        &assigned
                    } else {
                        &insert.columns
                    };
                    self.check_target_columns(name, columns);
                    if let Some(source) = insert.source.as_deref() {
                        let listed: Vec<Option<String>> = if insert.columns.is_empty() {
                            let table = self.schema.table(&table_key(name, self.dialect));
                            let visible = table.into_iter().flatten().filter(|c| !c.invisible);
                            visible.map(|column| Some(column.name.clone())).collect()
                        } else {
                            columns
                                .iter()
                                .map(|column| self.target_name(column))
                                .collect()
                        };
                        let mut rows = Vec::new();
                        written_rows(&source.body, &mut rows);
                        for row in rows {
                            for (column, value) in listed.iter().zip(row) {
                                self.check_written_value(name, column.as_deref(), value);
                            }
                        }
                    }
                    let widths = insert_widths(insert, source_columns.as_ref());
                    if insert.columns.is_empty() && insert.assignments.is_empty() {
                        self.check_implicit_columns(name, widths.as_deref());
                    } else {
                        let set: Vec<String> = columns
                            .iter()
                            .filter_map(|column| column.0.last().and_then(|part| part.as_ident()))
                            .map(|ident| fold(ident, self.dialect))
                            .collect();
                        self.check_missing_columns(name, &set);
                        if !insert.columns.is_empty() {
                            self.check_value_count(name, columns.len(), widths.as_deref());
                        }
                    }
                    // PostgreSQL's `ON CONFLICT DO UPDATE` reads the target and `EXCLUDED`, the
                    // row that was not inserted. SQLite reads a bare name as the target's.
                    if let (Some(OnInsert::OnConflict(on_conflict)), SqlDialect::Postgres) =
                        (&insert.on, self.dialect)
                    {
                        if let OnConflictAction::DoUpdate(update) = &on_conflict.action {
                            let set: Vec<ObjectName> = assigned_columns(&update.assignments);
                            self.check_target_columns(name, &set);
                            self.check_assigned_values(name, &update.assignments);
                            let mut upsert = scope.clone();
                            upsert.push(Source::named(
                                "excluded".to_string(),
                                table_key(name, self.dialect).into_owned(),
                            ));
                            let check = |expr| analyzer.check_references(expr, &upsert);
                            update.assignments.iter().for_each(|a| check(&a.value));
                            update.selection.iter().for_each(check);
                        }
                    }
                }
            }
            Statement::Update(update) => {
                let targets: Vec<ObjectName> = assigned_columns(&update.assignments);
                // A target of a join is a column of any of its tables.
                if multi.is_some() {
                    let exprs = targets.iter().filter_map(|target| {
                        let mut idents = idents(target)?;
                        Some(match idents.len() {
                            1 => Expr::Identifier(idents.remove(0)),
                            _ => Expr::CompoundIdentifier(idents),
                        })
                    });
                    exprs.for_each(|expr| analyzer.check_written(&expr, &scope));
                } else if let sqlparser::ast::TableFactor::Table { name, .. } =
                    &update.table.relation
                {
                    self.check_target_columns(name, &targets);
                    self.check_assigned_values(name, &update.assignments);
                }
                update.assignments.iter().for_each(|a| check(&a.value));
                update.selection.iter().for_each(check);
                let unmatched = |selection| analyzer.check_unmatched(selection, &scope);
                update.selection.iter().for_each(unmatched);
            }
            Statement::Delete(delete) => {
                delete.selection.iter().for_each(check);
                let unmatched = |selection| analyzer.check_unmatched(selection, &scope);
                delete.selection.iter().for_each(unmatched);
            }
            _ => {}
        }
    }

    /// Records `insert-value-count` for a row of `widths` values that doesn't match the
    /// `count` columns an INSERT names.
    fn check_value_count(&self, table: &ObjectName, count: usize, widths: Option<&[usize]>) {
        if !self.judges_names() {
            return;
        }
        if let Some(&width) = widths.into_iter().flatten().find(|&&width| width != count) {
            self.record_value_count(table, width, count);
        }
    }

    /// The checks of an INSERT without a column list, which writes the visible columns of
    /// `table` in order. PostgreSQL fills the columns past the values with their defaults;
    /// MySQL and SQLite want a value for each, or none at all (`VALUES ()`, `DEFAULT VALUES`).
    fn check_implicit_columns(&self, table: &ObjectName, widths: Option<&[usize]>) {
        if !self.judges_names() {
            return;
        }
        let (Some(known), Some(widths)) =
            (self.schema.table(&table_key(table, self.dialect)), widths)
        else {
            return;
        };
        let visible: Vec<String> = known
            .iter()
            .filter(|column| !column.invisible)
            .map(|column| column.name.clone())
            .collect();
        let fits = |width: usize| {
            width == visible.len()
                || width == 0
                || (self.dialect == SqlDialect::Postgres && width < visible.len())
        };
        match widths.iter().find(|&&width| !fits(width)) {
            Some(&width) => self.record_value_count(table, width, visible.len()),
            None => widths
                .iter()
                .min()
                .into_iter()
                .for_each(|&width| self.check_missing_columns(table, &visible[..width])),
        }
    }

    fn record_value_count(&self, table: &ObjectName, width: usize, count: usize) {
        self.diag.record(
            Rule::InsertValueCount,
            format!(
                "INSERT into `{}` gives {width} value{} for {count} column{}",
                object_name(table),
                if width == 1 { "" } else { "s" },
                if count == 1 { "" } else { "s" },
            ),
        );
    }

    /// Records `insert-missing-column` for each `NOT NULL` column of the schema table
    /// `table` without a default that the folded column names `set` leave out.
    fn check_missing_columns(&self, table: &ObjectName, set: &[String]) {
        if !self.judges_names() {
            return;
        }
        let Some(known) = self.schema.table(&table_key(table, self.dialect)) else {
            return;
        };
        let set: HashSet<&str> = set.iter().map(String::as_str).collect();
        for column in known {
            if column.not_null && !column.has_default && !set.contains(column.name.as_str()) {
                self.diag.record_about(
                    Rule::InsertMissingColumn,
                    format!(
                        "INSERT into `{}` leaves out `{}`, which is NOT NULL and has no default",
                        object_name(table),
                        column.ident
                    ),
                    Some(&table_key(table, self.dialect)),
                    Some(&column.ident),
                );
            }
        }
    }

    /// The folded name of the column `column` (`col` or `t.col`) names.
    fn target_name(&self, column: &ObjectName) -> Option<String> {
        let ident = column.0.last().and_then(|part| part.as_ident())?;
        Some(fold(ident, self.dialect))
    }

    /// [`Analyzer::check_written_value`] of each `SET column = value` and each column of
    /// `SET (a, b) = (x, y)`.
    fn check_assigned_values(&self, table: &ObjectName, assignments: &[Assignment]) {
        for assignment in assignments {
            let targets = match &assignment.target {
                AssignmentTarget::ColumnName(column) => std::slice::from_ref(column),
                AssignmentTarget::Tuple(columns) => columns.as_slice(),
            };
            let values = match &assignment.value {
                Expr::Tuple(values) => values.iter().collect(),
                Expr::Nested(value) => vec![value.as_ref()],
                value => vec![value],
            };
            if targets.len() == values.len() {
                for (column, value) in targets.iter().zip(values) {
                    let column = self.target_name(column);
                    self.check_written_value(table, column.as_deref(), value);
                }
            }
        }
    }

    /// Records `literal-write` when `value` is a constant the values of `column` of `table`
    /// (an enum, or a `CHECK (column IN (...))`) leave out. Comparison is exact only in
    /// PostgreSQL and SQLite: MySQL compares with case-insensitive collations by default.
    fn check_written_value(&self, table: &ObjectName, column: Option<&str>, value: &Expr) {
        if !self.judges_names()
            || !matches!(self.dialect, SqlDialect::Postgres | SqlDialect::Sqlite)
        {
            return;
        }
        let key = table_key(table, self.dialect);
        let Some(schema_column) = column.and_then(|c| self.schema.column(&key, c).flatten()) else {
            return;
        };
        let (Some(allowed), Some(written)) = (
            schema_column.values.as_deref(),
            crate::values::constant(value),
        ) else {
            return;
        };
        if !crate::values::excludes(allowed, std::slice::from_ref(&written)) {
            return;
        }
        self.diag.record_about(
            Rule::LiteralWrite,
            format!(
                "`{}.{}` is {}, not {}",
                object_name(table),
                schema_column.ident,
                crate::values::spell(allowed, "or"),
                crate::values::spell(std::slice::from_ref(&written), "or")
            ),
            Some(&key),
            Some(&schema_column.ident),
        );
    }

    /// Records `unknown-column` for each of `columns` (`col` or `t.col`) that the schema
    /// table `table` does not have.
    fn check_target_columns(&self, table: &ObjectName, columns: &[ObjectName]) {
        if !self.judges_names() {
            return;
        }
        let key = table_key(table, self.dialect);
        if self.schema.table(&key).is_none() {
            return;
        }
        let idents = columns
            .iter()
            .filter_map(|column| column.0.last().and_then(|part| part.as_ident()));
        for ident in idents {
            let name = fold(ident, self.dialect);
            if self.schema.column(&key, &name).flatten().is_none() {
                self.diag.record_about(
                    Rule::UnknownColumn,
                    format!(
                        "column `{}` does not exist in `{}`",
                        ident.value,
                        object_name(table)
                    ),
                    Some(&table_key(table, self.dialect)),
                    Some(&ident.value),
                );
            }
        }
    }
}

/// RETURNING may read the tables of `FROM`, `USING` and `MERGE ... USING`, which are not
/// resolved here.
fn other_sources() -> Source {
    Source {
        reference: String::new(),
        table: Table::ExtraSources,
        span: sqlparser::tokenizer::Span::empty(),
    }
}

/// The number of values in each row an INSERT writes: those of its VALUES rows, the columns
/// of its query, or none for `DEFAULT VALUES`. `None` when unknown.
fn insert_widths(
    insert: &sqlparser::ast::Insert,
    columns: Option<&super::Inferred<Option<Vec<super::query::Column>>>>,
) -> Option<Vec<usize>> {
    let Some(source) = insert.source.as_deref() else {
        return insert.assignments.is_empty().then(|| vec![0]);
    };
    if let SetExpr::Values(values) = source.body.as_ref() {
        return Some(values.rows.iter().map(|row| row.len()).collect());
    }
    match columns {
        Some(Ok(Some(columns))) if !columns.iter().any(super::query::Column::is_unknown_star) => {
            Some(vec![columns.len()])
        }
        _ => None,
    }
}

fn idents(name: &ObjectName) -> Option<Vec<Ident>> {
    name.0.iter().map(|part| part.as_ident().cloned()).collect()
}

/// The names of the columns `assignments` set one by one, in order.
pub(super) fn assigned_names(assignments: &[Assignment]) -> Vec<String> {
    assignments
        .iter()
        .filter_map(|assignment| match &assignment.target {
            AssignmentTarget::ColumnName(column) => Some(object_name(column)),
            AssignmentTarget::Tuple(_) => None,
        })
        .collect()
}

/// The columns `assignments` set, those of `(a, b) = ...` included.
fn assigned_columns(assignments: &[Assignment]) -> Vec<ObjectName> {
    assignments
        .iter()
        .flat_map(|assignment| match &assignment.target {
            AssignmentTarget::ColumnName(column) => vec![column.clone()],
            AssignmentTarget::Tuple(columns) => columns.clone(),
        })
        .collect()
}

/// The expressions of each row `body`, an INSERT source, writes: the rows of `VALUES` and
/// the items of a `SELECT`, of each side of `UNION` and the left of `EXCEPT` or `INTERSECT`.
/// A `SELECT` with a `*` gives none; `TABLE t` is read as `SELECT * FROM t`.
pub(super) fn written_rows<'q>(body: &'q SetExpr, into: &mut Vec<Vec<&'q Expr>>) {
    match body {
        SetExpr::Values(values) => into.extend(values.rows.iter().map(|row| row.iter().collect())),
        SetExpr::SetOperation {
            op, left, right, ..
        } => {
            written_rows(left, into);
            if *op == SetOperator::Union {
                written_rows(right, into);
            }
        }
        SetExpr::Query(query) => written_rows(&query.body, into),
        body => {
            let items: Option<Vec<&Expr>> = body
                .as_select()
                .into_iter()
                .flat_map(|select| &select.projection)
                .map(|item| match item {
                    SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
                        Some(expr)
                    }
                    _ => None,
                })
                .collect();
            into.extend(items);
        }
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::super::test_support::{reports, statement};
    use super::*;

    fn returns_rows(sql: &str) -> bool {
        statement_returns_rows(&statement(sql, SqlDialect::Postgres))
    }

    fn scope_of(sql: &str, dialect: SqlDialect) -> Vec<String> {
        dml_scope(&statement(sql, dialect), dialect)
            .iter()
            .map(|source| format!("{}={:?}", source.reference, source.table))
            .collect()
    }

    #[test]
    fn assigned_columns_spread_tuples() {
        let Statement::Update(update) =
            statement("UPDATE t SET (a, b) = (1, 2), c = 3", SqlDialect::Postgres)
        else {
            panic!("not an UPDATE");
        };
        let names: Vec<String> = assigned_columns(&update.assignments)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(names, ["a", "b", "c"]);
    }

    #[test]
    fn queries_return_rows() {
        assert!(returns_rows("SELECT 1"));
        assert!(returns_rows("VALUES (1)"));
        assert!(returns_rows("WITH c AS (SELECT 1) SELECT * FROM c"));
    }

    #[test]
    fn dml_returns_rows_only_with_returning() {
        assert!(!returns_rows("INSERT INTO t (a) VALUES (1)"));
        assert!(returns_rows("INSERT INTO t (a) VALUES (1) RETURNING a"));
        assert!(!returns_rows("UPDATE t SET a = 1"));
        assert!(returns_rows("UPDATE t SET a = 1 RETURNING *"));
        assert!(!returns_rows("DELETE FROM t"));
        assert!(returns_rows("DELETE FROM t RETURNING a"));
    }

    #[test]
    fn a_cte_around_dml_follows_the_dml() {
        assert!(!returns_rows(
            "WITH d AS (SELECT 1) DELETE FROM t WHERE a IN (SELECT * FROM d)"
        ));
        assert!(returns_rows(
            "WITH d AS (SELECT 1) DELETE FROM t WHERE a IN (SELECT * FROM d) RETURNING a"
        ));
    }

    #[test]
    fn ddl_and_transaction_control_return_no_rows() {
        assert!(!returns_rows("CREATE TABLE t (a INT)"));
        assert!(!returns_rows("BEGIN"));
        assert!(!returns_rows("COMMIT"));
    }

    #[test]
    fn merge_returns_rows_only_with_returning() {
        let merge = "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE";
        assert!(!returns_rows(merge));
        assert!(returns_rows(&format!("{merge} RETURNING t.id")));
    }

    #[test]
    fn returning_lists_the_items_of_the_statement() {
        let items =
            |sql: &str| returning(&statement(sql, SqlDialect::Postgres)).map(<[SelectItem]>::len);
        assert_eq!(
            items("INSERT INTO t (a) VALUES (1) RETURNING a, b"),
            Some(2)
        );
        assert_eq!(items("UPDATE t SET a = 1 RETURNING *"), Some(1));
        assert_eq!(items("DELETE FROM t RETURNING a"), Some(1));
        assert_eq!(items("DELETE FROM t"), None);
        assert_eq!(items("SELECT 1"), None);
    }

    #[test]
    fn scope_of_an_insert_is_its_target() {
        assert_eq!(
            scope_of(
                "INSERT INTO Public.Users (a) VALUES (1)",
                SqlDialect::Postgres
            ),
            ["users=Named(\"public.users\")"]
        );
    }

    #[test]
    fn scope_of_an_update_adds_extra_sources_for_from_and_joins() {
        assert_eq!(
            scope_of("UPDATE t SET a = 1", SqlDialect::Postgres),
            ["t=Named(\"t\")"]
        );
        assert_eq!(
            scope_of("UPDATE t SET a = u.a FROM u", SqlDialect::Postgres),
            ["t=Named(\"t\")", "=ExtraSources"]
        );
        assert_eq!(
            scope_of(
                "UPDATE t JOIN u ON t.id = u.id SET t.a = 1",
                SqlDialect::MySql
            ),
            ["t=Named(\"t\")", "=ExtraSources"]
        );
    }

    #[test]
    fn scope_of_a_delete_adds_extra_sources_for_using() {
        assert_eq!(
            scope_of("DELETE FROM t AS x", SqlDialect::Postgres),
            ["x=Named(\"t\")"]
        );
        assert_eq!(
            scope_of(
                "DELETE FROM t USING u WHERE t.id = u.id",
                SqlDialect::Postgres
            ),
            ["t=Named(\"t\")", "=ExtraSources"]
        );
    }

    #[test]
    fn scope_of_a_merge_always_has_extra_sources() {
        assert_eq!(
            scope_of(
                "MERGE INTO t USING s ON t.id = s.id WHEN MATCHED THEN DELETE",
                SqlDialect::Postgres
            ),
            ["t=Named(\"t\")", "=ExtraSources"]
        );
    }

    #[test]
    fn other_statements_have_no_scope() {
        assert!(scope_of("SELECT 1", SqlDialect::Postgres).is_empty());
    }

    #[test]
    fn returning_columns_in_queries() {
        let ddl = "CREATE TABLE t (id SERIAL PRIMARY KEY, name TEXT NOT NULL, note TEXT); \
                   CREATE TABLE u (id INT, extra INT)";
        let queries = [
            "INSERT INTO t (name) VALUES ('a') RETURNING id, name, note",
            "INSERT INTO t (name) VALUES ('a') RETURNING *",
            "INSERT INTO t (name) VALUES ('a') RETURNING id AS new_id, upper(name) AS shout",
            "UPDATE t SET note = 'x' RETURNING *",
            "UPDATE t SET note = u.extra::text FROM u WHERE t.id = u.id RETURNING *",
            "UPDATE t SET note = u.extra::text FROM u WHERE t.id = u.id RETURNING t.id, extra",
            "DELETE FROM t USING u WHERE t.id = u.id RETURNING t.*",
            "DELETE FROM t WHERE id = 1 RETURNING nope",
            "INSERT INTO t (name) VALUES ('a')",
            "INSERT INTO missing (a) VALUES (1) RETURNING a",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(ddl)));
    }
}
