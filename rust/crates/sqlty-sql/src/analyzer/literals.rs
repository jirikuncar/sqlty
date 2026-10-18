//! Literal value sets in expression types.
//!
//! A column or expression whose values are known is typed `typing.Literal[...]` (see
//! [`crate::values`]); the SQL type stays next to it. Constants are `Literal`s too, with no
//! SQL type, so `CASE`, `COALESCE`, `UNION` and `VALUES` over constants unify into the set of
//! their values. Everything that computes a new value from its inputs ([`strip`]) goes back
//! to the plain type, so a set is always a superset of the values an expression can take.

use std::rc::Rc;

use crate::values::{self, Value as Lit};

use super::dialect::SqlDialect;
use super::operators::common_type;
use super::python_type;
use super::typed::Typed;

/// A string or integer constant.
pub(super) fn constant(value: Lit) -> Typed {
    Typed::of_values(vec![value], None).unwrap_or(Typed::UNKNOWN)
}

/// The values `typed` is limited to, when it is a `Literal`.
pub(super) fn value_set(typed: &Typed) -> Option<Vec<Lit>> {
    typed.values.as_deref().map(<[Lit]>::to_vec)
}

/// The Python type with a `Literal` collapsed to `str` or `int`, for code that tells
/// constants by their Python type.
pub(super) fn python_kind(typed: &Typed) -> Option<&str> {
    let python = typed.python()?;
    Some(values::base(python).unwrap_or(python))
}

/// `typed` without its value set: what a computation over it gives.
pub(super) fn strip(typed: Typed, dialect: SqlDialect) -> Typed {
    let Some(kind) = typed.python().and_then(values::base) else {
        return typed;
    };
    match typed.sql {
        None => Typed::python_only(kind),
        // A user-defined enum has no plain Python type.
        Some(sql_type) => Typed {
            python: python_type(&sql_type, dialect).map(Rc::from),
            sql: Some(sql_type),
            values: None,
        },
    }
}

/// The type of a set operation's column (or `CASE`, `COALESCE`...) over `left` and `right`
/// when either has a value set: the plain types unify as usual, and the sets unite when both
/// have one. `None` when neither has, so the caller unifies as usual.
pub(super) fn merge(left: &Typed, right: &Typed, dialect: SqlDialect) -> Option<Option<Typed>> {
    let (known_left, known_right) = (value_set(left), value_set(right));
    if known_left.is_none() && known_right.is_none() {
        return None;
    }
    let plain = common_type(
        strip(left.clone(), dialect),
        strip(right.clone(), dialect),
        dialect,
    );
    let Some(plain) = plain else {
        return Some(None);
    };
    if let (Some(a), Some(b)) = (known_left, known_right) {
        let all = values::union(a, b);
        // An enum keeps every label it has; other sets widen past the cap.
        let enum_type = plain.python.is_none() && plain.sql.is_some();
        if enum_type || all.len() <= values::MAX_VALUES {
            if let Some(typed) = Typed::of_values(all, plain.sql.clone()) {
                return Some(Some(typed));
            }
        }
    }
    Some(Some(plain))
}

#[cfg(test)]
mod tests {
    use super::super::test_support::reports;
    use super::*;

    fn text(value: &str) -> Lit {
        Lit::Str(value.to_string())
    }

    fn set(values: &[&str]) -> Typed {
        constants(&values.iter().map(|value| text(value)).collect::<Vec<_>>())
    }

    fn constants(values: &[Lit]) -> Typed {
        Typed::of_values(values.to_vec(), None).unwrap()
    }

    fn show(typed: Option<Option<Typed>>) -> String {
        match typed {
            None => "no sets".to_string(),
            Some(None) => "conflict".to_string(),
            Some(Some(typed)) => format!(
                "{} | {}",
                typed.python().unwrap_or("?"),
                typed.sql().unwrap_or("?")
            ),
        }
    }

    #[test]
    fn a_constant_is_a_literal_without_a_sql_type() {
        let typed = constant(text("a"));
        assert_eq!(typed.python(), Some("typing.Literal['a']"));
        assert_eq!(typed.sql(), None);
        assert_eq!(constant(Lit::Int(-3)).python(), Some("typing.Literal[-3]"));
    }

    #[test]
    fn value_set_reads_only_literals() {
        assert_eq!(
            value_set(&set(&["a", "b"])),
            Some(vec![text("a"), text("b")])
        );
        assert_eq!(value_set(&Typed::python_only("str")), None);
        assert_eq!(value_set(&Typed::UNKNOWN), None);
    }

    #[test]
    fn python_kind_collapses_a_literal_to_its_base() {
        assert_eq!(python_kind(&set(&["a"])), Some("str"));
        assert_eq!(python_kind(&constant(Lit::Int(1))), Some("int"));
        assert_eq!(python_kind(&Typed::python_only("float")), Some("float"));
        assert_eq!(python_kind(&Typed::UNKNOWN), None);
    }

