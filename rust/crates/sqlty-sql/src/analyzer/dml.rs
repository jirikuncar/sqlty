//! DML statements: whether they return rows and the scope of their target tables.

use sqlparser::ast::{
    AssignmentTarget, Expr, FromTable, Ident, ObjectName, OnConflict, OnConflictAction, OnInsert,
    OutputClause, Query, SelectItem, SetExpr, Statement, TableObject, TableWithJoins,
    UpdateTableFromKind,
};

use super::dialect::SqlDialect;

use super::names::{object_name, table_key};
use super::narrowing::Learned;
use super::nullability::{Facts, Nullability};
use super::query::Analyzer;
use super::scope::{table_scope, Scope, Source, Table};
use crate::catalog::fold;
use crate::ddl::RowEvents;
use crate::diagnostics::{Diagnostics, NameRule};

pub(super) fn statement_returns_rows(stmt: &Statement) -> bool {
    match stmt {
        Statement::Query(query) => match query.body.as_ref() {
            SetExpr::Insert(inner)
            | SetExpr::Update(inner)
            | SetExpr::Delete(inner)
            | SetExpr::Merge(inner) => statement_returns_rows(inner),
            _ => true,
        },
        _ => returning(stmt).is_some(),
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
                vec![Source::named(reference, table_key(name, dialect))]
            }
            _ => Vec::new(),
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
        _ => Vec::new(),
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
        // A `BEFORE` row trigger may change any column of the row `RETURNING` returns.
        let triggers = match scope.first().map(|source| &source.table) {
            Some(Table::Named(key)) => self.schema.row_triggers(key),
            _ => RowEvents::default(),
        };
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
                    .map(|ident| {
                        let key = quiet.column_key(&target, &ident.value, scope);
                        (target.clone(), key)
                    })
                    .collect();
                self.facts.with(learned.without(&set))
            }
            _ => seen.clone(),
        };
        let non_null = |nullability| nullability == Nullability::NonNull;
        let written: Vec<String> = match stmt {
            Statement::Insert(insert) => {
                let TableObject::TableName(name) = &insert.table else {
                    return self.facts.clone();
                };
                let columns: Vec<String> = if insert.columns.is_empty() {
                    self.schema
                        .table(&table_key(name, self.dialect))
                        .map(|columns| columns.iter().map(|c| c.ident.clone()).collect())
                        .unwrap_or_default()
                } else {
                    insert
                        .columns
                        .iter()
                        .filter_map(|column| column.0.last().and_then(|part| part.as_ident()))
                        .map(|ident| ident.value.clone())
                        .collect()
                };
                let values = insert
                    .source
                    .as_deref()
                    .and_then(|source| quiet.query_columns(source).ok().flatten())
                    .unwrap_or_default();
                let inserted: Vec<String> = columns
                    .into_iter()
                    .zip(&values)
                    .filter(|(_, value)| non_null(value.nullability))
                    .map(|(column, _)| column)
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
                        upsert.push(Source::named(
                            "excluded".to_string(),
                            table_key(name, self.dialect),
                        ));
                        let excluded = inserted
                            .iter()
                            .map(|column| {
                                let key = quiet.column_key("excluded", column, &upsert);
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
                        .map(|ident| ident.value.clone()),
                    AssignmentTarget::Tuple(_) => None,
                })
                .collect(),
            _ => Vec::new(),
        };
        let columns = written
            .iter()
            .map(|column| (target.clone(), quiet.column_key(&target, column, scope)))
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
            self.check_table(&source.table);
        }
        // Conditions and values may also read the tables of UPDATE's FROM and DELETE's USING,
        // and those MySQL's multi-table UPDATE and DELETE join, targets included.
        let multi = multi_table(stmt);
        let (analyzer, scope) = match (multi, joined_tables(stmt)) {
            (None, None) => (self.clone(), scope),
            (multi, from) => {
                let tables: Vec<TableWithJoins> =
                    multi.into_iter().chain(from).flatten().cloned().collect();
                let Ok((analyzer, joined)) = self.scope_of(&tables) else {
                    return;
                };
                let targets = scope.iter().filter(|source| {
                    multi.is_none() && !matches!(source.table, Table::ExtraSources)
                });
                (analyzer, targets.cloned().chain(joined).collect())
            }
        };
        let check = |expr| analyzer.check_references(expr, &scope);
        match stmt {
            Statement::Insert(insert) => {
                if let TableObject::TableName(name) = &insert.table {
                    let assigned: Vec<ObjectName> = insert
                        .assignments
                        .iter()
                        .flat_map(|assignment| match &assignment.target {
                            AssignmentTarget::ColumnName(column) => vec![column.clone()],
                            AssignmentTarget::Tuple(columns) => columns.clone(),
                        })
                        .collect();
                    let columns = if insert.columns.is_empty() {
                        &assigned
                    } else {
                        &insert.columns
                    };
                    self.check_target_columns(name, columns);
                    if !columns.is_empty() {
                        self.check_missing_columns(name, columns);
                    }
                    // PostgreSQL's `ON CONFLICT DO UPDATE` reads the target and `EXCLUDED`, the
                    // row that was not inserted. SQLite reads a bare name as the target's.
                    if let (Some(OnInsert::OnConflict(on_conflict)), SqlDialect::Postgres) =
                        (&insert.on, self.dialect)
                    {
                        if let OnConflictAction::DoUpdate(update) = &on_conflict.action {
                            let set: Vec<ObjectName> = update
                                .assignments
                                .iter()
                                .flat_map(|assignment| match &assignment.target {
                                    AssignmentTarget::ColumnName(column) => vec![column.clone()],
                                    AssignmentTarget::Tuple(columns) => columns.clone(),
                                })
                                .collect();
                            self.check_target_columns(name, &set);
                            let mut upsert = scope.clone();
                            upsert.push(Source::named(
                                "excluded".to_string(),
                                table_key(name, self.dialect),
                            ));
                            let check = |expr| analyzer.check_references(expr, &upsert);
                            update.assignments.iter().for_each(|a| check(&a.value));
                            update.selection.iter().for_each(check);
                        }
                    }
                }
                // Its diagnostics are recorded; the columns are not needed. Nothing reads the
                // types of a plain VALUES source, so outside PostgreSQL, where they are not
                // inferred, that is not reported.
                let untyped_values = |source: &Query| {
                    self.dialect != SqlDialect::Postgres
                        && insert.returning.is_none()
                        && source.with.is_none()
                        && matches!(source.body.as_ref(), SetExpr::Values(_))
                };
                if let Some(source) = insert.source.as_deref().filter(|s| !untyped_values(s)) {
                    let _ = self.query_columns(source);
                }
            }
            Statement::Update(update) => {
                let targets: Vec<ObjectName> = update
                    .assignments
                    .iter()
                    .flat_map(|assignment| match &assignment.target {
                        AssignmentTarget::ColumnName(column) => vec![column.clone()],
                        AssignmentTarget::Tuple(columns) => columns.clone(),
                    })
                    .collect();
                // A target of a join is a column of any of its tables.
                if multi.is_some() {
                    for target in targets {
                        let idents: Option<Vec<Ident>> = target
                            .0
                            .iter()
                            .map(|part| part.as_ident().cloned())
                            .collect();
                        let expr = match idents {
                            Some(mut idents) if idents.len() == 1 => {
                                Expr::Identifier(idents.remove(0))
                            }
                            Some(idents) => Expr::CompoundIdentifier(idents),
                            None => continue,
                        };
                        analyzer.check_references(&expr, &scope);
                    }
                } else if let sqlparser::ast::TableFactor::Table { name, .. } =
                    &update.table.relation
                {
                    self.check_target_columns(name, &targets);
                }
                update.assignments.iter().for_each(|a| check(&a.value));
                update.selection.iter().for_each(check);
            }
            Statement::Delete(delete) => {
                delete.selection.iter().for_each(check);
            }
            _ => {}
        }
    }

    /// Records `insert-missing-column` for each `NOT NULL` column of the schema table
    /// `table` without a default that `columns` leave out.
    fn check_missing_columns(&self, table: &ObjectName, columns: &[ObjectName]) {
        if !self.judges_names() {
            return;
        }
        let Some(known) = self.schema.table(&table_key(table, self.dialect)) else {
            return;
        };
        let set: Vec<String> = columns
            .iter()
            .filter_map(|column| column.0.last().and_then(|part| part.as_ident()))
            .map(|ident| fold(ident, self.dialect))
            .collect();
        for column in known {
            if column.not_null && !column.has_default && !set.contains(&column.name) {
                self.diag.record(
                    NameRule::InsertMissingColumn,
                    format!(
                        "INSERT into `{}` leaves out `{}`, which is NOT NULL and has no default",
                        object_name(table),
                        column.ident
                    ),
                );
            }
        }
    }

    /// Records `unknown-column` for each of `columns` (`col` or `t.col`) that the schema
    /// table `table` does not have.
    fn check_target_columns(&self, table: &ObjectName, columns: &[ObjectName]) {
        if !self.judges_names() {
            return;
        }
        let Some(known) = self.schema.table(&table_key(table, self.dialect)) else {
            return;
        };
        for column in columns {
            let Some(ident) = column.0.last().and_then(|part| part.as_ident()) else {
                continue;
            };
            let name = fold(ident, self.dialect);
            if !known.iter().any(|c| c.name == name) {
                self.diag.record(
                    NameRule::UnknownColumn,
                    format!(
                        "column `{}` does not exist in `{}`",
                        ident.value,
                        object_name(table)
                    ),
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
    }
}

#[cfg(test)]
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
