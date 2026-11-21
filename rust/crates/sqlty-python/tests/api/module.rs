use sqlty_python::module::*;

#[test]
fn nested_imports_keep_ast_order() {
    // `ast` lists the deeper import last, so its binding wins.
    let found = facts(
        "if x:\n    from typing import Any as T\nfrom typing import List as T\nq = SQL[T](1)\n",
    )
    .unwrap();
    assert_eq!(found.any_calls, [(71, "SQL".to_string())]);
}

#[test]
fn relative_typing_import_aliases_too() {
    // `typing_aliases` reads `node.module` alone, as Python does.
    let found = facts("from .typing import Any\nq = SQL[Any](1)\n").unwrap();
    assert_eq!(found.any_calls, [(28, "SQL".to_string())]);
}

#[test]
fn invalid_source_has_no_facts() {
    assert_eq!(facts("def ("), None);
}

#[test]
fn annotated_without_metadata_is_not_any() {
    let found = facts("import typing\nSQL[typing.Annotated[typing.Any]](1)\n").unwrap();
    assert_eq!(found.any_calls, []);
}

#[test]
fn opaque_base() {
    let found = facts("import typing\nf().SQL[typing.Any](1)\n").unwrap();
    assert_eq!(found.any_calls, [(14, "?.SQL".to_string())]);
}
