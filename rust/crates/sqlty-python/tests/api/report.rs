use sqlty_python::report::*;

#[test]
fn a_link_prefills_the_bug_report_form() {
    let url = issue_url("index out of bounds: the len is 1\nnote: details");
    let version = env!("CARGO_PKG_VERSION");
    assert!(url.starts_with(
        "https://github.com/jirikuncar/sqlty/issues/new?template=bug_report.yml\
             &title=Crash%3A%20index%20out%20of%20bounds%3A%20the%20len%20is%201\
             &output=index%20out%20of%20bounds%3A%20the%20len%20is%201%0Anote%3A%20details"
    ));
    assert!(url.contains(&format!("&version={version}&environment=")));
}

#[test]
fn a_long_cause_is_cut_on_a_character() {
    let url = issue_url(&"é".repeat(3000));
    assert!(url.contains(&format!("title=Crash%3A%20{}&", "%C3%A9".repeat(80))));
    assert!(url.contains(&format!("output={}&", "%C3%A9".repeat(2000))));
}

#[test]
fn an_empty_cause_keeps_every_field() {
    assert!(issue_url("").contains("&title=Crash%3A%20&output=&version="));
}

#[test]
fn a_panic_without_a_message_has_an_unknown_cause() {
    assert_eq!(cause(&42), "unknown cause");
    assert_eq!(cause(&"static"), "static");
}
