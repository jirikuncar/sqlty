//! `ALTER TABLE` and `ALTER TYPE`: how each operation changes a raw table or type.
//!
//! An operation this module cannot follow gives up the object (see `AlterFail`) rather
//! than guessing.

use sqlparser::ast::{
    AlterColumnOperation, AlterTable, AlterTableOperation, AlterType, AlterTypeAddValuePosition,
    AlterTypeOperation, ColumnDef, ColumnOptionDef, DataType, Ident, MySQLColumnPosition,
    ObjectName, RenameTableNameKind, TableConstraint,
};

use crate::analyzer::SqlDialect;

use super::columns::{apply_check, mark_primary_key, nonnull_groups, raw_column};
use super::loader::{AlterFail, Loader, RawTable, Rel, Taint, TypeDef};
use super::names::{fold, key_of, parts};

impl Loader {
    /// Returns `false` when the table (or a column it changes) is not defined yet.
    pub(super) fn alter_table(&mut self, alter: &AlterTable, path: &[String]) -> bool {
        let name = parts(&alter.name, self.dialect);
        let Some(key) = self.table_key(&name, path) else {
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
        let Some(Rel::Table(root)) = self.relations.get(&key) else {
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
                if let Some(fk) = self.foreign_key(fk, &fk.columns, &key, &root, path, !not_valid) {
                    root.foreign_keys.push(fk);
                }
                continue;
            }
            if let Err(fail) = alter_op(&mut root, op, true, &mut rename, self.dialect) {
                failed = Some(fail);
                break;
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
            for child in self.descendants(&key) {
                let Some(Rel::Table(table)) = self.relations.get(&child) else {
                    continue;
                };
                let mut table = table.clone();
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
                other => key_of(other, self.dialect),
            };
            self.cache = None;
            if let Some(table) = self.relations.remove(&key) {
                self.relations.insert(new_key.clone(), table);
            }
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
        true
    }

    pub(super) fn alter_type(&mut self, alter: &AlterType, path: &[String]) -> bool {
        let Some(key) = self.lookup(&self.types, &parts(&alter.name, self.dialect), path) else {
            return false;
        };
        self.cache = None;
        match (&alter.operation, self.types.get_mut(&key)) {
            (AlterTypeOperation::AddValue(add), Some(TypeDef::Enum(labels))) => {
                let value = add.value.value.clone();
                if !labels.contains(&value) {
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
            (AlterTypeOperation::RenameValue(rename), Some(TypeDef::Enum(labels))) => {
                match labels.iter_mut().find(|label| **label == rename.from.value) {
                    Some(label) => label.clone_from(&rename.to.value),
                    None => return false,
                }
            }
            (AlterTypeOperation::Rename(rename), Some(_)) => {
                let schema = key.split_once('.').map_or("public", |(schema, _)| schema);
                let new_key = format!("{schema}.{}", fold(&rename.new_name, self.dialect));
                if let Some(def) = self.types.remove(&key) {
                    for target in self.type_aliases.values_mut() {
                        if *target == key {
                            target.clone_from(&new_key);
                        }
                    }
                    self.type_aliases.remove(&new_key);
                    self.type_aliases.insert(key, new_key.clone());
                    self.types.insert(new_key, def);
                }
            }
            (_, Some(def)) => {
                *def = TypeDef::Unknown;
                self.skipped.push(("type", alter.name.to_string()));
            }
            _ => {}
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
            column.primary_key |= old.primary_key;
            column.not_null |= old.primary_key;
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
                        table.columns.remove(at);
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
                let column = &mut table.columns[at];
                column.name = fold(new_column_name, dialect);
                column.ident = match new_column_name.quote_style {
                    Some(_) => new_column_name.to_string(),
                    None => column.name.clone(),
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
                AlterColumnOperation::SetDefault { .. }
                | AlterColumnOperation::AddGenerated { .. } => {
                    table.columns[at].has_default = true;
                }
                AlterColumnOperation::DropDefault => table.columns[at].has_default = false,
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
            for index_column in &pk.columns {
                mark_primary_key(&mut table.columns, &index_column.column.expr, dialect);
            }
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
        }
        // The dropped constraint may be a `CHECK`, and constraints are not tracked by name.
        AlterTableOperation::DropConstraint { name, .. } => {
            for column in &mut table.columns {
                column.values = None;
            }
            table.nonnull_groups.clear();
            drop_foreign_key(table, &fold(name, dialect));
        }
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
        AlterTableOperation::SwapWith { .. } => return Err(AlterFail::Unsupported),
        // Storage, options, tablespace, triggers, RLS, partitions, comments and the like
        // never change a column.
        _ => {}
    }
    Ok(())
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
