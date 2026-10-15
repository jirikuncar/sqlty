//! Queries, set operations and projections: the output columns of a statement.

use std::rc::Rc;

use sqlparser::ast::{
    Expr, Query, Select, SelectItem, SelectItemQualifiedWildcardKind, SetExpr, Statement,
    TableAlias, TableWithJoins, Value,
};

use crate::catalog::fold;
use crate::diagnostics::{Diagnostics, NameRule};
use crate::json::JsonShape;

use super::dialect::SqlDialect;
use super::dml::{dml_scope, returning};
use super::literals::strip;
use super::names::{alias_name, column_name, object_name};
use super::nullability::{Facts, Nullability};
use super::operators::{common_of, common_type};
use super::scope::{Scope, Virtuals};
use super::typed::Typed;
use super::{Inferred, SchemaData};

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
    /// The result column's name, as PostgreSQL names it (`?column?` for an unnamed expression).
    pub name: String,
    /// The Python type, e.g. `int` or `list[str]`; `None` when it is not known.
    pub python_type: Option<String>,
    /// The canonical SQL type, e.g. `BIGINT`; `None` for a constant or an unknown type.
    pub sql_type: Option<String>,
    /// Whether the column can be NULL.
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

#[cfg(test)]
mod tests {
    use sqlparser::ast::{Ident, Statement, TableAlias, TableAliasColumnDef};

    use super::super::test_support::{reports, statement};
    use super::*;

    fn body_of(sql: &str) -> Box<SetExpr> {
        let Statement::Query(query) = statement(sql, SqlDialect::Postgres) else {
            panic!("not a query");
        };
        query.body
    }

    fn column(name: &str, python: Option<&str>, nullability: Nullability) -> Column {
        Column {
            name: name.to_string(),
            python_type: python.map(str::to_string),
            sql_type: None,
            nullability,
            json: None,
        }
    }

    #[test]
    fn display_type_is_optional_only_for_nullable_columns() {
        let display =
            |python: &str, nullability| column("c", Some(python), nullability).display_type();
        assert_eq!(display("int", Nullability::NonNull).as_deref(), Some("int"));
        assert_eq!(display("int", Nullability::Unknown).as_deref(), Some("int"));
        assert_eq!(
            display("int", Nullability::Nullable).as_deref(),
            Some("int | None")
        );
        assert_eq!(
            column("c", None, Nullability::Nullable).display_type(),
            None
        );
    }

    #[test]
    fn display_type_does_not_double_none() {
        let display =
            |python: &str| column("c", Some(python), Nullability::Nullable).display_type();
        assert_eq!(display("typing.Any").as_deref(), Some("typing.Any"));
        assert_eq!(display("None").as_deref(), Some("None"));
        assert_eq!(display("int | None").as_deref(), Some("int | None"));
    }

    #[test]
    fn unknown_star_is_the_marker_of_an_unreadable_wildcard() {
        assert!(Column::unknown("*").is_unknown_star());
        assert!(!Column::unknown("id").is_unknown_star());
        assert!(!column("*", Some("int"), Nullability::NonNull).is_unknown_star());
    }

    #[test]
    fn only_nullable_columns_are_nullable() {
        assert!(column("c", None, Nullability::Nullable).nullable());
        assert!(!column("c", None, Nullability::NonNull).nullable());
        assert!(!column("c", None, Nullability::Unknown).nullable());
    }

    #[test]
    fn from_typed_splits_the_type_and_typed_joins_it_again() {
        let typed = Typed::of_sql("BIGINT", SqlDialect::Postgres);
        let column = Column::from_typed("n".into(), typed.clone(), Nullability::NonNull);
        assert_eq!(column.python_type.as_deref(), Some("int"));
        assert_eq!(column.sql_type.as_deref(), Some("BIGINT"));
        assert_eq!(column.typed(), typed);
        assert!(column.json.is_none());
    }

