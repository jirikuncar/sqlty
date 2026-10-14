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
///
/// `nullability` is kept next to the type, never inside it. Callers decide whether to
/// spell it (see [`Column::display_type`]); only [`Nullability::Nullable`] makes a type optional.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Column {
    pub name: String,
    pub python_type: Option<String>,
    pub sql_type: Option<String>,
    pub nullability: Nullability,
    /// The shape of the JSON value the column holds, when the query spells it out.
    pub json: Option<JsonShape>,
}

impl Column {
    pub(super) fn unknown(name: &str) -> Self {
        Column {
            name: name.to_string(),
            python_type: None,
            sql_type: None,
            nullability: Nullability::Unknown,
            json: None,
        }
    }

    /// Whether the column can be NULL; unknown nullability counts as no.
    pub(crate) fn nullable(&self) -> bool {
        self.nullability == Nullability::Nullable
    }

    /// A column of the given type, with no JSON shape.
    pub(super) fn from_typed(name: String, typed: Typed, nullability: Nullability) -> Self {
        let (python_type, sql_type) = typed.into_strings();
        Column {
            name,
            python_type,
            sql_type,
            nullability,
            json: None,
        }
    }

    /// The column's type as the analyzer passes it around.
    pub(super) fn typed(&self) -> Typed {
        Typed::new(self.python_type.clone(), self.sql_type.clone())
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
    pub(super) outer: Rc<Scope>,
    /// Columns the joins of this SELECT merge (`USING`, `NATURAL`), so they are not ambiguous.
    pub(super) merged: Rc<[String]>,
    /// What is known about NULLs in the SELECT being analyzed: null-extended relations
    /// and columns its conditions rule out as NULL.
    pub(super) facts: Facts,
}

impl<'a> Analyzer<'a> {
    pub(super) fn new(schema: &'a SchemaData, dialect: SqlDialect) -> Self {
        Analyzer {
            schema,
            dialect,
            virtuals: Virtuals::default(),
            diag: Diagnostics::default(),
            outer: Rc::default(),
            merged: Rc::default(),
            facts: Facts::default(),
        }
    }

