use ruff_python_ast::Expr;
use ruff_python_parser::parse_module;
use sqlty_python::scopes::*;

#[test]
fn one_walk_collects_calls_templates_strings_and_stacks() {
    let text = "class A:\n    def m(self):\n        f(lambda: g(1), [h(x) for x in y])\n\
                    k = f'a{b}' + 'c'\nt = t'd'\ndef n():\n    return 'e'\n";
    let parsed = parse_module(text).unwrap();
    let scan = Scan::new(&parsed.syntax().body);
    let names: Vec<(String, Scope)> = scan
        .calls
        .iter()
        .map(|(call, scope)| (dotted(&call.func).unwrap(), *scope))
        .collect();
    let expected = [("f", 2), ("g", 2), ("h", 2)].map(|(n, s)| (n.to_string(), s));
    assert_eq!(names, expected);
    assert_eq!(scan.stack(0), &[] as &[Scope]);
    assert_eq!(scan.stack(1), &[1]);
    assert_eq!(scan.stack(2), &[1, 2]);
    assert_eq!(scan.stack(3), &[3]);
    assert_eq!(scan.templates.len(), 1);
    assert_eq!(scan.templates[0].1, 0);
    let strings: Vec<bool> = scan.strings.iter().map(|(_, operand)| *operand).collect();
    assert_eq!(strings, [true, true, false, false]);
    assert!(matches!(scan.strings[2].0, Expr::TString(_)));
    assert!(matches!(scan.strings[3].0, Expr::StringLiteral(_)));
}
