//! Every request of the playground on arbitrary input: the first byte picks the operation
//! (bits 0-1), the dialect (bits 2-3) and the offset (bits 4-7, in sixteenths of the source);
//! the rest is the Python source, then a NUL and the schema.

#![no_main]

use libfuzzer_sys::fuzz_target;

const OPS: [&str; 4] = ["analyze", "complete", "hover", "format"];
const DIALECTS: [&str; 4] = ["postgres", "sqlite", "duckdb", "mysql"];

fuzz_target!(|data: &[u8]| {
    let Some((&head, rest)) = data.split_first() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(rest) else {
        return;
    };
    let (source, schema) = text.split_once('\0').unwrap_or((text, ""));
    let offset = source.encode_utf16().count() * usize::from(head >> 4) / 16;
    let request = serde_json::json!({
        "op": OPS[usize::from(head & 3)],
        "dialect": DIALECTS[usize::from(head >> 2 & 3)],
        "source": source,
        "schema": schema,
        "offset": offset,
    });
    sqlty_wasm::run(&request.to_string());
});
