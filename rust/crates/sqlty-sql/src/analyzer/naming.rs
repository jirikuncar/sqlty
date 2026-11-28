//! What the relation names of a statement stand for, for highlighting: a schema table or
//! view, a CTE, an alias where it is defined, the schema that qualifies a table, and the
//! columns an `INSERT` lists or an `UPDATE` sets. Column references in expressions are named by the analyzer
//! as it resolves them ([`Analyzer::resolve_reference`](super::query::Analyzer)).

use std::ops::ControlFlow;

use rustc_hash::FxHashSet;
use sqlparser::ast::{
    AssignmentTarget, Insert, ObjectName, Query, Statement, TableAlias, TableFactor, TableObject,
    TableWithJoins, Update, Visit, Visitor,
};
use sqlparser::tokenizer::Span;

use super::dialect::SqlDialect;
use super::names::table_key;
use crate::catalog::{fold, folded, Lookup};
use crate::highlight::Highlight;

/// The names `stmt` defines or reads relations by, one pass over its tree.
pub(super) fn relation_names(
    stmt: &Statement,
    schema: &Lookup<'_>,
    dialect: SqlDialect,
) -> Vec<(Span, Highlight)> {
    let mut visitor = Names {
        schema,
        dialect,
        ctes: FxHashSet::default(),
        found: Vec::new(),
    };
    let _ = stmt.visit(&mut visitor);
    visitor.found
}

struct Names<'s, 'c> {
    schema: &'s Lookup<'c>,
    dialect: SqlDialect,
    /// The CTEs defined so far, folded: a CTE is defined before the queries that read it.
    ctes: FxHashSet<String>,
    found: Vec<(Span, Highlight)>,
}

impl Names<'_, '_> {
    /// What the relation `name` is, when the query or the schema defines it.
    fn relation(&self, name: &ObjectName) -> Option<Highlight> {
        if let [part] = name.0.as_slice() {
            let ident = part.as_ident()?;
            if self.ctes.contains(&fold(ident, self.dialect)) {
                return Some(Highlight::Variable { declaration: false });
            }
        }
        let key = table_key(name, self.dialect);
        if self.schema.view(&key).is_some() {
            Some(Highlight::View)
        } else if self.schema.defines(&key) {
            Some(Highlight::Table)
        } else {
            None
        }
    }

    fn declared(&mut self, alias: &TableAlias) {
        self.found
            .push((alias.name.span, Highlight::Variable { declaration: true }));
    }
}

impl Visitor for Names<'_, '_> {
    type Break = ();

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<()> {
        for cte in query.with.iter().flat_map(|with| &with.cte_tables) {
            self.ctes.insert(fold(&cte.alias.name, self.dialect));
            self.declared(&cte.alias);
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_relation(&mut self, name: &ObjectName) -> ControlFlow<()> {
        let Some(kind) = self.relation(name) else {
            return ControlFlow::Continue(());
        };
        let mut parts = name.0.iter().rev().filter_map(|part| part.as_ident());
        if let Some(last) = parts.next() {
            self.found.push((last.span, kind));
        }
        if let Some(schema) = parts.next() {
            self.found.push((schema.span, Highlight::Schema));
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> ControlFlow<()> {
        let alias = match factor {
            TableFactor::Table { alias, .. }
            | TableFactor::Derived { alias, .. }
            | TableFactor::TableFunction { alias, .. }
            | TableFactor::Function { alias, .. }
            | TableFactor::UNNEST { alias, .. }
            | TableFactor::JsonTable { alias, .. }
            | TableFactor::NestedJoin { alias, .. } => alias.as_ref(),
            _ => None,
        };
        if let Some(alias) = alias {
            self.declared(alias);
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_statement(&mut self, stmt: &Statement) -> ControlFlow<()> {
        let (table, columns): (_, Vec<&ObjectName>) = match stmt {
            Statement::Insert(Insert {
                table: TableObject::TableName(table),
                columns,
                ..
            }) => (table, columns.iter().collect()),
            Statement::Update(Update {
                table:
                    TableWithJoins {
                        relation: TableFactor::Table { name, .. },
                        ..
                    },
                assignments,
                ..
            }) => (
                name,
                assignments
                    .iter()
                    .flat_map(|assignment| match &assignment.target {
                        AssignmentTarget::ColumnName(column) => std::slice::from_ref(column),
                        AssignmentTarget::Tuple(columns) => columns.as_slice(),
                    })
                    .collect(),
            ),
            _ => return ControlFlow::Continue(()),
        };
        let key = table_key(table, self.dialect);
        for column in columns {
            let Some(ident) = column.0.last().and_then(|part| part.as_ident()) else {
                continue;
            };
            if let Some(Some(_)) = self.schema.column(&key, &folded(ident, self.dialect)) {
                self.found
                    .push((ident.span, Highlight::Column { deprecated: false }));
            }
        }
        ControlFlow::Continue(())
    }
}
