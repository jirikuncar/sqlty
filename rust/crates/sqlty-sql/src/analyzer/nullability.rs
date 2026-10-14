//! Nullability: whether a result column can be NULL.
//!
//! Nullability is a field next to a column's type (see [`Column::nullability`]), never part
//! of the type string. It is three-valued: a column is [`Nullability::Nullable`] only where
//! the schema or the query's shape proves NULL can occur. Columns of an unresolved source and
//! expressions the analyzer does not read are [`Nullability::Unknown`], which row types treat
//! as non-optional.

use super::*;

/// Whether a column can be NULL.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Nullability {
    /// NULL can occur.
    Nullable,
    /// NULL cannot occur.
    NonNull,
    /// Not known: treated as non-null when typing a row.
    #[default]
    Unknown,
}

impl Nullability {
    /// What a schema column's `NOT NULL` / `PRIMARY KEY` says.
    pub(super) fn declared(not_null: bool) -> Self {
        if not_null {
            Nullability::NonNull
        } else {
            Nullability::Nullable
        }
    }
}

/// What a SELECT's FROM and conditions tell about NULLs.
#[derive(Clone, Default)]
pub(super) struct Facts {
    /// References of relations an outer join null-extends: all their columns can be NULL.
    null_extended: Vec<String>,
    /// Whether the SELECT has a plain GROUP BY, so every group holds at least one row.
    pub(super) grouped: bool,
}

impl Facts {
    /// Whether the relation `reference` can be a row of NULLs.
    pub(super) fn is_null_extended(&self, reference: &str) -> bool {
        self.null_extended.iter().any(|r| r == reference)
    }

    /// Nullability of a column of `reference`, given what its source declares.
    pub(super) fn column(
        &self,
        reference: &str,
        _column: &str,
        declared: Nullability,
    ) -> Nullability {
        if self.is_null_extended(reference) {
            Nullability::Nullable
        } else {
            declared
        }
    }

    /// Forget what is known about relations a nested SELECT defines itself.
    pub(super) fn without(&mut self, scope: &Scope) {
        self.null_extended
            .retain(|r| !scope.iter().any(|(s, _)| s == r));
    }
}

/// References of the relations in `from` that outer joins null-extend, nested joins included.
pub(super) fn null_extended(from: &[TableWithJoins]) -> Vec<String> {
    let mut extended = Vec::new();
    for table in from {
        group(table, &mut extended);
    }
    extended
}

/// References of the relations of one `a JOIN b JOIN c` chain; null-extended ones go to `extended`.
fn group(table: &TableWithJoins, extended: &mut Vec<String>) -> Vec<String> {
    let mut left = factor_references(&table.relation, extended);
    for join in &table.joins {
        let right = factor_references(&join.relation, extended);
        match &join.join_operator {
            JoinOperator::Left(_) | JoinOperator::LeftOuter(_) | JoinOperator::OuterApply => {
                extended.extend(right.iter().cloned())
            }
            JoinOperator::Right(_) | JoinOperator::RightOuter(_) => {
                extended.extend(left.iter().cloned())
            }
            JoinOperator::FullOuter(_) => {
                extended.extend(left.iter().cloned());
                extended.extend(right.iter().cloned());
            }
            _ => {}
        }
        left.extend(right);
    }
    left
}

fn factor_references(factor: &TableFactor, extended: &mut Vec<String>) -> Vec<String> {
    match factor {
        TableFactor::NestedJoin {
            table_with_joins,
            alias: None,
        } => group(table_with_joins, extended),
        TableFactor::Table { name, alias, .. } => vec![alias
            .as_ref()
            .map_or_else(|| object_name(name), |a| a.name.value.to_lowercase())],
        other => vec![opaque_source(other).0],
    }
}

/// Strictest of several parts: NULL if any part can be NULL, else unknown if any is unknown.
fn any_nullable(parts: impl IntoIterator<Item = Nullability>) -> Nullability {
    let mut result = Nullability::NonNull;
    for part in parts {
        match part {
            Nullability::Nullable => return Nullability::Nullable,
            Nullability::Unknown => result = Nullability::Unknown,
            Nullability::NonNull => {}
        }
    }
    result
}

