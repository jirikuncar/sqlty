//! What the new statements of a PostgreSQL migration lock, as `sqlty breaking` reports it.
//!
//! A statement is new when the baseline schema does not have it, compared without layout and
//! comments. Each new statement is judged against the tables the baseline and the earlier
//! statements define: a table created by a new statement is empty, so nothing it does to it
//! blocks anyone. Statements sqlparser does not read (`DROP INDEX CONCURRENTLY`, `REINDEX`,
//! `CLUSTER`, `DETACH PARTITION`) are judged by their tokens.
//!
//! Lock levels and rewrites follow the PostgreSQL 17 documentation of
//! [`ALTER TABLE`](https://www.postgresql.org/docs/17/sql-altertable.html),
//! [`CREATE INDEX`](https://www.postgresql.org/docs/17/sql-createindex.html) and
//! [explicit locking](https://www.postgresql.org/docs/17/explicit-locking.html).

use std::ops::{ControlFlow, Range};

use rustc_hash::{FxHashMap, FxHashSet};
use sqlparser::ast::{
    AlterColumnOperation, AlterTableOperation, ColumnDef, ColumnOption, CopySource, DataType,
    DetachPartitionMode, Expr, Insert, ObjectName, ObjectType, SetExpr, Statement, TableConstraint,
    TableObject,
};

use crate::analyzer::{parse_ddl, SqlDialect};
use crate::catalog::fold;
use crate::ddl::{self, Statement as Raw};

pub use sqlty_rules::Rule;

/// One thing a new statement of a migration locks or breaks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    /// The rule that fired.
    pub rule: Rule,
    /// What the statement locks or breaks, and the safe way to do it.
    pub message: String,
    /// The bytes of the statement in its file.
    pub span: Range<usize>,
}

/// Functions PostgreSQL marks `VOLATILE` (`provolatile = 'v'` in `pg_proc`) that column
/// defaults call, with those of `pgcrypto` and `uuid-ossp`: a default calling one is
/// evaluated for every existing row.
const VOLATILE: [&str; 12] = [
    "clock_timestamp",
    "gen_random_bytes",
    "gen_random_uuid",
    "gen_salt",
    "nextval",
    "random",
    "random_normal",
    "setseed",
    "timeofday",
    "uuid_generate_v1",
    "uuid_generate_v1mc",
    "uuid_generate_v4",
];

const SERIAL: [&str; 6] = [
    "smallserial",
    "serial2",
    "serial",
    "serial4",
    "bigserial",
    "serial8",
];

/// The findings of each of `files`, the migrations of the new schema in the order they run,
/// for the statements `baseline` does not have.
pub fn check(baseline: &str, files: &[&str]) -> Vec<Vec<Finding>> {
    let mut model = Model::default();
    for stmt in ddl::statements(baseline, false) {
        *model.applied.entry(stmt.normalized()).or_default() += 1;
        if let Some((parsed, likes)) = model.parse_if_shaping(&stmt) {
            model.apply(&parsed, &likes);
        }
    }
    files.iter().map(|sql| model.file(sql)).collect()
}

#[derive(Default)]
struct Model {
    /// The statements of the baseline, by their normalized text.
    applied: FxHashMap<String, usize>,
    /// Tables, materialized views and indexes created by new statements: empty and unused.
    created: FxHashSet<String>,
    /// The tables the baseline and earlier statements create.
    tables: FxHashSet<String>,
    /// The declared type of each column.
    types: FxHashMap<(String, String), DataType>,
    /// The `CHECK (column IS NOT NULL)` constraints of each table: name, column, validated.
    not_null: FxHashMap<String, Vec<(Option<String>, String, bool)>>,
}

/// What the statements of one file so far have set.
#[derive(Default)]
struct Session {
    transaction: bool,
    lock_timeout: bool,
    /// Set by `SET LOCAL`, until the transaction ends.
    local_timeout: bool,
    reported_timeout: bool,
}

impl Model {
    fn file(&mut self, sql: &str) -> Vec<Finding> {
        let mut found = Vec::new();
        if is_dump(sql) {
            return found;
        }
        let mut session = Session::default();
        let spanned = ddl::statements(sql, false);
        for (span, stmt) in spanned.filter_map(|s| Some((s.range_in(sql)?, s))) {
            let tokens: Vec<String> = stmt.tokens().iter().map(|t| t.to_lowercase()).collect();
            if session.track(&tokens) {
                continue;
            }
            let new = match self.applied.get_mut(&stmt.normalized()) {
                Some(count) if *count > 0 => {
                    *count -= 1;
                    false
                }
                _ => true,
            };
            let (parsed, likes) = parse(&stmt).unzip();
            if new {
                let mut report = |rule: Rule, message: String| {
                    found.push(Finding {
                        rule,
                        message,
                        span: span.clone(),
                    });
                };
                let judged = match &parsed {
                    Some(parsed) => self.judge(parsed, &mut report),
                    None => self.judge_tokens(&stmt, &tokens, &mut report),
                };
                if judged.concurrently && session.transaction {
                    report(
                        Rule::ConcurrentlyInTransaction,
                        "PostgreSQL refuses `CONCURRENTLY` inside a transaction block: run it in a migration without one".to_string(),
                    );
                }
                if judged.strong_lock && !session.has_timeout() && !session.reported_timeout {
                    session.reported_timeout = true;
                    report(
                        Rule::MissingLockTimeout,
                        "takes a lock that blocks writes without `SET lock_timeout` first: while it waits for the lock, the queries it blocks wait behind it".to_string(),
                    );
                }
                let created = parsed.as_ref().and_then(created);
                if let Some(created) = created.filter(|c| !self.tables.contains(c)) {
                    self.created.insert(created);
                }
            }
            if let Some(filled) = parsed.as_ref().and_then(filled) {
                self.created.remove(&filled);
            }
            if let Some(parsed) = &parsed {
                self.apply(parsed, &likes.unwrap_or_default());
            }
        }
        found
    }

