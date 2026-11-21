use sqlty_python::frozen::Table;

#[test]
fn a_later_pair_wins() {
    let table: Table = [("b", "1"), ("a", "2"), ("b", "3")]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    assert_eq!(table.keys().collect::<Vec<_>>(), ["a", "b"]);
    assert_eq!(table.get("b").map(String::as_str), Some("3"));
    assert_eq!(table.get("c"), None);
}
