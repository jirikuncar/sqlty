//! `ALTER TABLE` and `ALTER TYPE`: how each operation changes a raw table or type.
//!
//! An operation this module cannot follow gives up the object (see `AlterFail`) rather
//! than guessing.

use std::ops::ControlFlow;

use sqlparser::ast::{
    visit_expressions, AlterColumnOperation, AlterTable, AlterTableOperation, AlterType,
    AlterTypeAddValuePosition, AlterTypeOperation, ColumnDef, ColumnOption, ColumnOptionDef,
    DataType, Expr, Ident, MySQLColumnPosition, ObjectName, RenameTableNameKind, TableConstraint,
};

use crate::analyzer::SqlDialect;

use super::columns::{
    apply_check, check_narrows, discriminates, mark_primary_key, nonnull_groups, raw_column,
    unique_key,
};
use super::loader::{AlterFail, Loader, RawTable, Rel, Taint, TypeDef, LARGE_ENUM};
use super::names::{fold, parts};
use super::policies;

impl Loader {
    /// Returns `false` when the table (or a column it changes) is not defined yet.
    pub(super) fn alter_table(&mut self, alter: &AlterTable, path: &[String]) -> bool {
        let name = parts(&alter.name, self.dialect);
        if let [op] = alter.operations.as_slice() {
            if self.alter_in_place(op, alter.only, &name, path) {
                return true;
            }
        }
        let Some((key, root)) = self.table_of(&name, path) else {
            if self.lookup(&self.relations, &name, path).is_none() {
                return alter.if_exists;
            }
            // A `CREATE TABLE ... AS` table holds its query's columns, not a column list.
            let reshapes = alter.operations.iter().any(|op| {
                matches!(
                    op,
                    AlterTableOperation::AddColumn { .. }
                        | AlterTableOperation::DropColumn { .. }
                        | AlterTableOperation::RenameColumn { .. }
                        | AlterTableOperation::ChangeColumn { .. }
                        | AlterTableOperation::ModifyColumn { .. }
                        | AlterTableOperation::AlterColumn { .. }
                        | AlterTableOperation::RenameTable { .. }
                )
            });
            if reshapes {
                self.taint(Taint::Relation, &name, path);
            }
            return true;
        };
        let mut root = root.clone();
        let mut rename = None;
        let mut failed = None;
        for op in &alter.operations {
            if let AlterTableOperation::AddConstraint {
                constraint: TableConstraint::ForeignKey(fk),
                not_valid,
            } = op
            {
                let fk = self.foreign_key(fk, &fk.columns, &key, &root, path, !not_valid);
                root.foreign_keys.push(fk);
                continue;
            }
            let added = match op {
                AlterTableOperation::AddColumn { column_def, .. } => {
                    let name = fold(&column_def.name, self.dialect);
                    (!root.columns.iter().any(|c| c.name == name)).then_some(column_def)
                }
                _ => None,
            };
            if let Err(fail) = alter_op(&mut root, op, true, &mut rename, self.dialect) {
                failed = Some(fail);
                break;
            }
            if let Some(def) = added {
                for option in &def.options {
                    if let ColumnOption::ForeignKey(fk) = &option.option {
                        let columns = [def.name.clone()];
                        let fk = self.foreign_key(fk, &columns, &key, &root, path, true);
                        root.foreign_keys.push(fk);
                    }
                }
            }
        }
        if matches!(failed, Some(AlterFail::Missing)) {
            return false;
        }
        let unsupported = failed.is_some();
        if unsupported {
            self.skipped.push(("table", alter.name.to_string()));
        }
        let mut changed = vec![(key.clone(), root)];
        if !alter.only {
            let children = self.descendants(&key).into_iter().filter_map(|child| {
                let table = self.relations.get(&child)?.table()?.clone();
                Some((child, table))
            });
            for (child, mut table) in children {
                for op in &alter.operations {
                    let _ = alter_op(&mut table, op, false, &mut None, self.dialect);
                }
                changed.push((child, table));
            }
        }
        for (name, mut table) in changed {
            table.unknown |= unsupported;
            // An `ALTER` that changes nothing (owner, a foreign key, a default) leaves
            // the resolved relation as it is.
            if matches!(self.relations.get(&name), Some(Rel::Table(old)) if *old == table) {
                continue;
            }
            self.set_relation(name, Some(Rel::Table(table)));
        }
        // Views were typed against the constraints the table had.
        if alter
            .operations
            .iter()
            .any(|op| matches!(op, AlterTableOperation::DropConstraint { .. }))
        {
            self.forget_view_values();
        }
        if let Some(new_name) = rename {
            let (schema, _) = key.split_once('.').unwrap_or(("public", ""));
            let new_key = match parts(&new_name, self.dialect).as_slice() {
                [only] => format!("{schema}.{only}"),
                _ => self.new_key(&new_name, path),
            };
            self.cache = None;
            self.move_relation(&key, new_key.clone());
            if self.inherited.contains(&key) {
                self.inherited.insert(new_key.clone());
                for rel in self.relations.values_mut() {
                    if let Rel::Table(table) = rel {
                        for parent in &mut table.parents {
                            if *parent == key {
                                parent.clone_from(&new_key);
                            }
                        }
                    }
                }
            }
        }
        true
    }

