use crate::common::*;
use sqlty_python::params::*;

#[test]
fn an_annotated_target_names_its_class() {
    let options = Options::new("postgres", None, None, false).unwrap();
    let source =
        "from sqlty import SQL\nq: SQL = SQL[tuple[int]](\"--sql\\nSELECT 1 WHERE %(a)s\")\n";
    assert!(add_param_types(source, &options, false).contains("SQL[tuple[int], QParams]"));
}
