use crate::common::*;
use sqlty_python::types::compat::*;
use sqlty_python::types::{extract::extract_type_definitions, symbols::Source};

#[test]
fn rarer_fields_and_args() {
    let text = "from typing import TypedDict\nclass Row(TypedDict):\n    a: int\n";
    let source = Arc::new(Source::new(text, (3, 14)).unwrap());
    let found = extract_type_definitions(&source, None, None);
    assert!(field_type(found.get("Row").unwrap(), "b").is_none());
    assert_eq!(split_type_args("int, list[str],"), ["int", "list[str]"]);
}
