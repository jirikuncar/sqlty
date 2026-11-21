//! JSON shapes: literals, unification and their JSON spelling.

use sqlty_sql::json::{JsonField, JsonShape, JsonValue};

fn array_of(shape: JsonShape, nullable: bool) -> JsonShape {
    JsonShape::Array(Box::new(JsonValue::new(shape, nullable)))
}

fn field(name: &str, shape: JsonShape, optional: bool) -> JsonField {
    JsonField {
        name: name.to_string(),
        value: JsonValue::new(shape, false),
        optional,
    }
}

#[test]
fn empty_array_literal_has_no_element() {
    assert_eq!(
        JsonShape::of_literal("[]"),
        Some(array_of(JsonShape::Never, false))
    );
}

#[test]
fn null_in_array_literal_makes_elements_nullable() {
    assert_eq!(
        JsonShape::of_literal("[1, null]"),
        Some(array_of(JsonShape::Integer, true))
    );
}

#[test]
fn object_literal_lists_members() {
    assert_eq!(
        JsonShape::of_literal(r#"{"a": 1, "b": "x", "a": 2.5}"#),
        Some(JsonShape::Object(vec![
            field("b", JsonShape::String, false),
            field("a", JsonShape::Number, false),
        ]))
    );
}

#[test]
fn invalid_literal_has_no_shape() {
    assert_eq!(JsonShape::of_literal("{"), None);
    assert_eq!(JsonShape::of_literal("[1,]"), None);
    assert_eq!(JsonShape::of_literal("1 2"), None);
}

#[test]
fn deeply_nested_literal_is_not_read() {
    let text = "[".repeat(10_000);
    assert_eq!(JsonShape::of_literal(&text), None);
}

#[test]
fn unify_with_empty_array_keeps_element() {
    let objects = array_of(
        JsonShape::Object(vec![field("a", JsonShape::String, false)]),
        false,
    );
    let empty = JsonShape::of_literal("[]").unwrap();
    assert_eq!(objects.unify(&empty), objects);
    assert_eq!(empty.unify(&objects), objects);
}

#[test]
fn unify_objects_makes_missing_members_optional() {
    let a = JsonShape::Object(vec![field("a", JsonShape::Integer, false)]);
    let b = JsonShape::Object(vec![field("b", JsonShape::String, false)]);
    assert_eq!(
        a.unify(&b),
        JsonShape::Object(vec![
            field("a", JsonShape::Integer, true),
            field("b", JsonShape::String, true),
        ])
    );
}

#[test]
fn unify_empty_object_with_map_is_map() {
    let map = JsonShape::Map(Box::new(JsonValue::new(JsonShape::Integer, false)));
    let empty = JsonShape::of_literal("{}").unwrap();
    assert_eq!(map.unify(&empty), map);
}

#[test]
fn unify_different_scalars_is_any() {
    assert_eq!(JsonShape::String.unify(&JsonShape::Integer), JsonShape::Any);
    assert_eq!(
        JsonShape::Integer.unify(&JsonShape::Number),
        JsonShape::Number
    );
}

#[test]
fn literal_scalars_escapes_and_errors() {
    for (text, shape) in [
        ("true", Some(JsonShape::Boolean)),
        (" false ", Some(JsonShape::Boolean)),
        ("-1e3", Some(JsonShape::Number)),
        (r#""\n\t\r\b\f\"\\\/\u00e9\ud800""#, Some(JsonShape::String)),
        ("nul", None),
        ("nulx", None),
        ("-", None),
        ("x", None),
        ("", None),
        (r#""\x""#, None),
        (r#""\u12""#, None),
        (r#""\uzzzz""#, None),
        (r#""\"#, None),
        (r#""abc"#, None),
        ("[1 2]", None),
        (r#"{"a" 1}"#, None),
        (r#"{"a": 1 "b": 2}"#, None),
        ("{1: 2}", None),
        (r#"{"\x": 1}"#, None),
    ] {
        assert_eq!(JsonShape::of_literal(text), shape, "{text}");
    }
}

#[test]
fn unify_null_and_maps() {
    let int = JsonValue::new(JsonShape::Integer, false);
    let null = JsonValue::new(JsonShape::Null, false);
    assert_eq!(
        JsonValue::unify(&int, &null),
        JsonValue::new(JsonShape::Integer, true)
    );
    assert_eq!(
        JsonValue::unify(&null, &int),
        JsonValue::new(JsonShape::Integer, true)
    );
    let map = |nullable| JsonShape::Map(Box::new(JsonValue::new(JsonShape::Integer, nullable)));
    assert_eq!(map(false).unify(&map(true)), map(true));
}

#[test]
fn to_json_spells_never_and_escapes_keys() {
    let shape = JsonShape::Object(vec![
        field("a\"\\\u{1}", JsonShape::Never, false),
        field("b", JsonShape::Null, false),
    ]);
    assert_eq!(
        shape.to_json(),
        r#"{"object":[{"name":"a\"\\\u0001","value":{"shape":"never","nullable":false},"optional":false},{"name":"b","value":{"shape":"null","nullable":false},"optional":false}]}"#
    );
}

#[test]
fn to_json_spells_nested_shapes() {
    let shape = JsonShape::Object(vec![field("tags", array_of(JsonShape::String, true), true)]);
    assert_eq!(
        shape.to_json(),
        r#"{"object":[{"name":"tags","value":{"shape":{"array":{"shape":"string","nullable":true}},"nullable":false},"optional":true}]}"#
    );
}
