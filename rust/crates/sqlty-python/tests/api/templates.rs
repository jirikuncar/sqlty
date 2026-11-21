use sqlty_python::templates::*;

#[test]
fn queries_and_types() {
    let source = "def f(n: int):\n    q = t\"--sql\\nSELECT {n}, {x:b}, 5%\" t'{n=:s}'\n\
                      r = t\"{n!r}\"\nw = t\"{n:{w}}\"\nv = t\"{n:}\"\n";
    let found = templates(source).unwrap();
    let queries: Vec<_> = found.iter().map(|t| t.query.as_deref()).collect();
    assert_eq!(
        queries,
        [Some("--sql\nSELECT %s, %s, 5%%n=%s"), None, None, None]
    );
    assert_eq!(
        found[0].params,
        [Some("int".to_string()), None, Some("int".to_string())]
    );
    assert_eq!((found[0].line, found[1].line), (2, 3));
}

#[test]
fn newlines_read_as_ast_reads_them() {
    let found = templates("q = t\"\"\"--sql\r\nSELECT {1}\r\n\"\"\"\r\n").unwrap();
    assert_eq!(found[0].query.as_deref(), Some("--sql\nSELECT %s\n"));
    assert_eq!(found[0].span, (4, 30));
}
