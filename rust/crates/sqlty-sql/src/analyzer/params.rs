//! Query parameters: the type and nullability each placeholder takes from its context.
//!
//! A placeholder has no type of its own. It takes one from what it is compared with, the
//! column an `INSERT` or `UPDATE` stores it in, an explicit cast, a known function
//! signature, `LIMIT`/`OFFSET`, or the array it is searched in (`= ANY(%s)`). A parameter
//! used where nothing says what it is stays untyped.

use std::collections::BTreeMap;

use sqlparser::ast::{
    Assignment, AssignmentTarget, Function, Insert, LimitClause, OnConflictAction, OnInsert, Query,
    UpdateTableFromKind, Visit, Visitor,
};

use super::*;
use crate::placeholders::{self, Styles};

/// A query parameter: `$N` in the rewritten query, and what its context says about it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Param {
    /// 1-based position: of the placeholder in the query, or its number (`$2`, `?2`).
    pub index: usize,
    /// The name of a `%(name)s` or `:name` parameter.
    pub name: Option<String>,
    /// Python type of the value to pass; `list[T]` for an array. `None` when unknown.
    pub python_type: Option<String>,
    pub sql_type: Option<String>,
    /// [`Nullability::NonNull`] when `None` cannot be passed (it is stored in a `NOT NULL`
    /// column), [`Nullability::Nullable`] when it can, else [`Nullability::Unknown`].
    pub nullability: Nullability,
    /// Whether the type is that of an explicit cast (`%s::text`): the server converts
    /// whatever is passed, so the value is not held to it.
    pub cast: bool,
}

/// What a parameter's uses say about it. The first use that types it wins, and so does the
/// first that says anything about nullability.
#[derive(Default)]
struct Hint {
    typed: Typed,
    nullability: Nullability,
    cast: bool,
}

#[derive(Default)]
pub(super) struct Hints(BTreeMap<usize, Hint>);

/// `python` with every `Literal[...]` replaced by its `str` or `int` base.
fn widen(python: &str) -> String {
    if let Some(base) = crate::values::base(python) {
        return base.to_string();
    }
    match python
        .strip_prefix("list[")
        .and_then(|s| s.strip_suffix(']'))
    {
        Some(inner) => format!("list[{}]", widen(inner)),
        None => python.to_string(),
    }
}

impl Hints {
    /// The type of an explicit cast.
    fn add_cast(&mut self, index: usize, typed: Typed) {
        let hint = self.0.entry(index).or_default();
        if hint.typed.python.is_none() && typed.python.is_some() {
            hint.typed = typed;
            hint.cast = true;
        }
    }

    fn add(&mut self, index: usize, mut typed: Typed, nullability: Nullability) {
        let hint = self.0.entry(index).or_default();
        if hint.typed.python.is_none() && typed.python.is_some() {
            // A parameter is compared with a value set, not limited to it: `str`, not `Literal`.
            typed.python = typed.python.map(|python| Rc::from(widen(&python)));
            hint.typed = typed;
        }
        if hint.nullability == Nullability::Unknown {
            hint.nullability = nullability;
        }
    }
}

/// The parameters of the first statement of `sql`, in order, with their inferred types.
/// `None` when the query does not parse.
///
/// Placeholders are read in every spelling `dialect` allows: `%s` and `%(name)s` always,
/// `$1` always, `?` outside PostgreSQL (where it is only a placeholder when the query does
/// not parse otherwise, being also a JSONB operator), `:name` in SQLite.
pub fn analyze_params(sql: &str, dialect: &str, schema: Option<&SchemaData>) -> Option<Vec<Param>> {
    let dialect = SqlDialect::from_name(dialect);
    let styles = Styles {
        question_marks: dialect != SqlDialect::Postgres,
        colon_names: dialect == SqlDialect::Sqlite,
        dollar_numbers: dialect == SqlDialect::Postgres,
        backslash_escapes: dialect == SqlDialect::MySql,
    };
    let read = |styles| {
        let rewritten = placeholders::rewrite(sql, styles);
        let stmt = parse(&parser_input(&rewritten.text, dialect), dialect)?
            .into_iter()
            .next()?;
        Some((stmt, rewritten.names))
    };
    let (stmt, names) = read(styles).or_else(|| {
        (dialect == SqlDialect::Postgres && sql.contains('?')).then(|| {
            read(Styles {
                question_marks: true,
                ..styles
            })
        })?
    })?;
    let analyzer = Analyzer::new(
        schema.unwrap_or_else(|| crate::catalog::empty(dialect)),
        dialect,
    );
    let mut hints = Hints::default();
    let _ = visit_expressions(&stmt, |expr| {
        if let Some(index) = placeholder_index(expr) {
            hints.0.entry(index).or_default();
        }
        ControlFlow::<()>::Continue(())
    });
    analyzer.statement_params(&stmt, &mut hints);
    let count = names
        .len()
        .max(hints.0.keys().next_back().copied().unwrap_or(0));
    Some(
        (1..=count)
            .map(|index| {
                let hint = hints.0.remove(&index).unwrap_or_default();
                Param {
                    index,
                    name: names.get(index - 1).cloned().flatten(),
                    python_type: hint.typed.python.map(|ty| ty.to_string()),
                    sql_type: hint.typed.sql.map(|ty| ty.to_string()),
                    nullability: hint.nullability,
                    cast: hint.cast,
                }
            })
            .collect(),
    )
}