    /// Apply `op`, the only operation of an `ALTER TABLE`, to the table where it is, when it
    /// can neither fail halfway nor change another table: a statement per column, key or
    /// index would otherwise copy the table each time. `false` leaves it to `alter_table`.
    fn alter_in_place(
        &mut self,
        op: &AlterTableOperation,
        only: bool,
        name: &[String],
        path: &[String],
    ) -> bool {
        let Some((key, table)) = self.table_of(name, path) else {
            return false;
        };
        if !only && self.inherited.contains(&key) {
            return false;
        }
        let has = |column: &Ident| {
            let name = fold(column, self.dialect);
            table.columns.iter().any(|c| c.name == name)
        };
        let mut fk = None;
        let changes = match op {
            AlterTableOperation::AddColumn {
                column_def,
                column_position,
                ..
            } => {
                let anchored = match column_position {
                    Some(MySQLColumnPosition::After(after)) => has(after),
                    _ => true,
                };
                // A key it declares may read the table as it was.
                let refers = column_def
                    .options
                    .iter()
                    .any(|option| matches!(option.option, ColumnOption::ForeignKey(_)));
                let typed = column_def.data_type != DataType::Unspecified
                    || self.dialect == SqlDialect::Sqlite;
                if !anchored || refers || !typed {
                    return false;
                }
                !has(&column_def.name)
            }
            AlterTableOperation::AddConstraint {
                constraint: TableConstraint::Unique(unique),
                ..
            } => {
                let name = unique.name.as_ref().or(unique.index_name.as_ref());
                unique_key(name, &unique.columns, self.dialect).is_some()
            }
            AlterTableOperation::AddConstraint {
                constraint: TableConstraint::Check(check),
                not_valid: false,
            } => {
                check_narrows(&table.columns, &check.expr, self.dialect)
                    || !nonnull_groups(&check.expr, self.dialect).is_empty()
                    || discriminates(&check.expr)
            }
            AlterTableOperation::AddConstraint {
                constraint: TableConstraint::ForeignKey(constraint),
                not_valid,
            } => {
                fk = Some(self.foreign_key(
                    constraint,
                    &constraint.columns,
                    &key,
                    table,
                    path,
                    !not_valid,
                ));
                true
            }
            _ => return false,
        };
        // As in `alter_table`, a statement that changes nothing leaves the table as it is.
        if changes {
            let dialect = self.dialect;
            self.change_table(key, |table| {
                let _ = alter_op(table, op, true, &mut None, dialect);
                table.foreign_keys.extend(fk);
            });
        }
        true
    }

