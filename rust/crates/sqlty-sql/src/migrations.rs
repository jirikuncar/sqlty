//! What the new statements of a PostgreSQL migration lock, as `sqlty breaking` reports it.
//!
//! A statement is new when the baseline schema does not have it, compared without layout and
//! comments. Each new statement is judged against the tables the baseline and the earlier
//! statements define: a table created by a new statement is empty, so nothing it does to it
//! blocks anyone. Statements sqlparser does not read (`DROP INDEX CONCURRENTLY`, `REINDEX`,
//! `CLUSTER`, `DETACH PARTITION`, `CREATE INDEX ... TABLESPACE`) are judged by their tokens.
//!
//! Lock levels and rewrites follow the PostgreSQL 17 documentation of
//! [`ALTER TABLE`](https://www.postgresql.org/docs/17/sql-altertable.html),
//! [`CREATE INDEX`](https://www.postgresql.org/docs/17/sql-createindex.html),
//! [`REINDEX`](https://www.postgresql.org/docs/17/sql-reindex.html),
//! [`CLUSTER`](https://www.postgresql.org/docs/17/sql-cluster.html),
//! [`VACUUM`](https://www.postgresql.org/docs/17/sql-vacuum.html),
//! [`REFRESH MATERIALIZED VIEW`](https://www.postgresql.org/docs/17/sql-refreshmaterializedview.html)
//! and [explicit locking](https://www.postgresql.org/docs/17/explicit-locking.html): `CREATE
//! INDEX` locks the table SHARE, `REINDEX` locks it SHARE and the index ACCESS EXCLUSIVE, and the
//! others lock the table ACCESS EXCLUSIVE. All of them block writes.

use std::ops::{ControlFlow, Range};