    pub(super) fn statement_columns(&self, stmt: &Statement) -> Inferred<Option<Vec<Column>>> {
        match stmt {
            Statement::Query(query) => self.query_columns(query),
            _ => match returning(stmt) {
                Some(items) => {
                    let scope = dml_scope(stmt, self.dialect);
                    for source in &scope {
                        self.check_table(&source.table);
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
                let name = fold(&cte.alias.name, self.dialect);
                let mut recursive = false;
                if let (true, SetExpr::SetOperation { left, .. }) =
                    (with.recursive, cte.query.body.as_ref())
                {
                    if let Some(select) = leftmost_select(left) {
                        let mut seed = inner.select_columns(select)?;
                        rename_columns(&mut seed, &cte.alias, self.dialect);
                        inner.virtuals.insert(name.clone(), seed);
                        recursive = true;
                    }
                }
                let mut columns = inner
                    .query_columns(&cte.query)?
                    .unwrap_or_else(|| vec![Column::unknown("*")]);
                rename_columns(&mut columns, &cte.alias, self.dialect);
                // A column of a recursive CTE is non-null only if it is in the anchor and in
                // the recursive term reading the rows so far: repeat until nothing changes.
                for round in 0..=columns.len() + 1 {
                    if !recursive {
                        break;
                    }
                    let known: Vec<Nullability> = inner
                        .virtuals
                        .get(&name)
                        .unwrap_or_default()
                        .iter()
                        .map(|col| col.nullability)
                        .collect();
                    let now: Vec<Nullability> = columns.iter().map(|col| col.nullability).collect();
                    if known == now {
                        break;
                    }
                    if round > columns.len() {
                        // Not settling: no column is known to be non-null.
                        for col in &mut columns {
                            col.nullability = Nullability::Nullable;
                        }
                        break;
                    }
                    inner.virtuals.insert(name.clone(), columns);
                    columns = inner
                        .query_columns(&cte.query)?
                        .unwrap_or_else(|| vec![Column::unknown("*")]);
                    rename_columns(&mut columns, &cte.alias, self.dialect);
                }
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
        let (mut analyzer, scope) = self.scope_of(&select.from)?;
        analyzer.facts = analyzer.select_facts(select, &scope);
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
                SetExpr::Values(values) if self.dialect == SqlDialect::Postgres => {
                    Some(self.values_columns(values)?)
                }
                other => {
                    if self.diag.enabled() {
                        self.diag.record(
                            NameRule::UnsupportedSyntax,
                            format!("{} is not typed", set_expr_kind(other)),
                        );
                    }
                    None
                }
            });
        }
        let shapes: Vec<Option<Vec<Nullability>>> = branches
            .iter()
            .map(|b| {
                b.as_ref()
                    .map(|cols| cols.iter().map(|c| c.nullability).collect())
            })
            .collect();
        let width = branches.first().and_then(|b| b.as_ref()).map(Vec::len);
        let nullability = set_nullability(body, &mut shapes.into_iter(), width);
        let mut branches = branches.into_iter();
        let Some(mut columns) = branches.next().flatten() else {
            return Ok(None);
        };
        let mut shapes: Vec<Option<JsonShape>> =
            columns.iter().map(|col| col.json.clone()).collect();
        let mut types: Vec<Option<Typed>> = columns.iter().map(|col| Some(col.typed())).collect();
        for branch in branches
            .flatten()
            .filter(|cols| cols.len() == columns.len())
        {
            for ((common, shape), col) in types.iter_mut().zip(&mut shapes).zip(branch) {
                *shape = shape.take().zip(col.json.clone()).map(|(a, b)| a.unify(&b));
                *common = common
                    .take()
                    .and_then(|ty| common_type(ty, col.typed(), self.dialect));
            }
        }
        for (column, common) in columns.iter_mut().zip(types) {
            if common.is_none() && self.diag.enabled() {
                self.diag.record(
                    NameRule::UnsupportedSyntax,
                    format!(
                        "set operation branches disagree on the type of column `{}`",
                        column.name
                    ),
                );
            }
            (column.python_type, column.sql_type) = common.unwrap_or(Typed::UNKNOWN).into_strings();
        }
        for ((column, nullability), shape) in columns.iter_mut().zip(nullability).zip(shapes) {
            column.nullability = nullability;
            column.json = shape;
        }
        Ok(Some(columns))
    }

    /// Columns of a `VALUES` list: `column1`, `column2`, ..., each typed by the common
    /// type of its rows (`VALUES (1), (2.5)` is `NUMERIC`) and nullable when a row's is.
    fn values_columns(&self, values: &sqlparser::ast::Values) -> Inferred<Vec<Column>> {
        let (analyzer, scope) = self.scope_of(&[])?;
        let width = values.rows.first().map_or(0, |row| row.content.len());
        let mut columns = Vec::with_capacity(width);
        for index in 0..width {
            let mut types = Vec::new();
            let mut nullability = Nullability::NonNull;
            for row in values.rows.iter().filter(|row| row.content.len() == width) {
                let expr = &row.content[index];
                types.push(analyzer.expr(expr, &scope)?);
                nullability = match (
                    nullability,
                    analyzer.nullability(expr, &scope, &analyzer.facts),
                ) {
                    (Nullability::Nullable, _) | (_, Nullability::Nullable) => {
                        Nullability::Nullable
                    }
                    (Nullability::NonNull, Nullability::NonNull) => Nullability::NonNull,
                    _ => Nullability::Unknown,
                };
            }
            let (python_type, sql_type) = common_of(types, self.dialect)
                .unwrap_or(Typed::UNKNOWN)
                .into_strings();
            columns.push(Column {
                name: format!("column{}", index + 1),
                python_type,
                sql_type,
                nullability,
                json: None,
            });
        }
        Ok(columns)
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
                SelectItem::ExprWithAlias { expr, alias } => {
                    (expr, alias_name(alias, self.dialect))
                }
                SelectItem::ExprWithAliases { expr, aliases } => (
                    expr,
                    aliases
                        .first()
                        .map_or_else(|| column_name(expr), |a| alias_name(a, self.dialect)),
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
            let mut typed = self.expr(expr, scope)?;
            // A bare integer column (`SELECT 1`) stays `int`; only strings are spelled as
            // a `Literal` there. Integer sets still come from `CASE`, `CHECK` and the like.
            if matches!(
                crate::values::constant(expr),
                Some(crate::values::Value::Int(_))
            ) {
                typed = strip(typed, self.dialect);
            }
            let sql_type = typed.sql.clone();
            let known = typed.is_known();
            let (python_type, _) = typed.into_strings();
            if !known && self.diag.enabled() && self.diag.explained() == explained && !is_wild(expr)
            {
                self.diag.record(
                    NameRule::UnsupportedSyntax,
                    format!("expression `{}` is not typed", shorten(&expr.to_string())),
                );
            }
            let nullability = self.nullability(expr, scope, &self.facts);
            let json = self.column_json(expr, sql_type.as_deref(), scope)?;
            columns.push(Column {
                name,
                python_type,
                sql_type: sql_type.map(|ty| ty.to_string()),
                nullability,
                json,
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
            Some(columns) => {
                if columns.iter().any(Column::is_unknown_star) {
                    self.explain_view_star(reference, scope);
                }
                self.explain_untyped_star_columns(reference, scope);
                columns
            }
            None => {
                self.explain_star(reference, scope, from);
                vec![Column::unknown("*")]
            }
        }
    }
}

/// Nullability of a set operation's columns, folding the branches' (taken from `shapes`
/// in `set_branches` order): UNION is nullable if either side is, INTERSECT only if both
/// are, EXCEPT follows its left side. A branch of another width or one not analyzed
/// leaves its columns unknown.
fn set_nullability(
    body: &SetExpr,
    shapes: &mut std::vec::IntoIter<Option<Vec<Nullability>>>,
    width: Option<usize>,
) -> Vec<Nullability> {
    use sqlparser::ast::SetOperator;
    let unknown = || vec![Nullability::Unknown; width.unwrap_or(0)];
    match body {
        SetExpr::SetOperation {
            op, left, right, ..
        } => {
            let left = set_nullability(left, shapes, width);
            let right = set_nullability(right, shapes, width);
            left.into_iter()
                .zip(right)
                .map(|(l, r)| match (op, l, r) {
                    (SetOperator::Except | SetOperator::Minus, l, _) => l,
                    (SetOperator::Intersect, Nullability::Nullable, Nullability::Nullable) => {
                        Nullability::Nullable
                    }
                    (SetOperator::Intersect, Nullability::NonNull, _)
                    | (SetOperator::Intersect, _, Nullability::NonNull) => Nullability::NonNull,
                    (SetOperator::Intersect, _, _) => Nullability::Unknown,
                    (_, Nullability::Nullable, _) | (_, _, Nullability::Nullable) => {
                        Nullability::Nullable
                    }
                    (_, Nullability::NonNull, Nullability::NonNull) => Nullability::NonNull,
                    _ => Nullability::Unknown,
                })
                .collect()
        }
        SetExpr::Query(query) if query.with.is_none() => {
            set_nullability(&query.body, shapes, width)
        }
        _ => match shapes.next().flatten() {
            Some(shape) if Some(shape.len()) == width => shape,
            _ => unknown(),
        },
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
pub(super) fn rename_columns(columns: &mut [Column], alias: &TableAlias, dialect: SqlDialect) {
    for (column, name) in columns.iter_mut().zip(&alias.columns) {
        column.name = alias_name(&name.name, dialect);
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
