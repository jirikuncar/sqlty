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
fn prefix_operators_get_their_own_match() {
    let rule = VALID
        .replace(r#""LongArrow""#, r#""Prefix:DoubleAt""#)
        .replace("left = ", "right = ")
        .replace("nullability = \"nullable\"\n", "");
    let code = compile_one("rules/common/a.inference.toml", &rule).unwrap();
    assert!(code.contains("UnaryOperator::DoubleAt { .. } => &BINARY_RULES[0..1]"));
    assert!(!code.contains("BinaryOperator::DoubleAt"));
}

#[test]
fn prefix_operators_have_one_operand_and_its_nullability() {
    let prefix = VALID.replace(r#""LongArrow""#, r#""Prefix:DoubleAt""#);
    let message = error("rules/common/a.inference.toml", &prefix);
    assert!(
        message.ends_with("a prefix operator has no left operand"),
        "{message}"
    );
    let nullable = prefix.replace("left = ", "right = ");
    let message = error("rules/common/a.inference.toml", &nullable);
    assert!(message.contains("drop `nullability`"), "{message}");
    let common = nullable
        .replace("nullability = \"nullable\"\n", "")
        .replace(r#"type = "TEXT""#, r#"type = "@common""#);
    let message = error("rules/common/a.inference.toml", &common);
    assert!(message.ends_with("a SQL type or `@right`"), "{message}");
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
        message.contains("rule test.specific (rules/common/a.inference.toml) never applies to LongArrow: rule test.long-arrow"),
        "{message}"
    );
}

#[test]
fn a_partial_overlap_with_another_result_is_an_error_unless_shadowed() {
    let json = VALID.replace("test.long-arrow", "test.left");
    let right = VALID
        .replace("test.long-arrow", "test.right")
        .replace("left = [\"@json\"]", "right = [\"@text\"]")
        .replace("type = \"TEXT\"", "type = \"JSONB\"");
    let message = error("rules/common/a.inference.toml", &format!("{json}\n{right}"));
    assert!(
        message.contains("rule test.right (rules/common/a.inference.toml) overlaps rule test.left"),
        "{message}"
    );
    let shadowing = json.replace("operator =", "shadows = [\"test.right\"]\noperator =");
    assert!(compile_one(
        "rules/common/a.inference.toml",
        &format!("{shadowing}\n{right}")
    )
    .is_ok());
    let unknown = json.replace("operator =", "shadows = [\"test.nope\"]\noperator =");
    let message = error("rules/common/a.inference.toml", &unknown);
    assert!(
        message.contains("`shadows` names unknown rule test.nope"),
        "{message}"
    );
}

#[test]
fn one_rule_can_cover_several_operators() {
    let rule = VALID.replace(
        "operator = \"LongArrow\"",
        "operators = [\"LongArrow\", \"HashLongArrow\"]",
    );
    let code = compile_one("rules/common/a.inference.toml", &rule).unwrap();
    assert!(code.contains("BinaryOperator::HashLongArrow { .. } => &BINARY_RULES[0..1]"));
    assert!(code.contains("BinaryOperator::LongArrow { .. } => &BINARY_RULES[1..2]"));
    let both = rule.replace("operators =", "operator = \"Eq\"\noperators =");
    let message = error("rules/common/a.inference.toml", &both);
    assert!(
        message.contains("give `operator` or `operators`, not both"),
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

#[test]
fn a_dialect_rule_repeating_a_common_one_is_an_error() {
    let sqlite = VALID
        .replace("operator =", "dialects = [\"sqlite\"]\noperator =")
        .replace("test.long-arrow", "test.sqlite");
    let files = [
        (
            "rules/common/a.inference.toml".to_string(),
            VALID.to_string(),
        ),
        ("rules/sqlite/a.inference.toml".to_string(), sqlite.clone()),
    ];
    let message = compile(&files).unwrap_err();
    assert!(
        message.contains(
            "rule test.sqlite (rules/sqlite/a.inference.toml) repeats rule test.long-arrow"
        ),
        "{message}"
    );
    let other = sqlite.replace("\"TEXT\"", "\"JSON\"");
    let files = [
        (
            "rules/common/a.inference.toml".to_string(),
            VALID.to_string(),
        ),
        ("rules/sqlite/a.inference.toml".to_string(), other),
    ];
    assert!(compile(&files).is_ok());
}

const FUNCTION: &str = r#"
[[function]]
id = "test.sum"
function = "SUM"
argument = ["@integer"]
type = "BIGINT"
nullability = "aggregate"
reference = "PostgreSQL 17 documentation, Table 9.60"
reasoning = "Wider."
"#;

#[test]
fn function_rules_become_their_own_table() {
    let code = compile_one("rules/common/a.inference.toml", FUNCTION).unwrap();
    assert!(code.contains("pub(crate) static FUNCTION_RULES: &[FunctionRule] = &["));
    assert!(code.contains(r#""SUM" => &FUNCTION_RULES[0..1]"#));
    assert!(code.contains("result: FunctionType::Sql(\"BIGINT\")"));
    assert!(code.contains("nullability: FunctionNullability::Aggregate"));
    let known = FUNCTION
        .replace("[\"@integer\"]", "[\"@known\"]")
        .replace("\"BIGINT\"", "\"@argument-as TEXT\"");
    let code = compile_one("rules/common/a.inference.toml", &known).unwrap();
    assert!(code.contains(r#"argument: Some(&["@known"])"#));
    assert!(code.contains(r#"result: FunctionType::ArgumentAs("TEXT")"#));
}

#[test]
fn function_rules_read_the_first_two_arguments() {
    let second = FUNCTION
        .replace("argument = [\"@integer\"]", "arguments = [[], [\"JSON\"]]")
        .replace("\"BIGINT\"", "\"@argument(1)\"");
    let code = compile_one("rules/common/a.inference.toml", &second).unwrap();
    assert!(code.contains("argument: None,\n        second: Some(&[\"JSON\"])"));
    assert!(code.contains("result: FunctionType::Argument(1)"));
    let cases = [
        (
            FUNCTION.replace("argument =", "arguments = [[], []]\nargument ="),
            "give `argument` or `arguments`, not both",
        ),
        (
            FUNCTION.replace("argument = [\"@integer\"]", "arguments = [[], [], []]"),
            "`arguments` reads the first two arguments only",
        ),
    ];
    for (text, expected) in cases {
        let message = error("rules/common/a.inference.toml", &text);
        assert!(message.contains(expected), "{message}");
    }
}

#[test]
fn function_rules_read_any_argument() {
    let any = FUNCTION.replace("argument = [\"@integer\"]", "any-argument = [\"@binary\"]");
    let code = compile_one("rules/common/a.inference.toml", &any).unwrap();
    assert!(
        code.contains(
            "second: None,\n        any: Some(&[\"BINARY\", \"BLOB\", \"BYTEA\", \"VARBINARY\"])"
        ),
        "{code}"
    );
    let unconditional = format!(
        "{any}{}",
        FUNCTION
            .replace("argument = [\"@integer\"]\n", "")
            .replace("test.", "test.all-")
    );
    compile_one("rules/common/a.inference.toml", &unconditional).unwrap();
    let error = error(
        "rules/common/a.inference.toml",
        &format!("{unconditional}{}", any.replace("test.", "test.again-")),
    );
    assert!(error.contains("never applies"), "{error}");
}

#[test]
fn function_rules_name_functions_and_their_own_results() {
    let cases = [
        (
            FUNCTION.replace("\"SUM\"", "\"sum\""),
            "function \"sum\" is not an upper-case name",
        ),
        (
            FUNCTION.replace("\"BIGINT\"", "\"@promote\""),
            "unknown result type \"@promote\"",
        ),
        (
            FUNCTION.replace("\"BIGINT\"", "\"@argument-as STRING\""),
            "unknown result type",
        ),
        (
            FUNCTION.replace("\"aggregate\"", "\"operands\""),
            "unknown nullability \"operands\"",
        ),
    ];
    for (text, expected) in cases {
        let message = error("rules/common/a.inference.toml", &text);
        assert!(message.contains(expected), "{message}");
    }
}

#[test]
fn parameter_types_are_generated_and_checked() {
    let rule = VALID.replace(
        "operator =",
        "right-parameter = \"ARRAY<TEXT>\"\noperator =",
    );
    let code = compile_one("rules/common/a.inference.toml", &rule).unwrap();
    assert!(code.contains("left_parameter: None"));
    assert!(code.contains(r#"right_parameter: Some("ARRAY<TEXT>")"#));
    let unknown = VALID.replace("operator =", "left-parameter = \"STRING\"\noperator =");
    let message = error("rules/common/a.inference.toml", &unknown);
    assert!(
        message.contains("unknown parameter type \"STRING\""),
        "{message}"
    );
    let function = FUNCTION.replace("function =", "right-parameter = \"TEXT\"\nfunction =");
    let message = error("rules/common/a.inference.toml", &function);
    assert!(
        message.contains("unknown field `right-parameter`"),
        "{message}"
    );
}