use rustc_hash::{FxHashMap, FxHashSet};
use sqlparser::ast::{
    visit_expressions, AlterColumnOperation, AlterTableOperation, AlterType, AlterTypeOperation,
    BinaryOperator, CascadeOption, CharacterLength, CheckConstraint, ColumnDef, ColumnOption,
    CopySource, CreateTable, DataType, Delete, DetachPartitionMode, ExactNumberInfo, Expr,
    ForValues, FromTable, InTarget, Insert, LockTableMode, Merge, ObjectName, ObjectType, SetExpr,
    Statement, TableConstraint, TableFactor, TableObject,
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
        let parsed = model.parse_if_shaping(&stmt);
        model.record_indexes(&stmt, parsed.as_ref().map(|(parsed, _)| parsed));
        if let Some((parsed, likes)) = parsed {
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
    /// The table of each index the baseline or a new statement created, by its name in the
    /// schema of the table.
    index_tables: FxHashMap<String, String>,
    /// The tables the baseline and earlier statements create.
    tables: FxHashSet<String>,
    /// The declared type of each column.
    types: FxHashMap<(String, String), DataType>,
    /// The tables on either side of a foreign key.
    referencing: FxHashSet<String>,
    /// The tables a foreign key references.
    referenced: FxHashSet<String>,
    /// The `CHECK` constraints of each table.
    checks: FxHashMap<String, Vec<Check>>,
    /// The columns declared `NOT NULL`, by table.
    required: FxHashSet<(String, String)>,
    /// The columns of each partitioned table's partition key.
    partition_keys: FxHashMap<String, Vec<String>>,
    /// The default partition of each partitioned table.
    defaults: FxHashMap<String, String>,
    /// The temporary tables new statements create: they stay exempt when rows are copied in.
    temporary: FxHashSet<String>,
    /// The columns that alone are a primary key or unique, by table.
    keys: FxHashSet<(String, String)>,
    /// The partitioned tables whose partitions no `CHECK` can prove: hash partitioning or an
    /// expression in the key.
    unprovable: FxHashSet<String>,
}

struct Check {
    name: Option<String>,
    /// The column of a `CHECK (column IS NOT NULL)`.
    not_null: Option<String>,
    /// The lowercased words of its expression.
    words: Vec<String>,
    valid: bool,
}

/// What the statements of one file so far have set.
#[derive(Default)]
struct Session {
    transaction: bool,
    lock_timeout: bool,
    /// Set by `SET LOCAL`, until the transaction ends.
    local_timeout: bool,
    reported_timeout: bool,
    /// The enum values `ALTER TYPE ... ADD VALUE` added in the open transaction, as string
    /// literals, with their type.
    added_values: Vec<(String, ObjectName)>,
    /// The tables with rows that the open transaction locks against writes, in order.
    locked: Vec<String>,
    reported_tables: bool,
    reported_backfill: bool,
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
                if let Some(used) = session
                    .added_values
                    .iter()
                    .position(|(value, ty)| self.uses_value(&stmt, value, ty))
                {
                    let (value, ty) = session.added_values.remove(used);
                    report(
                        Rule::EnumValueInTransaction,
                        format!(
                            "{value} was added to `{ty}` in this transaction: PostgreSQL refuses a new enum value until the transaction that added it commits, so `COMMIT` after `ADD VALUE`"
                        ),
                    );
                }
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
                if session.transaction {
                    let held = session.locked.len();
                    let locked = match &parsed {
                        Some(parsed) => self.write_locked(parsed),
                        None => self.write_locked_tokens(&stmt, &tokens),
                    };
                    for table in locked {
                        if !session.locked.contains(&table) {
                            session.locked.push(table);
                        }
                    }
                    if let (held @ [_, ..], [last, ..]) = session.locked.split_at(held) {
                        if !session.reported_tables {
                            session.reported_tables = true;
                            report(
                                Rule::MultiTableLock,
                                format!(
                                    "locks `{last}` while this transaction still holds `{}`: queries of any of them now wait for all of them, until `COMMIT`; change one table per transaction",
                                    held.join("`, `")
                                ),
                            );
                        }
                    }
                }
                if let Some((target, selection)) = parsed
                    .as_ref()
                    .and_then(backfilled)
                    .filter(|(target, _)| !self.created.contains(&target.table))
                {
                    let table = target.table.clone();
                    if !selection.is_some_and(|selection| self.batched(&target, selection)) {
                        report(
                            Rule::UnbatchedBackfill,
                            format!(
                                "changes every matching row of `{table}` in one statement: it holds their row locks and grows the WAL until it ends; change them in batches by key, `WHERE id IN (SELECT id ... LIMIT 5000)`"
                            ),
                        );
                    }
                    if session.locked.contains(&table) && !session.reported_backfill {
                        session.reported_backfill = true;
                        report(
                            Rule::LockedBackfill,
                            format!(
                                "an earlier statement of this transaction locks `{table}` against writes, so they wait for the whole backfill: `COMMIT` the schema change first"
                            ),
                        );
                    }
                }
                if let Some(Statement::AlterType(AlterType {
                    name,
                    operation: AlterTypeOperation::AddValue(add),
                })) = parsed.as_ref().filter(|_| session.transaction)
                {
                    let value = format!("'{}'", add.value.value.replace('\'', "''"));
                    session.added_values.push((value, name.clone()));
                }
                self.record_indexes(&stmt, parsed.as_ref());
                let created = match &parsed {
                    Some(parsed) => created(parsed),
                    None => created_by_tokens(&stmt.tokens()),
                };
                if let Some(created) = created.filter(|c| !self.tables.contains(c)) {
                    if matches!(&parsed, Some(Statement::CreateTable(create)) if create.temporary) {
                        self.temporary.insert(created.clone());
                    }
                    self.created.insert(created);
                }
            }
            if let Some(filled) = parsed.as_ref().and_then(filled) {
                if !self.temporary.contains(&filled) {
                    self.created.remove(&filled);
                }
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
                    self.require(&table, column);
                }
                for constraint in &create.constraints {
                    self.require_key(&table, constraint);
                    self.record_key(&table, constraint);
                }
                if let Some(by) = &create.partition_by {
                    let by = by.to_string();
                    let parts = by
                        .split_once('(')
                        .and_then(|(_, rest)| rest.rsplit_once(')'))
                        .map_or("", |(parts, _)| parts);
                    let keys: Vec<String> = parts
                        .split(',')
                        .map(|part| fold_token(part.trim()))
                        .collect();
                    let plain = keys.iter().all(|key| {
                        create
                            .columns
                            .iter()
                            .any(|column| name(&column.name) == *key)
                    });
                    if !plain || by.to_lowercase().starts_with("hash") {
                        self.unprovable.insert(table.clone());
                    }
                    self.partition_keys.insert(table.clone(), keys);
                }
                if let (Some(parent), Some(ForValues::Default)) =
                    (&create.partition_of, &create.for_values)
                {
                    self.defaults.insert(key(parent), table.clone());
                }
                let inline = create.columns.iter().flat_map(|column| &column.options);
                let checks =
                    inline
                        .filter_map(|option| match &option.option {
                            ColumnOption::Check(check) => Some(check),
                            _ => None,
                        })
                        .chain(create.constraints.iter().filter_map(
                            |constraint| match constraint {
                                TableConstraint::Check(check) => Some(check),
                                _ => None,
                            },
                        ));
                for check in checks {
                    self.add_check(&table, check, true);
                }
            }
            Statement::AlterTable(alter) => {
                let table = key(&alter.name);
                for op in &alter.operations {
                    if let AlterTableOperation::AddConstraint { constraint, .. } = op {
                        self.record_key(&table, constraint);
                    }
                    self.apply_op(&table, op);
                }
            }
            Statement::CreateIndex(index) if index.unique && index.predicate.is_none() => {
                if let [column] = index.columns.as_slice() {
                    self.keys
                        .insert((key(&index.table_name), column_name(&column.column.expr)));
                }
            }
            Statement::Drop {
                object_type: ObjectType::Table,
                names,
                ..
            } => {
                let dropped: Vec<String> = names.iter().map(key).collect();
                self.defaults.retain(|parent, default| {
                    !dropped.contains(parent) && !dropped.contains(default)
                });
            }
            _ => {}
        }
        self.referencing.extend(foreign_keys(stmt));
        self.referenced.extend(referenced_tables(stmt));
    }

    fn apply_op(&mut self, table: &str, op: &AlterTableOperation) {
        match op {
            AlterTableOperation::AddColumn { column_def, .. } => {
                self.types.insert(
                    (table.to_string(), name(&column_def.name)),
                    column_def.data_type.clone(),
                );
                self.require(table, column_def);
            }
            AlterTableOperation::AlterColumn {
                column_name,
                op: AlterColumnOperation::SetNotNull,
            } => {
                self.required.insert((table.to_string(), name(column_name)));
            }
            AlterTableOperation::AlterColumn {
                column_name,
                op: AlterColumnOperation::DropNotNull,
            } => {
                self.required
                    .remove(&(table.to_string(), name(column_name)));
            }
            AlterTableOperation::DetachPartitionOf { partition, .. } => {
                if self.defaults.get(table) == Some(&key(partition)) {
                    self.defaults.remove(table);
                }
            }
            AlterTableOperation::AddConstraint { constraint, .. }
                if matches!(constraint, TableConstraint::PrimaryKey(_)) =>
            {
                self.require_key(table, constraint);
            }
            AlterTableOperation::AttachPartitionOf {
                partition,
                for_values: ForValues::Default,
            } => {
                self.defaults.insert(table.to_string(), key(partition));
            }
            AlterTableOperation::DropConstraint { name: dropped, .. } => {
                let dropped = name(dropped);
                if let Some(checks) = self.checks.get_mut(table) {
                    checks.retain(|check| check.name.as_ref() != Some(&dropped));
                }
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
            } => self.add_check(table, check, !not_valid),
            AlterTableOperation::ValidateConstraint { name: validated } => {
                let validated = name(validated);
                for check in self.checks.entry(table.to_string()).or_default() {
                    if check.name.as_ref() == Some(&validated) {
                        check.valid = true;
                    }
                }
            }
            _ => {}
        }
    }

    /// Whether `stmt` puts the new enum `value` of `ty` to use: it casts to `ty`, names a
    /// column of type `ty`, or names no table whose columns are known.
    fn uses_value(&self, stmt: &Raw, value: &str, ty: &ObjectName) -> bool {
        let tokens = stmt.tokens();
        if !tokens.contains(&value)
            || tokens
                .iter()
                .take(2)
                .map(|t| t.to_lowercase())
                .eq(["alter", "type"])
        {
            return false;
        }
        let words: Vec<String> = tokens.iter().map(|token| fold_token(token)).collect();
        let ty = key(ty);
        let short = ty.rsplit('.').next().unwrap_or(&ty);
        let typed = |column: &(String, String)| {
            self.types.get(column).is_some_and(|declared| {
                let declared = declared.to_string().to_lowercase();
                declared == ty || declared == short
            })
        };
        let cast = words
            .windows(2)
            .any(|pair| matches!(pair[0].as_str(), "::" | "as") && pair[1] == short);
        let positional = match parse(stmt) {
            Some((
                Statement::Insert(Insert {
                    table: TableObject::TableName(table),
                    columns,
                    ..
                }),
                _,
            )) if columns.is_empty() => {
                let table = key(&table);
                self.types
                    .keys()
                    .any(|column| column.0 == table && typed(column))
            }
            _ => false,
        };
        cast || positional
            || words.iter().all(|word| !self.tables.contains(word))
            || self
                .types
                .keys()
                .any(|column| typed(column) && words.contains(&column.1))
    }

    fn add_check(&mut self, table: &str, check: &CheckConstraint, valid: bool) {
        self.checks
            .entry(table.to_string())
            .or_default()
            .push(Check {
                name: check.name.as_ref().map(name),
                not_null: is_not_null(&check.expr),
                words: words(&check.expr.to_string()),
                valid,
            });
    }

    fn require(&mut self, table: &str, column: &ColumnDef) {
        let required = column.options.iter().any(|option| {
            matches!(
                option.option,
                ColumnOption::NotNull | ColumnOption::PrimaryKey(_)
            )
        });
        if required {
            self.required
                .insert((table.to_string(), name(&column.name)));
        }
        let unique = column.options.iter().any(|option| {
            matches!(
                option.option,
                ColumnOption::Unique(_) | ColumnOption::PrimaryKey(_)
            )
        });
        if unique {
            self.keys.insert((table.to_string(), name(&column.name)));
        }
    }

    fn record_key(&mut self, table: &str, constraint: &TableConstraint) {
        let columns = match constraint {
            TableConstraint::PrimaryKey(key) => &key.columns,
            TableConstraint::Unique(key) => &key.columns,
            _ => return,
        };
        if let [column] = columns.as_slice() {
            self.keys
                .insert((table.to_string(), column_name(&column.column.expr)));
        }
    }

    /// Whether `selection` changes rows a few at a time: it pins a key column to listed values,
    /// takes a range with `BETWEEN`, or picks the rows with a `LIMIT` subquery.
    fn batched(&self, target: &Backfill, selection: &Expr) -> bool {
        let table = target.table.as_str();
        let short = table.rsplit('.').next().unwrap_or(table);
        let keyed = |column: &Expr| match column {
            Expr::Identifier(column) => self.keys.contains(&(table.to_string(), name(column))),
            Expr::CompoundIdentifier(parts) => {
                let mut parts = parts.iter().rev().map(name);
                let (column, qualifier) = (parts.next(), parts.next());
                qualifier.is_some_and(|qualifier| {
                    qualifier == short || target.alias.as_ref() == Some(&qualifier)
                }) && column.is_some_and(|column| self.keys.contains(&(table.to_string(), column)))
            }
            _ => false,
        };
        let literal = |value: &Expr| matches!(value, Expr::Value(_));
        conjuncts(selection).any(|expr| match expr {
            Expr::Between { negated: false, .. } => true,
            Expr::BinaryOp {
                left,
                op: BinaryOperator::Eq,
                right,
            } => (keyed(left) && literal(right)) || (keyed(right) && literal(left)),
            Expr::In {
                expr,
                target: InTarget::List(list),
                negated: false,
                ..
            } => keyed(expr) && list.iter().all(literal),
            Expr::In {
                target: InTarget::Subquery(query),
                negated: false,
                ..
            } => query.limit_clause.is_some(),
            _ => false,
        })
    }

    /// The tables with rows that `stmt` locks against writes: `SHARE` or a stronger lock.
    fn write_locked(&self, stmt: &Statement) -> Vec<String> {
        let mut tables = match stmt {
            Statement::AlterTable(alter) => {
                let table = key(&alter.name);
                let mut tables = Vec::new();
                for op in &alter.operations {
                    match op {
                        op if keeps_writes(op) => {}
                        AlterTableOperation::AttachPartitionOf { partition, .. } => {
                            tables.push(key(partition));
                            tables.extend(self.defaults.get(&table).cloned());
                        }
                        _ => tables.push(table.clone()),
                    }
                }
                tables.extend(referenced_tables(stmt));
                tables
            }
            Statement::CreateTable(_) => referenced_tables(stmt),
            Statement::Truncate(truncate) => truncate
                .table_names
                .iter()
                .map(|target| key(&target.name))
                .collect(),
            Statement::Lock(lock) if blocks_writes(lock.lock_mode.as_ref()) => {
                lock.tables.iter().map(|target| key(&target.name)).collect()
            }
            Statement::Drop {
                object_type: ObjectType::Table,
                names,
                ..
            } => names.iter().map(key).collect(),
            Statement::CreateIndex(index) if !index.concurrently => vec![key(&index.table_name)],
            Statement::RefreshMaterializedView(refresh) if !refresh.concurrently => {
                vec![key(&refresh.name)]
            }
            Statement::Vacuum(vacuum) if vacuum.full => vacuum.table_name.iter().map(key).collect(),
            _ => Vec::new(),
        };
        tables.retain(|table| !self.created.contains(table));
        tables
    }

    /// The tables with rows that a statement sqlparser does not read locks against writes, as
    /// `judge_tokens` judges it. `REINDEX INDEX` of an index of unknown table locks none.
    fn write_locked_tokens(&self, stmt: &Raw, tokens: &[String]) -> Vec<String> {
        let original = stmt.tokens();
        let words: Vec<&str> = tokens.iter().map(String::as_str).collect();
        let mut tables = match words.as_slice() {
            ["create", "index" | "unique", ..] => create_index(&original)
                .filter(|index| !index.concurrently)
                .map(|index| index.table)
                .into_iter()
                .collect(),
            ["reindex", ..] => {
                let (options, rest) = split_options(&original[1..]);
                let name = token_name(rest.get(1..).unwrap_or_default(), &["concurrently"], &[]);
                match rest.first().map(|kind| kind.to_lowercase()).as_deref() {
                    _ if reindexes_concurrently(options, rest) => Vec::new(),
                    Some("table") => vec![name],
                    Some("index") => self.index_tables.get(&name).cloned().into_iter().collect(),
                    _ => Vec::new(),
                }
            }
            ["cluster", ..] => vec![cluster_table(&original[1..])],
            ["vacuum", ..] => {
                let (options, rest) = split_options(&original[1..]);
                match vacuum_full(options, rest) {
                    (true, tables) => tables,
                    (false, _) => Vec::new(),
                }
            }
            ["alter", "table", rest @ ..]
                if rest.windows(2).any(|pair| pair == ["set", "tablespace"])
                    && !rest.starts_with(&["all", "in"]) =>
            {
                vec![set_tablespace_table(&original)]
            }
            _ => Vec::new(),
        };
        tables.retain(|table| !table.is_empty() && !self.created.contains(table));
        tables
    }

    fn require_key(&mut self, table: &str, constraint: &TableConstraint) {
        if let TableConstraint::PrimaryKey(key) = constraint {
            for column in &key.columns {
                for word in words(&column.column.expr.to_string()) {
                    self.required.insert((table.to_string(), word));
                }
            }
        }
    }

    /// Whether `column` of `table` can hold no NULL.
    fn is_required(&self, table: &str, column: &str) -> bool {
        self.required
            .contains(&(table.to_string(), column.to_string()))
            || self.checks.get(table).is_some_and(|checks| {
                checks
                    .iter()
                    .any(|check| check.valid && check.not_null.as_deref() == Some(column))
            })
    }

    /// Whether a validated `CHECK` of `partition` may prove that its rows fit a partition of
    /// `parent`: it names a partition key column, and every key column is `NOT NULL`. Without
    /// a known key, a `NOT NULL` column it names.
    fn proves_bounds(&self, partition: &str, parent: &str) -> bool {
        let keys = self
            .partition_keys
            .get(parent)
            .filter(|keys| !keys.is_empty());
        self.bounds_checks(partition, parent)
            .any(|check| match keys {
                Some(keys) => keys.iter().all(|key| self.is_required(partition, key)),
                None => check
                    .words
                    .iter()
                    .any(|word| self.is_required(partition, word)),
            })
    }

    /// The validated checks of `table` that may bound the partition key of `parent`: they
    /// name a key column and are more than `IS NOT NULL`.
    fn bounds_checks<'a>(&'a self, table: &str, parent: &str) -> impl Iterator<Item = &'a Check> {
        let keys = self
            .partition_keys
            .get(parent)
            .filter(|keys| !keys.is_empty());
        let provable = !self.unprovable.contains(parent);
        self.checks
            .get(table)
            .into_iter()
            .flatten()
            .filter(move |check| provable && check.valid && check.not_null.is_none())
            .filter(move |check| {
                keys.is_none_or(|keys| {
                    keys.iter()
                        .any(|key| check.words.contains(&key.to_lowercase()))
                })
            })
    }

    /// Report the scan of `parent`'s default partition that a new partition of it makes.
    fn default_scan(&self, parent: &str, report: &mut impl FnMut(Rule, String)) {
        let Some(default) = self.defaults.get(parent) else {
            return;
        };
        if !self.created.contains(default) && self.bounds_checks(default, parent).next().is_none() {
            report(
                Rule::ConstraintScan,
                format!(
                    "a new partition of `{parent}` scans its default partition `{default}` for rows that belong to the new one, under an ACCESS EXCLUSIVE lock on it: move those rows out and add a validated `CHECK` to `{default}` that excludes the new bounds first"
                ),
            );
        }
    }

    /// Record the table of each index `stmt` creates, explicitly or for a key of a new table.
    fn record_indexes(&mut self, stmt: &Raw, parsed: Option<&Statement>) {
        let indexes = match parsed {
            Some(Statement::CreateIndex(index)) => index
                .name
                .iter()
                .map(|name| (key(name), key(&index.table_name)))
                .collect(),
            Some(Statement::CreateTable(create)) => implicit_indexes(create)
                .into_iter()
                .map(|name| (name, key(&create.name)))
                .collect(),
            Some(Statement::AlterTable(alter)) => {
                let table = bare_name(&alter.name);
                alter
                    .operations
                    .iter()
                    .flat_map(|op| match op {
                        AlterTableOperation::AddConstraint { constraint, .. } => {
                            constraint_index(&table, constraint).into_iter().collect()
                        }
                        AlterTableOperation::AddColumn { column_def, .. } => {
                            column_indexes(&table, column_def)
                        }
                        _ => Vec::new(),
                    })
                    .map(|name| (name, key(&alter.name)))
                    .collect()
            }
            Some(_) => Vec::new(),
            None => create_index(&stmt.tokens())
                .and_then(|index| Some((index.name?, index.table)))
                .into_iter()
                .collect(),
        };
        for (name, table) in indexes {
            let index = match table.rsplit_once('.') {
                Some((schema, _)) => format!("{schema}.{name}"),
                None => name,
            };
            self.index_tables.insert(index, table);
        }
    }

    /// Whether the `TABLE t` or `INDEX i` a `REINDEX` names was created by a new statement.
    fn is_new_target(&self, tokens: &[&str]) -> bool {
        let name = token_name(tokens.get(1..).unwrap_or_default(), &["concurrently"], &[]);
        match tokens.first().map(|kind| kind.to_lowercase()).as_deref() {
            Some("table") => self.created.contains(&name),
            Some("index") => self
                .index_tables
                .get(&name)
                .is_some_and(|table| self.created.contains(table)),
            _ => false,
        }
    }

    /// `CREATE INDEX` on `index.table`, written `written`.
    fn judge_index(
        &self,
        index: &NewIndex,
        written: &str,
        report: &mut impl FnMut(Rule, String),
    ) -> Judged {
        let new = self.created.contains(&index.table);
        if index.concurrently && index.if_not_exists && !new {
            report(
                Rule::ConcurrentIndexIfNotExists,
                format!(
                    "a failed `CREATE INDEX CONCURRENTLY` leaves an invalid index on `{written}`, and `IF NOT EXISTS` then skips building it again: `DROP INDEX CONCURRENTLY IF EXISTS` first and create it without `IF NOT EXISTS`"
                ),
            );
        }
        let waits = !index.concurrently && !new;
        // PostgreSQL builds no index on a partitioned table `CONCURRENTLY`: build one on each
        // partition, then on the parent, which `ON ONLY` leaves invalid and empty.
        if waits && !self.partition_keys.contains_key(&index.table) {
            report(
                Rule::MissingConcurrently,
                format!(
                    "`CREATE INDEX` blocks writes to `{written}` until the index is built: create it `CONCURRENTLY`"
                ),
            );
        }
        Judged {
            concurrently: index.concurrently,
            ..Judged::strong_unless(!waits)
        }
    }

    fn is_new(&self, name: &ObjectName) -> bool {
        self.created.contains(&key(name))
    }

    fn judge(&self, stmt: &Statement, report: &mut impl FnMut(Rule, String)) -> Judged {
        match stmt {
            Statement::CreateIndex(index) => self.judge_index(
                &NewIndex {
                    name: None,
                    table: key(&index.table_name),
                    concurrently: index.concurrently,
                    if_not_exists: index.if_not_exists,
                },
                &index.table_name.to_string(),
                report,
            ),
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
                let waits = !refresh.concurrently && !self.is_new(&refresh.name);
                if waits {
                    report(
                        Rule::MissingConcurrently,
                        format!(
                            "`REFRESH MATERIALIZED VIEW` blocks reads of `{}` until it finishes: refresh it `CONCURRENTLY`, which needs a unique index on the view",
                            refresh.name
                        ),
                    );
                }
                Judged {
                    concurrently: refresh.concurrently,
                    ..Judged::strong_unless(!waits)
                }
            }
            Statement::Vacuum(vacuum) if vacuum.full => {
                let rewrites = !vacuum.table_name.as_ref().is_some_and(|t| self.is_new(t));
                if rewrites {
                    report(Rule::TableRewrite, rewrite_command_message("VACUUM FULL"));
                }
                Judged::strong_unless(!rewrites)
            }
            Statement::Lock(lock) => Judged::strong_unless(
                !blocks_writes(lock.lock_mode.as_ref())
                    || lock.tables.iter().all(|target| self.is_new(&target.name)),
            ),
            Statement::CreateTable(create) => {
                if let (Some(parent), Some(bounds)) = (&create.partition_of, &create.for_values) {
                    if *bounds != ForValues::Default {
                        self.default_scan(&key(parent), report);
                    }
                }
                Judged::default()
            }
            Statement::Truncate(truncate) => {
                let created = truncate
                    .table_names
                    .iter()
                    .all(|target| self.is_new(&target.name));
                let referenced = truncate.table_names.iter().any(|target| {
                    let table = key(&target.name);
                    self.referenced.contains(&table) || !self.tables.contains(&table)
                });
                if !created && referenced && truncate.cascade == Some(CascadeOption::Cascade) {
                    let names: Vec<String> = truncate
                        .table_names
                        .iter()
                        .map(|target| format!("`{}`", target.name))
                        .collect();
                    report(
                        Rule::TruncateCascade,
                        format!(
                            "`TRUNCATE ... CASCADE` also empties every table with a foreign key to {}, and the tables that reference those: truncate the tables by name",
                            names.join(", ")
                        ),
                    );
                }
                Judged::strong_unless(created)
            }
            Statement::AlterTable(alter) => {
                if self.is_new(&alter.name) {
                    return Judged::default();
                }
                let table = key(&alter.name);
                for op in &alter.operations {
                    self.judge_op(&table, &alter.name, op, report);
                }
                let validates = alter.operations.iter().all(keeps_writes);
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
                let checked = self.checks.get(table).is_some_and(|checks| {
                    checks
                        .iter()
                        .any(|check| check.valid && check.not_null.as_ref() == Some(&column))
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
            AlterTableOperation::AttachPartitionOf {
                partition,
                for_values,
            } => {
                if !self.is_new(partition) && !self.proves_bounds(&key(partition), table) {
                    report(
                        Rule::ConstraintScan,
                        format!(
                            "`ATTACH PARTITION` scans `{partition}` for rows outside its bounds under an ACCESS EXCLUSIVE lock on it: `SET NOT NULL` on the partition key, add a `CHECK` that matches the bounds `NOT VALID` and `VALIDATE` it, then attach"
                        ),
                    );
                }
                if *for_values != ForValues::Default {
                    self.default_scan(table, report);
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
                TableConstraint::Exclude(_) => {
                    report(
                        Rule::ConstraintScan,
                        format!(
                            "the exclusion constraint builds its index over every row of `{written}` under an ACCESS EXCLUSIVE lock, and PostgreSQL has no `NOT VALID` or `CONCURRENTLY` for it: add it while the table is small"
                        ),
                    );
                }
                _ => {}
            },
            AlterTableOperation::SetLogged | AlterTableOperation::SetUnlogged => {
                report(
                    Rule::TableRewrite,
                    format!(
                        "`{op}` rewrites `{written}` under an ACCESS EXCLUSIVE lock: create the table that way, or copy the rows into a new one"
                    ),
                );
            }
            AlterTableOperation::DisableTrigger { name }
                if name.value.eq_ignore_ascii_case("all")
                    && (self.referencing.contains(table) || !self.tables.contains(table)) =>
            {
                report(
                    Rule::DisableTriggerAll,
                    format!(
                        "`DISABLE TRIGGER ALL` also turns off the foreign key checks of `{written}`: rows written meanwhile are never checked; disable `USER` triggers or name them"
                    ),
                );
            }
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
            ["create", "index" | "unique", ..] => match create_index(&stmt.tokens()) {
                Some(index) => self.judge_index(&index, &index.table, report),
                None => Judged::default(),
            },
            ["reindex", ..] => {
                let original = stmt.tokens();
                let (options, rest) = split_options(&original[1..]);
                let concurrently = reindexes_concurrently(options, rest);
                let waits = !concurrently && !self.is_new_target(rest);
                if waits {
                    report(
                        Rule::MissingConcurrently,
                        "`REINDEX` blocks writes to the table, and reads that use the index, until it finishes: reindex `CONCURRENTLY`".to_string(),
                    );
                }
                Judged {
                    concurrently,
                    ..Judged::strong_unless(!waits)
                }
            }
            ["cluster", ..] => {
                let rewrites = !self.created.contains(&cluster_table(&stmt.tokens()[1..]));
                if rewrites {
                    report(Rule::TableRewrite, rewrite_command_message("CLUSTER"));
                }
                Judged::strong_unless(!rewrites)
            }
            ["vacuum", ..] => {
                let original = stmt.tokens();
                let (options, rest) = split_options(&original[1..]);
                let (full, tables) = vacuum_full(options, rest);
                let rewrites = full
                    && !(!tables.is_empty() && tables.iter().all(|t| self.created.contains(t)));
                if rewrites {
                    report(Rule::TableRewrite, rewrite_command_message("VACUUM FULL"));
                }
                // SKIP_LOCKED skips a table it cannot lock at once instead of waiting.
                Judged::strong_unless(!rewrites || is_option_on(options, "skip_locked"))
            }
            ["alter", "table", rest @ ..]
                if rest.windows(2).any(|pair| pair == ["set", "tablespace"])
                    && !rest.starts_with(&["all", "in"]) =>
            {
                let table = set_tablespace_table(&stmt.tokens());
                let exempt = table.is_empty() || self.created.contains(&table);
                if !exempt {
                    report(
                        Rule::TableRewrite,
                        format!(
                            "`SET TABLESPACE` copies `{table}` under an ACCESS EXCLUSIVE lock, blocking reads and writes until it finishes: move it online with pg_repack, outside migrations"
                        ),
                    );
                }
                Judged::strong_unless(exempt)
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
            ["rollback", "to", ..] | ["release" | "savepoint", ..] => {}
            ["commit" | "end" | "rollback" | "abort", ..] => {
                (self.transaction, self.local_timeout) = (false, false);
                self.added_values.clear();
                self.locked.clear();
                (self.reported_tables, self.reported_backfill) = (false, false);
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
/// waits for a lock that blocks writes, SHARE or stronger, on a table that has rows.
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
    let found = visit_expressions(expr, |e| {
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
        (Some(Storage::Varchar(old)), Some(Storage::Varchar(new)))
        | (Some(Storage::Varbit(old)), Some(Storage::Varbit(new))) => {
            widens(old, new, |old, new| new >= old)
        }
        (Some(Storage::Numeric(old)), Some(Storage::Numeric(new))) => {
            widens(old, new, |(p, s), (q, t)| s == t && q >= p)
        }
        (Some(Storage::Cidr), Some(Storage::Inet)) => true,
        _ => false,
    }
}

/// Whether the type modifier `new` admits every value `old` does, by `admits`; no modifier
/// admits all.
fn widens<T>(old: Option<T>, new: Option<T>, admits: impl FnOnce(T, T) -> bool) -> bool {
    new.is_none_or(|new| old.is_some_and(|old| admits(old, new)))
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
        Some(match ty {
            DataType::Text => Storage::Varchar(None),
            DataType::Varchar(length) | DataType::CharacterVarying(length) => {
                Storage::Varchar(match length {
                    None => None,
                    Some(CharacterLength::IntegerLength { length, unit: None }) => Some(*length),
                    Some(_) => return None,
                })
            }
            DataType::VarBit(length) | DataType::BitVarying(length) => Storage::Varbit(*length),
            DataType::Numeric(info) | DataType::Decimal(info) => Storage::Numeric(match *info {
                ExactNumberInfo::None => None,
                ExactNumberInfo::Precision(p) => Some((p, 0)),
                ExactNumberInfo::PrecisionAndScale(p, s) => Some((p, u64::try_from(s).ok()?)),
            }),
            // `cidr`, `inet` and `text()`.
            DataType::Custom(name, modifiers) if modifiers.is_empty() => {
                match name.to_string().to_lowercase().as_str() {
                    "text" => Storage::Varchar(None),
                    "cidr" => Storage::Cidr,
                    "inet" => Storage::Inet,
                    _ => return None,
                }
            }
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
    let likes = likes.iter().map(|parts| unqualified(parts)).collect();
    Some((parsed, likes))
}

/// The empty table, materialized view or index `stmt` creates.
fn created(stmt: &Statement) -> Option<String> {
    match stmt {
        Statement::CreateTable(create)
            if create.query.is_none() || create.with_data.is_some_and(|with| !with.data) =>
        {
            Some(key(&create.name))
        }
        Statement::CreateView(view) if view.materialized => Some(key(&view.name)),
        Statement::CreateIndex(index) => index.name.as_ref().map(key),
        _ => None,
    }
}

/// The table `stmt` copies rows into from a query or a file: `INSERT ... VALUES` adds only the
/// rows it lists.
fn filled(stmt: &Statement) -> Option<String> {
    match stmt {
        Statement::Query(query) => match &*query.body {
            SetExpr::Insert(insert) => filled(insert),
            _ => None,
        },
        Statement::Merge(Merge {
            table: TableFactor::Table { name, .. },
            ..
        }) => Some(key(name)),
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

/// Whether `op` takes a lock weaker than `SHARE`, which lets writes go on.
fn keeps_writes(op: &AlterTableOperation) -> bool {
    match op {
        AlterTableOperation::SetOptionsParens { options } => options.iter().all(|option| {
            let option = option.to_string().to_lowercase();
            let parameter = option.split('=').next().unwrap_or_default().trim();
            parameter == "fillfactor"
                || parameter == "parallel_workers"
                || parameter.starts_with("autovacuum_")
                || parameter.starts_with("toast")
        }),
        _ => matches!(
            op,
            AlterTableOperation::ValidateConstraint { .. }
                | AlterTableOperation::DetachPartitionOf {
                    mode: Some(DetachPartitionMode::Concurrently),
                    ..
                }
        ),
    }
}

/// The column `expr` names, folded; any other expression as its lowercased text.
fn column_name(expr: &Expr) -> String {
    match expr {
        Expr::Identifier(ident) => name(ident),
        _ => expr.to_string().to_lowercase(),
    }
}

/// Whether a `LOCK TABLE` in `mode` blocks writes; without a mode it takes `ACCESS EXCLUSIVE`.
fn blocks_writes(mode: Option<&LockTableMode>) -> bool {
    !matches!(
        mode,
        Some(
            LockTableMode::AccessShare
                | LockTableMode::RowShare
                | LockTableMode::RowExclusive
                | LockTableMode::ShareUpdateExclusive
        )
    )
}

/// The terms `expr` joins with `AND`.
fn conjuncts(expr: &Expr) -> Box<dyn Iterator<Item = &Expr> + '_> {
    match expr {
        Expr::BinaryOp {
            left,
            op: BinaryOperator::And,
            right,
        } => Box::new(conjuncts(left).chain(conjuncts(right))),
        Expr::Nested(inner) => conjuncts(inner),
        _ => Box::new(std::iter::once(expr)),
    }
}

/// The table an `UPDATE` or `DELETE` changes rows of, and its alias.
struct Backfill {
    table: String,
    alias: Option<String>,
}

/// The table an `UPDATE` or `DELETE` changes rows of, with its `WHERE` clause.
fn backfilled(stmt: &Statement) -> Option<(Backfill, Option<&Expr>)> {
    let (relation, selection) = match stmt {
        Statement::Query(query) => {
            return match &*query.body {
                SetExpr::Update(stmt) | SetExpr::Delete(stmt) => backfilled(stmt),
                _ => None,
            }
        }
        Statement::Update(update) => (&update.table.relation, &update.selection),
        Statement::Delete(Delete {
            from: FromTable::WithFromKeyword(from) | FromTable::WithoutKeyword(from),
            selection,
            ..
        }) => (&from.first()?.relation, selection),
        _ => return None,
    };
    match relation {
        TableFactor::Table {
            name: table, alias, ..
        } => {
            let target = Backfill {
                table: key(table),
                alias: alias.as_ref().map(|alias| name(&alias.name)),
            };
            Some((target, selection.as_ref()))
        }
        _ => None,
    }
}

/// The tables on either side of the foreign keys `stmt` adds.
fn foreign_keys(stmt: &Statement) -> Vec<String> {
    let mut tables = referenced_tables(stmt);
    match stmt {
        Statement::CreateTable(create) if !tables.is_empty() => tables.push(key(&create.name)),
        Statement::AlterTable(alter) if !tables.is_empty() => tables.push(key(&alter.name)),
        _ => {}
    }
    tables
}

/// The tables the foreign keys `stmt` adds reference.
fn referenced_tables(stmt: &Statement) -> Vec<String> {
    let (columns, constraints): (Vec<&ColumnDef>, Vec<&TableConstraint>) = match stmt {
        Statement::CreateTable(create) => (
            create.columns.iter().collect(),
            create.constraints.iter().collect(),
        ),
        Statement::AlterTable(alter) => (
            alter
                .operations
                .iter()
                .filter_map(|op| match op {
                    AlterTableOperation::AddColumn { column_def, .. } => Some(column_def),
                    _ => None,
                })
                .collect(),
            alter
                .operations
                .iter()
                .filter_map(|op| match op {
                    AlterTableOperation::AddConstraint { constraint, .. } => Some(constraint),
                    _ => None,
                })
                .collect(),
        ),
        _ => return Vec::new(),
    };
    let inline = columns.into_iter().flat_map(|column| &column.options);
    inline
        .filter_map(|option| match &option.option {
            ColumnOption::ForeignKey(fk) => Some(key(&fk.foreign_table)),
            _ => None,
        })
        .chain(
            constraints
                .into_iter()
                .filter_map(|constraint| match constraint {
                    TableConstraint::ForeignKey(fk) => Some(key(&fk.foreign_table)),
                    _ => None,
                }),
        )
        .collect()
}

/// The lowercased words of `text`.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// The table name in `tokens` after the leading `skip` words and before `end`, folded and
/// without the `public` schema.
fn token_name(tokens: &[&str], skip: &[&str], ends: &[&str]) -> String {
    let table: String = tokens
        .iter()
        .skip_while(|t| skip.iter().any(|word| t.eq_ignore_ascii_case(word)))
        .take_while(|t| !ends.iter().any(|end| t.eq_ignore_ascii_case(end)))
        .map(|t| fold_token(t))
        .collect();
    match table.strip_prefix("public.") {
        Some(rest) => rest.to_string(),
        None => table,
    }
}

/// A `CREATE [UNIQUE] INDEX` read from its tokens: names folded, the table without `public`.
struct NewIndex {
    name: Option<String>,
    table: String,
    concurrently: bool,
    if_not_exists: bool,
}

/// `CREATE [UNIQUE] INDEX [CONCURRENTLY] [IF NOT EXISTS] [name] ON [ONLY] table ...`, for the
/// forms sqlparser does not read, such as `TABLESPACE` or `ON ONLY`.
fn create_index(tokens: &[&str]) -> Option<NewIndex> {
    let mut rest = tokens;
    let mut take = |word: &str| match rest.split_first() {
        Some((first, after)) if first.eq_ignore_ascii_case(word) => {
            rest = after;
            true
        }
        _ => false,
    };
    if !take("create") || !(take("index") || take("unique") && take("index")) {
        return None;
    }
    let concurrently = take("concurrently");
    let if_not_exists = take("if") && take("not") && take("exists");
    let on = rest.iter().position(|t| t.eq_ignore_ascii_case("on"))?;
    let table = token_name(&rest[on + 1..], &["only"], &["using", "("]);
    Some(NewIndex {
        name: (on > 0).then(|| token_name(&rest[..on], &[], &[])),
        concurrently,
        if_not_exists,
        table: Some(table).filter(|table| !table.is_empty())?,
    })
}

/// The index or materialized view a statement sqlparser does not read creates, such as
/// `CREATE MATERIALIZED VIEW m AS ... WITH NO DATA`.
fn created_by_tokens(tokens: &[&str]) -> Option<String> {
    let lower: Vec<String> = tokens.iter().take(3).map(|t| t.to_lowercase()).collect();
    match lower
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["create", "materialized", "view"] => Some(token_name(
            &tokens[3..],
            &["if", "not", "exists"],
            &["as", "(", "using", "with", "tablespace"],
        ))
        .filter(|name| !name.is_empty()),
        _ => create_index(tokens).and_then(|index| index.name),
    }
}

/// The indexes `CREATE TABLE` makes for its primary key and unique constraints, by the names
/// PostgreSQL gives them: the constraint name, else `t_pkey` and `t_a_b_key`. A default name
/// over 63 bytes, which PostgreSQL truncates, or of an expression, is left out.
fn implicit_indexes(create: &CreateTable) -> Vec<String> {
    let table = bare_name(&create.name);
    let mut found: Vec<String> = create
        .columns
        .iter()
        .flat_map(|column| column_indexes(&table, column))
        .collect();
    found.extend(
        create
            .constraints
            .iter()
            .filter_map(|constraint| constraint_index(&table, constraint)),
    );
    found
}

/// The last part of `name`, folded: the table name PostgreSQL derives index names from.
fn bare_name(name: &ObjectName) -> String {
    name.0
        .last()
        .and_then(|part| part.as_ident())
        .map(self::name)
        .unwrap_or_default()
}

/// `constraint`, or `default` when it fits PostgreSQL's 63 bytes.
fn index_name(constraint: Option<&sqlparser::ast::Ident>, default: String) -> Option<String> {
    constraint
        .map(name)
        .or((default.len() <= 63).then_some(default))
}

/// The indexes of the `PRIMARY KEY` and `UNIQUE` options of `column` of `table`.
fn column_indexes(table: &str, column: &ColumnDef) -> Vec<String> {
    column
        .options
        .iter()
        .filter_map(|option| match &option.option {
            ColumnOption::PrimaryKey(pk) => index_name(
                option.name.as_ref().or(pk.name.as_ref()),
                format!("{table}_pkey"),
            ),
            ColumnOption::Unique(unique) => index_name(
                option.name.as_ref().or(unique.name.as_ref()),
                format!("{table}_{}_key", name(&column.name)),
            ),
            _ => None,
        })
        .collect()
}

/// The index of a `PRIMARY KEY` or `UNIQUE` table constraint of `table`.
fn constraint_index(table: &str, constraint: &TableConstraint) -> Option<String> {
    match constraint {
        TableConstraint::PrimaryKey(pk) => index_name(pk.name.as_ref(), format!("{table}_pkey")),
        TableConstraint::Unique(unique) => match &unique.name {
            Some(constraint) => Some(name(constraint)),
            None => {
                let columns = unique
                    .columns
                    .iter()
                    .map(|column| match &column.column.expr {
                        Expr::Identifier(ident) => Some(name(ident)),
                        _ => None,
                    })
                    .collect::<Option<Vec<_>>>()?;
                index_name(None, format!("{table}_{}_key", columns.join("_")))
            }
        },
        _ => None,
    }
}

/// Whether a `REINDEX` with `options` and the `rest` of its tokens runs `CONCURRENTLY`.
fn reindexes_concurrently(options: &[&str], rest: &[&str]) -> bool {
    rest.iter().any(|t| t.eq_ignore_ascii_case("concurrently"))
        || is_option_on(options, "concurrently")
}

/// Whether a `VACUUM` with `options` and the `rest` of its tokens is `FULL`, and its tables.
fn vacuum_full(options: &[&str], rest: &[&str]) -> (bool, Vec<String>) {
    let legacy = rest.iter().take_while(|t| is_vacuum_word(t)).count();
    let full = rest[..legacy]
        .iter()
        .any(|t| t.eq_ignore_ascii_case("full"))
        || is_option_on(options, "full");
    (full, vacuum_tables(&rest[legacy..]))
}

/// The table of `ALTER TABLE [IF EXISTS] [ONLY] t ... SET TABLESPACE`.
fn set_tablespace_table(tokens: &[&str]) -> String {
    let skip = ["if", "exists", "only"];
    let ends = ["set", "add", "alter", "drop", "rename", "owner", ","];
    token_name(tokens.get(2..).unwrap_or_default(), &skip, &ends)
}

/// `tokens` after a leading parenthesised option list, and the options inside it.
fn split_options<'a>(tokens: &'a [&'a str]) -> (&'a [&'a str], &'a [&'a str]) {
    match (tokens.first(), tokens.iter().position(|t| *t == ")")) {
        (Some(&"("), Some(close)) => (&tokens[1..close], &tokens[close + 1..]),
        _ => (&[], tokens),
    }
}

/// Whether a parenthesised option list turns `name` on: `name`, or `name` and a value other
/// than `false`, `off` or `0`, quoted or not, in any case.
fn is_option_on(options: &[&str], name: &str) -> bool {
    options.iter().enumerate().any(|(at, option)| {
        option.eq_ignore_ascii_case(name)
            && !options.get(at + 1).is_some_and(|value| {
                let value = value.trim_matches(['\'', '"']);
                ["false", "off", "0"]
                    .iter()
                    .any(|off| value.eq_ignore_ascii_case(off))
            })
    })
}

fn is_vacuum_word(token: &str) -> bool {
    ["full", "freeze", "verbose", "analyze", "analyse"]
        .iter()
        .any(|word| token.eq_ignore_ascii_case(word))
}

/// The folded tables of a `VACUUM` list, without their column lists.
fn vacuum_tables(tokens: &[&str]) -> Vec<String> {
    let (mut names, mut current, mut depth) = (Vec::new(), Vec::new(), 0_usize);
    for token in tokens {
        match *token {
            "(" => depth += 1,
            ")" => depth = depth.saturating_sub(1),
            "," if depth == 0 => names.push(token_name(&std::mem::take(&mut current), &[], &[])),
            _ if depth == 0 => current.push(*token),
            _ => {}
        }
    }
    if !current.is_empty() {
        names.push(token_name(&current, &[], &[]));
    }
    names
}

/// The table a `CLUSTER` names: `CLUSTER [(options)] t [USING i]`, or `CLUSTER i ON t`.
fn cluster_table(tokens: &[&str]) -> String {
    let (_, rest) = split_options(tokens);
    match rest.iter().position(|t| t.eq_ignore_ascii_case("on")) {
        Some(on) => token_name(&rest[on + 1..], &[], &[]),
        None => token_name(rest, &["verbose"], &["using"]),
    }
}

/// `token` as PostgreSQL folds an identifier: quoted keeps its case.
fn fold_token(token: &str) -> String {
    match token.strip_prefix('"') {
        Some(quoted) => quoted.trim_end_matches('"').to_string(),
        None => token.to_lowercase(),
    }
}

/// `name` as PostgreSQL folds it, without the `public` schema.
fn key(name: &ObjectName) -> String {
    let parts: Vec<String> = name
        .0
        .iter()
        .filter_map(|part| part.as_ident())
        .map(|ident| fold(ident, SqlDialect::Postgres))
        .collect();
    unqualified(&parts)
}

/// The folded name `parts` joined, without the `public` schema.
fn unqualified(parts: &[String]) -> String {
    match parts {
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
            ("ALTER TABLE users ALTER COLUMN email TYPE text()", &[]),
            ("ALTER TABLE users ALTER COLUMN email TYPE varchar(200)", &[]),
            ("ALTER TABLE users ALTER COLUMN email TYPE varchar(50)", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN email TYPE varchar(MAX)", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN email TYPE varchar(200 CHARACTERS)", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN bio TYPE varchar", &[]),
            ("ALTER TABLE users ALTER COLUMN bio TYPE varchar(10)", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN score TYPE numeric(7, 2)", &[]),
            ("ALTER TABLE users ALTER COLUMN score TYPE numeric(7, 3)", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN score TYPE numeric(7, -2)", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN score TYPE numeric", &[]),
            ("ALTER TABLE users ALTER COLUMN whole TYPE numeric(9)", &[]),
            ("ALTER TABLE users ALTER COLUMN whole TYPE numeric(9, 2)", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN free TYPE numeric(9)", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN net TYPE inet", &[]),
            ("ALTER TABLE users ALTER COLUMN net TYPE macaddr", &[TableRewrite]),
            ("ALTER TABLE users ALTER COLUMN net TYPE \"inet\"", &[TableRewrite]),
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
            ("ALTER TABLE users SET LOGGED", &[TableRewrite]),
            ("ALTER TABLE users SET UNLOGGED", &[TableRewrite]),
            ("ALTER TABLE users SET TABLESPACE fast", &[TableRewrite]),
            ("ALTER TABLE IF EXISTS ONLY public.users SET TABLESPACE fast", &[TableRewrite]),
            ("ALTER TABLE users ADD CONSTRAINT no_overlap EXCLUDE USING gist (team_id WITH =)", &[ConstraintScan]),
            ("ALTER TABLE users DISABLE TRIGGER ALL", &[]),
            ("ALTER TABLE orders DISABLE TRIGGER ALL", &[DisableTriggerAll]),
            ("ALTER TABLE users DISABLE TRIGGER USER", &[]),
            ("ALTER TABLE users DISABLE TRIGGER audit", &[]),
            ("CREATE INDEX CONCURRENTLY IF NOT EXISTS users_bio ON users (bio)", &[ConcurrentIndexIfNotExists]),
            ("CREATE INDEX IF NOT EXISTS users_bio ON users (bio)", &[MissingConcurrently]),
            ("TRUNCATE users CASCADE", &[]),
            ("TRUNCATE orders CASCADE", &[TruncateCascade]),
            ("TRUNCATE TABLE users, teams", &[]),
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
        assert_eq!(
            rules(&baseline, layout),
            [Rule::MissingConcurrently, Rule::MissingLockTimeout]
        );
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
        assert_eq!(
            rules(USERS, quoted),
            [Rule::TableRewrite, Rule::MissingLockTimeout]
        );
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
            (
                "WITH s AS (SELECT id FROM users) INSERT INTO t SELECT id FROM s",
                &[MissingConcurrently],
            ),
            (
                "MERGE INTO t USING users u ON t.id = u.id WHEN NOT MATCHED THEN INSERT VALUES (u.id)",
                &[MissingConcurrently],
            ),
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
        assert_eq!(
            rules(USERS, created_as),
            [MissingConcurrently, MissingLockTimeout]
        );
        let no_data = created_as.replace(" FROM users", " FROM users WITH NO DATA");
        assert_eq!(rules(USERS, &no_data), []);
    }

    #[test]
    fn new_tables_are_exempt_from_the_command_rules() {
        let migration = "CREATE TABLE t (id int);
            ALTER TABLE t SET TABLESPACE fast;
            ALTER TABLE t DISABLE TRIGGER ALL;
            CREATE INDEX CONCURRENTLY IF NOT EXISTS t_id ON t (id);
            TRUNCATE t CASCADE;";
        assert_eq!(rules(USERS, migration), []);
    }

    #[test]
    fn truncate_takes_a_strong_lock() {
        assert_eq!(rules(USERS, "TRUNCATE users"), [Rule::MissingLockTimeout]);
    }

    #[test]
    fn builds_and_rewrites_wait_for_a_lock_that_blocks_writes() {
        let baseline = format!(
            "{USERS}\nCREATE MATERIALIZED VIEW mv AS SELECT 1;\nCREATE TABLE events (at timestamptz) PARTITION BY RANGE (at);"
        );
        for (sql, rule) in [
            (
                "CREATE INDEX users_bio ON users (bio)",
                Rule::MissingConcurrently,
            ),
            ("REINDEX TABLE users", Rule::MissingConcurrently),
            (
                "CREATE INDEX users_e ON users (email) TABLESPACE fast",
                Rule::MissingConcurrently,
            ),
            (
                "CREATE INDEX users_e ON users USING gist (box(email) range_ops (siglen = 12))",
                Rule::MissingConcurrently,
            ),
            (
                "CREATE UNIQUE INDEX users_e ON users (email) WITH (deduplicate_items = off) TABLESPACE x WHERE id > 0",
                Rule::MissingConcurrently,
            ),
            ("REFRESH MATERIALIZED VIEW mv", Rule::MissingConcurrently),
            ("VACUUM FULL users", Rule::TableRewrite),
            ("CLUSTER users", Rule::TableRewrite),
        ] {
            assert_eq!(
                rules(&baseline, sql),
                [rule, Rule::MissingLockTimeout],
                "{sql}"
            );
            let set = format!("SET lock_timeout = '1s'; {sql}");
            assert_eq!(rules(&baseline, &set), [rule], "{sql}");
        }
        for sql in [
            "CREATE INDEX CONCURRENTLY users_bio ON users (bio)",
            "REINDEX TABLE CONCURRENTLY users",
            "REINDEX (CONCURRENTLY, VERBOSE) TABLE users",
            "REFRESH MATERIALIZED VIEW CONCURRENTLY mv",
            "CREATE TABLE t (id int); CREATE INDEX t_id ON t (id); CLUSTER t; VACUUM FULL t",
            "CREATE TABLE t (id int); CREATE INDEX ON t (id); REINDEX TABLE t",
            "CREATE TABLE t (id int); CREATE INDEX t_id ON t (id); REINDEX INDEX t_id",
            "CREATE TABLE s.t (id int); CREATE INDEX t_id ON s.t (id); REINDEX INDEX s.t_id",
            "CREATE TABLE t (id int); REINDEX (VERBOSE) TABLE t",
            "CREATE TABLE t (id int); CLUSTER (VERBOSE) t",
            "CREATE TABLE t (id int); CREATE INDEX i ON t (id); CLUSTER i ON t",
            "CREATE TABLE t (id int); CLUSTER VERBOSE t USING t_pkey",
            "CREATE TABLE t (id int); VACUUM (FULL) t",
            "CREATE TABLE t (id int); VACUUM (FULL, ANALYZE) public.t (id)",
            "CREATE TABLE t (id int); VACUUM FULL t, t",
            "CREATE INDEX CONCURRENTLY users_e ON users (email) TABLESPACE x",
            "CREATE TABLE t (id int); CREATE INDEX t_id ON t (id) TABLESPACE x; REINDEX INDEX t_id",
            "CREATE TABLE s.t (id int); CREATE INDEX ON s.t (id) TABLESPACE x; REINDEX TABLE s.t",
            "CREATE MATERIALIZED VIEW m2 AS SELECT 1 AS x WITH NO DATA;
             CREATE UNIQUE INDEX m2_i ON m2 (x); REFRESH MATERIALIZED VIEW m2",
            "CREATE TABLE t (id int PRIMARY KEY, a int UNIQUE, b int, c int,
                 CONSTRAINT named_u UNIQUE (b, c), UNIQUE (c, b));
             REINDEX INDEX t_pkey; REINDEX INDEX t_a_key; REINDEX INDEX named_u;
             REINDEX INDEX t_c_b_key",
            "CREATE TABLE s.t (id int CONSTRAINT my_pk PRIMARY KEY, a int CONSTRAINT my_u UNIQUE);
             REINDEX INDEX s.my_pk; REINDEX INDEX s.my_u",
            "CREATE TABLE t (id int, PRIMARY KEY (id)); REINDEX INDEX t_pkey",
        ] {
            assert_eq!(rules(&baseline, sql), [], "{sql}");
        }
        for sql in [
            "REINDEX (VERBOSE) TABLE users",
            "REINDEX (CONCURRENTLY false) TABLE users",
            "REINDEX INDEX users_pkey",
            "REINDEX SCHEMA public",
            "CREATE TABLE s.t (id int); CREATE INDEX t_id ON s.t (id); REINDEX INDEX t_id",
            "CREATE INDEX users_email ON users (email); REINDEX INDEX users_email",
            "CLUSTER (VERBOSE) users",
            "CLUSTER users_pkey ON users",
            "CLUSTER",
            "CREATE TABLE t (id int PRIMARY KEY); REINDEX INDEX t_id_key",
            "CREATE TABLE t (id int, CHECK (id > 0), UNIQUE (id, (id + 1))); REINDEX INDEX t_id_key",
        ] {
            let rule = if sql.contains("CLUSTER") {
                Rule::TableRewrite
            } else {
                Rule::MissingConcurrently
            };
            assert_eq!(
                rules(&baseline, sql).last(),
                Some(&Rule::MissingLockTimeout),
                "{sql}"
            );
            assert!(rules(&baseline, sql).contains(&rule), "{sql}");
        }
        for sql in [
            "VACUUM (FULL) users",
            "VACUUM (FULL, ANALYZE) users",
            "VACUUM (FULL true) users (id)",
            "VACUUM FULL users, mv",
            "VACUUM FULL ANALYZE users",
            "VACUUM (FULL)",
            "CREATE TABLE t (id int); VACUUM FULL t, users",
            "VACUUM FULL",
        ] {
            assert_eq!(
                rules(&baseline, sql),
                [Rule::TableRewrite, Rule::MissingLockTimeout],
                "{sql}"
            );
        }
        for sql in [
            "VACUUM users",
            "VACUUM (FULL false) users",
            "VACUUM (FULL 'off') users",
            "VACUUM (FULL \"OFF\") users",
            "VACUUM (ANALYZE) users",
            "VACUUM",
        ] {
            assert_eq!(rules(&baseline, sql), [], "{sql}");
        }
        assert_eq!(
            rules(&baseline, "VACUUM (FULL, SKIP_LOCKED) users"),
            [Rule::TableRewrite]
        );
        assert_eq!(
            rules(&baseline, "VACUUM (FULL, SKIP_LOCKED off) users"),
            [Rule::TableRewrite, Rule::MissingLockTimeout]
        );
        let retry = "SET lock_timeout = '1s'; CREATE INDEX CONCURRENTLY IF NOT EXISTS users_e ON users (email) TABLESPACE x";
        assert_eq!(rules(&baseline, retry), [Rule::ConcurrentIndexIfNotExists]);
        for sql in [
            "CREATE INDEX",
            "CREATE UNIQUE TABLE t (id int)",
            "CREATE MATERIALIZED VIEW",
        ] {
            assert_eq!(rules(&baseline, sql), [], "{sql}");
        }
        let twice = "CREATE INDEX users_bio ON users (bio); REINDEX TABLE users; CLUSTER users";
        assert_eq!(
            rules(&baseline, twice),
            [
                Rule::MissingConcurrently,
                Rule::MissingLockTimeout,
                Rule::MissingConcurrently,
                Rule::TableRewrite
            ]
        );
    }

    #[test]
    fn an_enum_value_is_used_only_after_its_transaction_commits() {
        let baseline = format!(
            "{USERS}\nCREATE TYPE status AS ENUM ('new');
            CREATE TABLE orders (id BIGINT, state status, note TEXT);"
        );
        let migration = |between: &str, using: &str| {
            format!(
                "SET lock_timeout = '1s';
                BEGIN;
                ALTER TYPE status ADD VALUE IF NOT EXISTS 'It''s';
                {between}
                {using};
                {using};
                COMMIT;"
            )
        };
        let used = "UPDATE orders SET state = 'It''s' WHERE id = 1";
        let enum_findings = |migration: &str| -> Vec<Finding> {
            check(&baseline, &[migration])
                .remove(0)
                .into_iter()
                .filter(|finding| finding.rule == Rule::EnumValueInTransaction)
                .collect()
        };
        assert_eq!(
            enum_findings(&migration("", used)),
            [Finding {
                rule: Rule::EnumValueInTransaction,
                message: "'It''s' was added to `status` in this transaction: PostgreSQL refuses a new enum value until the transaction that added it commits, so `COMMIT` after `ADD VALUE`".to_string(),
                span: 148..194,
            }]
        );
        let reported =
            |between: &str, using: &str| !enum_findings(&migration(between, using)).is_empty();
        assert!(reported("", "SELECT 'It''s'::status"));
        assert!(reported("", "SELECT CAST('It''s' AS status)"));
        assert!(reported("", "INSERT INTO orders VALUES (1, 'It''s', NULL)"));
        assert!(reported("", "INSERT INTO orders SELECT 1, 'It''s'"));
        assert!(!reported("", "UPDATE \"orders\" SET note = 'It''s'"));
        assert!(!reported(
            "",
            "INSERT INTO orders (id, note) VALUES (1, 'It''s')"
        ));
        assert!(reported("", "UPDATE elsewhere SET state = 'It''s'"));
        assert!(reported(
            "SAVEPOINT s; ROLLBACK TO SAVEPOINT s; RELEASE s;",
            used
        ));
        assert!(!reported("COMMIT; BEGIN;", used));
        assert!(!reported("", "UPDATE orders SET note = 'It''s'"));
        assert!(!reported("", "UPDATE orders SET state = 'it''s'"));
        assert!(!reported(
            "",
            "ALTER TYPE status ADD VALUE 'y' AFTER 'It''s'"
        ));
        let outside = "ALTER TYPE status ADD VALUE 'x'; UPDATE orders SET state = 'x';";
        assert_eq!(enum_findings(outside), []);
    }

    #[test]
    fn foreign_keys_decide_what_trigger_and_cascade_reach() {
        let baseline = format!(
            "{USERS}\nCREATE TABLE posts (id BIGINT, user_id BIGINT REFERENCES users (id));
            CREATE TABLE tags (id BIGINT);
            CREATE TABLE notes (id BIGINT);
            ALTER TABLE notes ADD CONSTRAINT fk FOREIGN KEY (id) REFERENCES tags (id) NOT VALID;"
        );
        for (migration, expected) in [
            ("ALTER TABLE posts DISABLE TRIGGER ALL", &[Rule::DisableTriggerAll][..]),
            ("ALTER TABLE users DISABLE TRIGGER ALL", &[Rule::DisableTriggerAll]),
            ("ALTER TABLE notes DISABLE TRIGGER ALL", &[Rule::DisableTriggerAll]),
            ("ALTER TABLE teams DISABLE TRIGGER ALL", &[]),
            ("TRUNCATE tags CASCADE", &[Rule::TruncateCascade]),
            ("TRUNCATE teams CASCADE", &[]),
            ("TRUNCATE posts CASCADE", &[]),
            (
                "ALTER TABLE teams ADD COLUMN owner BIGINT REFERENCES users (id); TRUNCATE teams CASCADE",
                &[],
            ),
            (
                "ALTER TABLE users ADD COLUMN team BIGINT REFERENCES teams (id); TRUNCATE teams CASCADE",
                &[Rule::TruncateCascade],
            ),
        ] {
            let migration = format!("SET lock_timeout = '1s'; {migration};");
            assert_eq!(rules(&baseline, &migration), expected, "{migration}");
        }
    }

    #[test]
    fn set_tablespace_names_one_table() {
        for (migration, expected) in [
            (
                "ALTER TABLE users SET TABLESPACE fast NOWAIT",
                &[Rule::TableRewrite][..],
            ),
            (
                "ALTER TABLE ALL IN TABLESPACE pg_default SET TABLESPACE fast",
                &[],
            ),
        ] {
            assert_eq!(one(migration), expected, "{migration}");
        }
    }

    #[test]
    fn attaching_a_partition_scans_it_without_a_validated_check() {
        let baseline = "CREATE TABLE events (at date, price int) PARTITION BY RANGE (at);
            CREATE TABLE open (at date, price int CHECK (price > 0));
            CREATE TABLE bounded (at date NOT NULL, price int, CHECK (at >= '2026-10-01'));
            CREATE TABLE nullable (at date, CHECK (at >= '2026-10-01'));
            CREATE TABLE proven (at date CHECK (at IS NOT NULL), CHECK (at >= '2026-10-01'));
            CREATE TABLE bare (at date);
            CREATE TABLE keyed (at date, PRIMARY KEY (at), CHECK (at >= '2026-10-01'));";
        let bounds = "FOR VALUES FROM ('2026-10-01') TO ('2026-11-01')";
        let validated = "ALTER TABLE open ADD CONSTRAINT b CHECK (at >= '2026-10-01') NOT VALID;
            ALTER TABLE open VALIDATE CONSTRAINT b;";
        for (setup, table, expected) in [
            ("", "open", &[Rule::ConstraintScan][..]),
            ("", "bounded", &[]),
            ("", "nullable", &[Rule::ConstraintScan]),
            (validated, "open", &[Rule::ConstraintScan]),
            (
                &format!("{validated} ALTER TABLE open ALTER COLUMN at SET NOT NULL;"),
                "open",
                &[Rule::ConstraintScan],
            ),
            ("", "proven", &[]),
            ("ALTER TABLE keyed DROP CONSTRAINT gone;", "keyed", &[]),
            ("ALTER TABLE bare DROP CONSTRAINT gone;", "bare", &[Rule::ConstraintScan]),
            (
                "ALTER TABLE bare ADD CONSTRAINT nn CHECK (at IS NOT NULL) NOT VALID; ALTER TABLE bare VALIDATE CONSTRAINT nn;",
                "bare",
                &[Rule::ConstraintScan],
            ),
            ("ALTER TABLE bounded ALTER COLUMN at DROP NOT NULL;", "bounded", &[Rule::ConstraintScan]),
            ("CREATE TABLE fresh (at date);", "fresh", &[]),
            (
                "CREATE TABLE fresh (LIKE open); INSERT INTO fresh SELECT * FROM open;",
                "fresh",
                &[Rule::ConstraintScan],
            ),
        ] {
            let migration = format!(
                "SET lock_timeout = '1s';\n{setup}\nALTER TABLE events ATTACH PARTITION {table} {bounds};"
            );
            assert_eq!(rules(baseline, &migration), expected, "{setup} {table}");
        }
        let named = "SET lock_timeout = '1s';
            ALTER TABLE bounded DROP CONSTRAINT b;
            ALTER TABLE events ATTACH PARTITION bounded FOR VALUES FROM ('2026-10-01') TO ('2026-11-01');";
        let named_baseline = baseline.replace(
            "CHECK (at >= '2026-10-01'));",
            "CONSTRAINT b CHECK (at >= '2026-10-01'));",
        );
        assert_eq!(rules(&named_baseline, named), [Rule::ConstraintScan]);
        let unknown_key = "SET lock_timeout = '1s';
            ALTER TABLE logs ATTACH PARTITION bounded FOR VALUES FROM ('2026-10-01') TO ('2026-11-01');";
        assert_eq!(rules(baseline, unknown_key), []);
    }

    #[test]
    fn a_new_partition_scans_the_default_partition() {
        let baseline = "CREATE TABLE events (at date NOT NULL) PARTITION BY RANGE (at);
            CREATE TABLE events_rest PARTITION OF events DEFAULT;
            CREATE TABLE logs (at date NOT NULL) PARTITION BY RANGE (at);
            CREATE TABLE logs_rest (at date NOT NULL);
            ALTER TABLE logs ATTACH PARTITION logs_rest DEFAULT;";
        for (migration, expected) in [
            ("CREATE TABLE events_10 PARTITION OF events FOR VALUES FROM ('2026-10-01') TO ('2026-11-01')", &[Rule::ConstraintScan][..]),
            ("CREATE TABLE logs_10 (at date NOT NULL); ALTER TABLE logs ATTACH PARTITION logs_10 FOR VALUES FROM ('2026-10-01') TO ('2026-11-01')", &[Rule::ConstraintScan]),
            ("CREATE TABLE events_other PARTITION OF events DEFAULT", &[]),
            (
                "ALTER TABLE events_rest ADD CONSTRAINT x CHECK (NOT (at >= '2026-10-01' AND at < '2026-11-01')); CREATE TABLE events_10 PARTITION OF events FOR VALUES FROM ('2026-10-01') TO ('2026-11-01')",
                &[Rule::ConstraintScan],
            ),
            (
                "DROP TABLE events_rest; CREATE TABLE events_10 PARTITION OF events FOR VALUES FROM ('2026-10-01') TO ('2026-11-01')",
                &[],
            ),
            (
                "ALTER TABLE logs DETACH PARTITION logs_rest; CREATE TABLE logs_10 PARTITION OF logs FOR VALUES FROM ('2026-10-01') TO ('2026-11-01')",
                &[Rule::MissingConcurrently],
            ),
        ] {
            let migration = format!("SET lock_timeout = '1s'; {migration};");
            assert_eq!(rules(baseline, &migration), expected, "{migration}");
        }
    }

    #[test]
    fn hash_and_expression_keys_are_never_proven() {
        let baseline = "CREATE TABLE h (id int) PARTITION BY HASH (id);
            CREATE TABLE h0 (id int NOT NULL, CHECK (id > 0));
            CREATE TABLE e (name text) PARTITION BY RANGE (lower(name));
            CREATE TABLE e0 (name text NOT NULL, CHECK (lower(name) >= 'a'));
            CREATE TABLE q (\"At\" date) PARTITION BY RANGE (\"At\");
            CREATE TABLE q0 (\"At\" date NOT NULL, CHECK (\"At\" >= '2026-10-01'));";
        for (migration, expected) in [
            ("ALTER TABLE h ATTACH PARTITION h0 FOR VALUES WITH (MODULUS 4, REMAINDER 0)", &[Rule::ConstraintScan][..]),
            ("ALTER TABLE e ATTACH PARTITION e0 FOR VALUES FROM ('a') TO ('m')", &[Rule::ConstraintScan]),
            ("ALTER TABLE q ATTACH PARTITION q0 FOR VALUES FROM ('2026-10-01') TO ('2026-11-01')", &[]),
        ] {
            let migration = format!("SET lock_timeout = '1s'; {migration};");
            assert_eq!(rules(baseline, &migration), expected, "{migration}");
        }
    }

    #[test]
    fn a_transaction_holds_its_locks_until_it_ends() {
        use Rule::*;
        for (migration, expected) in [
            (
                "BEGIN; ALTER TABLE users ADD COLUMN a int; ALTER TABLE teams ADD COLUMN b int; ALTER TABLE users ADD COLUMN c int; COMMIT;",
                &[MultiTableLock][..],
            ),
            (
                "BEGIN; ALTER TABLE users ADD COLUMN a int; COMMIT; BEGIN; ALTER TABLE teams ADD COLUMN b int; COMMIT;",
                &[],
            ),
            ("ALTER TABLE users ADD COLUMN a int; ALTER TABLE teams ADD COLUMN b int;", &[]),
            (
                "BEGIN; LOCK TABLE users, teams; COMMIT;",
                &[],
            ),
            (
                "BEGIN; LOCK TABLE users; LOCK TABLE teams; COMMIT;",
                &[MultiTableLock],
            ),
            (
                "BEGIN; TRUNCATE users; DROP TABLE teams; COMMIT;",
                &[MultiTableLock],
            ),
            (
                "BEGIN; CREATE TABLE t (id int); ALTER TABLE t ADD COLUMN a int; ALTER TABLE users ADD COLUMN a int; COMMIT;",
                &[],
            ),
            (
                "BEGIN; ALTER TABLE users ADD COLUMN a int; UPDATE users SET a = 1 WHERE id IN (SELECT id FROM users LIMIT 100); COMMIT;",
                &[LockedBackfill],
            ),
            (
                "BEGIN; ALTER TABLE users ADD COLUMN a int; DELETE FROM users WHERE id BETWEEN 1 AND 100; COMMIT;",
                &[LockedBackfill],
            ),
            (
                "BEGIN; ALTER TABLE teams ADD COLUMN a int; UPDATE users SET bio = '' WHERE id BETWEEN 1 AND 9; COMMIT;",
                &[],
            ),
            ("UPDATE users SET bio = ''", &[UnbatchedBackfill]),
            ("DELETE FROM users WHERE bio IS NULL", &[UnbatchedBackfill]),
            ("UPDATE users SET bio = '' WHERE id BETWEEN 1 AND 9", &[]),
            ("CREATE TABLE t (id int); UPDATE t SET id = 1; DELETE FROM t", &[]),
            ("UPDATE users AS u SET bio = t.id::text FROM teams t", &[UnbatchedBackfill]),
            ("UPDATE users SET bio = 'x' WHERE id = 1", &[]),
            ("DELETE FROM users WHERE id IN (1, 2, 3)", &[]),
            ("DELETE FROM users WHERE team_id IN (1, 2, 3)", &[UnbatchedBackfill]),
            ("UPDATE users SET bio = (SELECT 'x' FROM teams LIMIT 1)", &[UnbatchedBackfill]),
            ("WITH t AS (SELECT 1) UPDATE users SET bio = ''", &[UnbatchedBackfill]),
            (
                "CREATE TEMP TABLE tmp (id int); INSERT INTO tmp SELECT id FROM users; UPDATE tmp SET id = id + 1",
                &[],
            ),
            (
                "BEGIN; CREATE TABLE t (id int); LOCK TABLE t; ALTER TABLE users ADD COLUMN a int; COMMIT;",
                &[],
            ),
            ("BEGIN; CREATE TABLE t (id int); DROP TABLE t, teams; COMMIT;", &[]),
            (
                "BEGIN; LOCK TABLE users IN ACCESS SHARE MODE; LOCK TABLE teams IN ROW EXCLUSIVE MODE; COMMIT;",
                &[],
            ),
            (
                "BEGIN; ALTER TABLE users SET (fillfactor = 70); ALTER TABLE teams SET (fillfactor = 70); COMMIT;",
                &[],
            ),
            (
                "BEGIN; ALTER TABLE users ADD FOREIGN KEY (team_id) REFERENCES teams (id) NOT VALID; COMMIT;",
                &[],
            ),
            (
                "BEGIN; ALTER TABLE users ADD COLUMN t2 BIGINT REFERENCES teams (id); UPDATE teams SET id = 1 WHERE id BETWEEN 1 AND 2; COMMIT;",
                &[LockedBackfill],
            ),
            (
                "BEGIN; CREATE TABLE child (id int, team_id BIGINT REFERENCES teams (id)); ALTER TABLE users ADD COLUMN z int; COMMIT;",
                &[MultiTableLock],
            ),
            ("UPDATE users SET bio = '' WHERE id = 1 OR bio IS NULL", &[UnbatchedBackfill]),
            ("DELETE FROM users WHERE NOT (id BETWEEN 1 AND 10)", &[UnbatchedBackfill]),
            (
                "DELETE FROM users WHERE EXISTS (SELECT 1 FROM teams WHERE teams.id = users.team_id LIMIT 1)",
                &[UnbatchedBackfill],
            ),
            (
                "UPDATE users u SET bio = '' FROM teams t WHERE t.id = 1 AND u.team_id = t.id",
                &[UnbatchedBackfill],
            ),
            ("UPDATE users u SET bio = '' WHERE u.id = 1", &[]),
            ("ALTER TABLE users SET (user_catalog_table = true)", &[]),
            (
                "BEGIN; ALTER TABLE users SET (user_catalog_table = true); ALTER TABLE teams ADD COLUMN q int; COMMIT;",
                &[MultiTableLock],
            ),
            (
                "BEGIN; CREATE INDEX i ON users (bio); UPDATE users SET bio = '' WHERE id BETWEEN 1 AND 9; COMMIT;",
                &[MissingConcurrently, LockedBackfill],
            ),
            (
                "BEGIN; ALTER TABLE users ADD COLUMN c int; WITH t AS (SELECT 1) UPDATE users SET c = 1 WHERE id BETWEEN 1 AND 5; COMMIT;",
                &[LockedBackfill],
            ),
        ] {
            let migration = format!("SET lock_timeout = '1s'; {migration}");
            assert_eq!(rules(USERS, &migration), expected, "{migration}");
        }
    }

    #[test]
    fn builds_and_rewrites_hold_their_locks_until_the_transaction_ends() {
        use Rule::*;
        let baseline = format!("{USERS}\nCREATE MATERIALIZED VIEW mv AS SELECT 1 AS id;");
        let then_teams = "ALTER TABLE teams ADD COLUMN q int; COMMIT;";
        let then_backfill = "UPDATE users SET bio = '' WHERE id BETWEEN 1 AND 9; COMMIT;";
        for (migration, expected) in [
            (
                format!("BEGIN; CREATE INDEX i ON users (bio) TABLESPACE x; {then_teams}"),
                &[MissingConcurrently, MultiTableLock][..],
            ),
            (
                format!("BEGIN; REINDEX TABLE users; {then_teams}"),
                &[MissingConcurrently, MultiTableLock],
            ),
            (
                format!("BEGIN; REINDEX (VERBOSE) INDEX users_email; {then_backfill}"),
                &[MissingConcurrently, LockedBackfill],
            ),
            (
                format!("BEGIN; REINDEX INDEX users_pkey; {then_backfill}"),
                &[MissingConcurrently, LockedBackfill],
            ),
            (
                format!("BEGIN; REINDEX INDEX teams_pkey; {then_backfill}"),
                &[MissingConcurrently],
            ),
            (
                format!("BEGIN; REINDEX INDEX elsewhere; {then_teams}"),
                &[MissingConcurrently],
            ),
            (
                format!("BEGIN; REFRESH MATERIALIZED VIEW mv; {then_teams}"),
                &[MissingConcurrently, MultiTableLock],
            ),
            (
                format!("BEGIN; CLUSTER users USING users_pkey; {then_teams}"),
                &[TableRewrite, MultiTableLock],
            ),
            (
                format!("BEGIN; ALTER TABLE users SET TABLESPACE x; {then_teams}"),
                &[TableRewrite, MultiTableLock],
            ),
            (
                format!("BEGIN; VACUUM (FULL, SKIP_LOCKED) users; {then_teams}"),
                &[TableRewrite, MultiTableLock],
            ),
            (format!("BEGIN; VACUUM (FULL off) users; {then_teams}"), &[]),
            (format!("BEGIN; VACUUM FULL; {then_teams}"), &[TableRewrite]),
            (format!("BEGIN; CLUSTER; {then_teams}"), &[TableRewrite]),
            (
                format!("BEGIN; REINDEX SCHEMA public; {then_teams}"),
                &[MissingConcurrently],
            ),
            (
                format!("BEGIN; REINDEX TABLE CONCURRENTLY users; {then_teams}"),
                &[ConcurrentlyInTransaction],
            ),
            (
                format!(
                    "BEGIN; CREATE INDEX CONCURRENTLY i ON users (bio) TABLESPACE x; {then_teams}"
                ),
                &[ConcurrentlyInTransaction],
            ),
            (
                format!(
                    "BEGIN; CREATE TABLE t (id int PRIMARY KEY); REINDEX INDEX t_pkey; CLUSTER t;
                     VACUUM FULL t; CREATE INDEX ON t (id) TABLESPACE x; REINDEX TABLE t;
                     CREATE MATERIALIZED VIEW m2 AS SELECT 1 AS x WITH NO DATA;
                     REFRESH MATERIALIZED VIEW m2; {then_teams}"
                ),
                &[],
            ),
        ] {
            let migration = format!("SET lock_timeout = '1s'; {migration}");
            assert_eq!(rules(&baseline, &migration), expected, "{migration}");
        }
    }

    #[test]
    fn keys_added_by_alter_table_name_their_indexes() {
        use Rule::*;
        let baseline =
            "CREATE TABLE users (id BIGINT PRIMARY KEY, email TEXT UNIQUE, team_id BIGINT);
            CREATE TABLE teams (id BIGINT, name TEXT);
            ALTER TABLE ONLY teams ADD CONSTRAINT teams_pkey PRIMARY KEY (id);
            ALTER TABLE teams ADD UNIQUE (name), ADD CONSTRAINT one CHECK (id > 0);
            CREATE TABLE events (id int, at date) PARTITION BY RANGE (at);";
        let backfill = "UPDATE teams SET id = 1 WHERE id BETWEEN 1 AND 9; COMMIT;";
        for (migration, expected) in [
            (
                "BEGIN; ALTER TABLE teams ADD COLUMN x int; REINDEX INDEX teams_pkey; COMMIT;"
                    .to_string(),
                &[MissingConcurrently][..],
            ),
            (
                format!("BEGIN; REINDEX INDEX teams_pkey; {backfill}"),
                &[MissingConcurrently, LockedBackfill],
            ),
            (
                format!("BEGIN; REINDEX INDEX teams_name_key; {backfill}"),
                &[MissingConcurrently, LockedBackfill],
            ),
            (
                "BEGIN; ALTER TABLE users ADD COLUMN x int; REINDEX INDEX users_email_key; COMMIT;"
                    .to_string(),
                &[MissingConcurrently],
            ),
            (
                "BEGIN; ALTER TABLE teams ADD COLUMN x int; REINDEX INDEX nowhere; COMMIT;"
                    .to_string(),
                &[MissingConcurrently],
            ),
            (
                "CREATE TABLE t (id int); ALTER TABLE t ADD CONSTRAINT t_pk PRIMARY KEY (id);
                 ALTER TABLE t ADD COLUMN u int UNIQUE, ADD COLUMN v int;
                 REINDEX INDEX t_pk; REINDEX INDEX t_u_key"
                    .to_string(),
                &[],
            ),
            (
                "CREATE INDEX events_at ON ONLY events (at)".to_string(),
                &[],
            ),
            ("CREATE INDEX events_at ON events (at)".to_string(), &[]),
            ("CREATE INDEX ON".to_string(), &[]),
            ("CREATE INDEX i ON ONLY".to_string(), &[]),
            ("ALTER TABLE SET TABLESPACE x".to_string(), &[]),
        ] {
            let migration = format!("SET lock_timeout = '1s'; {migration}");
            assert_eq!(rules(baseline, &migration), expected, "{migration}");
        }
        assert_eq!(
            rules(baseline, "CREATE INDEX events_at ON ONLY events (at)"),
            [MissingLockTimeout]
        );
    }

    #[test]
    fn only_single_column_keys_bound_a_backfill() {
        use Rule::*;
        let baseline = format!("{USERS} CREATE TABLE pairs (a int, b int, PRIMARY KEY (a, b));");
        let indexes = "CREATE UNIQUE INDEX CONCURRENTLY users_team_email ON users (team_id, email);
            CREATE UNIQUE INDEX CONCURRENTLY users_lower_email ON users (lower(email));";
        for (migration, expected) in [
            (
                "BEGIN; ALTER TABLE users ADD COLUMN a int; ALTER TABLE teams ADD COLUMN b int; ALTER TABLE pairs ADD COLUMN c int; COMMIT;",
                &[MultiTableLock][..],
            ),
            ("UPDATE pairs SET b = 1 WHERE a = 1", &[UnbatchedBackfill]),
            (&format!("{indexes} UPDATE users SET bio = '' WHERE team_id = 1"), &[UnbatchedBackfill]),
            (&format!("{indexes} UPDATE users SET bio = '' WHERE lower(email) = 'x'"), &[UnbatchedBackfill]),
            ("UPDATE users SET bio = '' WHERE (id BETWEEN 1 AND 9)", &[]),
            ("UPDATE (SELECT * FROM users) AS u SET bio = ''", &[]),
        ] {
            let migration = format!("SET lock_timeout = '1s'; {migration}");
            assert_eq!(rules(&baseline, &migration), expected, "{migration}");
        }
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
            REINDEX (CONCURRENTLY) INDEX a;
            ROLLBACK;";
        assert_eq!(
            rules(USERS, migration),
            [Rule::ConcurrentlyInTransaction; 5]
        );
        let off = "SET lock_timeout = '1s'; BEGIN; REINDEX (CONCURRENTLY 'off') INDEX a; COMMIT;";
        assert_eq!(rules(USERS, off), [Rule::MissingConcurrently]);
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
