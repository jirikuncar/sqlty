use sqlty_python::breaking::*;

#[test]
fn only_what_the_other_schema_adds_breaks() {
    let own = [(1, "unknown-column", "a")];
    let against = vec![
        (1, "unknown-column", "a"),
        (2, "unknown-column", "a"),
        (3, "stale-type", "b"),
        (4, "unused-ignore", "c"),
    ];
    let found = breaking(
        &own,
        against,
        |&(line, rule, message)| (line, rule, message),
        |found| found.1,
    );
    assert_eq!(found, [(2, "unknown-column", "a")]);
}
