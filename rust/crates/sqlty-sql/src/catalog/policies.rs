//! PostgreSQL row-level security policies: `CREATE`, `ALTER` and `DROP POLICY`, and what
//! they prove about the tenant a row belongs to.
//!
//! A policy that compares a column to a session value (`current_setting(...)`,
//! `current_user`, `session_user` or a `tenancy.session-functions` call) names a tenant
//! key. A table whose policies pin one for every command, with `FORCE ROW LEVEL SECURITY`,
//! is scoped by the database: <https://www.postgresql.org/docs/17/ddl-rowsecurity.html>.

use std::ops::{ControlFlow, Range};

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
    /// The bytes of the latest `CREATE` or `ALTER POLICY` that set it.
    at: Range<usize>,
}

/// A `CREATE`, `ALTER` or `DROP POLICY` (by name), or a policy statement sqlparser does
/// not read, which may let any row through.
pub(super) enum Change {
    Create(CreatePolicy),
    Alter(AlterPolicy),
    Drop(Ident),
    Unknown,
}

/// Applies a `CREATE`, `ALTER` or `DROP POLICY` to the policies of its table.
pub(super) fn apply(
    policies: &mut Vec<Policy>,
    change: &Change,
    dialect: SqlDialect,
    at: Range<usize>,
) {
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
                at,
            });
        }
        Change::Alter(alter) => {
            let name = fold(&alter.name, dialect);
            let Some(policy) = policies.iter_mut().find(|policy| policy.name == name) else {
                return;
            };
            policy.at = at;
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
        Change::Unknown => policies.push(Policy {
            name: String::new(),
            restrictive: false,
            command: None,
            public: true,
            using: None,
            with_check: None,
            at,
        }),
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
            visit_expressions(expr, |expr| match last_ident(expr) {
                Some(ident) if fold(ident, dialect) == column => ControlFlow::Break(()),
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
            let ident = match expr {
                Expr::Identifier(ident) => Some(ident),
                Expr::CompoundIdentifier(parts) => parts.last_mut(),
                _ => None,
            };
            if let Some(ident) = ident.filter(|ident| fold(ident, dialect) == old) {
                *ident = Ident::with_quote('"', new);
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
    gaps(policies, keys, functions).is_empty()
}

/// A permissive policy that lets a command through for rows of any tenant.
pub(super) struct Gap<'p> {
    pub policy: &'p Policy,
    pub command: CreatePolicyCommand,
    /// Whether it is the rows the command writes that the policy does not pin.
    pub written: bool,
}

/// What keeps [`enforce`] from holding: for each command no restrictive policy scopes, the
/// permissive policies that do not, in order.
pub(super) fn gaps<'p>(
    policies: &'p [Policy],
    keys: &[Vec<String>],
    functions: &[String],
) -> Vec<Gap<'p>> {
    use CreatePolicyCommand::{Delete, Insert, Select, Update};
    let pins = |expr: Option<&Expr>| {
        let pinned = expr.map(|expr| pinned(expr, functions)).unwrap_or_default();
        keys.iter()
            .any(|key| key.iter().all(|column| pinned.contains(column)))
    };
    // `Some(written)` when the policy lets the command through for another tenant's rows.
    let gap = |policy: &Policy, command: CreatePolicyCommand| {
        let reads = !matches!(command, Insert) && !pins(policy.using.as_ref());
        let writes = matches!(command, Insert | Update)
            && !pins(policy.with_check.as_ref().or(policy.using.as_ref()));
        (reads || writes).then_some(!reads)
    };
    let mut found = Vec::new();
    for command in [Select, Insert, Update, Delete] {
        let applies: Vec<&Policy> = policies
            .iter()
            .filter(|policy| policy.command.is_none_or(|own| own == command))
            .collect();
        let restricted = applies
            .iter()
            .any(|policy| policy.restrictive && policy.public && gap(policy, command).is_none());
        if restricted {
            continue;
        }
        for policy in applies.into_iter().filter(|policy| !policy.restrictive) {
            if let Some(written) = gap(policy, command) {
                found.push(Gap {
                    policy,
                    command,
                    written,
                });
            }
        }
    }
    found
}

impl Policy {
    /// The policy's name, empty for one sqlparser could not read.
    pub(super) fn name(&self) -> &str {
        &self.name
    }

    /// The bytes of the latest statement that set it.
    pub(super) fn at(&self) -> Range<usize> {
        self.at.clone()
    }
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
    last_ident(uncast(expr)).map(|ident| fold(ident, SqlDialect::Postgres))
}

/// The column a plain or qualified name reads: `org_id`, `t.org_id`.
fn last_ident(expr: &Expr) -> Option<&Ident> {
    match expr {
        Expr::Identifier(ident) => Some(ident),
        Expr::CompoundIdentifier(parts) => parts.last(),
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
                matches!(&select.projection[0], SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } if is_session(expr, functions))
            }
            _ => false,
        },
        _ => false,
    }
}
