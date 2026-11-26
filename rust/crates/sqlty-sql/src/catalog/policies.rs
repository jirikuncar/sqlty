//! PostgreSQL row-level security policies: `CREATE`, `ALTER` and `DROP POLICY`, and what
//! they prove about the tenant a row belongs to.
//!
//! A policy that compares a column to a session value (`current_setting(...)`,
//! `current_user`, `session_user` or a `tenancy.session-functions` call) names a tenant
//! key. A table whose policies pin one for every command, with `FORCE ROW LEVEL SECURITY`,
//! is scoped by the database: <https://www.postgresql.org/docs/17/ddl-rowsecurity.html>.

use std::ops::ControlFlow;

use sqlparser::ast::{
    visit_expressions, visit_expressions_mut, AlterPolicy, AlterPolicyOperation, BinaryOperator,
    CreatePolicy, CreatePolicyCommand, CreatePolicyType, Expr, Ident, Owner, SelectItem, SetExpr,
};

use super::names::fold;
use crate::analyzer::fanout::{conjuncts, strip};
use crate::analyzer::SqlDialect;

/// A policy of a table, as `CREATE POLICY` and later `ALTER POLICY` leave it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Policy {
    name: String,
    restrictive: bool,
    /// `None` for `ALL`.
    command: Option<CreatePolicyCommand>,
    /// `TO PUBLIC`, or no `TO`: it applies to every role.
    public: bool,
    using: Option<Expr>,
    with_check: Option<Expr>,
}

/// A `CREATE`, `ALTER` or `DROP POLICY` (by name).
pub(super) enum Change {
    Create(CreatePolicy),
    Alter(AlterPolicy),
    Drop(Ident),
}

/// Applies a `CREATE`, `ALTER` or `DROP POLICY` to the policies of its table.
pub(super) fn apply(policies: &mut Vec<Policy>, change: &Change, dialect: SqlDialect) {
    match change {
        Change::Create(create) => {
            let name = fold(&create.name, dialect);
            policies.retain(|policy| policy.name != name);
            policies.push(Policy {
                name,
                restrictive: create.policy_type == Some(CreatePolicyType::Restrictive),
                command: create
                    .command
                    .filter(|command| *command != CreatePolicyCommand::All),
                public: is_public(create.to.as_deref()),
                using: create.using.clone(),
                with_check: create.with_check.clone(),
            });
        }
        Change::Alter(alter) => {
            let name = fold(&alter.name, dialect);
            let Some(policy) = policies.iter_mut().find(|policy| policy.name == name) else {
                return;
            };
            match &alter.operation {
                AlterPolicyOperation::Rename { new_name } => policy.name = fold(new_name, dialect),
                AlterPolicyOperation::Apply {
                    to,
                    using,
                    with_check,
                } => {
                    policy.public = to.as_ref().map_or(policy.public, |to| is_public(Some(to)));
                    policy.using = using.clone().or(policy.using.take());
                    policy.with_check = with_check.clone().or(policy.with_check.take());
                }
            }
        }
        Change::Drop(name) => {
            let name = fold(name, dialect);
            policies.retain(|policy| policy.name != name);
        }
    }
}

fn is_public(to: Option<&[Owner]>) -> bool {
    to.is_none_or(|to| {
        to.iter()
            .any(|owner| matches!(owner, Owner::Ident(ident) if ident.value.eq_ignore_ascii_case("public")))
    })
}

/// PostgreSQL drops the policies that read a dropped column (`DROP COLUMN ... CASCADE`).
pub(super) fn drop_column(policies: &mut Vec<Policy>, column: &str, dialect: SqlDialect) {
    policies.retain(|policy| {
        let reads = |expr: &Option<Expr>| {
            visit_expressions(expr, |expr| match expr {
                Expr::Identifier(ident) if fold(ident, dialect) == column => ControlFlow::Break(()),
                _ => ControlFlow::Continue(()),
            })
            .is_break()
        };
        !reads(&policy.using) && !reads(&policy.with_check)
    });
}