    #[test]
    fn strip_goes_back_to_the_plain_type() {
        let dialect = SqlDialect::Postgres;
        assert_eq!(strip(set(&["a"]), dialect), Typed::python_only("str"));
        let column = Typed::new(
            Some("typing.Literal['a', 'b']".to_string()),
            Some("TEXT".to_string()),
        );
        assert_eq!(strip(column, dialect), Typed::of_sql("TEXT", dialect));
    }

    #[test]
    fn strip_leaves_plain_types_alone() {
        let int = Typed::of_sql("INT", SqlDialect::Postgres);
        assert_eq!(strip(int.clone(), SqlDialect::Postgres), int);
        assert_eq!(strip(Typed::UNKNOWN, SqlDialect::Postgres), Typed::UNKNOWN);
    }

    #[test]
    fn strip_keeps_the_sql_type_of_an_enum_without_a_python_type() {
        let enum_value = Typed::new(
            Some("typing.Literal['a']".to_string()),
            Some("mood".to_string()),
        );
        let stripped = strip(enum_value, SqlDialect::Postgres);
        assert_eq!(stripped.python(), None);
        assert_eq!(stripped.sql(), Some("mood"));
    }

    #[test]
    fn merge_unites_two_sets() {
        let merged = merge(&set(&["a", "b"]), &set(&["b", "c"]), SqlDialect::Postgres);
        assert_eq!(show(merged), "typing.Literal['a', 'b', 'c'] | ?");
    }

    #[test]
    fn merge_ignores_inputs_without_a_set() {
        let plain = Typed::python_only("str");
        assert_eq!(show(merge(&plain, &plain, SqlDialect::Postgres)), "no sets");
    }

    #[test]
    fn merge_with_one_set_gives_the_plain_type() {
        let text_column = Typed::of_sql("TEXT", SqlDialect::Postgres);
        let merged = merge(&set(&["a"]), &text_column, SqlDialect::Postgres);
        assert_eq!(show(merged), "str | TEXT");
    }

    #[test]
    fn merge_of_strings_and_integers_follows_the_dialect() {
        let numbers = constants(&[Lit::Int(1)]);
        assert_eq!(
            show(merge(&set(&["a"]), &numbers, SqlDialect::Postgres)),
            "int | ?",
            "PostgreSQL reads a string constant as `unknown`, which takes the integer type"
        );
        assert_eq!(
            show(merge(&set(&["a"]), &numbers, SqlDialect::MySql)),
            "str | TEXT",
            "MySQL converts the integer to a string"
        );
        assert_eq!(
            show(merge(&set(&["a"]), &numbers, SqlDialect::Sqlite)),
            "conflict"
        );
    }

    #[test]
    fn merge_widens_a_set_past_the_cap() {
        let many = |from: usize| {
            constants(
                &(from..from + values::MAX_VALUES)
                    .map(|n| Lit::Int(n as i64))
                    .collect::<Vec<_>>(),
            )
        };
        let merged = merge(&many(0), &many(1), SqlDialect::Postgres);
        assert_eq!(show(merged), "int | ?");
    }

    #[test]
    fn merge_keeps_every_label_of_an_enum() {
        let labels = |from: usize| {
            Typed::of_values(
                (from..from + values::MAX_VALUES)
                    .map(|n| text(&format!("v{n}")))
                    .collect(),
                Some(Rc::from("big_enum")),
            )
            .unwrap()
        };
        let merged = merge(&labels(0), &labels(1), SqlDialect::Postgres)
            .flatten()
            .expect("an enum unites its labels");
        assert_eq!(merged.values.as_deref().map(<[Lit]>::len), Some(33));
        assert_eq!(merged.sql(), Some("big_enum"));
    }

    #[test]
    fn value_sets_flow_through_expressions() {
        let ddl = "CREATE TABLE t (status TEXT NOT NULL CHECK (status IN ('new', 'done')), \
                   n INT NOT NULL CHECK (n IN (1, 2)))";
        let queries = [
            "SELECT 'a' AS c, 1 AS d",
            "SELECT CASE WHEN n = 1 THEN 'x' ELSE 'y' END AS c FROM t",
            "SELECT CASE WHEN n = 1 THEN 'x' END AS c FROM t",
            "SELECT COALESCE(status, 'other') AS c FROM t",
            "SELECT 'a' AS c UNION SELECT 'b'",
            "SELECT status FROM t UNION ALL SELECT 'other'",
            "SELECT n FROM t UNION SELECT 3",
            "SELECT status || 'x' AS c, lower(status) AS d, upper('a') AS e FROM t",
            "SELECT status, n + 1 AS next FROM t",
            "SELECT * FROM (VALUES ('a'), ('b')) AS v(c)",
            "SELECT * FROM (VALUES ('a'), (1)) AS v(c)",
            "SELECT NULLIF(status, 'new') AS c FROM t",
        ];
        insta::assert_snapshot!(reports(&queries, "postgres", Some(ddl)));
    }
}