/// The number of a `$N` placeholder, looking through parentheses.
fn placeholder_index(expr: &Expr) -> Option<usize> {
    match expr {
        Expr::Value(value) => match &value.value {
            Value::Placeholder(text) => text
                .strip_prefix(['$', '?'])?
                .parse()
                .ok()
                .filter(|n| *n > 0),
            _ => None,
        },
        Expr::Nested(inner) => placeholder_index(inner),
        _ => None,
    }
}

fn is_typed(typed: &Typed) -> bool {
    typed.python.is_some()
}

/// The comparisons whose operands share a type.
fn is_comparison(op: &BinaryOperator) -> bool {
    use BinaryOperator::*;
    matches!(op, Eq | NotEq | Lt | LtEq | Gt | GtEq | Spaceship)
}

impl<'a> Analyzer<'a> {
    fn param_typed(&self, sql_type: &str) -> Typed {
        from_sql_type(sql_type, self.dialect)
    }

    pub(super) fn statement_params(&self, stmt: &Statement, out: &mut Hints) {
        match stmt {
            Statement::Query(query) => self.params_query(query, out),
            Statement::Insert(insert) => self.params_insert(insert, out),
            Statement::Update(update) => {
                let mut from = vec![update.table.clone()];
                if let Some(
                    UpdateTableFromKind::BeforeSet(tables) | UpdateTableFromKind::AfterSet(tables),
                ) = &update.from
                {
                    from.extend(tables.iter().cloned());
                }
                let (analyzer, scope) = self.scope_or_empty(&from);
                let target = match &update.table.relation {
                    TableFactor::Table { name, .. } => Some(table_key(name, self.dialect)),
                    _ => None,
                };
                analyzer.params_assignments(&update.assignments, target.as_deref(), &scope, out);
                if let Some(selection) = &update.selection {
                    analyzer.walk_params(selection, &scope, out);
                }
            }
            Statement::Delete(delete) => {
                let (FromTable::WithFromKeyword(tables) | FromTable::WithoutKeyword(tables)) =
                    &delete.from;
                let mut from = tables.clone();
                from.extend(delete.using.iter().flatten().cloned());
                let (analyzer, scope) = self.scope_or_empty(&from);
                if let Some(selection) = &delete.selection {
                    analyzer.walk_params(selection, &scope, out);
                }
                if let Some(limit) = &delete.limit {
                    analyzer.hint_int(limit, out);
                }
            }
            _ => {}
        }
    }

    fn scope_or_empty(&self, from: &[TableWithJoins]) -> (Analyzer<'a>, Scope) {
        self.scope_of(from)
            .unwrap_or_else(|_| (self.clone(), Scope::new()))
    }

