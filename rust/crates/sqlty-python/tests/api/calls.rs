use crate::common::*;
use sqlty_python::calls::*;

#[test]
fn a_module_without_markers_has_no_helpers() {
    let source = Source::new("from typing import Annotated\nx = 1\n", (3, 14)).unwrap();
    assert!(index_functions(&source).is_empty());
}