    #[test]
    fn leftmost_select_descends_into_set_operations() {
        let name = |sql: &str| {
            leftmost_select(&body_of(sql)).map(|select| select.projection[0].to_string())
        };
        assert_eq!(name("SELECT 1 AS a").as_deref(), Some("1 AS a"));
        assert_eq!(
            name("SELECT 1 AS a UNION SELECT 2 AS b").as_deref(),
            Some("1 AS a")
        );
        assert_eq!(
            name("(SELECT 3 AS c) UNION SELECT 2 AS b").as_deref(),
            Some("3 AS c")
        );
        assert_eq!(name("VALUES (1)"), None);
    }

    #[test]
    fn set_branches_flattens_nested_operations_left_to_right() {
        let count = |sql: &str| set_branches(&body_of(sql)).len();
        assert_eq!(count("SELECT 1"), 1);
        assert_eq!(count("SELECT 1 UNION SELECT 2"), 2);
        assert_eq!(count("SELECT 1 UNION SELECT 2 EXCEPT SELECT 3"), 3);
        assert_eq!(count("(SELECT 1 UNION SELECT 2) INTERSECT SELECT 3"), 3);
    }

    #[test]
    fn set_branches_keeps_a_with_query_whole() {
        let body = body_of("SELECT 1 UNION (WITH c AS (SELECT 2) SELECT * FROM c)");
        let branches = set_branches(&body);
        assert_eq!(branches.len(), 2);
        assert!(matches!(branches[1], SetExpr::Query(_)));
    }