    pub(super) fn alter_type(&mut self, alter: &AlterType, path: &[String]) -> bool {
        let found = self
            .lookup(&self.types, &parts(&alter.name, self.dialect), path)
            .and_then(|key| Some((self.types.get_mut(&key)?, key)));
        let Some((def, key)) = found else {
            return false;
        };
        self.cache = None;
        if !matches!(alter.operation, AlterTypeOperation::AddValue(_)) {
            self.enum_labels.remove(&key);
        }
        match (&alter.operation, def) {
            (AlterTypeOperation::AddValue(add), TypeDef::Enum(labels)) => {
                let value = add.value.value.clone();
                let new = if labels.len() > LARGE_ENUM {
                    self.enum_labels
                        .entry(key)
                        .or_insert_with(|| labels.iter().cloned().collect())
                        .insert(value.clone())
                } else {
                    !labels.contains(&value)
                };
                if new {
                    let at = add.position.as_ref().and_then(|position| {
                        let (anchor, after) = match position {
                            AlterTypeAddValuePosition::Before(anchor) => (anchor, 0),
                            AlterTypeAddValuePosition::After(anchor) => (anchor, 1),
                        };
                        labels
                            .iter()
                            .position(|label| *label == anchor.value)
                            .map(|at| at + after)
                    });
                    labels.insert(at.unwrap_or(labels.len()), value);
                }
            }
            (AlterTypeOperation::RenameValue(rename), TypeDef::Enum(labels)) => {
                match labels.iter_mut().find(|label| **label == rename.from.value) {
                    Some(label) => label.clone_from(&rename.to.value),
                    None => return false,
                }
            }
            (AlterTypeOperation::Rename(rename), def) => {
                let def = std::mem::replace(def, TypeDef::Unknown);
                let schema = key.split_once('.').map_or("public", |(schema, _)| schema);
                let new_key = format!("{schema}.{}", fold(&rename.new_name, self.dialect));
                self.types.remove(&key);
                for target in self.type_aliases.values_mut() {
                    if *target == key {
                        target.clone_from(&new_key);
                    }
                }
                self.type_aliases.remove(&new_key);
                self.type_aliases.insert(key, new_key.clone());
                self.types.insert(new_key, def);
            }
            (_, def) => {
                *def = TypeDef::Unknown;
                self.skipped.push(("type", alter.name.to_string()));
            }
        }
        true
    }
}

