use crate::common::*;
use sqlty_python::index::*;
use sqlty_python::types::info::Dict;

#[test]
fn a_module_without_a_name_has_no_relative_imports() {
    let source = Source::new(
        "from . import a\nfrom .b import c\nfrom d import e\n",
        (3, 14),
    )
    .unwrap();
    let (functions, modules) = imports_of(&source.entries, None);
    assert_eq!(functions.keys().collect::<Vec<_>>(), ["e"]);
    assert_eq!(modules.keys().collect::<Vec<_>>(), ["e"]);
}

#[test]
fn a_source_imports_relative_to_the_module_it_is_read_as() {
    let source = Source::new("from .b import c\n", (3, 14))
        .unwrap()
        .named(Some("app.a"));
    let own = source.targets_in(None);
    let other = source.targets_in(Some("lib.a"));
    assert_eq!(
        own.0.get("c").map(|(module, _)| module.as_str()),
        Some("app.b")
    );
    assert_eq!(
        other.0.get("c").map(|(module, _)| module.as_str()),
        Some("lib.b")
    );
}

#[test]
fn a_top_level_module_imports_relative_to_the_root() {
    let source = Source::new(
        "from . import a\nfrom .. import b\nfrom .c import d\n",
        (3, 14),
    )
    .unwrap();
    let (_, modules) = imports_of(&source.entries, Some("app"));
    let modules: Vec<_> = modules.iter().collect();
    assert_eq!(
        modules,
        [(&"a".into(), &"a".into()), (&"d".into(), &"c.d".into())]
    );
}

#[test]
fn a_module_keeps_its_tree_once_a_lookup_parses_it() {
    let module = Module::new(Arc::from("x = 1\n"), "app.a", (3, 14));
    let loaded = module.load().unwrap();
    assert_eq!(loaded.module.as_deref(), Some("app.a"));
    let kept = Arc::clone(module.source().unwrap());
    assert!(!Arc::ptr_eq(&loaded, &kept));
    assert!(Arc::ptr_eq(&module.load().unwrap(), &kept));
}

#[test]
fn a_fork_takes_the_sources_it_was_built_with() {
    let a = Arc::new(Module::new(Arc::from("x = 1\n"), "a", (3, 14)));
    let mut builder = Builder::new((3, 14));
    builder.add_found("a", a, &Dict::new());
    let mut fork = builder.build().fork();
    let b = Arc::new(Module::parsed(Arc::new(
        Source::new("y = 1\n", (3, 14)).unwrap().named(Some("b")),
    )));
    assert!(!fork.add_found("b", b, &Dict::new()));
    let index = fork.build();
    assert!(index.source("a").is_some() && index.source("b").is_some());
}