    fn params_query(&self, query: &Query, out: &mut Hints) {
        let mut analyzer = self.clone();
        if let Some(with) = &query.with {
            for cte in &with.cte_tables {
                analyzer.params_query(&cte.query, out);
                let mut columns = analyzer
                    .query_columns(&cte.query)
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| vec![Column::unknown("*")]);
                rename_columns(&mut columns, &cte.alias, self.dialect);
                analyzer
                    .virtuals
                    .insert(fold(&cte.alias.name, self.dialect), columns);
            }
        }
        analyzer.params_body(&query.body, out);
        match &query.limit_clause {
            Some(LimitClause::LimitOffset { limit, offset, .. }) => {
                for expr in limit.iter().chain(offset.iter().map(|o| &o.value)) {
                    analyzer.hint_int(expr, out);
                }
            }
            Some(LimitClause::OffsetCommaLimit { offset, limit }) => {
                analyzer.hint_int(offset, out);
                analyzer.hint_int(limit, out);
            }
            None => {}
        }
        if let Some(quantity) = query.fetch.as_ref().and_then(|f| f.quantity.as_ref()) {
            analyzer.hint_int(quantity, out);
        }
    }

    fn params_body(&self, body: &SetExpr, out: &mut Hints) {
        match body {
            SetExpr::Select(select) => self.params_select(select, out),
            SetExpr::SetOperation { left, right, .. } => {
                self.params_body(left, out);
                self.params_body(right, out);
            }
            SetExpr::Query(query) => self.params_query(query, out),
            SetExpr::Values(values) => {
                for expr in values.rows.iter().flat_map(|row| &row.content) {
                    self.walk_params(expr, &Scope::new(), out);
                }
            }
            SetExpr::Insert(stmt)
            | SetExpr::Update(stmt)
            | SetExpr::Delete(stmt)
            | SetExpr::Merge(stmt) => self.statement_params(stmt, out),
            _ => {}
        }
    }

    fn params_select(&self, select: &Select, out: &mut Hints) {
        let (analyzer, scope) = self.scope_or_empty(&select.from);
        for factor in relations(&select.from) {
            if let TableFactor::Derived { subquery, .. } = factor {
                analyzer.params_query(subquery, out);
            }
        }
        for join in select.from.iter().flat_map(|table| &table.joins) {
            if let Some(JoinConstraint::On(on)) = join_on(&join.join_operator) {
                analyzer.walk_params(on, &scope, out);
            }
        }
        for item in &select.projection {
            if let SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } = item {
                analyzer.walk_params(expr, &scope, out);
            }
        }
        let filters = [&select.selection, &select.having, &select.qualify];
        for expr in filters.into_iter().flatten() {
            analyzer.walk_params(expr, &scope, out);
        }
    }

    fn params_insert(&self, insert: &Insert, out: &mut Hints) {
        let TableObject::TableName(name) = &insert.table else {
            return;
        };
        let table = table_key(name, self.dialect);
        let scope = vec![Source::named(object_name(name), table.clone())];
        let targets: Vec<String> = if insert.columns.is_empty() {
            self.schema
                .table(&table)
                .map(|columns| columns.iter().map(|col| col.name.clone()).collect())
                .unwrap_or_default()
        } else {
            insert
                .columns
                .iter()
                .filter_map(|name| name.0.last()?.as_ident())
                .map(|ident| fold(ident, self.dialect))
                .collect()
        };
        match insert.source.as_deref() {
            Some(Query {
                body, with: None, ..
            }) if matches!(body.as_ref(), SetExpr::Values(_)) => {
                let SetExpr::Values(values) = body.as_ref() else {
                    return;
                };
                for row in &values.rows {
                    for (position, expr) in row.content.iter().enumerate() {
                        match (targets.get(position), placeholder_index(expr)) {
                            (Some(column), Some(index)) => {
                                self.hint_column(&table, column, index, out);
                            }
                            _ => self.walk_params(expr, &Scope::new(), out),
                        }
                    }
                }
            }
            Some(query) => self.params_query(query, out),
            None => {}
        }
        self.params_assignments(&insert.assignments, Some(&table), &scope, out);
        match &insert.on {
            Some(OnInsert::DuplicateKeyUpdate(assignments)) => {
                self.params_assignments(assignments, Some(&table), &scope, out);
            }
            Some(OnInsert::OnConflict(conflict)) => {
                if let OnConflictAction::DoUpdate(update) = &conflict.action {
                    self.params_assignments(&update.assignments, Some(&table), &scope, out);
                    if let Some(selection) = &update.selection {
                        self.walk_params(selection, &scope, out);
                    }
                }
            }
            _ => {}
        }
    }

    /// `SET column = value`: a placeholder value takes the column's type and nullability.
    fn params_assignments(
        &self,
        assignments: &[Assignment],
        table: Option<&str>,
        scope: &Scope,
        out: &mut Hints,
    ) {
        for assignment in assignments {
            let column = match &assignment.target {
                AssignmentTarget::ColumnName(name) => {
                    name.0.last().and_then(|part| part.as_ident())
                }
                AssignmentTarget::Tuple(_) => None,
            };
            match (table, column, placeholder_index(&assignment.value)) {
                (Some(table), Some(column), Some(index)) => {
                    self.hint_column(table, &fold(column, self.dialect), index, out);
                }
                _ => self.walk_params(&assignment.value, scope, out),
            }
        }
    }

    /// Parameter `index` is stored in `column` of the schema `table`.
    fn hint_column(&self, table: &str, column: &str, index: usize, out: &mut Hints) {
        let Some(col) = self
            .schema
            .table(table)
            .and_then(|columns| columns.iter().find(|col| col.name == column))
        else {
            return;
        };
        out.add(
            index,
            col.typed(self.dialect),
            Nullability::declared(col.not_null),
        );
    }

    /// `LIMIT`, `OFFSET` and `FETCH` take integers.
    fn hint_int(&self, expr: &Expr, out: &mut Hints) {
        if let Some(index) = placeholder_index(expr) {
            out.add(index, self.param_typed("BIGINT"), Nullability::Unknown);
        }
    }

    /// Visit `expr` and everything in it with `scope`; its subqueries get their own scopes.
    fn walk_params(&self, expr: &Expr, scope: &Scope, out: &mut Hints) {
        let mut walker = Walker {
            analyzer: self,
            scope,
            out,
            depth: 0,
        };
        let _ = expr.visit(&mut walker);
    }

    /// Parameter `placeholder` meets `other`: it takes the type of `other`.
    fn hint_pair(&self, placeholder: &Expr, other: &Expr, scope: &Scope, out: &mut Hints) {
        let Some(index) = placeholder_index(placeholder) else {
            return;
        };
        if placeholder_index(other).is_some() {
            return;
        }
        if let Ok(typed) = self.expr(other, scope) {
            if is_typed(&typed) {
                out.add(index, typed, Nullability::Unknown);
            }
        }
    }

    /// A parameter searched with `= ANY(%s)` is an array of what the other side is.
    fn hint_array(&self, array: &Expr, element: &Expr, scope: &Scope, out: &mut Hints) {
        let Some(index) = placeholder_index(array) else {
            return;
        };
        if let Ok(Typed {
            python: Some(python),
            sql,
        }) = self.expr(element, scope)
        {
            let typed = Typed::new(
                Some(format!("list[{python}]")),
                sql.map(|sql| format!("ARRAY<{sql}>")),
            );
            out.add(index, typed, Nullability::Unknown);
        }
    }

    fn expr_params(&self, expr: &Expr, scope: &Scope, out: &mut Hints) {
        match expr {
            Expr::BinaryOp { left, op, right } if is_comparison(op) => {
                if let (Some(array), true) = (any_argument(right), is_array_search(op)) {
                    self.hint_array(array, left, scope, out);
                } else if let (Some(array), true) = (any_argument(left), is_array_search(op)) {
                    self.hint_array(array, right, scope, out);
                } else {
                    self.hint_pair(left, right, scope, out);
                    self.hint_pair(right, left, scope, out);
                }
            }
            Expr::AnyOp { left, right, .. } | Expr::AllOp { left, right, .. } => {
                self.hint_array(right, left, scope, out);
                if let Some(index) = placeholder_index(left) {
                    if let Ok(Typed { sql: Some(sql), .. }) = self.expr(right, scope) {
                        if let Some(element) = array_element(&sql) {
                            out.add(
                                index,
                                from_sql_type(element, self.dialect),
                                Nullability::Unknown,
                            );
                        }
                    }
                }
            }
            Expr::IsDistinctFrom(left, right) | Expr::IsNotDistinctFrom(left, right) => {
                self.hint_pair(left, right, scope, out);
                self.hint_pair(right, left, scope, out);
            }
            Expr::Between {
                expr, low, high, ..
            } => {
                for bound in [low, high] {
                    self.hint_pair(bound, expr, scope, out);
                    self.hint_pair(expr, bound, scope, out);
                }
            }
            Expr::InList { expr, list, .. } => {
                for item in list {
                    self.hint_pair(item, expr, scope, out);
                    self.hint_pair(expr, item, scope, out);
                }
            }
            Expr::InSubquery { expr, subquery, .. } => {
                if let Some(index) = placeholder_index(expr) {
                    if let Ok(Some(columns)) = self.correlated(scope).query_columns(subquery) {
                        if let [column] = columns.as_slice() {
                            let typed =
                                Typed::new(column.python_type.clone(), column.sql_type.clone());
                            if is_typed(&typed) {
                                out.add(index, typed, Nullability::Unknown);
                            }
                        }
                    }
                }
            }
            Expr::Like { expr, pattern, .. }
            | Expr::ILike { expr, pattern, .. }
            | Expr::SimilarTo { expr, pattern, .. } => {
                for operand in [expr, pattern] {
                    if let Some(index) = placeholder_index(operand) {
                        out.add(index, self.param_typed("TEXT"), Nullability::Unknown);
                    }
                }
            }
            Expr::Cast { expr: inner, .. } => {
                if let (Some(index), Ok(typed)) = (placeholder_index(inner), self.expr(expr, scope))
                {
                    if is_typed(&typed) {
                        out.add_cast(index, typed);
                    }
                }
            }
            Expr::Function(function) => self.function_params(function, scope, out),
            _ => {}
        }
    }

    fn function_params(&self, function: &Function, scope: &Scope, out: &mut Hints) {
        let FunctionArguments::List(list) = &function.args else {
            return;
        };
        let args: Vec<Option<&Expr>> = list
            .args
            .iter()
            .map(|arg| match arg {
                FunctionArg::Unnamed(FunctionArgExpr::Expr(expr)) => Some(expr),
                _ => None,
            })
            .collect();
        let name = object_name(&function.name).to_uppercase();
        let info = lookup(&name);
        if info.is_some_and(|info| info.same_type) {
            for placeholder in args.iter().flatten() {
                for other in args.iter().flatten() {
                    self.hint_pair(placeholder, other, scope, out);
                }
            }
        } else if let Some(types) = info.and_then(|info| info.signature) {
            for (arg, sql_type) in args.iter().zip(types) {
                if let (Some(index), false) = (arg.and_then(placeholder_index), sql_type.is_empty())
                {
                    out.add(index, self.param_typed(sql_type), Nullability::Unknown);
                }
            }
        }
    }
}