/// Apply one `ALTER TABLE` operation to `table`. `strict` fails on a missing column
/// (the table the statement names); the tables below it skip what does not apply.
pub(super) fn alter_op(
    table: &mut RawTable,
    op: &AlterTableOperation,
    strict: bool,
    rename: &mut Option<ObjectName>,
    dialect: SqlDialect,
) -> Result<(), AlterFail> {
    let missing = || {
        if strict {
            Err(AlterFail::Missing)
        } else {
            Ok(())
        }
    };
    let position = |table: &RawTable, name: &Ident| {
        let name = fold(name, dialect);
        table.columns.iter().position(|c| c.name == name)
    };
    // MySQL's `FIRST` / `AFTER col`: where a column added or changed goes.
    let place = |table: &mut RawTable, column, at: Option<&MySQLColumnPosition>| {
        let index = match at {
            None => table.columns.len(),
            Some(MySQLColumnPosition::First) => 0,
            Some(MySQLColumnPosition::After(after)) => match position(table, after) {
                Some(index) => index + 1,
                None => return missing(),
            },
        };
        table.columns.insert(index, column);
        Ok(())
    };
    match op {
        AlterTableOperation::AddColumn {
            column_def,
            column_position,
            ..
        } => {
            if column_def.data_type == DataType::Unspecified && dialect != SqlDialect::Sqlite {
                return Err(AlterFail::Unsupported);
            }
            if position(table, &column_def.name).is_none() {
                place(
                    table,
                    raw_column(column_def, dialect),
                    column_position.as_ref(),
                )?;
            }
        }
        AlterTableOperation::ModifyColumn {
            col_name: old_name,
            data_type,
            options,
            column_position,
        }
        | AlterTableOperation::ChangeColumn {
            old_name,
            data_type,
            options,
            column_position,
            ..
        } => {
            let Some(at) = position(table, old_name) else {
                return missing();
            };
            let name = match op {
                AlterTableOperation::ChangeColumn { new_name, .. } => new_name,
                _ => old_name,
            };
            let def = ColumnDef {
                name: name.clone(),
                data_type: data_type.clone(),
                options: options
                    .iter()
                    .map(|option| ColumnOptionDef {
                        name: None,
                        option: option.clone(),
                    })
                    .collect(),
            };
            let old = table.columns.remove(at);
            let mut column = raw_column(&def, dialect);
            rename_in_foreign_keys(table, &old.name, &column.name);
            column.primary_key |= old.primary_key;
            column.not_null |= old.primary_key;
            column.key_position = old.key_position;
            match column_position {
                None => table.columns.insert(at, column),
                Some(_) => place(table, column, column_position.as_ref())?,
            }
        }
        AlterTableOperation::DropColumn {
            column_names,
            if_exists,
            ..
        } => {
            for name in column_names {
                match position(table, name) {
                    Some(at) => {
                        let name = table.columns.remove(at).name;
                        table
                            .foreign_keys
                            .retain(|fk| !fk.key.columns.contains(&name));
                        // PostgreSQL drops the constraints that read the column.
                        table.checks.retain(|check| !reads(check, &name, dialect));
                        policies::drop_column(&mut table.policies, &name, dialect);
                    }
                    None if !if_exists => missing()?,
                    None => {}
                }
            }
        }
        AlterTableOperation::RenameColumn {
            old_column_name,
            new_column_name,
        } => match position(table, old_column_name) {
            Some(at) => {
                let name = fold(new_column_name, dialect);
                let old = std::mem::replace(&mut table.columns[at].name, name.clone());
                rename_in_foreign_keys(table, &old, &name);
                table.checks.retain(|check| !reads(check, &old, dialect));
                policies::rename_column(&mut table.policies, &old, &name, dialect);
                let column = &mut table.columns[at];
                column.ident = match new_column_name.quote_style {
                    Some(_) => new_column_name.to_string(),
                    None if dialect == SqlDialect::Postgres => column.name.clone(),
                    None => new_column_name.value.clone(),
                };
            }
            None => missing()?,
        },
        AlterTableOperation::RenameTable { table_name } => {
            if strict {
                let (RenameTableNameKind::As(name) | RenameTableNameKind::To(name)) = table_name;
                *rename = Some(name.clone());
            }
        }
        AlterTableOperation::AlterColumn { column_name, op } => {
            let Some(at) = position(table, column_name) else {
                return missing();
            };
            match op {
                AlterColumnOperation::SetNotNull => table.columns[at].not_null = true,
                AlterColumnOperation::DropNotNull => table.columns[at].not_null = false,
                AlterColumnOperation::SetDataType { data_type, .. } => {
                    table.columns[at].ty = data_type.clone();
                    table.columns[at].values = None;
                }
                AlterColumnOperation::SetDefault { value } => {
                    table.columns[at].has_default = true;
                    table.columns[at].default = Some(Box::new(value.clone()));
                }
                AlterColumnOperation::AddGenerated { .. } => {
                    table.columns[at].has_default = true;
                }
                // Also `DROP EXPRESSION`, read as `DROP DEFAULT`.
                AlterColumnOperation::DropDefault => {
                    table.columns[at].has_default = false;
                    table.columns[at].default = None;
                    table.columns[at].generated = None;
                }
                AlterColumnOperation::GenericOptions { .. } => {}
            }
        }
        AlterTableOperation::DropPrimaryKey { .. } => {
            for column in &mut table.columns {
                column.primary_key = false;
            }
        }
        AlterTableOperation::AddConstraint {
            constraint: TableConstraint::PrimaryKey(pk),
            ..
        } => {
            for (position, index_column) in pk.columns.iter().enumerate() {
                mark_primary_key(
                    &mut table.columns,
                    &index_column.column.expr,
                    dialect,
                    position,
                );
            }
        }
        AlterTableOperation::AddConstraint {
            constraint: TableConstraint::Unique(unique),
            ..
        } => {
            let name = unique.name.as_ref().or(unique.index_name.as_ref());
            table
                .unique
                .extend(unique_key(name, &unique.columns, dialect));
        }
        // A `CHECK` added `NOT VALID` does not hold for the rows already there.
        AlterTableOperation::AddConstraint {
            constraint: TableConstraint::Check(check),
            not_valid: false,
        } => {
            apply_check(&mut table.columns, &check.expr, dialect);
            table
                .nonnull_groups
                .extend(nonnull_groups(&check.expr, dialect));
            table
                .checks
                .extend(discriminates(&check.expr).then(|| (*check.expr).clone()));
        }
        // The dropped constraint may be a `CHECK`, and constraints are not tracked by name.
        AlterTableOperation::DropConstraint { name, .. } => {
            for column in &mut table.columns {
                column.values = None;
            }
            table.nonnull_groups.clear();
            table.checks.clear();
            let name = fold(name, dialect);
            drop_foreign_key(table, &name);
            drop_unique(table, &name);
        }
        AlterTableOperation::DropIndex { name } => drop_unique(table, &fold(name, dialect)),
        AlterTableOperation::DropForeignKey { name, .. } => {
            drop_foreign_key(table, &fold(name, dialect));
        }
        AlterTableOperation::ValidateConstraint { name } => {
            let name = fold(name, dialect);
            for fk in &mut table.foreign_keys {
                fk.validated |= fk.name.as_ref() == Some(&name);
            }
        }
        AlterTableOperation::EnableRowLevelSecurity => table.row_security = true,
        AlterTableOperation::DisableRowLevelSecurity => table.row_security = false,
        AlterTableOperation::ForceRowLevelSecurity => table.force_row_security = true,
        AlterTableOperation::NoForceRowLevelSecurity => table.force_row_security = false,
        AlterTableOperation::SwapWith { .. } => return Err(AlterFail::Unsupported),
        // Storage, options, tablespace, triggers, RLS, partitions, comments and the like
        // never change a column.
        _ => {}
    }
    Ok(())
}

