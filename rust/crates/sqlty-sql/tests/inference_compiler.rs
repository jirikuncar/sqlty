//! The inference rule compiler of `build.rs`: what it accepts, and the rule and file each
//! error names.

#[path = "../rules/compile.rs"]
#[allow(dead_code, unreachable_pub)]
mod inference;

use inference::compile;

const VALID: &str = r#"
[[rule]]
id = "test.long-arrow"
operator = "LongArrow"
left = ["@json"]
type = "TEXT"
nullability = "nullable"
reference = "PostgreSQL 17 documentation, Table 9.45"
reasoning = "Text out."
"#;

fn compile_one(path: &str, text: &str) -> Result<String, String> {
    compile(&[(path.to_string(), text.to_string())])
}

fn error(path: &str, text: &str) -> String {
    compile_one(path, text).expect_err("the rules must not compile")
}

#[test]
fn a_rule_becomes_data_and_a_match_arm() {
    let code = compile_one("rules/common/a.inference.toml", VALID).unwrap();
    assert!(code.contains(r#"id: "test.long-arrow""#));
    assert!(code.contains(r#"left: Some(&["JSON", "JSONB"])"#));
    assert!(code.contains("BinaryOperator::LongArrow { .. } => &BINARY_RULES[0..1]"));
}

#[test]
fn no_rules_compile_to_an_empty_table() {
    let code = compile(&[]).unwrap();
    assert!(code.contains("pub(crate) static BINARY_RULES: &[BinaryRule] = &[\n];"));
}

#[test]
fn custom_operators_match_their_text() {
    let rule = VALID.replace(r#""LongArrow""#, r#""Custom:<<=""#);
    let code = compile_one("rules/common/a.inference.toml", &rule).unwrap();
    assert!(code.contains(r#""<<=" => &BINARY_RULES[0..1]"#));
}

#[test]
fn malformed_toml_names_the_file() {
    let message = error("rules/common/bad.inference.toml", "[[rule]\nid = ");
    assert!(
        message.starts_with("rules/common/bad.inference.toml: "),
        "{message}"
    );
}

#[test]
fn unknown_fields_are_errors() {
    let message = error(
        "rules/common/a.inference.toml",
        &VALID.replace("type =", "typo = 1\ntype ="),
    );
    assert!(message.contains("unknown field `typo`"), "{message}");
}

#[test]
fn errors_name_the_rule_and_file() {
    let cases = [
        (
            VALID.replace(r#"["@json"]"#, r#"["JSONZ"]"#),
            "unknown operand type \"JSONZ\"",
        ),
        (
            VALID.replace(r#""TEXT""#, r#""STRING""#),
            "unknown result type \"STRING\"",
        ),
        (
            VALID.replace(r#""nullable""#, r#""maybe""#),
            "unknown nullability \"maybe\"",
        ),
        (
            VALID.replace(r#""LongArrow""#, r#""long-arrow""#),
            "operator \"long-arrow\"",
        ),
        (
            VALID.replace("Text out.", ""),
            "`reference` and `reasoning` must not be empty",
        ),
        (
            VALID.replace("operator =", "dialects = [\"postgres\"]\noperator ="),
            "a rule in `common/` applies to every dialect",
        ),
    ];
    for (text, expected) in cases {
        let message = error("rules/common/a.inference.toml", &text);
        assert!(
            message.starts_with("rule test.long-arrow (rules/common/a.inference.toml): "),
            "{message}"
        );
        assert!(message.contains(expected), "{message}");
    }
}

#[test]
fn dialect_rules_name_known_dialects() {
    let rule = VALID.replace("operator =", "dialects = [\"postgress\"]\noperator =");
    let message = error("rules/postgres/a.inference.toml", &rule);
    assert!(
        message.contains("unknown dialect \"postgress\""),
        "{message}"
    );
    let message = error("rules/postgres/a.inference.toml", VALID);
    assert!(message.contains("must name its `dialects`"), "{message}");
}

#[test]
fn duplicate_ids_are_errors() {
    let files = [
        (
            "rules/common/a.inference.toml".to_string(),
            VALID.to_string(),
        ),
        (
            "rules/common/b.inference.toml".to_string(),
            VALID.replace(r#""LongArrow""#, r#""HashLongArrow""#),
        ),
    ];
    let message = compile(&files).unwrap_err();
    assert!(
        message.contains("duplicate id, also in rules/common/a.inference.toml"),
        "{message}"
    );
}

#[test]
fn a_later_rule_an_earlier_one_covers_is_an_error() {
    let general = VALID.replace("left = [\"@json\"]\n", "");
    let specific = VALID.replace("test.long-arrow", "test.specific");
    // Specific before general is fine: the general rule catches the rest.
    let ok = format!(
        "{specific}\n{}",
        general.replace("test.long-arrow", "test.general")
    );
    assert!(compile_one("rules/common/a.inference.toml", &ok).is_ok());
    // General before specific makes the specific one unreachable.
    let shadowed = format!("{general}\n{specific}");
    let message = error("rules/common/a.inference.toml", &shadowed);
    assert!(
        message.contains(
            "rule test.specific (rules/common/a.inference.toml) overlaps rule test.long-arrow"
        ),
        "{message}"
    );
}

#[test]
fn rules_of_different_dialects_do_not_overlap() {
    let sqlite = VALID.replace("operator =", "dialects = [\"sqlite\"]\noperator =");
    let mysql = sqlite
        .replace("sqlite", "mysql")
        .replace("test.long-arrow", "test.mysql");
    let text = format!("{sqlite}\n{mysql}");
    assert!(compile_one("rules/dialects/a.inference.toml", &text).is_ok());
}
