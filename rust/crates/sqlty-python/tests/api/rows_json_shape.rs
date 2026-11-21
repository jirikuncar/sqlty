use crate::common::*;
use sqlty_python::frozen::Table;
use sqlty_python::rows::json_shape::*;
use sqlty_python::types::info::{Annotation, Dict};
use sqlty_sql::json::{JsonField, JsonShape, JsonValue};

fn problems(shape: &JsonShape, annotation: &str) -> Vec<String> {
    let parsed = sqlty_python::parse::expression(annotation).unwrap();
    let annotation = Annotation {
        expr: *parsed.into_syntax().body,
        source: Arc::from(annotation),
    };
    json_problems(shape, &annotation, &Dict::new(), &Table::default(), "")
}

#[test]
fn the_problems_of_an_unnamed_value_start_at_its_members() {
    let int = JsonValue::new(JsonShape::Integer, false);
    let object = JsonShape::Object(vec![JsonField {
        name: "a".into(),
        value: int.clone(),
        optional: false,
    }]);
    assert_eq!(
        problems(&JsonShape::Array(Box::new(int)), "str"),
        ["is str but the query builds an array"]
    );
    assert_eq!(
        problems(&object, "dict[str, str]"),
        ["a is str but the query builds an integer"]
    );
}
