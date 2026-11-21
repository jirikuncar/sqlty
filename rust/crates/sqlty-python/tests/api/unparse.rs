use sqlty_python::unparse::*;

fn round(source: &str) -> String {
    let parsed = sqlty_python::parse::expression(source).unwrap();
    unparse(&parsed.syntax().body, source)
}

#[test]
fn expressions() {
    for (source, expected) in [
        ("dict[str,list[int|None]]", "dict[str, list[int | None]]"),
        ("Literal[\"a\",'b\"',1]", "Literal['a', 'b\"', 1]"),
        ("(a,)", "(a,)"),
        ("x[(a,b)]", "x[a, b]"),
        ("x[()]", "x[()]"),
        ("-(-a)**-b", "-(-a) ** (-b)"),
        ("(a+b)*c-d", "(a + b) * c - d"),
        ("not a and (b or c)", "not a and (b or c)"),
        ("a if b else(c if d else e)", "a if b else c if d else e"),
        (
            "lambda a,/,b=1,*c,d,**e:0",
            "lambda a, /, b=1, *c, d, **e: 0",
        ),
        ("f(x for x in y)", "f((x for x in y))"),
        ("{**a,'b':1}", "{**a, 'b': 1}"),
        (
            "1e100+1.5+1e16+1e-5+2j+0x10",
            "1e+100 + 1.5 + 1e+16 + 1e-05 + 2j + 16",
        ),
        ("0xffffffffffffffffffff", "1208925819614629174706175"),
        ("3 .real", "3 .real"),
        ("f'{a!r:>{w}} {b=}'", "f'{a!r:>{w}} b={b!r}'"),
        ("b'a\\x00\\''", "b\"a\\x00'\""),
        ("'a\\n\\u00e9\\x7f'", "'a\\né\\x7f'"),
        ("'\\r\\t\\\\'", "'\\r\\t\\\\'"),
    ] {
        assert_eq!(round(source), expected, "{source}");
    }
}
