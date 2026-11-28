//! The function catalog compiler of `build.rs`: what it accepts, and the line each error
//! names.

#![expect(clippy::expect_used, reason = "a failed setup fails the test")]

#[path = "../functions/compile.rs"]
#[allow(dead_code, unreachable_pub, clippy::unwrap_used)]
mod functions;

use functions::compile;

fn error(line: &str) -> String {
    compile("t.txt", "T", &format!("# docs: https://x/\n{line}\n"))
        .expect_err("the catalog must not compile")
}

#[test]
fn parameters_options_and_repeats() {
    let code = compile(
        "t.txt",
        "T",
        "# A comment.\n# docs: https://x/\n\nb(VARIADIC \"any\") -> int\tb.html\na(s: text[], [n: int[, m: int]], ...) -> setof record(k text)\ta.html\n",
    )
    .expect("the catalog compiles");
    assert!(code.starts_with("const T_DOCS: &str = \"https://x/\";\npub(crate) static T: Catalog"));
    let a = code.find("name: \"a\"").expect("a");
    assert!(a < code.find("name: \"b\"").expect("b"), "sorted by name");
    assert!(code.contains(
        "returns: \"setof record(k text)\", docs: T_DOCS, page: \"a.html\", required: 1, variadic: true, parameters: &[\
         Parameter { name: Some(\"s\"), label: (2, 11), optional: false }, \
         Parameter { name: Some(\"n\"), label: (14, 20), optional: true }, \
         Parameter { name: Some(\"m\"), label: (23, 29), optional: true }, ]"
    ), "{code}");
    assert!(code.contains(
        "required: 1, variadic: true, parameters: &[Parameter { name: None, label: (11, 16), optional: false }, ]"
    ), "{code}");
}

#[test]
fn types_hold_parentheses_and_brackets() {
    let code = compile(
        "t.txt",
        "T",
        "# docs: https://x/\nc(m: MAP(K, V), a: FLOAT[3][, b: STRUCT(x INT[])]) -> K\tc.html\n",
    )
    .expect("the catalog compiles");
    assert!(
        code.contains(
            "required: 2, variadic: false, parameters: &[\
         Parameter { name: Some(\"m\"), label: (2, 14), optional: false }, \
         Parameter { name: Some(\"a\"), label: (16, 27), optional: false }, \
         Parameter { name: Some(\"b\"), label: (30, 48), optional: true }, ]"
        ),
        "{code}"
    );
}

#[test]
fn a_result_is_optional() {
    let code = compile("t.txt", "T", "# docs: https://x/\nabs(X)\tabs.html\n")
        .expect("the catalog compiles");
    assert!(
        code.contains("label: \"abs(X)\", returns: \"\", "),
        "{code}"
    );
}

#[test]
fn errors_name_the_line() {
    assert_eq!(
        compile("t.txt", "T", "a() -> int\tp\n").expect_err("no docs"),
        "t.txt: no `# docs:` line"
    );
    assert_eq!(error("a() -> int"), "t.txt:2: no tab before the page");
    assert_eq!(
        error("a(é) -> int\tp"),
        "t.txt:2: a signature must be ASCII"
    );
    assert_eq!(error("a -> int\tp"), "t.txt:2: no `(`");
    assert_eq!(
        error("A() -> int\tp"),
        "t.txt:2: `A` is not a lower-case function name"
    );
    assert_eq!(
        error("(x) -> int\tp"),
        "t.txt:2: `` is not a lower-case function name"
    );
    assert_eq!(
        error("a(x\tp"),
        "t.txt:2: no `)` at the end or before ` -> `"
    );
    assert_eq!(error("a(x]) -> int\tp"), "t.txt:2: unbalanced `]`");
    assert_eq!(error("a([x) -> int\tp"), "t.txt:2: unbalanced `[` or `(`");
    assert_eq!(error("a(x), b) -> int\tp"), "t.txt:2: unbalanced `)`");
    assert_eq!(
        error("a(...) -> int\tp"),
        "t.txt:2: `...` repeats no parameter"
    );
}