/// NULL only if every part can be NULL (`COALESCE`): one non-null part settles it.
fn all_nullable(parts: impl IntoIterator<Item = Nullability>) -> Nullability {
    let mut result = Nullability::Nullable;
    for part in parts {
        match part {
            Nullability::NonNull => return Nullability::NonNull,
            Nullability::Unknown => result = Nullability::Unknown,
            Nullability::Nullable => {}
        }
    }
    result
}

/// Aggregate functions, whose result over no rows is NULL (except `COUNT`).
const AGGREGATES: &[&str] = &[
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "STDDEV",
    "STDDEV_POP",
    "STDDEV_SAMP",
    "VARIANCE",
    "VAR_POP",
    "VAR_SAMP",
    "BOOL_AND",
    "BOOL_OR",
    "EVERY",
    "BIT_AND",
    "BIT_OR",
    "BIT_XOR",
    "ANY_VALUE",
    "STRING_AGG",
    "GROUP_CONCAT",
    "ARRAY_AGG",
    "JSON_AGG",
    "JSONB_AGG",
    "JSON_OBJECT_AGG",
    "JSONB_OBJECT_AGG",
    "JSON_ARRAYAGG",
    "JSON_OBJECTAGG",
    "XMLAGG",
    "PERCENTILE_CONT",
    "PERCENTILE_DISC",
    "MODE",
    "CORR",
    "COVAR_POP",
    "COVAR_SAMP",
    "TOTAL",
];

/// Aggregates whose group of one or more rows always gives a value, whatever the rows hold.
const ALWAYS_VALUED_AGGREGATES: &[&str] = &[
    "ARRAY_AGG",
    "JSON_AGG",
    "JSONB_AGG",
    "JSON_OBJECT_AGG",
    "JSONB_OBJECT_AGG",
    "JSON_ARRAYAGG",
    "JSON_OBJECTAGG",
    "XMLAGG",
    "TOTAL",
];

/// Functions that can return NULL for non-null arguments.
const ALWAYS_NULLABLE: &[&str] = &[
    "NULLIF",
    "REGEXP_MATCH",
    "REGEXP_SUBSTR",
    "REGEXP_INSTR",
    "ARRAY_LENGTH",
    "ARRAY_UPPER",
    "ARRAY_LOWER",
    "ARRAY_POSITION",
    "JSON_EXTRACT",
    "JSON_EXTRACT_PATH",
    "JSON_EXTRACT_PATH_TEXT",
    "JSONB_EXTRACT_PATH",
    "JSONB_EXTRACT_PATH_TEXT",
    "JSONB_PATH_QUERY_FIRST",
    "JSON_VALUE",
    "JSON_QUERY",
    "NTH_VALUE",
];

/// Window and other functions that never return NULL.
const NEVER_NULL: &[&str] = &[
    "ROW_NUMBER",
    "RANK",
    "DENSE_RANK",
    "NTILE",
    "PERCENT_RANK",
    "CUME_DIST",
    "NOW",
    "CURRENT_TIMESTAMP",
    "CURRENT_DATE",
    "CURRENT_TIME",
    "LOCALTIME",
    "LOCALTIMESTAMP",
    "RANDOM",
    "RAND",
    "GEN_RANDOM_UUID",
    "UUID",
    "UUID_GENERATE_V4",
    "JSON_BUILD_OBJECT",
    "JSONB_BUILD_OBJECT",
    "JSON_BUILD_ARRAY",
    "JSONB_BUILD_ARRAY",
    "JSON_OBJECT",
    "JSON_ARRAY",
    "ROW",
];

/// Whether `expr` holds an aggregate call outside any window or subquery.
fn contains_aggregate(expr: &Expr) -> bool {
    match expr {
        Expr::Function(func) => {
            let name = function_name(func);
            let args = positional_args(&func.args);
            let scalar_min_max = matches!(name.as_str(), "MIN" | "MAX") && args.len() >= 2;
            (func.over.is_none()
                && !scalar_min_max
                && (name == "COUNT" || AGGREGATES.contains(&name.as_str())))
                || args.into_iter().any(contains_aggregate)
        }
        Expr::Nested(inner)
        | Expr::UnaryOp { expr: inner, .. }
        | Expr::Cast { expr: inner, .. }
        | Expr::IsNull(inner)
        | Expr::IsNotNull(inner) => contains_aggregate(inner),
        Expr::BinaryOp { left, right, .. } => contains_aggregate(left) || contains_aggregate(right),
        Expr::Case {
            operand,
            conditions,
            else_result,
            ..
        } => {
            operand.as_deref().is_some_and(contains_aggregate)
                || conditions.iter().any(|when| {
                    contains_aggregate(&when.condition) || contains_aggregate(&when.result)
                })
                || else_result.as_deref().is_some_and(contains_aggregate)
        }
        _ => false,
    }
}

