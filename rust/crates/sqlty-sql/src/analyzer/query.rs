//! Queries, set operations and projections: the output columns of a statement.

use super::*;

/// The SELECT whose columns a query returns: the left arm of a UNION, unparenthesized.
pub(super) fn leftmost_select(body: &SetExpr) -> Option<&Select> {
    match body {
        SetExpr::Select(select) => Some(select),
        SetExpr::SetOperation { left, .. } => leftmost_select(left),
        SetExpr::Query(query) => leftmost_select(&query.body),
        _ => None,
    }
}

/// A result column: name, Python type and SQL type; unknown types are `None`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub python_type: Option<String>,
    pub sql_type: Option<String>,
}

impl Column {
    pub(super) fn unknown(name: &str) -> Self {
        Column {
            name: name.to_string(),
            python_type: None,
            sql_type: None,
        }
    }

    pub(super) fn is_unknown_star(&self) -> bool {
        self.name == "*" && self.python_type.is_none()
    }
}

#[derive(Clone)]
pub(super) struct Analyzer<'a> {
    pub(super) schema: &'a SchemaData,
    pub(super) dialect: SqlDialect,
    pub(super) virtuals: Virtuals,
    /// Why results are untyped, shared with the analyzers of subqueries and CTEs.
    pub(super) diag: Diagnostics,
    /// FROM sources of the enclosing queries, for correlated subqueries and `LATERAL`.
    pub(super) outer: Scope,
    /// Columns the joins of this SELECT merge (`USING`, `NATURAL`), so they are not ambiguous.
    pub(super) merged: Vec<String>,
}

impl<'a> Analyzer<'a> {
    pub(super) fn new(schema: &'a SchemaData, dialect: SqlDialect) -> Self {
        Analyzer {
            schema,
            dialect,
            virtuals: HashMap::new(),
            diag: Diagnostics::default(),
            outer: Scope::new(),
            merged: Vec::new(),
        }
    }

    pub(super) fn statement_columns(&self, stmt: &Statement) -> Inferred<Option<Vec<Column>>> {
        match stmt {
            Statement::Query(query) => self.query_columns(query),
            _ => match returning(stmt) {
                Some(items) => {
                    let scope = dml_scope(stmt);
                    for (_, table) in &scope {
                        self.check_table(table);
                    }
                    self.projection(items, &scope, None).map(non_empty)
                }
                None => Ok(None),
            },
        }
    }

    pub(super) fn query_columns(&self, query: &Query) -> Inferred<Option<Vec<Column>>> {
        if let Some(with) = &query.with {
            let mut inner = self.clone();
            for cte in &with.cte_tables {
                let name = cte.alias.name.value.to_lowercase();
                if let (true, SetExpr::SetOperation { left, .. }) =
                    (with.recursive, cte.query.body.as_ref())
                {
                    if let Some(select) = leftmost_select(left) {
                        let mut seed = inner.select_columns(select)?;
                        rename_columns(&mut seed, &cte.alias);
                        inner.virtuals.insert(name.clone(), seed);
                    }
                }
                let mut columns = inner
                    .query_columns(&cte.query)?
                    .unwrap_or_else(|| vec![Column::unknown("*")]);
                rename_columns(&mut columns, &cte.alias);
                inner.virtuals.insert(name, columns);
            }
            return inner.query_body(query);
        }
        self.query_body(query)
    }

    pub(super) fn query_body(&self, query: &Query) -> Inferred<Option<Vec<Column>>> {
        match query.body.as_ref() {
            SetExpr::Insert(stmt)
            | SetExpr::Update(stmt)
            | SetExpr::Delete(stmt)
            | SetExpr::Merge(stmt) => self.statement_columns(stmt),
            body => self.set_columns(body),
        }
    }

    pub(super) fn select_columns(&self, select: &Select) -> Inferred<Vec<Column>> {
        let (analyzer, scope) = self.scope_of(&select.from)?;
        analyzer.projection(&select.projection, &scope, Some(&select.from))
    }

    /// Columns of a SELECT or set operation. Names come from the leftmost branch; each
    /// type is resolved across all branches as PostgreSQL resolves `UNION` (skipping
    /// branches of unknown type), and branches of conflicting types leave it unknown.
    pub(super) fn set_columns(&self, body: &SetExpr) -> Inferred<Option<Vec<Column>>> {
        let mut branches = Vec::new();
        for branch in set_branches(body) {
            branches.push(match branch {
                SetExpr::Select(select) => Some(self.select_columns(select)?),
                SetExpr::Query(query) => self.query_columns(query)?,
                other => {
                    self.diag.record(
                        NameRule::UnsupportedSyntax,
                        format!("{} is not typed", set_expr_kind(other)),
                    );
                    None
                }
            });
        }
        let mut branches = branches.into_iter();
        let Some(mut columns) = branches.next().flatten() else {
            return Ok(None);
        };
        let mut types: Vec<Option<Typed>> = columns
            .iter()
            .map(|col| Some((col.python_type.clone(), col.sql_type.clone())))
            .collect();
        for branch in branches
            .flatten()
            .filter(|cols| cols.len() == columns.len())
        {
            for (common, col) in types.iter_mut().zip(branch) {
                *common = common
                    .take()
                    .and_then(|ty| common_type(ty, (col.python_type, col.sql_type), self.dialect));
            }
        }
        for (column, common) in columns.iter_mut().zip(types) {
            if common.is_none() {
                self.diag.record(
                    NameRule::UnsupportedSyntax,
                    format!(
                        "set operation branches disagree on the type of column `{}`",
                        column.name
                    ),
                );
            }
            (column.python_type, column.sql_type) = common.unwrap_or((None, None));
        }
        Ok(Some(columns))
    }

