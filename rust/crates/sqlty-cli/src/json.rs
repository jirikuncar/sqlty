//! JSON as Python's `json.dumps(value, indent=2)` writes it: keys in order, non-ASCII
//! escaped.

/// A JSON value whose objects keep their keys in order.
#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Int(i64),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    /// An object of `fields`.
    pub fn object<const N: usize>(fields: [(&str, Json); N]) -> Self {
        Self::Object(
            fields
                .into_iter()
                .map(|(key, value)| (key.to_string(), value))
                .collect(),
        )
    }

    /// The value, indented by two spaces a level.
    pub fn dumps(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }

    fn write(&self, out: &mut String, depth: usize) {
        let indent = |out: &mut String, depth: usize| {
            out.push('\n');
            out.push_str(&"  ".repeat(depth));
        };
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
            Self::Int(value) => out.push_str(&value.to_string()),
            Self::Str(value) => string(out, value),
            Self::Array(items) if items.is_empty() => out.push_str("[]"),
            Self::Object(fields) if fields.is_empty() => out.push_str("{}"),
            Self::Array(items) => {
                out.push('[');
                for (at, item) in items.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }
                    indent(out, depth + 1);
                    item.write(out, depth + 1);
                }
                indent(out, depth);
                out.push(']');
            }
            Self::Object(fields) => {
                out.push('{');
                for (at, (key, value)) in fields.iter().enumerate() {
                    if at > 0 {
                        out.push(',');
                    }
                    indent(out, depth + 1);
                    string(out, key);
                    out.push_str(": ");
                    value.write(out, depth + 1);
                }
                indent(out, depth);
                out.push('}');
            }
        }
    }
}

impl From<&str> for Json {
    fn from(value: &str) -> Self {
        Self::Str(value.to_string())
    }
}

impl From<String> for Json {
    fn from(value: String) -> Self {
        Self::Str(value)
    }
}

impl From<usize> for Json {
    fn from(value: usize) -> Self {
        Self::Int(i64::try_from(value).unwrap_or(i64::MAX))
    }
}

impl From<bool> for Json {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl<T: Into<Json>> From<Option<T>> for Json {
    fn from(value: Option<T>) -> Self {
        value.map_or(Self::Null, Into::into)
    }
}

fn string(out: &mut String, text: &str) {
    out.push('"');
    for char in text.chars() {
        match char {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(char),
            char => {
                let mut units = [0; 2];
                for unit in char.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dumps_as_python() {
        let value = Json::Array(vec![Json::object([
            ("file", "é😀\"".into()),
            ("fix", Json::Array(Vec::new())),
            ("line", 3.into()),
            ("none", Json::Null),
        ])]);
        let expected = "[\n  {\n    \"file\": \"\\u00e9\\ud83d\\ude00\\\"\",\n    \"fix\": [],\n    \"line\": 3,\n    \"none\": null\n  }\n]";
        assert_eq!(value.dumps(), expected);
        assert_eq!(Json::Array(Vec::new()).dumps(), "[]");
    }
}