/// The upper-case name of a function, without its schema.
fn function_name(func: &sqlparser::ast::Function) -> String {
    func.name
        .0
        .last()
        .and_then(|part| part.as_ident())
        .map(|ident| ident.value.to_uppercase())
        .unwrap_or_default()
}

/// Whether the SELECT has a plain GROUP BY, so that every group holds at least one row.
/// `GROUP BY ()`, `ROLLUP`, `CUBE` and `GROUPING SETS` also make a group of all rows, which
/// may be empty.
fn is_grouped(select: &Select) -> bool {
    match &select.group_by {
        sqlparser::ast::GroupByExpr::Expressions(by, modifiers) if modifiers.is_empty() => {
            let aggregate_all = |expr: &Expr| {
                matches!(expr, Expr::Tuple(items) if items.is_empty())
                    || matches!(
                        expr,
                        Expr::Rollup(_) | Expr::Cube(_) | Expr::GroupingSets(_)
                    )
            };
            !by.is_empty() && !by.iter().any(aggregate_all)
        }
        _ => false,
    }
}

/// Whether the query always returns exactly one row: no FROM, or an aggregate without GROUP BY.
fn returns_one_row(query: &Query) -> bool {
    let SetExpr::Select(select) = query.body.as_ref() else {
        return false;
    };
    if select.having.is_some() || query.limit_clause.is_some() {
        return false;
    }
    let ungrouped = !is_grouped(select);
    let aggregated = select.projection.iter().any(|item| match item {
        SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
            contains_aggregate(expr)
        }
        _ => false,
    });
    ungrouped && (aggregated || (select.from.is_empty() && select.selection.is_none()))
}

impl<'a> Analyzer<'a> {
    /// The facts in force inside `select`: those of enclosing queries, minus the names it
    /// redefines, plus its own joins.
    pub(super) fn select_facts(&self, select: &Select, scope: &Scope) -> Facts {
        let mut facts = self.facts.clone();
        facts.without(scope);
        facts.null_extended.extend(null_extended(&select.from));
        facts.grouped = is_grouped(select);
        facts
    }