    /// The statement parsed, when it can change a column type or a `NOT NULL` check.
    fn parse_if_shaping(&self, stmt: &Raw) -> Option<(Statement, Vec<String>)> {
        let tokens = stmt.tokens();
        let shaping = match tokens.as_slice() {
            [create, ..] if create.eq_ignore_ascii_case("create") => stmt.is_create_table(),
            [alter, table, ..] => {
                alter.eq_ignore_ascii_case("alter") && table.eq_ignore_ascii_case("table")
            }
            _ => false,
        };
        if !shaping {
            return None;
        }
        parse(stmt)
    }

    /// Record the columns and checks `stmt` defines, with those of the tables it copies with
    /// `LIKE`.
    fn apply(&mut self, stmt: &Statement, likes: &[String]) {
        match stmt {
            Statement::CreateTable(create) => {
                let table = key(&create.name);
                self.tables.insert(table.clone());
                let copied: Vec<_> = self
                    .types
                    .iter()
                    .filter(|((source, _), _)| likes.contains(source))
                    .map(|((_, column), ty)| ((table.clone(), column.clone()), ty.clone()))
                    .collect();
                self.types.extend(copied);
                for column in &create.columns {
                    self.types.insert(
                        (table.clone(), name(&column.name)),
                        column.data_type.clone(),
                    );
                }
            }
            Statement::AlterTable(alter) => {
                let table = key(&alter.name);
                for op in &alter.operations {
                    self.apply_op(&table, op);
                }
            }
            _ => {}
        }
    }

    fn apply_op(&mut self, table: &str, op: &AlterTableOperation) {
        match op {
            AlterTableOperation::AddColumn { column_def, .. } => {
                self.types.insert(
                    (table.to_string(), name(&column_def.name)),
                    column_def.data_type.clone(),
                );
            }
            AlterTableOperation::AlterColumn {
                column_name,
                op: AlterColumnOperation::SetDataType { data_type, .. },
            } => {
                self.types
                    .insert((table.to_string(), name(column_name)), data_type.clone());
            }
            AlterTableOperation::AddConstraint {
                constraint: TableConstraint::Check(check),
                not_valid,
            } => {
                if let Some(column) = is_not_null(&check.expr) {
                    self.not_null.entry(table.to_string()).or_default().push((
                        check.name.as_ref().map(name),
                        column,
                        !not_valid,
                    ));
                }
            }
            AlterTableOperation::ValidateConstraint { name: validated } => {
                let validated = name(validated);
                for check in self.not_null.entry(table.to_string()).or_default() {
                    if check.0.as_ref() == Some(&validated) {
                        check.2 = true;
                    }
                }
            }
            _ => {}
        }
    }

    fn is_new(&self, name: &ObjectName) -> bool {
        self.created.contains(&key(name))
    }

    fn judge(&self, stmt: &Statement, report: &mut impl FnMut(Rule, String)) -> Judged {
        match stmt {
            Statement::CreateIndex(index) => {
                if !index.concurrently && !self.is_new(&index.table_name) {
                    report(
                        Rule::MissingConcurrently,
                        format!(
                            "`CREATE INDEX` blocks writes to `{}` until the index is built: create it `CONCURRENTLY`",
                            index.table_name
                        ),
                    );
                }
                Judged::concurrently(index.concurrently)
            }
            Statement::Drop {
                object_type: ObjectType::Index,
                names,
                ..
            } => {
                let created = names.iter().all(|name| self.is_new(name));
                if !created {
                    report(Rule::MissingConcurrently, drop_index_message());
                }
                Judged::strong_unless(created)
            }
            Statement::Drop {
                object_type: ObjectType::Table,
                names,
                ..
            } => Judged::strong_unless(names.iter().all(|name| self.is_new(name))),
            Statement::RefreshMaterializedView(refresh) => {
                if !refresh.concurrently && !self.is_new(&refresh.name) {
                    report(
                        Rule::MissingConcurrently,
                        format!(
                            "`REFRESH MATERIALIZED VIEW` blocks reads of `{}` until it finishes: refresh it `CONCURRENTLY`, which needs a unique index on the view",
                            refresh.name
                        ),
                    );
                }
                Judged::concurrently(refresh.concurrently)
            }
            Statement::Vacuum(vacuum) if vacuum.full => {
                if !vacuum.table_name.as_ref().is_some_and(|t| self.is_new(t)) {
                    report(Rule::TableRewrite, rewrite_command_message("VACUUM FULL"));
                }
                Judged::default()
            }
            Statement::Lock(_) => Judged::strong(),
            Statement::AlterTable(alter) => {
                if self.is_new(&alter.name) {
                    return Judged::default();
                }
                let table = key(&alter.name);
                for op in &alter.operations {
                    self.judge_op(&table, &alter.name, op, report);
                }
                let validates = alter
                    .operations
                    .iter()
                    .all(|op| matches!(op, AlterTableOperation::ValidateConstraint { .. }));
                let concurrently = alter.operations.iter().any(|op| {
                    matches!(
                        op,
                        AlterTableOperation::DetachPartitionOf {
                            mode: Some(DetachPartitionMode::Concurrently),
                            ..
                        }
                    )
                });
                Judged {
                    concurrently,
                    strong_lock: !validates && !concurrently,
                }
            }
            _ => Judged::default(),
        }
    }

