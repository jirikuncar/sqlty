//! Name resolution: table scopes, CTE and view sources, `*` expansion and column lookup.

use super::*;

/// `(reference, table)` pairs for the tables a statement reads, in query order.
pub(super) type Scope = Vec<(String, String)>;

pub(super) fn table_scope(factor: &TableFactor) -> Scope {
    match factor {
        TableFactor::Table { name, alias, .. } => {
            let table = object_name(name);
            let reference = alias
                .as_ref()
                .map(|alias| alias.name.value.to_lowercase())
                .unwrap_or_else(|| table.clone());
            vec![(reference, table)]
        }
        _ => Vec::new(),
    }
}

/// Sources of a SELECT: each FROM relation followed by its joins (`FROM a, b` counts two),
/// looking into unaliased parentheses such as `FROM (a JOIN b ON ...)`.
pub(super) fn relations(from: &[TableWithJoins]) -> std::vec::IntoIter<&TableFactor> {
    fn push<'a>(table: &'a TableWithJoins, into: &mut Vec<&'a TableFactor>) {
        for factor in
            std::iter::once(&table.relation).chain(table.joins.iter().map(|j| &j.relation))
        {
            match factor {
                TableFactor::NestedJoin {
                    table_with_joins,
                    alias: None,
                } => push(table_with_joins, into),
                other => into.push(other),
            }
        }
    }
    let mut factors = Vec::new();
    for table in from {
        push(table, &mut factors);
    }
    factors.into_iter()
}

/// Whether any join in `from`, nested ones included, merges columns.
pub(super) fn any_merging_join(from: &[TableWithJoins]) -> bool {
    from.iter().any(|table| {
        table.joins.iter().any(merges_columns)
            || match &table.relation {
                TableFactor::NestedJoin {
                    table_with_joins,
                    alias: None,
                } => any_merging_join(std::slice::from_ref(table_with_joins)),
                _ => false,
            }
    })
}

/// The name a relation is referred to by: its alias, else its table name.
pub(super) fn relation_name(factor: &TableFactor) -> Option<String> {
    let alias = match factor {
        TableFactor::Table { name, alias, .. } => {
            return Some(alias.as_ref().map_or_else(
                || object_name(name),
                |alias| alias.name.value.to_lowercase(),
            ))
        }
        TableFactor::Derived { alias, .. } => alias.as_ref(),
        _ => None,
    };
    alias.map(|alias| alias.name.value.to_lowercase())
}

/// Whether a join merges same-named columns (`USING`, `NATURAL`), so `*` does not list them all.
pub(super) fn merges_columns(join: &Join) -> bool {
    use JoinOperator::*;
    let constraint = match &join.join_operator {
        Join(c)
        | Inner(c)
        | Left(c)
        | LeftOuter(c)
        | Right(c)
        | RightOuter(c)
        | FullOuter(c)
        | CrossJoin(c)
        | Semi(c)
        | LeftSemi(c)
        | RightSemi(c)
        | Anti(c)
        | LeftAnti(c)
        | RightAnti(c)
        | StraightJoin(c)
        | AsOf { constraint: c, .. } => c,
        _ => return false,
    };
    matches!(
        constraint,
        JoinConstraint::Using(_) | JoinConstraint::Natural
    )
}

/// The table an alias (or table name) refers to.
pub(super) fn resolve(reference: &str, scope: &Scope) -> String {
    let reference = reference.to_lowercase();
    scope
        .iter()
        .find(|(alias, _)| *alias == reference)
        .map(|(_, table)| table.clone())
        .unwrap_or(reference)
}

/// Output columns of the CTEs and derived tables in scope, by table key: a CTE's
/// name, or `(alias)` for a derived table.
pub(super) type Virtuals = HashMap<String, Vec<Column>>;

impl<'a> Analyzer<'a> {
    /// Columns of a CTE, derived table or schema view; a CTE hides a view of its name.
    pub(super) fn virtual_columns(&self, table: &str) -> Option<&Vec<Column>> {
        self.virtuals
            .get(table)
            .or_else(|| self.schema.views.get(table))
    }

    /// Whether a table reference reads a virtual source: a CTE or view by its bare
    /// name, or a view by its schema-qualified name.
    pub(super) fn is_virtual(&self, name: &ObjectName) -> bool {
        let table = object_name(name);
        if name.0.len() == 1 {
            self.virtual_columns(&table).is_some()
        } else {
            self.schema.views.contains_key(&table)
        }
    }

