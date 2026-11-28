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
    assert_eq!(client.tokens("file:///work/q.py")[2], (0, 19, 3, 6));
    client.shutdown();
}