/// A policy follows a renamed column.
pub(super) fn rename_column(policies: &mut [Policy], old: &str, new: &str, dialect: SqlDialect) {
    for expr in policies
        .iter_mut()
        .flat_map(|policy| [&mut policy.using, &mut policy.with_check])
    {
        let _ = visit_expressions_mut(expr, |expr| {
            if let Expr::Identifier(ident) = expr {
                if fold(ident, dialect) == old {
                    *ident = Ident::new(new);
                }
            }
            ControlFlow::<()>::Continue(())
        });
    }
}

impl Policy {
    /// The columns of `columns` its `USING` or `WITH CHECK` pins to a session value.
    pub(super) fn session_columns<'c>(
        &self,
        columns: &'c [String],
        functions: &[String],
    ) -> impl Iterator<Item = &'c String> {
        let pinned: Vec<String> = [&self.using, &self.with_check]
            .into_iter()
            .flatten()
            .flat_map(|expr| pinned(expr, functions))
            .collect();
        columns.iter().filter(move |column| pinned.contains(column))
    }
}

/// Whether the policies scope every command to a tenant key of `keys`: for `SELECT`,
/// `INSERT`, `UPDATE` and `DELETE`, a restrictive policy for every role pins one, or each
/// permissive one does (none at all denies every row). A row an `INSERT` or `UPDATE`
/// writes passes `WITH CHECK`, or `USING` when the policy has none.
pub(super) fn enforce(policies: &[Policy], keys: &[Vec<String>], functions: &[String]) -> bool {
    use CreatePolicyCommand::{Delete, Insert, Select, Update};
    let pins = |expr: Option<&Expr>| {
        let pinned = expr.map(|expr| pinned(expr, functions)).unwrap_or_default();
        keys.iter()
            .any(|key| key.iter().all(|column| pinned.contains(column)))
    };
    let scoped = |policy: &Policy, command: CreatePolicyCommand| {
        let new_rows = || pins(policy.with_check.as_ref().or(policy.using.as_ref()));
        match command {
            Insert => new_rows(),
            Update => pins(policy.using.as_ref()) && new_rows(),
            _ => pins(policy.using.as_ref()),
        }
    };
    [Select, Insert, Update, Delete].into_iter().all(|command| {
        let applies = policies
            .iter()
            .filter(|policy| policy.command.is_none_or(|own| own == command));
        let (restrictive, permissive): (Vec<&Policy>, Vec<&Policy>) =
            applies.partition(|policy| policy.restrictive);
        restrictive
            .iter()
            .any(|policy| policy.public && scoped(policy, command))
            || permissive.iter().all(|policy| scoped(policy, command))
    })
}

/// The columns `expr` compares to a session value with `=` in a conjunct, folded.
fn pinned(expr: &Expr, functions: &[String]) -> Vec<String> {
    conjuncts(expr)
        .into_iter()
        .filter_map(|term| match strip(term) {
            Expr::BinaryOp {
                left,
                op: BinaryOperator::Eq,
                right,
            } => match (column(left), column(right)) {
                (Some(column), None) if is_session(right, functions) => Some(column),
                (None, Some(column)) if is_session(left, functions) => Some(column),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn column(expr: &Expr) -> Option<String> {
    match uncast(expr) {
        Expr::Identifier(ident) => Some(fold(ident, SqlDialect::Postgres)),
        _ => None,
    }
}

fn uncast(expr: &Expr) -> &Expr {
    match strip(expr) {
        Expr::Cast { expr, .. } => uncast(expr),
        expr => expr,
    }
}

/// `current_setting(...)`, `current_user`, `session_user`, or a call of a session
/// function, cast or not, or alone in a scalar subquery (`(SELECT auth.uid())`).
fn is_session(expr: &Expr, functions: &[String]) -> bool {
    match uncast(expr) {
        Expr::Function(function) => {
            let name = function.name.to_string().to_lowercase();
            let short = name.rsplit('.').next().unwrap_or(&name);
            matches!(short, "current_setting" | "current_user" | "session_user")
                || functions.iter().any(|f| f.eq_ignore_ascii_case(&name))
        }
        Expr::Subquery(query) => match query.body.as_ref() {
            SetExpr::Select(select) if select.from.is_empty() && select.projection.len() == 1 => {
                matches!(&select.projection[0], SelectItem::UnnamedExpr(expr) if is_session(expr, functions))
            }
            _ => false,
        },
        _ => false,
    }
}