/// The comparison operators `= ANY(...)` can follow.
fn is_array_search(op: &BinaryOperator) -> bool {
    is_comparison(op)
}

/// `x` of `ANY(x)`, `SOME(x)` or `ALL(x)`.
fn any_argument(expr: &Expr) -> Option<&Expr> {
    let Expr::Function(function) = expr else {
        return None;
    };
    let name = object_name(&function.name).to_uppercase();
    if !matches!(name.as_str(), "ANY" | "SOME" | "ALL") {
        return None;
    }
    match &function.args {
        FunctionArguments::List(list) => match list.args.as_slice() {
            [FunctionArg::Unnamed(FunctionArgExpr::Expr(arg))] => Some(arg),
            _ => None,
        },
        _ => None,
    }
}

/// The `ON` condition of a join.
fn join_on(operator: &JoinOperator) -> Option<&JoinConstraint> {
    use JoinOperator::*;
    match operator {
        Join(c)
        | Inner(c)
        | Left(c)
        | LeftOuter(c)
        | Right(c)
        | RightOuter(c)
        | FullOuter(c)
        | Semi(c)
        | LeftSemi(c)
        | RightSemi(c)
        | Anti(c)
        | LeftAnti(c)
        | RightAnti(c)
        | StraightJoin(c)
        | AsOf { constraint: c, .. } => Some(c),
        _ => None,
    }
}

