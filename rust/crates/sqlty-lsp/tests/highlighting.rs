//! Semantic tokens through the server.

mod common;

use common::Client;
use serde_json::json;

#[test]
fn a_function_name_is_a_function_token() {
    let mut client = Client::start(&json!({}));
    let legend = &client.initialized["capabilities"]["semanticTokensProvider"]["legend"];
    assert_eq!(legend["tokenTypes"][6], "function");
    client.open("file:///work/q.py", "q = \"--sql\\nSELECT now()\"\n");
    assert_eq!(client.tokens("file:///work/q.py")[2], (0, 19, 3, 6, 0));
    client.shutdown();
}

const SCHEMA: &str = "CREATE TABLE users (id INT NOT NULL, name TEXT);";

#[test]
fn names_are_colored_by_what_the_checked_schema_says_they_are() {
    let mut client = Client::with(&json!({}), &json!({}), Some(SCHEMA));
    client.open(
        "file:///work/q.py",
        "q = \"--sql\\nSELECT u.name, now() FROM users AS u\"\n",
    );
    client.notification("textDocument/publishDiagnostics");
    let (class, variable, property, function) = (7, 10, 11, 6);
    let (declaration, default_library) = (1, 2);
    let kinds: Vec<_> = client
        .tokens("file:///work/q.py")
        .into_iter()
        .map(|(_, _, length, kind, modifiers)| (length, kind, modifiers))
        .collect();
    assert_eq!(
        kinds[2..],
        [
            (1, variable, 0),
            (4, property, 0),
            (3, function, default_library),
            (4, 0, 0),
            (5, class, 0),
            (2, 0, 0),
            (1, variable, declaration),
        ]
    );
    client.shutdown();
}

#[test]
fn a_check_that_loads_the_schema_asks_for_the_tokens_again() {
    let capabilities = json!({"workspace": {"semanticTokens": {"refreshSupport": true}}});
    let client = Client::with(&capabilities, &json!({}), Some(SCHEMA));
    client.open(
        "file:///work/q.py",
        "q = \"--sql\\nSELECT name FROM users\"\n",
    );
    client.asked("workspace/semanticTokens/refresh");
    client.shutdown();
}