    /// Whether `expr` can be NULL under `facts`.
    pub(super) fn nullability(&self, expr: &Expr, scope: &Scope, facts: &Facts) -> Nullability {
        let of = |inner: &Expr| self.nullability(inner, scope, facts);
        match expr {
            Expr::Nested(inner) | Expr::Collate { expr: inner, .. } => of(inner),
            Expr::Cast {
                expr: inner, kind, ..
            } => match kind {
                sqlparser::ast::CastKind::TryCast | sqlparser::ast::CastKind::SafeCast => {
                    Nullability::Nullable
                }
                _ => of(inner),
            },
            Expr::Identifier(_) | Expr::CompoundIdentifier(_) => {
                match self.locate_column(expr, scope) {
                    Some((reference, table, column)) => facts.column(
                        &reference,
                        &column,
                        self.declared_nullability(&table, &column),
                    ),
                    None => Nullability::Unknown,
                }
            }
            Expr::Value(value) => match value.value {
                Value::Null => Nullability::Nullable,
                Value::Placeholder(_) => Nullability::Unknown,
                _ => Nullability::NonNull,
            },
            Expr::TypedString(_)
            | Expr::Interval(_)
            | Expr::Array(_)
            | Expr::Tuple(_)
            | Expr::Exists { .. }
            | Expr::IsNull(_)
            | Expr::IsNotNull(_)
            | Expr::IsTrue(_)
            | Expr::IsNotTrue(_)
            | Expr::IsFalse(_)
            | Expr::IsNotFalse(_)
            | Expr::IsUnknown(_)
            | Expr::IsNotUnknown(_)
            | Expr::IsDistinctFrom(..)
            | Expr::IsNotDistinctFrom(..) => Nullability::NonNull,
            Expr::UnaryOp { expr: inner, .. } => of(inner),
            Expr::BinaryOp { left, op, right } => {
                use BinaryOperator::*;
                match op {
                    // MySQL's null-safe equality.
                    Spaceship => Nullability::NonNull,
                    // JSON accessors yield NULL for a missing key or a JSON null.
                    Arrow | LongArrow | HashArrow | HashLongArrow => Nullability::Nullable,
                    _ => any_nullable([of(left), of(right)]),
                }
            }
            Expr::Between {
                expr: inner,
                low,
                high,
                ..
            } => any_nullable([of(inner), of(low), of(high)]),
            Expr::InList {
                expr: inner, list, ..
            } => any_nullable(std::iter::once(of(inner)).chain(list.iter().map(of))),
            Expr::InSubquery {
                expr: inner,
                subquery,
                ..
            } => any_nullable([of(inner), self.subquery_column(subquery, scope)]),
            Expr::InUnnest {
                expr: inner,
                array_expr,
                ..
            } => any_nullable([of(inner), of(array_expr)]),
            Expr::Like {
                expr: inner,
                pattern,
                ..
            }
            | Expr::ILike {
                expr: inner,
                pattern,
                ..
            }
            | Expr::SimilarTo {
                expr: inner,
                pattern,
                ..
            }
            | Expr::RLike {
                expr: inner,
                pattern,
                ..
            } => any_nullable([of(inner), of(pattern)]),
            Expr::AnyOp { left, right, .. } | Expr::AllOp { left, right, .. } => {
                any_nullable([of(left), of(right)])
            }
            Expr::Extract { expr: inner, .. }
            | Expr::Ceil { expr: inner, .. }
            | Expr::Floor { expr: inner, .. } => of(inner),
            Expr::Position { expr: inner, r#in } => any_nullable([of(inner), of(r#in)]),
            Expr::Substring {
                expr: inner,
                substring_from,
                substring_for,
                ..
            } => any_nullable(
                std::iter::once(of(inner))
                    .chain(substring_from.as_deref().map(of))
                    .chain(substring_for.as_deref().map(of)),
            ),
            Expr::Trim {
                expr: inner,
                trim_what,
                trim_characters,
                ..
            } => any_nullable(
                std::iter::once(of(inner))
                    .chain(trim_what.as_deref().map(of))
                    .chain(trim_characters.iter().flatten().map(of)),
            ),
            Expr::Subquery(query) => {
                if returns_one_row(query) {
                    self.subquery_column(query, scope)
                } else {
                    // No row at all reads as NULL.
                    Nullability::Nullable
                }
            }
            Expr::Case {
                operand,
                conditions,
                else_result,
                ..
            } => self.case_nullability(
                operand.as_deref(),
                conditions,
                else_result.as_deref(),
                scope,
                facts,
            ),
            Expr::CompoundFieldAccess { root, access_chain } => {
                let (mut result, chain) = match (root.as_ref(), access_chain.as_slice()) {
                    (
                        Expr::Identifier(table),
                        [AccessExpr::Dot(Expr::Identifier(column)), rest @ ..],
                    ) => {
                        let column = Expr::CompoundIdentifier(vec![table.clone(), column.clone()]);
                        (of(&column), rest)
                    }
                    _ => (of(root), access_chain.as_slice()),
                };
                for access in chain {
                    result = match access {
                        // An index past the end reads as NULL.
                        AccessExpr::Subscript(Subscript::Index { .. }) => Nullability::Nullable,
                        AccessExpr::Subscript(Subscript::Slice { .. }) => result,
                        // A field of a row can hold NULL; a NULL row has only NULL fields.
                        AccessExpr::Dot(_) if result == Nullability::Nullable => result,
                        AccessExpr::Dot(_) => Nullability::Unknown,
                    };
                }
                result
            }
            Expr::Function(func) => self.function_nullability(func, scope, facts),
            _ => Nullability::Unknown,
        }
    }

    /// Nullability of the first column of a subquery, as its own query reads it.
    fn subquery_column(&self, query: &Query, scope: &Scope) -> Nullability {
        let mut inner = self.correlated(scope);
        inner.diag = Diagnostics::default();
        match inner.query_columns(query) {
            Ok(Some(columns)) => columns
                .first()
                .map_or(Nullability::Unknown, |col| col.nullability),
            _ => Nullability::Unknown,
        }
    }

    fn case_nullability(
        &self,
        operand: Option<&Expr>,
        conditions: &[sqlparser::ast::CaseWhen],
        else_result: Option<&Expr>,
        scope: &Scope,
        facts: &Facts,
    ) -> Nullability {
        let _ = operand;
        let mut results: Vec<Nullability> = conditions
            .iter()
            .map(|when| self.nullability(&when.result, scope, facts))
            .collect();
        // Without ELSE, a row no WHEN matches yields NULL.
        results.push(else_result.map_or(Nullability::Nullable, |expr| {
            self.nullability(expr, scope, facts)
        }));
        any_nullable(results)
    }

    fn function_nullability(
        &self,
        func: &sqlparser::ast::Function,
        scope: &Scope,
        facts: &Facts,
    ) -> Nullability {
        let name = function_name(func);
        let args = positional_args(&func.args);
        let all_positional = match &func.args {
            FunctionArguments::List(list) => list.args.len() == args.len(),
            _ => true,
        };
        let of = |inner: &Expr| self.nullability(inner, scope, facts);
        let first = || args.first().map_or(Nullability::Unknown, |arg| of(arg));
        let postgres = self.dialect == SqlDialect::Postgres;

        if name == "COUNT" {
            return Nullability::NonNull;
        }
        // SQLite's two-argument MIN and MAX are scalar functions.
        if matches!(name.as_str(), "MIN" | "MAX") && args.len() >= 2 {
            return any_nullable(args.iter().map(|arg| of(arg)));
        }
        if AGGREGATES.contains(&name.as_str()) {
            // A window function over its frame sees the current row, unless the frame is
            // spelled out.
            let framed = match &func.over {
                Some(sqlparser::ast::WindowType::WindowSpec(spec)) => spec.window_frame.is_some(),
                Some(_) => true,
                None => false,
            };
            let empty_possible = if func.over.is_some() {
                framed
            } else {
                !facts.grouped
            } || func.filter.is_some();
            let arg = if ALWAYS_VALUED_AGGREGATES.contains(&name.as_str()) {
                Nullability::NonNull
            } else {
                first()
            };
            return if empty_possible {
                Nullability::Nullable
            } else {
                arg
            };
        }
        if ALWAYS_NULLABLE.contains(&name.as_str()) {
            return Nullability::Nullable;
        }
        match name.as_str() {
            "COALESCE" | "IFNULL" | "NVL" => all_nullable(args.iter().map(|arg| of(arg))),
            "GREATEST" | "LEAST" if postgres => all_nullable(args.iter().map(|arg| of(arg))),
            "GREATEST" | "LEAST" => any_nullable(args.iter().map(|arg| of(arg))),
            "IF" | "IIF" if args.len() == 3 => any_nullable([of(args[1]), of(args[2])]),
            "LAG" | "LEAD" => match args.as_slice() {
                [value, _, default] => any_nullable([of(value), of(default)]),
                _ => Nullability::Nullable,
            },
            "FIRST_VALUE" | "LAST_VALUE" => first(),
            // PostgreSQL's and SQLite's concat skip NULLs.
            "CONCAT" if self.dialect != SqlDialect::MySql => Nullability::NonNull,
            "CONCAT" => any_nullable(args.iter().map(|arg| of(arg))),
            // Only a NULL separator gives NULL.
            "CONCAT_WS" => first(),
            name if NEVER_NULL.contains(&name) => Nullability::NonNull,
            name if self.returns(name).is_some() && all_positional => {
                any_nullable(args.iter().map(|arg| of(arg)))
            }
            // A user-defined function may or may not be strict.
            _ => Nullability::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCHEMA: &str =
        "CREATE TABLE t (id INTEGER PRIMARY KEY, a INTEGER, b TEXT NOT NULL, c TEXT);
        CREATE TABLE u (id INTEGER PRIMARY KEY, t_id INTEGER NOT NULL, n INTEGER);";

    /// One mark per result column: `N` nullable, `-` non-null, `?` unknown.
    fn marks(sql: &str) -> String {
        marks_in(sql, "postgres")
    }

    fn marks_in(sql: &str, dialect: &str) -> String {
        marks_with(sql, dialect, SCHEMA)
    }

    fn marks_with(sql: &str, dialect: &str, schema: &str) -> String {
        let schema = Some(load_schema(schema, dialect));
        analyze_select(sql, dialect, &schema)
            .unwrap()
            .unwrap()
            .iter()
            .map(|col| match col.nullability {
                Nullability::Nullable => 'N',
                Nullability::NonNull => '-',
                Nullability::Unknown => '?',
            })
            .collect()
    }

    #[test]
    fn test_columns_follow_the_schema() {
        assert_eq!(marks("SELECT id, a, b, c FROM t"), "-N-N");
        assert_eq!(marks("SELECT * FROM t"), "-N-N");
        assert_eq!(marks("SELECT t.id, x.c FROM t, t AS x"), "-N");
        assert_eq!(marks("SELECT nope FROM t"), "?");
        assert_eq!(marks("SELECT id FROM missing"), "?");
    }

    #[test]
    fn test_outer_joins_extend_relations() {
        let join = |kind: &str| {
            marks(&format!(
                "SELECT t.id, u.id FROM t {kind} JOIN u ON u.t_id = t.id"
            ))
        };
        assert_eq!(join("INNER"), "--");
        assert_eq!(join("LEFT"), "-N");
        assert_eq!(join("RIGHT"), "N-");
        assert_eq!(join("FULL"), "NN");
        assert_eq!(
            marks("SELECT t.id, u.id, v.id FROM t LEFT JOIN u ON u.t_id = t.id LEFT JOIN t v ON v.id = u.n"),
            "-NN"
        );
        assert_eq!(
            marks("SELECT t.id, u.id, v.id FROM t LEFT JOIN (u JOIN t v ON v.id = u.n) ON u.t_id = t.id"),
            "-NN"
        );
        assert_eq!(
            marks("SELECT * FROM t LEFT JOIN u ON u.t_id = t.id"),
            "-N-NNNN"
        );
    }

    #[test]
    fn test_literals_casts_and_operators() {
        assert_eq!(marks("SELECT 1, 'x', true, NULL"), "---N");
        assert_eq!(
            marks("SELECT a::BIGINT, CAST(b AS TEXT), (a) FROM t"),
            "N-N"
        );
        assert_eq!(
            marks("SELECT a + 1, b || 'x', -a, NOT (a > 1) FROM t"),
            "N-NN"
        );
        assert_eq!(
            marks("SELECT a IS NULL, a IS NOT NULL, a IS DISTINCT FROM 1 FROM t"),
            "---"
        );
        assert_eq!(
            marks("SELECT a IN (1, 2), id IN (1, NULL), b LIKE 'x%' FROM t"),
            "NN-"
        );
        assert_eq!(marks("SELECT $1, nosuch(1) FROM t"), "??");
        assert_eq!(marks("SELECT EXISTS (SELECT 1)"), "-");
    }

    #[test]
    fn test_functions() {
        assert_eq!(
            marks("SELECT coalesce(a, 0), coalesce(a, c::INT), coalesce(a, id) FROM t"),
            "-N-"
        );
        assert_eq!(
            marks("SELECT nullif(id, 0), lower(c), lower(b), length(b) FROM t"),
            "NN--"
        );
        assert_eq!(marks("SELECT now(), random(), lower('x') FROM t"), "---");
        assert_eq!(marks("SELECT concat(c, b), concat_ws(',', c) FROM t"), "--");
        assert_eq!(
            marks_in("SELECT concat(c, b), concat_ws(',', c) FROM t", "mysql"),
            "N-"
        );
        assert_eq!(
            marks("SELECT greatest(a, 1), least(a, c::INT) FROM t"),
            "-N"
        );
        assert_eq!(
            marks_in("SELECT greatest(a, 1), ifnull(a, 1) FROM t", "mysql"),
            "N-"
        );
        assert_eq!(
            marks_in("SELECT if(a > 1, b, c), ifnull(c, 'x') FROM t", "mysql"),
            "N-"
        );
        assert_eq!(
            marks_in(
                "SELECT iif(a > 1, b, 'x'), coalesce(c, 'x') FROM t",
                "sqlite"
            ),
            "--"
        );
        assert_eq!(marks("SELECT user_defined(1) FROM t"), "?");
    }

    #[test]
    fn test_json_and_array_accessors_can_be_null() {
        let schema = "CREATE TABLE d (doc JSONB NOT NULL, tags TEXT[] NOT NULL);";
        let run = |sql: &str| marks_with(sql, "postgres", schema);
        assert_eq!(
            run("SELECT doc -> 'k', doc ->> 'k', doc #>> '{a,b}' FROM d"),
            "NNN"
        );
        assert_eq!(
            run("SELECT tags[1], array_length(tags, 1), regexp_match('a', 'b') FROM d"),
            "NNN"
        );
        assert_eq!(run("SELECT doc, tags FROM d"), "--");
    }

    #[test]
    fn test_aggregates() {
        assert_eq!(
            marks("SELECT count(*), count(a), max(id), sum(b::INT), min(a) FROM t"),
            "--NNN"
        );
        assert_eq!(
            marks("SELECT count(*), max(id), avg(id), array_agg(a) FROM t"),
            "-NNN"
        );
        // Every group has a row, so only a nullable argument yields NULL.
        assert_eq!(
            marks("SELECT b, count(*), max(id), max(a), array_agg(a) FROM t GROUP BY b"),
            "---N-"
        );
        // A FILTER can leave a group empty.
        assert_eq!(
            marks("SELECT b, max(id) FILTER (WHERE a > 1) FROM t GROUP BY b"),
            "-N"
        );
        // GROUP BY () is a single group over possibly no rows.
        assert_eq!(marks("SELECT max(id) FROM t GROUP BY ()"), "N");
        assert_eq!(marks("SELECT max(id) FROM t GROUP BY ROLLUP (b)"), "N");
    }

    #[test]
    fn test_window_functions() {
        assert_eq!(
            marks("SELECT row_number() OVER (ORDER BY id), rank() OVER (ORDER BY id), count(a) OVER () FROM t"),
            "---"
        );
        assert_eq!(
            marks("SELECT sum(id) OVER (ORDER BY id), sum(a) OVER (), max(id) OVER (ORDER BY id ROWS BETWEEN 2 PRECEDING AND 1 PRECEDING) FROM t"),
            "-NN"
        );
        assert_eq!(
            marks("SELECT lag(id) OVER (ORDER BY id), lag(id, 1, 0) OVER (ORDER BY id), lead(a, 1, 0) OVER (ORDER BY id), first_value(id) OVER (ORDER BY id) FROM t"),
            "N-N-"
        );
    }

    #[test]
    fn test_case_without_else_can_be_null() {
        assert_eq!(
            marks("SELECT CASE WHEN a > 1 THEN b END, CASE WHEN a > 1 THEN b ELSE 'x' END, CASE WHEN a > 1 THEN c ELSE 'x' END FROM t"),
            "N-N"
        );
        assert_eq!(
            marks("SELECT CASE a WHEN 1 THEN 'x' ELSE NULL END FROM t"),
            "N"
        );
    }

    #[test]
    fn test_scalar_subqueries() {
        // Zero rows read as NULL.
        assert_eq!(
            marks("SELECT (SELECT b FROM u WHERE u.id = t.id) FROM t"),
            "N"
        );
        assert_eq!(marks("SELECT (SELECT id FROM t LIMIT 1) FROM t"), "N");
        // One row, whose column decides.
        assert_eq!(
            marks("SELECT (SELECT 1), (SELECT NULL), (SELECT count(*) FROM u) FROM t"),
            "-N-"
        );
        assert_eq!(
            marks("SELECT (SELECT max(id) FROM u), (SELECT count(*) + 1 FROM u) FROM t"),
            "N-"
        );
        assert_eq!(
            marks("SELECT (SELECT sum(id) FROM u WHERE u.t_id = t.id) FROM t"),
            "N"
        );
        assert_eq!(
            marks("SELECT id IN (SELECT t_id FROM u), id IN (SELECT n FROM u) FROM t"),
            "-N"
        );
    }
}