/// Reads the expressions of one `SELECT` clause with its scope; a subquery is handed back
/// to the analyzer so its names resolve against its own `FROM`.
struct Walker<'x, 'a> {
    analyzer: &'x Analyzer<'a>,
    scope: &'x Scope,
    out: &'x mut Hints,
    /// Subqueries entered; expressions inside one belong to its own walk.
    depth: usize,
}

impl Visitor for Walker<'_, '_> {
    type Break = ();

    fn pre_visit_query(&mut self, query: &Query) -> ControlFlow<()> {
        if self.depth == 0 {
            self.analyzer
                .correlated(self.scope)
                .params_query(query, self.out);
        }
        self.depth += 1;
        ControlFlow::Continue(())
    }

    fn post_visit_query(&mut self, _: &Query) -> ControlFlow<()> {
        self.depth -= 1;
        ControlFlow::Continue(())
    }

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<()> {
        if self.depth == 0 {
            self.analyzer.expr_params(expr, self.scope, self.out);
        }
        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, \
                          age INTEGER, born DATE, tags TEXT[], uid UUID NOT NULL);
                          CREATE TABLE orders (id INT, user_id INT, total NUMERIC(10, 2));";

    /// The schema without the PostgreSQL-only types, for the other dialects.
    fn schema_for(dialect: &str) -> String {
        match dialect {
            "postgres" => SCHEMA.to_string(),
            _ => SCHEMA.replace(", tags TEXT[]", "").replace("UUID", "TEXT"),
        }
    }