    #[test]
    fn rename_columns_applies_the_alias_list_by_position() {
        let mut columns = vec![
            Column::unknown("a"),
            Column::unknown("b"),
            Column::unknown("c"),
        ];
        let alias = TableAlias {
            explicit: true,
            at: None,
            name: Ident::new("t"),
            columns: vec![
                TableAliasColumnDef::from_name("X"),
                TableAliasColumnDef::from_name("Y"),
            ],
        };
        rename_columns(&mut columns, &alias, SqlDialect::Postgres);
        let names: Vec<_> = columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["x", "y", "c"]);
        rename_columns(&mut columns, &alias, SqlDialect::MySql);
        let names: Vec<_> = columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["X", "Y", "c"]);
    }

    #[test]
    fn non_empty_drops_an_empty_column_list() {
        assert!(non_empty(Vec::new()).is_none());
        assert_eq!(
            non_empty(vec![Column::unknown("a")]).map(|c| c.len()),
            Some(1)
        );
    }

    #[test]
    fn shorten_cuts_long_text_on_a_character_boundary() {
        assert_eq!(shorten("short"), "short");
        let exactly = "a".repeat(60);
        assert_eq!(shorten(&exactly), exactly);
        let long = "b".repeat(61);
        assert_eq!(shorten(&long), format!("{}...", "b".repeat(60)));
        let wide = "é".repeat(70);
        assert_eq!(shorten(&wide), format!("{}...", "é".repeat(60)));
    }

    const DDL: &str = "CREATE TABLE a (id INT NOT NULL, label TEXT, score NUMERIC(5, 2) NOT NULL); \
                       CREATE TABLE b (id BIGINT, label VARCHAR(20) NOT NULL, score DOUBLE PRECISION); \
                       CREATE TABLE c (id INT NOT NULL, flag BOOLEAN NOT NULL)";

    #[test]
    fn set_operations_unify_types() {
        let queries = [
            "SELECT id, label FROM a UNION SELECT id, label FROM b",
            "SELECT id, score FROM a UNION ALL SELECT id, score FROM b",
            "SELECT id FROM a UNION SELECT flag FROM c",
            "SELECT id FROM a UNION SELECT 1",
            "SELECT id FROM a UNION SELECT NULL",
            "SELECT NULL AS x UNION SELECT 1",
            "SELECT id FROM a UNION SELECT id, label FROM b",
            "SELECT id FROM a UNION SELECT id FROM b UNION SELECT id FROM c",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn set_operations_combine_nullability() {
        let queries = [
            "SELECT id FROM b UNION SELECT id FROM a",
            "SELECT id FROM a UNION SELECT id FROM a",
            "SELECT id FROM b INTERSECT SELECT id FROM a",
            "SELECT id FROM b INTERSECT SELECT id FROM b",
            "SELECT id FROM a EXCEPT SELECT id FROM b",
            "SELECT id FROM b EXCEPT SELECT id FROM a",
            "SELECT label FROM a UNION ALL SELECT label FROM b",
            "(SELECT id FROM a) UNION (SELECT id FROM b)",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn column_names_come_from_the_first_branch() {
        let queries = [
            "SELECT id AS first FROM a UNION SELECT id AS second FROM b",
            "SELECT 1 AS n UNION ALL SELECT 2 UNION ALL SELECT 3",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn values_lists() {
        let queries = [
            "VALUES (1, 'a'), (2, 'b')",
            "VALUES (1), (2.5)",
            "VALUES (1), (NULL)",
            "VALUES (1), (1, 2)",
            "SELECT * FROM (VALUES (1, 'a')) AS v(n, s)",
            "SELECT column1 FROM (VALUES (1)) AS v",
            "VALUES (1) UNION SELECT id FROM a",
        ];
        let postgres = reports(&queries, "postgres", Some(DDL));
        let mysql = reports(&queries[..1], "mysql", None);
        insta::assert_snapshot!(format!("postgres:\n{postgres}\nmysql:\n{mysql}"));
    }

    #[test]
    fn ctes() {
        let queries = [
            "WITH x AS (SELECT id FROM a) SELECT id FROM x",
            "WITH x AS (SELECT id FROM a), y AS (SELECT id FROM x WHERE id > 1) SELECT * FROM y",
            "WITH x(n) AS (SELECT id FROM a) SELECT n FROM x",
            "WITH x AS (SELECT id FROM a) SELECT id FROM x UNION SELECT id FROM b",
            "WITH x AS (SELECT 1) SELECT * FROM x",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn recursive_ctes_settle_nullability() {
        let queries = [
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 5) SELECT i FROM n",
            "WITH RECURSIVE t(id, parent) AS (SELECT id, NULL::INT FROM a UNION ALL SELECT a.id, t.id FROM a JOIN t ON true) SELECT id, parent FROM t",
            "WITH RECURSIVE t(x, y) AS (SELECT 1, 2 UNION ALL SELECT y, NULL FROM t) SELECT x, y FROM t",
            "WITH RECURSIVE t AS (SELECT id FROM a UNION ALL SELECT id FROM t) SELECT * FROM t",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn projection_forms() {
        let queries = [
            "SELECT 1, 'x', true, NULL",
            "SELECT id, id AS again, label l FROM a",
            "SELECT DISTINCT label FROM a",
            "SELECT a.id, count(*) AS n FROM a GROUP BY a.id HAVING count(*) > 1 ORDER BY n DESC LIMIT 3",
            "SELECT id FROM a ORDER BY label OFFSET 1",
            "SELECT row_number() OVER (ORDER BY id) AS rn, sum(score) OVER () AS total FROM a",
            "SELECT (SELECT 1) AS one, (SELECT label FROM a LIMIT 1) AS lbl",
            "SELECT",
            "SELECT FROM a",
            "SELECT * FROM a FOR UPDATE",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn unsupported_query_forms_are_explained() {
        let queries = [
            "TABLE a",
            "SELECT id FROM a UNION TABLE b",
            "SELECT 1 UNION VALUES (2)",
        ];
        let postgres = reports(&queries, "postgres", Some(DDL));
        insta::assert_snapshot!(postgres);
    }

    #[test]
    fn statements_that_are_not_queries_have_no_columns() {
        let queries = [
            "CREATE TABLE z (a INT)",
            "DROP TABLE a",
            "BEGIN",
            "UPDATE a SET label = 'x'",
            "SELECT FROM",
            "",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(DDL)));
    }

    #[test]
    fn dialects_name_and_type_the_same_query_differently() {
        let sql = ["SELECT Id, COUNT(*) AS Total, id + 1 FROM t GROUP BY Id"];
        let ddl = "CREATE TABLE t (id INT NOT NULL)";
        let rendered: Vec<String> = ["postgres", "mysql", "sqlite", "clickhouse", "bigquery"]
            .iter()
            .map(|dialect| format!("{dialect}:\n{}", reports(&sql, dialect, Some(ddl))))
            .collect();
        insta::assert_snapshot!(rendered.join("\n"));
    }
}
