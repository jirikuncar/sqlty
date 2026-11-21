use crate::support::*;
use sqlty_sql::analyzer::*;
const SCHEMA: &str = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT NOT NULL, bio TEXT);
     CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, total NUMERIC NOT NULL);";

fn shape(sql: &str) -> Option<String> {
    let schema = load_schema(SCHEMA, "postgres");
    let columns = analyze_select(sql, "postgres", Some(&schema)).unwrap();
    columns[0]
        .json
        .as_ref()
        .map(sqlty_sql::json::JsonShape::to_json)
}

#[test]
fn build_object_with_literal_keys_is_an_object() {
    assert_eq!(
        shape("SELECT jsonb_build_object('id', id, 'bio', bio) FROM users").unwrap(),
        r#"{"object":[{"name":"id","value":{"shape":"integer","nullable":false},"optional":false},{"name":"bio","value":{"shape":"string","nullable":true},"optional":false}]}"#
    );
}

#[test]
fn build_object_with_computed_keys_is_a_map() {
    assert_eq!(
        shape("SELECT jsonb_build_object(name, id) FROM users").unwrap(),
        r#"{"map":{"shape":"integer","nullable":false}}"#
    );
}

#[test]
fn coalesce_of_agg_and_empty_array_keeps_the_element() {
    assert_eq!(
        shape("SELECT coalesce(jsonb_agg(id), '[]') FROM users").unwrap(),
        r#"{"array":{"shape":"integer","nullable":false}}"#
    );
}

#[test]
fn row_to_json_of_a_table_lists_its_columns() {
    assert!(shape("SELECT row_to_json(u) FROM users u")
        .unwrap()
        .starts_with(r#"{"object":[{"name":"id""#));
}

#[test]
fn arrow_follows_a_member() {
    assert_eq!(
        shape("SELECT jsonb_build_object('a', jsonb_build_object('b', 1)) -> 'a' FROM users")
            .unwrap(),
        r#"{"object":[{"name":"b","value":{"shape":"integer","nullable":false},"optional":false}]}"#
    );
}

#[test]
fn json_columns_of_unknown_shape_have_none() {
    assert_eq!(
        shape("SELECT to_jsonb(bio) FROM users").as_deref(),
        Some("\"string\"")
    );
    assert_eq!(shape("SELECT '{}'::jsonb || '{}'::jsonb FROM users"), None);
}

#[test]
fn filter_keeps_only_the_matched_rows_of_an_outer_join() {
    let sql = "SELECT jsonb_agg(jsonb_build_object('t', o.total)) FILTER (WHERE o.id IS NOT NULL)
               FROM users u LEFT JOIN orders o ON o.user_id = u.id";
    assert_eq!(
        shape(sql).unwrap(),
        r#"{"array":{"shape":{"object":[{"name":"t","value":{"shape":"number","nullable":false},"optional":false}]},"nullable":false}}"#
    );
}

#[test]
fn row_guards_in_filter_count_as_null_guards() {
    for guard in ["o IS NOT NULL", "o.* IS NOT NULL", "o.user_id IS NOT NULL"] {
        let sql = format!(
            "SELECT jsonb_agg(jsonb_build_object('t', o.total)) FILTER (WHERE {guard})
             FROM users u LEFT JOIN orders o ON o.user_id = u.id"
        );
        assert!(
            shape(&sql).unwrap().contains(r#""nullable":false}"#),
            "{guard}"
        );
        assert!(
            !shape(&sql).unwrap().contains(r#""nullable":true"#),
            "{guard}"
        );
    }
}

#[test]
fn union_of_objects_makes_missing_members_optional() {
    let sql = "SELECT jsonb_build_object('a', 1) FROM users
               UNION ALL SELECT jsonb_build_object('b', 1) FROM users";
    assert!(shape(sql).unwrap().contains(r#""optional":true"#));
}

#[test]
fn union_with_a_branch_of_unknown_shape_has_none() {
    let sql = "SELECT jsonb_build_object('a', 1) FROM users UNION ALL SELECT '{}'::jsonb || '{}'::jsonb FROM users";
    assert_eq!(shape(sql), None);
}

#[test]
fn other_dialects_have_no_shapes() {
    let columns = analyze_select("SELECT json_object('a', 1)", "sqlite", None).unwrap();
    assert_eq!(columns[0].json, None);
}

#[test]
fn mysql_json_object_is_an_object() {
    let columns = analyze_select("SELECT json_object('a', 1, 'b', TRUE)", "mysql", None).unwrap();
    assert_eq!(
        columns[0]
            .json
            .as_ref()
            .map(sqlty_sql::json::JsonShape::to_json),
        analyze_select(
            "SELECT jsonb_build_object('a', 1, 'b', TRUE)",
            "postgres",
            None
        )
        .unwrap()[0]
            .json
            .as_ref()
            .map(sqlty_sql::json::JsonShape::to_json)
    );
}