    /// `"index:name:type"`, with `!` for non-null and `?` for nullable.
    fn params(sql: &str, dialect: &str) -> Vec<String> {
        let schema = load_schema(&schema_for(dialect), dialect);
        analyze_params(sql, dialect, Some(&schema))
            .unwrap_or_else(|| panic!("{sql} does not parse"))
            .iter()
            .map(|p| {
                let mark = match p.nullability {
                    Nullability::NonNull => "!",
                    Nullability::Nullable => "?",
                    Nullability::Unknown => "",
                };
                format!("{}{mark}", p.python_type.as_deref().unwrap_or("-"))
            })
            .collect()
    }

    fn pg(sql: &str) -> Vec<String> {
        params(sql, "postgres")
    }

    #[test]
    fn comparisons() {
        assert_eq!(
            pg("SELECT 1 FROM users WHERE id = %s AND name <> %s"),
            ["int", "str"]
        );
        assert_eq!(pg("SELECT 1 FROM users WHERE %s < age"), ["int"]);
        assert_eq!(
            pg("SELECT 1 FROM users u WHERE u.born >= $1"),
            ["datetime.date"]
        );
        assert_eq!(
            pg("SELECT 1 FROM users WHERE id BETWEEN %s AND %s"),
            ["int", "int"]
        );
        assert_eq!(pg("SELECT 1 FROM users WHERE name LIKE %s"), ["str"]);
        assert_eq!(
            pg("SELECT 1 FROM users WHERE id = %s OR id = %s"),
            ["int", "int"]
        );
        assert_eq!(pg("SELECT 1 FROM users WHERE 1 = %s"), ["int"]);
        assert_eq!(
            pg("SELECT 1 FROM users WHERE %s = ANY(ARRAY['a', 'b'])"),
            ["str"]
        );
    }

    #[test]
    fn named_parameters_are_shared() {
        let schema = load_schema(SCHEMA, "postgres");
        let found = analyze_params(
            "SELECT 1 FROM users WHERE id = %(id)s OR age > %(id)s OR name = %(n)s",
            "postgres",
            Some(&schema),
        )
        .unwrap();
        let summary: Vec<_> = found
            .iter()
            .map(|p| (p.index, p.name.as_deref(), p.python_type.as_deref()))
            .collect();
        assert_eq!(
            summary,
            [(1, Some("id"), Some("int")), (2, Some("n"), Some("str"))]
        );
    }

    #[test]
    fn lists_and_arrays() {
        assert_eq!(
            pg("SELECT 1 FROM users WHERE id IN (%s, %s)"),
            ["int", "int"]
        );
        assert_eq!(pg("SELECT 1 FROM users WHERE name IN (%s, 'a')"), ["str"]);
        assert_eq!(pg("SELECT 1 FROM users WHERE id = ANY(%s)"), ["list[int]"]);
        assert_eq!(pg("SELECT 1 FROM users WHERE id = ANY($1)"), ["list[int]"]);
        assert_eq!(pg("SELECT 1 FROM users WHERE %s = ANY(tags)"), ["str"]);
        assert_eq!(
            pg("SELECT 1 FROM users WHERE id IN (SELECT user_id FROM orders WHERE total > %s)"),
            ["decimal.Decimal"]
        );
        assert_eq!(
            pg("SELECT 1 FROM orders WHERE user_id IN (SELECT id FROM users WHERE name = %s)"),
            ["str"]
        );
        assert_eq!(
            pg("SELECT 1 FROM orders WHERE %s IN (SELECT id FROM users)"),
            ["int"]
        );
    }