/// A column of `table` renamed in the keys it declares.
fn rename_in_foreign_keys(table: &mut RawTable, old: &str, new: &str) {
    for fk in &mut table.foreign_keys {
        for column in &mut fk.key.columns {
            if column == old {
                *column = new.to_string();
            }
        }
    }
}

/// Drop the foreign key `name`. A key declared without a name has one the database
/// generates, so a name that matches none may be it: those are dropped too.
fn drop_foreign_key(table: &mut RawTable, name: &str) {
    let named = table
        .foreign_keys
        .iter()
        .any(|fk| fk.name.as_deref() == Some(name));
    table.foreign_keys.retain(|fk| match &fk.name {
        Some(own) => own != name,
        None => named,
    });
}

/// Drop the unique key `name`; like [`drop_foreign_key`], an unnamed key may be it.
pub(super) fn drop_unique(table: &mut RawTable, name: &str) {
    let named = table
        .unique
        .iter()
        .any(|key| key.name.as_deref() == Some(name));
    table.unique.retain(|key| match &key.name {
        Some(own) => own != name,
        None => named,
    });
}

/// Whether an `ALTER TABLE` that sqlparser cannot read may change the table's columns,
/// by the words after the table name. Constraints, storage, ownership, triggers, RLS,
/// partitions, tablespaces and comments cannot.
pub(super) fn changes_columns(action: &[String], mentions_type: bool) -> bool {
    let word = |i: usize| action.get(i).map_or("", String::as_str);
    match word(0) {
        "add" | "drop" => !matches!(
            word(1),
            "constraint" | "primary" | "unique" | "check" | "foreign" | "exclude"
        ),
        "rename" => !matches!(word(1), "constraint" | "index" | "key"),
        "alter" => mentions_type,
        "set" => word(1) == "schema",
        "of" => true,
        _ => false,
    }
}