    /// Scope of a FROM clause, and an analyzer that knows its derived tables. A
    /// `LATERAL` subquery is analyzed with the relations before it.
    pub(super) fn scope_of(&self, from: &[TableWithJoins]) -> Inferred<(Analyzer<'a>, Scope)> {
        let mut analyzer = self.clone();
        let mut scope = Scope::new();
        for factor in relations(from) {
            match factor {
                TableFactor::Table { name, alias, .. } if analyzer.is_virtual(name) => {
                    let table = object_name(name);
                    let reference = alias
                        .as_ref()
                        .map_or_else(|| table.clone(), |alias| alias.name.value.to_lowercase());
                    scope.push((reference, table));
                }
                TableFactor::Derived {
                    subquery,
                    alias: Some(alias),
                    ..
                } => {
                    let mut columns = analyzer
                        .query_columns(subquery)?
                        .unwrap_or_else(|| vec![Column::unknown("*")]);
                    rename_columns(&mut columns, alias);
                    let reference = alias.name.value.to_lowercase();
                    let table = format!("({reference})");
                    analyzer.virtuals.insert(table.clone(), columns);
                    scope.push((reference, table));
                }
                other => scope.extend(table_scope(other)),
            }
        }
        Ok((analyzer, scope))
    }

    /// Whether `*` (or `t.*`) reads only sources with known columns: schema tables, and
    /// CTEs and derived tables whose own columns are known.
    ///
    /// `USING`/`NATURAL` joins merge columns, so their stars are unresolved.
    pub(super) fn star_resolvable(&self, from: &[TableWithJoins], reference: Option<&str>) -> bool {
        if reference.is_none() && any_merging_join(from) {
            return false;
        }
        let reference = reference.map(str::to_lowercase);
        let mut matched = relations(from)
            .filter(|factor| reference.is_none() || relation_name(factor) == reference)
            .peekable();
        matched.peek().is_some()
            && matched.all(|factor| match factor {
                TableFactor::Table { name, .. } => self
                    .virtual_columns(&object_name(name))
                    .is_none_or(|columns| !columns.iter().any(Column::is_unknown_star)),
                TableFactor::Derived {
                    alias: Some(alias), ..
                } => self
                    .virtuals
                    .get(&format!("({})", alias.name.value.to_lowercase()))
                    .is_some_and(|columns| !columns.iter().any(Column::is_unknown_star)),
                _ => false,
            })
    }

    /// Tables a star reads: the one `reference` names, or every scope table.
    pub(super) fn star_tables(reference: Option<&str>, scope: &Scope) -> Vec<String> {
        match reference {
            Some(reference) => vec![resolve(reference, scope)],
            None => scope.iter().map(|(_, table)| table.clone()).collect(),
        }
    }

    /// Expand `*` (all scope tables) or `t.*` from the sources, or `None` if unknown.
    pub(super) fn star_columns(
        &self,
        reference: Option<&str>,
        scope: &Scope,
    ) -> Option<Vec<Column>> {
        let tables = Self::star_tables(reference, scope);
        if tables.is_empty() {
            return None;
        }
        let columns = tables
            .iter()
            .map(|table| {
                self.virtual_columns(table).cloned().or_else(|| {
                    self.schema.get(table).map(|columns| {
                        columns
                            .iter()
                            .map(|col| {
                                let (python_type, sql_type) = col.typed(self.dialect);
                                Column {
                                    name: col.name.clone(),
                                    python_type,
                                    sql_type,
                                }
                            })
                            .collect()
                    })
                })
            })
            .collect::<Option<Vec<Vec<Column>>>>()?;
        Some(columns.into_iter().flatten().collect())
    }

    /// Expand `*` or `t.*` into schema columns, as `SELECT *` rewriting writes them.
    pub(super) fn schema_star_columns(
        &self,
        reference: Option<&str>,
        scope: &Scope,
    ) -> Option<Vec<SchemaColumn>> {
        let tables = Self::star_tables(reference, scope);
        if tables.is_empty() {
            return None;
        }
        let columns = tables
            .iter()
            .map(|table| self.schema.get(table))
            .collect::<Option<Vec<_>>>()?;
        Some(columns.into_iter().flatten().cloned().collect())
    }

    /// Type of a column: its qualifier's source, else the first scope source having it.
    pub(super) fn column_type(&self, expr: &Expr, scope: &Scope) -> Option<Typed> {
        let lookup = |table: &str, column: &str| {
            if let Some(columns) = self.virtual_columns(table) {
                return columns
                    .iter()
                    .find(|col| col.name.to_lowercase() == column)
                    .map(|col| (col.python_type.clone(), col.sql_type.clone()));
            }
            self.schema
                .get(table)?
                .iter()
                .find(|col| col.name == column)
                .map(|col| col.typed(self.dialect))
        };
        match expr {
            Expr::Identifier(ident) => {
                let column = ident.value.to_lowercase();
                scope.iter().find_map(|(_, table)| lookup(table, &column))
            }
            Expr::CompoundIdentifier(parts) if parts.len() >= 2 => {
                let table = resolve(&parts[parts.len() - 2].value, scope);
                lookup(&table, &parts[parts.len() - 1].value.to_lowercase())
            }
            _ => None,
        }
    }
}