    /// One column per result column; a star may yield several, or one unknown `*`
    /// column when the schema cannot resolve it. `from` is the SELECT's FROM clause.
    pub(super) fn projection(
        &self,
        items: &[SelectItem],
        scope: &Scope,
        from: Option<&[TableWithJoins]>,
    ) -> Inferred<Vec<Column>> {
        let mut columns = Vec::new();
        for item in items {
            let (expr, name) = match item {
                SelectItem::UnnamedExpr(expr) => (expr, column_name(expr)),
                SelectItem::ExprWithAlias { expr, alias } => (expr, alias.value.clone()),
                SelectItem::ExprWithAliases { expr, aliases } => (
                    expr,
                    aliases
                        .first()
                        .map_or_else(|| column_name(expr), |a| a.value.clone()),
                ),
                SelectItem::Wildcard(_) => {
                    columns.extend(self.star(None, scope, from));
                    continue;
                }
                SelectItem::QualifiedWildcard(
                    SelectItemQualifiedWildcardKind::ObjectName(name),
                    _,
                ) => {
                    columns.extend(self.star(Some(&object_name(name)), scope, from));
                    continue;
                }
                SelectItem::QualifiedWildcard(SelectItemQualifiedWildcardKind::Expr(_), _) => {
                    self.diag.record(
                        NameRule::UnsupportedSyntax,
                        "`<expression>.*` is not expanded",
                    );
                    columns.push(Column::unknown("*"));
                    continue;
                }
            };
            let explained = self.diag.explained();
            let (python_type, sql_type) = self.expr(expr, scope)?;
            if python_type.is_none() && self.diag.explained() == explained && !is_wild(expr) {
                self.diag.record(
                    NameRule::UnsupportedSyntax,
                    format!("expression `{}` is not typed", shorten(&expr.to_string())),
                );
            }
            columns.push(Column {
                name,
                python_type,
                sql_type,
            });
        }
        Ok(columns)
    }

    /// Columns of a star from the schema, or a single unknown `*` column.
    pub(super) fn star(
        &self,
        reference: Option<&str>,
        scope: &Scope,
        from: Option<&[TableWithJoins]>,
    ) -> Vec<Column> {
        let resolvable = from.is_none_or(|from| self.star_resolvable(from, reference));
        match resolvable
            .then(|| self.star_columns(reference, scope))
            .flatten()
        {
            Some(columns) => columns,
            None => {
                self.explain_star(reference, scope, from);
                vec![Column::unknown("*")]
            }
        }
    }
}

/// The SELECTs and `WITH` queries a set operation combines, left to right.
pub(super) fn set_branches(body: &SetExpr) -> Vec<&SetExpr> {
    match body {
        SetExpr::SetOperation { left, right, .. } => {
            let mut branches = set_branches(left);
            branches.extend(set_branches(right));
            branches
        }
        SetExpr::Query(query) if query.with.is_none() => set_branches(&query.body),
        branch => vec![branch],
    }
}

/// Apply a column alias list (`t(a, b)`) to a derived table's columns, by position.
pub(super) fn rename_columns(columns: &mut [Column], alias: &TableAlias) {
    for (column, name) in columns.iter_mut().zip(&alias.columns) {
        column.name = name.name.value.clone();
    }
}

pub(super) fn non_empty(columns: Vec<Column>) -> Option<Vec<Column>> {
    (!columns.is_empty()).then_some(columns)
}

/// A NULL or placeholder: untyped by nature, not for want of analysis.
fn is_wild(expr: &Expr) -> bool {
    match expr {
        Expr::Nested(inner) => is_wild(inner),
        Expr::Value(value) => matches!(value.value, Value::Null | Value::Placeholder(_)),
        _ => false,
    }
}

fn set_expr_kind(body: &SetExpr) -> &'static str {
    match body {
        SetExpr::Values(_) => "a VALUES list",
        SetExpr::Table(_) => "a TABLE query",
        _ => "this query form",
    }
}

/// `text`, cut to a length that reads well in a message.
pub(super) fn shorten(text: &str) -> String {
    const MAX: usize = 60;
    match text.char_indices().nth(MAX) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text.to_string(),
    }
}
