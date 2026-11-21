use sqlty_python::types::stdlib::*;

#[test]
fn rarer_bases_and_modules() {
    let b = sqlty_python::types::ty::builtins();
    let version = (3, 14);
    let summary = stdlib_class("traceback.StackSummary", version).unwrap();
    assert!(sqlty_python::types::ty::is_subclass(&summary, &b.list));
    let not_implemented = stdlib_class("types.NotImplementedType", version).unwrap();
    assert!(sqlty_python::types::ty::is_subclass(
        &not_implemented,
        &b.any
    ));
    assert!(stdlib_class("asyncio.base_events.Server", version).is_some());
}