    fn judge_op(
        &self,
        table: &str,
        written: &ObjectName,
        op: &AlterTableOperation,
        report: &mut impl FnMut(Rule, String),
    ) {
        match op {
            AlterTableOperation::AddColumn { column_def, .. } => {
                add_column(written, column_def, report);
            }
            AlterTableOperation::AlterColumn {
                column_name,
                op: AlterColumnOperation::SetNotNull,
            } => {
                let column = name(column_name);
                let checked = self.not_null.get(table).is_some_and(|checks| {
                    checks
                        .iter()
                        .any(|(_, checked, valid)| *valid && *checked == column)
                });
                if !checked {
                    report(
                        Rule::ConstraintScan,
                        format!(
                            "`SET NOT NULL` scans `{written}` for NULLs under an ACCESS EXCLUSIVE lock: add `CHECK ({column_name} IS NOT NULL) NOT VALID` and `VALIDATE` it first"
                        ),
                    );
                }
            }
            AlterTableOperation::AlterColumn {
                column_name,
                op: AlterColumnOperation::SetDataType { data_type, .. },
            } => {
                let old = self.types.get(&(table.to_string(), name(column_name)));
                if !old.is_some_and(|old| keeps_storage(old, data_type)) {
                    report(
                        Rule::TableRewrite,
                        format!(
                            "changing `{column_name}` to {data_type} rewrites `{written}` and its indexes under an ACCESS EXCLUSIVE lock: add a new column, backfill it and switch over"
                        ),
                    );
                }
            }
            AlterTableOperation::DetachPartitionOf { mode, .. } => {
                if *mode != Some(DetachPartitionMode::Concurrently) {
                    report(
                        Rule::MissingConcurrently,
                        format!(
                            "`DETACH PARTITION` takes an ACCESS EXCLUSIVE lock on `{written}`: detach it `CONCURRENTLY`"
                        ),
                    );
                }
            }
            AlterTableOperation::AddConstraint {
                constraint,
                not_valid,
            } => match constraint {
                TableConstraint::ForeignKey(_) | TableConstraint::Check(_) if !not_valid => {
                    report(
                        Rule::ConstraintScan,
                        format!(
                            "adding the constraint checks every row of `{written}` while it blocks writes: add it `NOT VALID`, then `VALIDATE CONSTRAINT` it"
                        ),
                    );
                }
                TableConstraint::Unique(_) | TableConstraint::PrimaryKey(_) => {
                    report(Rule::MissingConcurrently, unique_message(written));
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// The statements sqlparser does not read, by their tokens.
    fn judge_tokens(
        &self,
        stmt: &Raw,
        tokens: &[String],
        report: &mut impl FnMut(Rule, String),
    ) -> Judged {
        let concurrently = stmt.has_word("concurrently");
        let words: Vec<&str> = tokens.iter().map(String::as_str).collect();
        match words.as_slice() {
            ["drop", "index", ..] if concurrently => Judged::concurrently(true),
            ["reindex", ..] => {
                if !concurrently {
                    report(
                        Rule::MissingConcurrently,
                        "`REINDEX` blocks writes to the table, and reads that use the index, until it finishes: reindex `CONCURRENTLY`".to_string(),
                    );
                }
                Judged::concurrently(concurrently)
            }
            ["cluster", ..] => {
                let table: String = stmt.tokens()[1..]
                    .iter()
                    .skip_while(|t| t.eq_ignore_ascii_case("verbose"))
                    .take_while(|t| !t.eq_ignore_ascii_case("using"))
                    .map(|t| match t.strip_prefix('"') {
                        Some(quoted) => quoted.trim_end_matches('"').to_string(),
                        None => t.to_lowercase(),
                    })
                    .collect();
                let table = table.strip_prefix("public.").unwrap_or(&table);
                if !self.created.contains(table) {
                    report(Rule::TableRewrite, rewrite_command_message("CLUSTER"));
                }
                Judged::default()
            }
            _ => Judged::default(),
        }
    }
}

impl Session {
    fn has_timeout(&self) -> bool {
        self.lock_timeout || self.local_timeout
    }

    /// Follow a transaction or `lock_timeout` statement; whether `tokens` was one.
    fn track(&mut self, tokens: &[String]) -> bool {
        let words: Vec<&str> = tokens.iter().map(String::as_str).collect();
        match words.as_slice() {
            ["begin", ..] | ["start", "transaction", ..] => self.transaction = true,
            ["commit" | "end" | "rollback" | "abort", ..] => {
                (self.transaction, self.local_timeout) = (false, false);
            }
            ["reset", "lock_timeout" | "all"] => {
                (self.lock_timeout, self.local_timeout) = (false, false);
            }
            ["set", "local", "lock_timeout", _, value] => self.local_timeout = !is_zero(value),
            ["set", "session", "lock_timeout", _, value] | ["set", "lock_timeout", _, value] => {
                self.lock_timeout = !is_zero(value);
            }
            _ => return false,
        }
        true
    }
}

/// What a statement does to the session: whether it runs `CONCURRENTLY`, and whether it
/// takes an ACCESS EXCLUSIVE lock on a table that has rows.
#[derive(Default)]
struct Judged {
    concurrently: bool,
    strong_lock: bool,
}

impl Judged {
    fn concurrently(concurrently: bool) -> Self {
        Judged {
            concurrently,
            strong_lock: false,
        }
    }

    fn strong() -> Self {
        Judged::strong_unless(false)
    }

    fn strong_unless(exempt: bool) -> Self {
        Judged {
            concurrently: false,
            strong_lock: !exempt,
        }
    }
}

fn add_column(table: &ObjectName, column: &ColumnDef, report: &mut impl FnMut(Rule, String)) {
    let ty = column.data_type.to_string().to_lowercase();
    let mut default = None;
    let (mut not_null, mut generated) = (false, false);
    let mut why = SERIAL
        .contains(&ty.as_str())
        .then(|| format!("{ty} fills every existing row from a sequence"));
    for option in &column.options {
        match &option.option {
            ColumnOption::NotNull => not_null = true,
            ColumnOption::Default(expr) => default = Some(expr),
            ColumnOption::PrimaryKey(_) | ColumnOption::Unique(_) => {
                not_null |= matches!(option.option, ColumnOption::PrimaryKey(_));
                report(Rule::MissingConcurrently, unique_message(table));
            }
            ColumnOption::Generated {
                generation_expr, ..
            } => {
                generated = true;
                // sqlparser reads PostgreSQL's `VIRTUAL` as an error, so an expression is `STORED`.
                why = why.or_else(|| {
                    Some(
                        match generation_expr {
                            None => "an identity column fills every existing row",
                            Some(_) => {
                                "a stored generated column is computed for every existing row"
                            }
                        }
                        .to_string(),
                    )
                });
            }
            _ => {}
        }
    }
    let volatile = default.and_then(volatile_call);
    why = why.or_else(|| {
        volatile.map(|f| {
            format!("its default calls {f}(), which is volatile, so each row gets its own value")
        })
    });
    if let Some(why) = why {
        report(
            Rule::TableRewrite,
            format!(
                "adding `{}` rewrites `{table}` under an ACCESS EXCLUSIVE lock: {why}; add it without the default, backfill in batches, then set the default",
                column.name
            ),
        );
    } else if not_null && default.is_none() && !generated {
        report(
            Rule::AddRequiredColumn,
            format!(
                "adding `{}` as NOT NULL without a DEFAULT fails when `{table}` has rows: add it nullable or with a default",
                column.name
            ),
        );
    }
}

/// The first volatile function `expr` calls.
fn volatile_call(expr: &Expr) -> Option<&'static str> {
    let found = sqlparser::ast::visit_expressions(expr, |e| {
        if let Expr::Function(f) = e {
            let called = f.name.0.last().and_then(|p| p.as_ident()).map(name);
            if let Some(hit) = VOLATILE.iter().find(|v| called.as_deref() == Some(**v)) {
                return ControlFlow::Break(*hit);
            }
        }
        ControlFlow::Continue(())
    });
    match found {
        ControlFlow::Break(hit) => Some(hit),
        ControlFlow::Continue(()) => None,
    }
}

/// Whether PostgreSQL changes the type from `old` to `new` without a rewrite: binary
/// coercible types with no narrower constraint, per the notes of `ALTER TABLE`.
fn keeps_storage(old: &DataType, new: &DataType) -> bool {
    if old == new {
        return true;
    }
    match (Storage::of(old), Storage::of(new)) {
        (Some(Storage::Varchar(old)), Some(Storage::Varchar(new))) => match (old, new) {
            (_, None) => true,
            (Some(old), Some(new)) => new >= old,
            (None, Some(_)) => false,
        },
        (Some(Storage::Varbit(old)), Some(Storage::Varbit(new))) => match (old, new) {
            (_, None) => true,
            (Some(old), Some(new)) => new >= old,
            (None, Some(_)) => false,
        },
        (Some(Storage::Numeric(old)), Some(Storage::Numeric(new))) => match (old, new) {
            (_, None) => true,
            (Some((p, s)), Some((q, t))) => s == t && q >= p,
            (None, Some(_)) => false,
        },
        (Some(Storage::Cidr), Some(Storage::Inet)) => true,
        _ => false,
    }
}

/// The types `keeps_storage` compares; `text` is a `varchar` without a length.
enum Storage {
    Varchar(Option<u64>),
    Varbit(Option<u64>),
    Numeric(Option<(u64, u64)>),
    Cidr,
    Inet,
}

impl Storage {
    fn of(ty: &DataType) -> Option<Storage> {
        let text = ty.to_string().to_lowercase();
        let (base, args) = match text.split_once('(') {
            Some((base, args)) => (base.trim(), Some(args.trim_end_matches(')'))),
            None => (text.trim(), None),
        };
        let numbers: Option<Vec<u64>> = match args {
            Some(args) => Some(
                args.split(',')
                    .map(|n| n.trim().parse().ok())
                    .collect::<Option<_>>()?,
            ),
            None => None,
        };
        Some(match (base, numbers.as_deref()) {
            ("text", None) => Storage::Varchar(None),
            ("varchar" | "character varying", None) => Storage::Varchar(None),
            ("varchar" | "character varying", Some([n])) => Storage::Varchar(Some(*n)),
            ("varbit" | "bit varying", None) => Storage::Varbit(None),
            ("varbit" | "bit varying", Some([n])) => Storage::Varbit(Some(*n)),
            ("numeric" | "decimal", None) => Storage::Numeric(None),
            ("numeric" | "decimal", Some([p])) => Storage::Numeric(Some((*p, 0))),
            ("numeric" | "decimal", Some([p, s])) => Storage::Numeric(Some((*p, *s))),
            ("cidr", None) => Storage::Cidr,
            ("inet", None) => Storage::Inet,
            _ => return None,
        })
    }
}

/// The column of a `column IS NOT NULL` check.
fn is_not_null(expr: &Expr) -> Option<String> {
    match expr {
        Expr::IsNotNull(inner) => match inner.as_ref() {
            Expr::Identifier(ident) => Some(name(ident)),
            _ => None,
        },
        Expr::Nested(inner) => is_not_null(inner),
        _ => None,
    }
}

/// Whether `sql` is the output of `pg_dump`, a schema rather than migrations: it never writes
/// `CONCURRENTLY` or `NOT VALID`.
fn is_dump(sql: &str) -> bool {
    sql.lines()
        .take(3)
        .any(|line| line == "-- PostgreSQL database dump")
}

/// `stmt` parsed, with the tables a `CREATE TABLE` copies with `LIKE`, whose `INCLUDING`
/// options sqlparser does not read.
fn parse(stmt: &Raw) -> Option<(Statement, Vec<String>)> {
    let (sql, likes) = stmt
        .is_create_table()
        .then(|| stmt.without_like())
        .flatten()
        .unwrap_or_else(|| (stmt.sql(), Vec::new()));
    let parsed = parse_ddl(&sql, SqlDialect::Postgres)?.into_iter().next()?;
    let likes = likes
        .into_iter()
        .map(|parts| match parts.as_slice() {
            [schema, rest @ ..] if schema == "public" && !rest.is_empty() => rest.join("."),
            _ => parts.join("."),
        })
        .collect();
    Some((parsed, likes))
}

/// The empty table, materialized view or index `stmt` creates.
fn created(stmt: &Statement) -> Option<String> {
    match stmt {
        Statement::CreateTable(create) if create.query.is_none() => Some(key(&create.name)),
        Statement::CreateView(view) if view.materialized => Some(key(&view.name)),
        Statement::CreateIndex(index) => index.name.as_ref().map(key),
        _ => None,
    }
}

/// The table `stmt` copies rows into from a query or a file: `INSERT ... VALUES` adds only the
/// rows it lists.
fn filled(stmt: &Statement) -> Option<String> {
    match stmt {
        Statement::Insert(Insert {
            table: TableObject::TableName(table),
            source: Some(source),
            ..
        }) if !matches!(*source.body, SetExpr::Values(_)) => Some(key(table)),
        Statement::Copy {
            source: CopySource::Table { table_name, .. },
            to: false,
            ..
        } => Some(key(table_name)),
        _ => None,
    }
}

fn unique_message(table: &ObjectName) -> String {
    format!(
        "the constraint builds its index under an ACCESS EXCLUSIVE lock on `{table}`: `CREATE UNIQUE INDEX CONCURRENTLY`, then add the constraint `USING INDEX`"
    )
}

fn drop_index_message() -> String {
    "`DROP INDEX` waits for an ACCESS EXCLUSIVE lock on its table, and every query of the table waits behind it: drop it `CONCURRENTLY`".to_string()
}

fn rewrite_command_message(command: &str) -> String {
    format!(
        "`{command}` rewrites the table under an ACCESS EXCLUSIVE lock, blocking reads and writes until it finishes: rebuild it online with pg_repack or pg_squeeze, outside migrations"
    )
}

/// `name` as PostgreSQL folds it, without the `public` schema.
fn key(name: &ObjectName) -> String {
    let parts: Vec<String> = name
        .0
        .iter()
        .filter_map(|part| part.as_ident())
        .map(|ident| fold(ident, SqlDialect::Postgres))
        .collect();
    match parts.as_slice() {
        [schema, rest @ ..] if schema == "public" && !rest.is_empty() => rest.join("."),
        _ => parts.join("."),
    }
}

fn name(ident: &sqlparser::ast::Ident) -> String {
    fold(ident, SqlDialect::Postgres)
}

/// Whether a `lock_timeout` value turns the timeout off: `0`, `'0'`, `'0s'` or `DEFAULT`.
fn is_zero(value: &str) -> bool {
    let value = value.trim_matches('\'');
    value == "default" || value.trim_end_matches(|c: char| c.is_ascii_alphabetic()) == "0"
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    const USERS: &str = "CREATE TABLE users (id BIGINT PRIMARY KEY, email VARCHAR(100), \
        bio TEXT, score NUMERIC(5, 2), whole NUMERIC(5), free NUMERIC, net CIDR, bits VARBIT(4), \
        flags VARBIT, odd GEOMETRY(POINT, 4326), team_id BIGINT);\n\
        CREATE TABLE teams (id BIGINT PRIMARY KEY);\n\
        CREATE INDEX users_email ON users (email);";

    fn rules(baseline: &str, migration: &str) -> Vec<Rule> {
        check(baseline, &[migration])
            .remove(0)
            .into_iter()
            .map(|f| f.rule)
            .collect()
    }

    fn one(migration: &str) -> Vec<Rule> {
        rules(USERS, &format!("SET lock_timeout = '2s';\n{migration}"))
    }

    #[test]
    fn locks_of_each_statement() {
        use Rule::*;
        for (migration, expected) in [
            ("CREATE INDEX users_bio ON users (bio)", &[MissingConcurrently][..]),
            ("CREATE INDEX CONCURRENTLY users_bio ON users (bio)", &[]),
            ("DROP INDEX users_email", &[MissingConcurrently]),
            ("DROP INDEX CONCURRENTLY IF EXISTS public.users_email", &[]),
            ("REINDEX INDEX users_email", &[MissingConcurrently]),
            ("REINDEX (VERBOSE) TABLE CONCURRENTLY users", &[]),
            ("CLUSTER users USING users_email", &[TableRewrite]),
            ("CLUSTER VERBOSE public.users", &[TableRewrite]),
            ("CLUSTER", &[TableRewrite]),
            ("VACUUM FULL users", &[TableRewrite]),
            ("VACUUM FULL", &[TableRewrite]),
            ("VACUUM users", &[]),
            ("REFRESH MATERIALIZED VIEW mv", &[MissingConcurrently]),
            ("REFRESH MATERIALIZED VIEW CONCURRENTLY mv", &[]),
            ("ALTER TABLE users ADD COLUMN x uuid DEFAULT gen_random_uuid()", &[TableRewrite]),
            ("ALTER TABLE users ADD COLUMN x uuid DEFAULT lower(md5(random()::text))::uuid", &[TableRewrite]),
            ("ALTER TABLE users ADD COLUMN x timestamptz DEFAULT now()", &[]),
            ("ALTER TABLE users ADD COLUMN x bigserial", &[TableRewrite]),
            ("ALTER TABLE users ADD COLUMN x bigint GENERATED ALWAYS AS IDENTITY", &[TableRewrite]),
            ("ALTER TABLE users ADD COLUMN x bigint GENERATED ALWAYS AS (id * 2) STORED", &[TableRewrite]),
            ("ALTER TABLE users ADD COLUMN x bigint GENERATED ALWAYS AS (id * 2) VIRTUAL", &[]),
            ("ALTER TABLE users ADD COLUMN x int NOT NULL", &[AddRequiredColumn]),
            ("ALTER TABLE users ADD COLUMN x int NOT NULL DEFAULT 0", &[]),
            ("ALTER TABLE users ADD COLUMN x int NOT NULL GENERATED BY DEFAULT AS IDENTITY", &[TableRewrite]),
            ("ALTER TABLE users ADD COLUMN x int UNIQUE", &[MissingConcurrently]),
            ("ALTER TABLE users ADD COLUMN x bigint NULL REFERENCES teams (id)", &[]),
            ("ALTER TABLE users ADD COLUMN x int PRIMARY KEY", &[MissingConcurrently, AddRequiredColumn]),
            ("ALTER TABLE users ALTER COLUMN bio SET NOT NULL", &[ConstraintScan]),
            ("ALTER TABLE users ALTER COLUMN bio DROP NOT NULL", &[]),
            ("ALTER TABLE users ALTER COLUMN email TYPE text", &[]),
            ("ALTER TABLE users ALTER COLUMN email TYPE varchar(200)", &[]),
            ("ALTER TABLE users ALTER COLUMN email TYPE varchar(50)", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN bio TYPE varchar", &[]),
            ("ALTER TABLE users ALTER COLUMN bio TYPE varchar(10)", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN score TYPE numeric(7, 2)", &[]),
            ("ALTER TABLE users ALTER COLUMN score TYPE numeric(7, 3)", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN score TYPE numeric", &[]),
            ("ALTER TABLE users ALTER COLUMN whole TYPE numeric(9)", &[]),
            ("ALTER TABLE users ALTER COLUMN whole TYPE numeric(9, 2)", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN free TYPE numeric(9)", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN net TYPE inet", &[]),
            ("ALTER TABLE users ALTER COLUMN bits TYPE varbit(8)", &[]),
            ("ALTER TABLE users ALTER COLUMN bits TYPE bit varying", &[]),
            ("ALTER TABLE users ALTER COLUMN bits TYPE varbit(2)", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN flags TYPE varbit(2)", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN id TYPE numeric", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN id TYPE BIGINT", &[]),
            ("ALTER TABLE users ALTER COLUMN unknown TYPE text", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN odd TYPE text", &[TableRewrite]),
            ("ALTER TABLE users ADD CONSTRAINT fk FOREIGN KEY (team_id) REFERENCES teams (id)", &[ConstraintScan]),
            ("ALTER TABLE users ADD CONSTRAINT fk FOREIGN KEY (team_id) REFERENCES teams (id) NOT VALID", &[]),
            ("ALTER TABLE users ADD CONSTRAINT c CHECK (id > 0)", &[ConstraintScan]),
            ("ALTER TABLE users ADD CONSTRAINT u UNIQUE (email)", &[MissingConcurrently]),
            ("ALTER TABLE users ADD CONSTRAINT u UNIQUE USING INDEX users_email", &[]),
            ("ALTER TABLE users ADD PRIMARY KEY (id)", &[MissingConcurrently]),
            ("ALTER TABLE users VALIDATE CONSTRAINT fk", &[]),
            ("ALTER TABLE events DETACH PARTITION events_2020", &[MissingConcurrently]),
            ("ALTER TABLE events DETACH PARTITION events_2020 CONCURRENTLY", &[]),
            ("ALTER TABLE users RENAME COLUMN bio TO about", &[]),
            ("LOCK TABLE users", &[]),
            ("DROP TABLE users", &[]),
            ("SELECT 1", &[]),
            ("ANALYZE users", &[]),
        ] {
            assert_eq!(one(migration), expected, "{migration}");
        }
    }

    #[test]
    fn messages_name_the_table_and_the_safe_way() {
        let found = check(
            USERS,
            &["SET lock_timeout = '2s';\nCREATE INDEX i ON users (bio);"],
        );
        assert_eq!(
            found,
            [[Finding {
                rule: Rule::MissingConcurrently,
                message: "`CREATE INDEX` blocks writes to `users` until the index is built: create it `CONCURRENTLY`".to_string(),
                span: 25..54,
            }]]
        );
    }

    #[test]
    fn statements_of_the_baseline_are_not_judged() {
        let baseline = format!("BEGIN;\n{USERS}\nCREATE INDEX users_bio ON users (bio);\nCOMMIT;");
        let layout = "CREATE  INDEX users_bio\n  ON users (bio); -- moved\nCREATE INDEX users_bio ON users (bio);";
        assert_eq!(rules(&baseline, layout), [Rule::MissingConcurrently]);
        assert_eq!(check(&baseline, &[]), Vec::<Vec<Finding>>::new());
    }

    #[test]
    fn what_a_migration_creates_is_empty() {
        let migration = "CREATE TABLE t (id int);
            CREATE INDEX t_id ON t (id);
            ALTER TABLE t ADD COLUMN x int NOT NULL, ADD COLUMN y uuid DEFAULT gen_random_uuid();
            ALTER TABLE public.t ALTER COLUMN x SET NOT NULL;
            DROP INDEX t_id;
            DROP INDEX CONCURRENTLY t_id;
            CLUSTER t;
            VACUUM FULL t;
            DROP TABLE t;
            CREATE MATERIALIZED VIEW mv AS SELECT 1;
            REFRESH MATERIALIZED VIEW mv;";
        assert_eq!(rules(USERS, migration), []);
        let quoted = r#"CREATE TABLE "T" (id int); CLUSTER "T"; CLUSTER t;"#;
        assert_eq!(rules(USERS, quoted), [Rule::TableRewrite]);
    }

    #[test]
    fn a_table_copied_with_like_is_new() {
        for like in [
            "LIKE users",
            "LIKE public.users INCLUDING DEFAULTS INCLUDING CONSTRAINTS",
            "LIKE users INCLUDING ALL, extra int",
            "LIKE users EXCLUDING INDEXES",
        ] {
            let migration = format!(
                "CREATE TABLE t ({like});
                CREATE INDEX t_bio ON t (bio);
                ALTER TABLE t ALTER COLUMN bio SET NOT NULL;"
            );
            assert_eq!(rules(USERS, &migration), [], "{like}");
        }
    }

    #[test]
    fn a_table_copies_the_column_types_of_its_like_source() {
        let migration = "CREATE TABLE t (LIKE users INCLUDING ALL);
            INSERT INTO t SELECT * FROM users;
            SET lock_timeout = '1s';
            ALTER TABLE t ALTER COLUMN email TYPE varchar(200);
            ALTER TABLE t ALTER COLUMN email TYPE varchar(50);";
        assert_eq!(rules(USERS, migration), [Rule::TableRewrite]);
    }

    #[test]
    fn rows_copied_into_a_new_table_end_its_exemption() {
        use Rule::*;
        for (fill, expected) in [
            (
                "INSERT INTO t SELECT * FROM users",
                &[MissingConcurrently][..],
            ),
            (
                "INSERT INTO public.t (id) SELECT id FROM users",
                &[MissingConcurrently],
            ),
            ("COPY t FROM '/tmp/t.csv'", &[MissingConcurrently]),
            ("INSERT INTO t VALUES (1), (2)", &[]),
            ("COPY t TO '/tmp/t.csv'", &[]),
            ("INSERT INTO users SELECT * FROM users", &[]),
        ] {
            let migration = format!(
                "SET lock_timeout = '1s';
                CREATE TABLE t (id int);
                {fill};
                CREATE INDEX t_id ON t (id);"
            );
            assert_eq!(rules(USERS, &migration), expected, "{fill}");
        }
        let created_as = "CREATE TABLE t AS SELECT id FROM users; CREATE INDEX t_id ON t (id);";
        assert_eq!(rules(USERS, created_as), [MissingConcurrently]);
    }

    #[test]
    fn a_table_the_baseline_has_is_not_new() {
        let index = "SET lock_timeout = '1s'; CREATE INDEX i ON users (bio);";
        for again in [
            "CREATE TABLE IF NOT EXISTS users (id BIGINT);",
            "create table users (id bigint);",
        ] {
            assert_eq!(
                rules(USERS, &format!("{again}\n{index}")),
                [Rule::MissingConcurrently],
                "{again}"
            );
        }
        let reworded = USERS
            .to_lowercase()
            .replace("create index", "CREATE  INDEX");
        assert_eq!(rules(USERS, &reworded), []);
    }

    #[test]
    fn a_pg_dump_schema_is_not_a_migration() {
        let dump = "--\n-- PostgreSQL database dump\n--\n\nCREATE INDEX i ON public.users USING btree (bio);";
        assert_eq!(rules(USERS, dump), []);
    }

    #[test]
    fn a_validated_not_null_check_spares_the_scan() {
        let checked = "SET lock_timeout = '1s';
            ALTER TABLE users ADD CONSTRAINT c CHECK (bio IS NOT NULL) NOT VALID;
            ALTER TABLE users VALIDATE CONSTRAINT c;
            ALTER TABLE users ALTER COLUMN bio SET NOT NULL;";
        assert_eq!(rules(USERS, checked), []);
        let unvalidated = "SET lock_timeout = '1s';
            ALTER TABLE users ADD CONSTRAINT c CHECK ((bio IS NOT NULL)) NOT VALID;
            ALTER TABLE users VALIDATE CONSTRAINT other;
            ALTER TABLE users ADD CONSTRAINT d CHECK (users.bio IS NOT NULL) NOT VALID;
            ALTER TABLE users ALTER COLUMN bio SET NOT NULL;";
        assert_eq!(rules(USERS, unvalidated), [Rule::ConstraintScan]);
        let baseline = format!("{USERS}\nALTER TABLE users ADD CHECK (email IS NOT NULL);");
        let set = "SET lock_timeout = '1s'; ALTER TABLE users ALTER COLUMN email SET NOT NULL;";
        assert_eq!(rules(&baseline, set), []);
    }

    #[test]
    fn concurrently_fails_in_a_transaction() {
        let migration = "SET lock_timeout = '1s';
            BEGIN;
            CREATE INDEX CONCURRENTLY a ON users (bio);
            DROP INDEX CONCURRENTLY users_email;
            COMMIT;
            CREATE INDEX CONCURRENTLY b ON users (bio);
            START TRANSACTION;
            ALTER TABLE events DETACH PARTITION p CONCURRENTLY;
            REINDEX INDEX CONCURRENTLY a;
            ROLLBACK;";
        assert_eq!(
            rules(USERS, migration),
            [Rule::ConcurrentlyInTransaction; 4]
        );
    }

    #[test]
    fn lock_timeout_is_asked_once_per_file() {
        let migration = "ALTER TABLE users ADD COLUMN a int;
            ALTER TABLE users ADD COLUMN b int;";
        assert_eq!(
            check(USERS, &[migration, migration]),
            [[Finding {
                rule: Rule::MissingLockTimeout,
                message: "takes a lock that blocks writes without `SET lock_timeout` first: while it waits for the lock, the queries it blocks wait behind it".to_string(),
                span: 0..34,
            }], [Finding {
                rule: Rule::MissingLockTimeout,
                message: "takes a lock that blocks writes without `SET lock_timeout` first: while it waits for the lock, the queries it blocks wait behind it".to_string(),
                span: 0..34,
            }]]
        );
        for set in [
            "SET lock_timeout TO 2000",
            "SET SESSION lock_timeout = '2s'",
            "SET LOCAL lock_timeout = '500ms'",
        ] {
            assert_eq!(
                rules(USERS, &format!("{set}; LOCK TABLE users;")),
                [],
                "{set}"
            );
        }
        for unset in [
            "SET lock_timeout = 0",
            "SET lock_timeout = '0s'",
            "SET lock_timeout = DEFAULT",
            "SET lock_timeout = '1s'; RESET lock_timeout",
            "SET lock_timeout = '1s'; RESET ALL",
            "BEGIN; SET LOCAL lock_timeout = '1s'; COMMIT",
        ] {
            assert_eq!(
                rules(USERS, &format!("{unset}; DROP TABLE users;")),
                [Rule::MissingLockTimeout],
                "{unset}"
            );
        }
        for weak in [
            "ALTER TABLE users VALIDATE CONSTRAINT c",
            "CREATE INDEX CONCURRENTLY i ON users (bio)",
            "DROP INDEX CONCURRENTLY users_email",
        ] {
            assert_eq!(rules(USERS, weak), [], "{weak}");
        }
    }
}
