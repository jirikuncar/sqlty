//! The rule page compiler of `build.rs`: what it accepts, and the file each error names.

#![expect(clippy::expect_used, reason = "a failed setup fails the test")]

#[path = "../compile.rs"]
#[allow(dead_code, unreachable_pub)]
mod compile;

use compile::compile;

const PAGE: &str = "+++
order = 10
group = \"names\"
severity = \"warn\"
fixable = false
summary = \"Two \\\"tables\\\" meet\"
origin = \"SQL\"
+++

# chasm-join

Default severity: **warn**

## What it does

Something.

## Why is this bad?

Because.
";

fn page(name: &str, text: &str) -> (String, String) {
    (name.to_string(), text.to_string())
}

fn error(text: &str) -> String {
    compile(&[page("chasm-join.md", text)]).expect_err("the page must not compile")
}

#[test]
fn a_page_becomes_a_variant_and_its_tables() {
    let code = compile(&[page("chasm-join.md", PAGE)]).expect("compiles");
    assert!(code
        .contains("pub enum Rule {\n    #[doc = \"Two \\\"tables\\\" meet\"]\n    ChasmJoin,\n}"));
    assert!(code.contains("pub const COUNT: usize = 1;"));
    assert!(code.contains("pub const SQL: &'static [Rule] = &[\n        Rule::ChasmJoin,\n    ];"));
    assert!(code.contains("Rule::ChasmJoin => \"chasm-join\","));
    assert!(code.contains("Rule::ChasmJoin => Severity::Warn,"));
    assert!(code.contains("Rule::ChasmJoin => Group::Names,"));
    assert!(code.contains("\"chasm-join\" => Some(Rule::ChasmJoin),"));
    assert!(code.contains("(\"chasm-join.md\", \"# chasm-join\\n"));
    assert!(!code.contains("+++"));
}

#[test]
fn rules_are_listed_by_order_and_pages_by_file_name() {
    let second = PAGE
        .replace("order = 10", "order = 5")
        .replace("chasm-join", "a-rule");
    let second = second.replace("origin = \"SQL\"", "origin = \"Python\"");
    let code =
        compile(&[page("chasm-join.md", PAGE), page("a-rule.md", &second)]).expect("compiles");
    assert!(code.find("ARule,").expect("listed") < code.find("ChasmJoin,").expect("listed"));
    assert!(
        code.find("(\"a-rule.md\"").expect("paged")
            < code.find("(\"chasm-join.md\"").expect("paged")
    );
    assert!(code.contains("pub const SQL: &'static [Rule] = &[\n        Rule::ChasmJoin,\n    ];"));
    assert!(code.contains("Rule::ARule => false,"));
}

#[test]
fn a_page_needs_front_matter() {
    let message = error("# chasm-join\n");
    assert!(
        message.starts_with("chasm-join.md: missing front matter"),
        "{message}"
    );
}

#[test]
fn front_matter_must_end() {
    let message = error("+++\norder = 10\n");
    assert!(
        message.starts_with("chasm-join.md: unterminated front matter"),
        "{message}"
    );
}

#[test]
fn front_matter_needs_a_blank_line_after_it() {
    let message = error(&PAGE.replace("+++\n\n#", "+++\n#"));
    assert_eq!(
        message,
        "chasm-join.md: front matter must be followed by a blank line"
    );
}

#[test]
fn an_unknown_key_is_an_error() {
    let message = error(&PAGE.replace("order = 10", "order = 10\nname = \"x\""));
    assert!(
        message.starts_with("chasm-join.md: front matter"),
        "{message}"
    );
    assert!(message.contains("unknown field `name`"), "{message}");
}

#[test]
fn orders_must_differ() {
    let other = PAGE.replace("chasm-join", "a-rule");
    let message = compile(&[page("chasm-join.md", PAGE), page("a-rule.md", &other)])
        .expect_err("the pages must not compile");
    assert_eq!(
        message,
        "chasm-join.md: order 10 is also the order of a-rule.md"
    );
}

#[test]
fn group_severity_and_origin_are_checked() {
    let message = error(&PAGE.replace("\"names\"", "\"nothing\""));
    assert!(
        message.starts_with("chasm-join.md: unknown group \"nothing\" (expected row-type, query"),
        "{message}"
    );
    let message = error(&PAGE.replace("severity = \"warn\"", "severity = \"loud\""));
    assert_eq!(
        message,
        "chasm-join.md: unknown severity \"loud\" (expected error, warn, ignore)"
    );
    let message = error(&PAGE.replace("\"SQL\"", "\"rust\""));
    assert_eq!(
        message,
        "chasm-join.md: unknown origin \"rust\" (expected SQL or Python)"
    );
}

#[test]
fn the_summary_must_say_something() {
    let message = error(&PAGE.replace("summary = \"Two \\\"tables\\\" meet\"", "summary = \" \""));
    assert_eq!(message, "chasm-join.md: the summary is empty");
}

#[test]
fn the_title_must_be_the_file_stem() {
    let message = error(&PAGE.replace("# chasm-join", "# other"));
    assert_eq!(
        message,
        "chasm-join.md: the page must start with `# chasm-join`"
    );
}

#[test]
fn the_default_severity_line_must_agree() {
    let message = error(&PAGE.replace("**warn**", "**error**"));
    assert_eq!(
        message,
        "chasm-join.md: missing the line `Default severity: **warn**`"
    );
}

#[test]
fn both_sections_are_required() {
    let message = error(&PAGE.replace("## What it does", "## What"));
    assert_eq!(
        message,
        "chasm-join.md: missing the section `## What it does`"
    );
    let message = error(&PAGE.replace("## Why is this bad?", "## Why"));
    assert_eq!(
        message,
        "chasm-join.md: missing the section `## Why is this bad?`"
    );
}

#[test]
fn a_page_is_a_markdown_file_named_in_kebab_case() {
    let message = compile(&[page("chasm-join.txt", PAGE)]).expect_err("not a page");
    assert_eq!(message, "chasm-join.txt: a rule page must be a .md file");
    for name in ["Chasm.md", "-a.md", "a--b.md", "1a.md", "a_b.md"] {
        let message = compile(&[page(name, PAGE)]).expect_err("not a rule name");
        assert!(message.contains("kebab-case"), "{name}: {message}");
    }
}

#[test]
fn at_most_sixty_four_rules() {
    let pages: Vec<_> = (0..65u32)
        .map(|i| {
            let name = format!(
                "rule-{}",
                char::from(b'a' + (i / 26) as u8).to_string()
                    + &char::from(b'a' + (i % 26) as u8).to_string()
            );
            let text = PAGE
                .replace("chasm-join", &name)
                .replace("order = 10", &format!("order = {i}"));
            (format!("{name}.md"), text)
        })
        .collect();
    assert!(compile(&pages[..64]).is_ok());
    let message = compile(&pages).expect_err("too many");
    assert_eq!(message, "65 rule pages, but a RuleSet holds at most 64");
}
