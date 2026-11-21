//! Links that open a prefilled GitHub bug report for a crash.

use std::any::Any;

const NEW_ISSUE: &str = "https://github.com/jirikuncar/sqlty/issues/new?template=bug_report.yml";

/// Characters of the cause kept in the title and the output field: a link stays well under
/// GitHub's URL limit.
const TITLE: usize = 80;
const OUTPUT: usize = 2000;

/// The message of a panic's `payload`.
pub fn cause(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("unknown cause")
}

/// A new-issue link for a crash with `cause`, prefilling the bug report form's title,
/// output, version and environment fields.
pub fn issue_url(cause: &str) -> String {
    let title = cause.lines().next().unwrap_or_default();
    let fields = [
        ("title", format!("Crash: {}", truncate(title, TITLE))),
        ("output", truncate(cause, OUTPUT).to_owned()),
        ("version", env!("CARGO_PKG_VERSION").to_owned()),
        (
            "environment",
            format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        ),
    ];
    let mut url = NEW_ISSUE.to_owned();
    for (key, value) in fields {
        url.push('&');
        url.push_str(key);
        url.push('=');
        url.push_str(&encode(&value));
    }
    url
}

fn truncate(text: &str, chars: usize) -> &str {
    text.char_indices()
        .nth(chars)
        .map_or(text, |(end, _)| &text[..end])
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}