/// Whether `check` reads the column `name`.
fn reads(check: &Expr, name: &str, dialect: SqlDialect) -> bool {
    visit_expressions(check, |expr| match expr {
        Expr::Identifier(ident) if fold(ident, dialect) == name => ControlFlow::Break(()),
        _ => ControlFlow::Continue(()),
    })
    .is_break()
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use sqlparser::ast::Statement;

    use super::*;
    use crate::analyzer::parse_ddl;

    /// A PostgreSQL loader holding `t (a INT)`, and the one statement of `sql` as
    /// `dialect` reads it.
    fn loader_and(sql: &str, dialect: SqlDialect) -> (Loader, Statement) {
        let mut loader = Loader::new(SqlDialect::Postgres);
        let Some(Statement::CreateTable(create)) =
            parse_ddl("CREATE TABLE t (a INT)", SqlDialect::Postgres)
                .and_then(|s| s.into_iter().next())
        else {
            unreachable!()
        };
        assert!(loader.create_table(&create, &[], &["public".to_string()]));
        let stmt = parse_ddl(sql, dialect).unwrap().remove(0);
        (loader, stmt)
    }

    /// A large enum is indexed, so `ADD VALUE` does not scan its labels; the index
    /// follows the labels through every kind of change.
    #[test]
    fn adding_values_to_a_large_enum_uses_its_index() {
        let path = ["public".to_string()];
        let mut loader = Loader::new(SqlDialect::Postgres);
        let labels = (0..=LARGE_ENUM).map(|i| format!("l{i}")).collect();
        loader
            .types
            .insert("public.e".to_string(), TypeDef::Enum(labels));
        let alter = |loader: &mut Loader, sql: &str| {
            let Some(Statement::AlterType(alter)) =
                parse_ddl(sql, SqlDialect::Postgres).and_then(|stmts| stmts.into_iter().next())
            else {
                unreachable!()
            };
            assert!(loader.alter_type(&alter, &path));
        };
        let labels = |loader: &Loader| match &loader.types["public.e"] {
            TypeDef::Enum(labels) => labels.clone(),
            _ => unreachable!(),
        };
        alter(&mut loader, "ALTER TYPE e ADD VALUE 'x'");
        alter(&mut loader, "ALTER TYPE e ADD VALUE 'x'");
        alter(&mut loader, "ALTER TYPE e ADD VALUE 'l3'");
        alter(&mut loader, "ALTER TYPE e ADD VALUE 'y' BEFORE 'l0'");
        alter(&mut loader, "ALTER TYPE e ADD VALUE 'z' AFTER 'l1'");
        assert_eq!(loader.enum_labels["public.e"].len(), LARGE_ENUM + 4);
        let after = labels(&loader);
        assert_eq!(after.len(), LARGE_ENUM + 4);
        assert_eq!(&after[..4], ["y", "l0", "l1", "z"]);
        assert_eq!(after.last().map(String::as_str), Some("x"));
        alter(&mut loader, "ALTER TYPE e RENAME VALUE 'x' TO 'w'");
        assert!(loader.enum_labels.is_empty());
        alter(&mut loader, "ALTER TYPE e ADD VALUE 'x'");
        assert_eq!(labels(&loader).len(), LARGE_ENUM + 5);

        // Only a statement that writes the type forgets its index.
        let drop = |loader: &mut Loader, sql: &str| {
            let stmt = parse_ddl(sql, SqlDialect::Postgres).unwrap().remove(0);
            loader.drop_objects(&stmt, &path);
        };
        let saved = labels(&loader);
        let indexed = |loader: &mut Loader| {
            let def = TypeDef::Enum(saved.clone());
            loader.types.insert("public.e".to_string(), def);
            alter(loader, "ALTER TYPE e ADD VALUE IF NOT EXISTS 'x'");
            assert!(loader.enum_labels.contains_key("public.e"));
        };
        drop(&mut loader, "DROP INDEX ix");
        drop(&mut loader, "DROP SCHEMA s");
        drop(&mut loader, "DROP DOMAIN d");
        assert!(loader.enum_labels.contains_key("public.e"));
        for sql in ["DROP TYPE e", "DROP DOMAIN e", "DROP SCHEMA public"] {
            drop(&mut loader, sql);
            assert!(loader.enum_labels.is_empty(), "{sql}");
            indexed(&mut loader);
        }
        assert!(loader.taint(Taint::Type, &["e".to_string()], &path));
        assert!(loader.enum_labels.is_empty());
    }

    #[test]
    fn changing_a_table_that_is_not_there_does_nothing() {
        let mut loader = Loader::new(SqlDialect::Postgres);
        loader.change_table("public.t".to_string(), |_| unreachable!());
        assert!(loader.relations.is_empty());
    }

    // Only dialects whose `ALTER` the loader does not read parse these.
    #[test]
    fn an_operation_it_cannot_follow_leaves_the_table_unknown() {
        let path = ["public".to_string()];
        for (sql, dialect) in [
            ("ALTER TABLE t SWAP WITH u", SqlDialect::Other("snowflake")),
            ("ALTER TABLE t ADD COLUMN c", SqlDialect::Sqlite),
        ] {
            let (mut loader, Statement::AlterTable(alter)) = loader_and(sql, dialect) else {
                unreachable!()
            };
            assert!(loader.alter_table(&alter, &path));
            assert_eq!(loader.skipped, [("table", "t".to_string())]);
            let Some(Rel::Table(table)) = loader.relations.get("public.t") else {
                unreachable!()
            };
            assert!(table.unknown);
        }
    }

    #[test]
    fn an_untyped_column_is_left_out_outside_sqlite() {
        let (mut loader, Statement::CreateTable(create)) =
            loader_and("CREATE TABLE u (a, b INT)", SqlDialect::Sqlite)
        else {
            unreachable!()
        };
        assert!(loader.create_table(&create, &[], &["public".to_string()]));
        let Some(Rel::Table(table)) = loader.relations.get("public.u") else {
            unreachable!()
        };
        let names: Vec<&str> = table.columns.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["b"]);
    }
}