    #[test]
    fn inserts_and_updates() {
        assert_eq!(
            pg("INSERT INTO users (id, name, age) VALUES (%s, %s, %s)"),
            ["int!", "str!", "int?"]
        );
        assert_eq!(
            pg("INSERT INTO users VALUES (%s, %s, %s), (%s, %s, %s)"),
            ["int!", "str!", "int?", "int!", "str!", "int?"]
        );
        assert_eq!(
            pg("UPDATE users SET name = %s, age = %s WHERE id = %s"),
            ["str!", "int?", "int"]
        );
        assert_eq!(
            pg("INSERT INTO users (id, name) VALUES (%s, %s) ON CONFLICT (id) DO UPDATE SET name = %s WHERE users.age > %s"),
            ["int!", "str!", "str!", "int"]
        );
        assert_eq!(pg("DELETE FROM users WHERE id = %s LIMIT 1"), ["int"]);
        assert_eq!(
            params("INSERT INTO users (id, name) VALUES (?, ?)", "sqlite"),
            ["int!", "str!"]
        );
        assert_eq!(
            pg("INSERT INTO users (id, name) SELECT id, %s FROM users WHERE age = %s"),
            ["-", "int"]
        );
    }

    #[test]
    fn casts_limits_and_functions() {
        assert_eq!(
            pg("SELECT %s::int, CAST(%s AS date), %s::text[]"),
            ["int", "datetime.date", "list[str]"]
        );
        assert_eq!(pg("SELECT 1 FROM users LIMIT %s OFFSET %s"), ["int", "int"]);
        assert_eq!(
            pg("SELECT 1 FROM users OFFSET $1 ROWS FETCH FIRST $2 ROWS ONLY"),
            ["int", "int"]
        );
        assert_eq!(
            pg("SELECT 1 FROM users WHERE lower(name) = lower(%s)"),
            ["str",]
        );
        assert_eq!(pg("SELECT lpad(name, %s, %s) FROM users"), ["int", "str"]);
        assert_eq!(pg("SELECT coalesce(age, %s) FROM users"), ["int"]);
        assert_eq!(
            pg("SELECT 1 FROM users WHERE name = %s::text AND age = %s"),
            ["str", "int"]
        );
        assert_eq!(pg("SELECT %s, %s"), ["-", "-"]);
    }

    #[test]
    fn scopes() {
        assert_eq!(
            pg("SELECT 1 FROM users u JOIN orders o ON o.user_id = u.id AND o.total > %s WHERE u.name = %s"),
            ["decimal.Decimal", "str"]
        );
        assert_eq!(
            pg("SELECT 1 FROM users u WHERE EXISTS (SELECT 1 FROM orders o WHERE o.user_id = u.id AND u.age > %s)"),
            ["int"]
        );
        assert_eq!(
            pg("WITH t AS (SELECT id AS k FROM users WHERE name = %s) SELECT 1 FROM t WHERE k = %s"),
            ["str", "int"]
        );
        assert_eq!(
            pg("SELECT 1 FROM (SELECT age AS a FROM users) s WHERE s.a = %s"),
            ["int"]
        );
        assert_eq!(pg("SELECT 1 FROM nosuch WHERE x = %s"), ["-"]);
        assert_eq!(
            pg("SELECT 1 UNION SELECT id FROM users WHERE name = %s"),
            ["str"]
        );
    }

    #[test]
    fn spellings() {
        assert_eq!(
            pg("SELECT 1 FROM users WHERE id = $2 AND name = $1"),
            ["str", "int"]
        );
        assert_eq!(
            params("SELECT 1 FROM users WHERE id = ?1", "sqlite"),
            ["int"]
        );
        assert_eq!(
            params("SELECT 1 FROM users WHERE id = :id AND name = :n", "sqlite"),
            ["int", "str"]
        );
        assert_eq!(params("SELECT 1 FROM users WHERE id = ?", "mysql"), ["int"]);
        assert_eq!(
            params("SELECT 1 FROM users WHERE id = %s", "mysql"),
            ["int"]
        );
        // `?` is a JSONB operator in PostgreSQL unless the query doesn't parse with it.
        assert_eq!(pg("SELECT 1 FROM users WHERE id = ?"), ["int"]);
        assert_eq!(
            pg("SELECT '[]'::jsonb ? 'a' FROM users WHERE id = %s"),
            ["int"]
        );
    }

    #[test]
    fn no_parameters_and_unparsed() {
        assert!(pg("SELECT 1").is_empty());
        let schema = load_schema(SCHEMA, "postgres");
        assert_eq!(
            analyze_params("SELEC 1 %s", "postgres", Some(&schema)),
            None
        );
    }
}
