//! Shapes of JSON documents.
//!
//! JSON built in SQL (`jsonb_build_object()`, `jsonb_agg()`, `to_jsonb()`, ...) has a
//! structure the query spells out. Drivers decode it into native values (dictionaries and
//! lists in Python), so the shape is what a row type is checked against.

use std::fmt::Write;

/// A value inside a JSON document, with whether it can be `null`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonValue {
    pub shape: JsonShape,
    pub nullable: bool,
}

impl JsonValue {
    pub fn new(shape: JsonShape, nullable: bool) -> Self {
        Self { shape, nullable }
    }

    /// The value that is `a` or `b`, such as the result of `COALESCE(a, b)`. A `null` is a
    /// nullable value of the other shape, so `[1, null]` holds nullable integers.
    pub fn unify(a: &JsonValue, b: &JsonValue) -> JsonValue {
        match (&a.shape, &b.shape) {
            (JsonShape::Null, _) => JsonValue::new(b.shape.clone(), true),
            (_, JsonShape::Null) => JsonValue::new(a.shape.clone(), true),
            _ => JsonValue::new(a.shape.unify(&b.shape), a.nullable || b.nullable),
        }
    }

    /// Spell a `null` as a nullable value of unknown shape.
    fn normalized(self) -> JsonValue {
        if self.shape == JsonShape::Null {
            JsonValue::new(JsonShape::Any, true)
        } else {
            self
        }
    }
}

/// A member of a JSON object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsonField {
    pub name: String,
    pub value: JsonValue,
    /// Whether the member can be missing from the object.
    pub optional: bool,
}

/// The shape of a JSON value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsonShape {
    /// Any JSON value: nothing is known.
    Any,
    /// No value at all: the elements of an empty array.
    Never,
    /// The JSON `null`.
    Null,
    Boolean,
    /// A number known to be integral.
    Integer,
    Number,
    String,
    Array(Box<JsonValue>),
    /// An object with known members.
    Object(Vec<JsonField>),
    /// An object with arbitrary keys, such as `jsonb_object_agg()` builds.
    Map(Box<JsonValue>),
}

/// Deepest nesting a JSON literal may have before it is not read.
const MAX_DEPTH: usize = 32;

impl JsonShape {
    /// The shape of a JSON text such as `'[]'` or `'{"a": 1}'`, or `None` if it isn't JSON.
    ///
    /// ```
    /// use sqlty_sql::json::JsonShape;
    /// assert!(JsonShape::of_literal("[1, 2]").is_some());
    /// assert!(JsonShape::of_literal("{").is_none());
    /// ```
    pub fn of_literal(text: &str) -> Option<JsonShape> {
        let mut parser = Literal {
            bytes: text.as_bytes(),
            at: 0,
        };
        let shape = parser.value(0)?;
        parser.skip_space();
        (parser.at == parser.bytes.len()).then_some(shape)
    }

    /// The shape of a value that is `self` or `other`.
    pub fn unify(&self, other: &JsonShape) -> JsonShape {
        match (self, other) {
            (a, b) if a == b => a.clone(),
            (JsonShape::Never, x) | (x, JsonShape::Never) => x.clone(),
            (JsonShape::Integer, JsonShape::Number) | (JsonShape::Number, JsonShape::Integer) => {
                JsonShape::Number
            }
            (JsonShape::Array(a), JsonShape::Array(b)) => {
                JsonShape::Array(Box::new(JsonValue::unify(a, b)))
            }
            (JsonShape::Object(a), JsonShape::Object(b)) => JsonShape::Object(unify_fields(a, b)),
            // An empty object (`COALESCE(jsonb_object_agg(...), '{}')`) is an empty map.
            (JsonShape::Map(values), JsonShape::Object(fields))
            | (JsonShape::Object(fields), JsonShape::Map(values))
                if fields.is_empty() =>
            {
                JsonShape::Map(values.clone())
            }
            (JsonShape::Map(a), JsonShape::Map(b)) => {
                JsonShape::Map(Box::new(JsonValue::unify(a, b)))
            }
            _ => JsonShape::Any,
        }
    }

    /// The shape as JSON text, for Python to read: a scalar is its name (`"integer"`), an
    /// array `{"array": V}`, a map `{"map": V}` and an object `{"object": [F, ...]}`, where
    /// a value `V` is `{"shape": S, "nullable": bool}` and a member `F` is
    /// `{"name": str, "value": V, "optional": bool}`.
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        let scalar = |name: &str| format!("\"{name}\"");
        match self {
            JsonShape::Any => out.push_str(&scalar("any")),
            JsonShape::Never => out.push_str(&scalar("never")),
            JsonShape::Null => out.push_str(&scalar("null")),
            JsonShape::Boolean => out.push_str(&scalar("boolean")),
            JsonShape::Integer => out.push_str(&scalar("integer")),
            JsonShape::Number => out.push_str(&scalar("number")),
            JsonShape::String => out.push_str(&scalar("string")),
            JsonShape::Array(value) => {
                out.push_str("{\"array\":");
                value.write(out);
                out.push('}');
            }
            JsonShape::Map(value) => {
                out.push_str("{\"map\":");
                value.write(out);
                out.push('}');
            }
            JsonShape::Object(fields) => {
                out.push_str("{\"object\":[");
                for (i, field) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str("{\"name\":");
                    write_string(out, &field.name);
                    out.push_str(",\"value\":");
                    field.value.write(out);
                    let _ = write!(out, ",\"optional\":{}}}", field.optional);
                }
                out.push_str("]}");
            }
        }
    }
}

impl JsonValue {
    fn write(&self, out: &mut String) {
        out.push_str("{\"shape\":");
        self.shape.write(out);
        let _ = write!(out, ",\"nullable\":{}}}", self.nullable);
    }
}

fn write_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Members of two objects: one in only one of them is optional.
fn unify_fields(a: &[JsonField], b: &[JsonField]) -> Vec<JsonField> {
    let mut fields: Vec<JsonField> = a
        .iter()
        .map(|field| match b.iter().find(|f| f.name == field.name) {
            Some(other) => JsonField {
                name: field.name.clone(),
                value: JsonValue::unify(&field.value, &other.value),
                optional: field.optional || other.optional,
            },
            None => JsonField {
                optional: true,
                ..field.clone()
            },
        })
        .collect();
    fields.extend(
        b.iter()
            .filter(|field| !a.iter().any(|f| f.name == field.name))
            .map(|field| JsonField {
                optional: true,
                ..field.clone()
            }),
    );
    fields
}

/// A reader of JSON text that keeps only the shape.
struct Literal<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Literal<'_> {
    fn skip_space(&mut self) {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        self.skip_space();
        let found = self.bytes.get(self.at) == Some(&byte);
        if found {
            self.at += 1;
        }
        found
    }

    fn keyword(&mut self, word: &str, shape: JsonShape) -> Option<JsonShape> {
        let end = self.at + word.len();
        if self.bytes.get(self.at..end)? == word.as_bytes() {
            self.at = end;
            Some(shape)
        } else {
            None
        }
    }

    fn value(&mut self, depth: usize) -> Option<JsonShape> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.skip_space();
        match *self.bytes.get(self.at)? {
            b'n' => self.keyword("null", JsonShape::Null),
            b't' => self.keyword("true", JsonShape::Boolean),
            b'f' => self.keyword("false", JsonShape::Boolean),
            b'"' => self.string().map(|_| JsonShape::String),
            b'[' => self.array(depth),
            b'{' => self.object(depth),
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    fn number(&mut self) -> Option<JsonShape> {
        let start = self.at;
        while self
            .bytes
            .get(self.at)
            .is_some_and(|b| matches!(b, b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'))
        {
            self.at += 1;
        }
        let text = std::str::from_utf8(&self.bytes[start..self.at]).ok()?;
        text.parse::<f64>().ok()?;
        Some(if text.contains(['.', 'e', 'E']) {
            JsonShape::Number
        } else {
            JsonShape::Integer
        })
    }

    /// A string, decoded far enough to compare object keys.
    fn string(&mut self) -> Option<String> {
        self.at += 1;
        let mut out = Vec::new();
        loop {
            let byte = *self.bytes.get(self.at)?;
            self.at += 1;
            match byte {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let escape = *self.bytes.get(self.at)?;
                    self.at += 1;
                    match escape {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'"' | b'\\' | b'/' => out.push(escape),
                        b'u' => {
                            let hex = self.bytes.get(self.at..self.at + 4)?;
                            let code =
                                u32::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
                            self.at += 4;
                            let c = char::from_u32(code).unwrap_or('\u{fffd}');
                            out.extend_from_slice(c.to_string().as_bytes());
                        }
                        _ => return None,
                    }
                }
                byte => out.push(byte),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Option<JsonShape> {
        self.at += 1;
        let mut element: Option<JsonValue> = None;
        if !self.eat(b']') {
            loop {
                let item = JsonValue::new(self.value(depth + 1)?, false);
                element = Some(match element {
                    Some(seen) => JsonValue::unify(&seen, &item),
                    None => item,
                });
                if self.eat(b']') {
                    break;
                }
                if !self.eat(b',') {
                    return None;
                }
            }
        }
        let element = element.unwrap_or_else(|| JsonValue::new(JsonShape::Never, false));
        Some(JsonShape::Array(Box::new(element.normalized())))
    }

    fn object(&mut self, depth: usize) -> Option<JsonShape> {
        self.at += 1;
        let mut fields: Vec<JsonField> = Vec::new();
        if !self.eat(b'}') {
            loop {
                self.skip_space();
                if self.bytes.get(self.at) != Some(&b'"') {
                    return None;
                }
                let name = self.string()?;
                if !self.eat(b':') {
                    return None;
                }
                let value = JsonValue::new(self.value(depth + 1)?, false).normalized();
                // The last of repeated keys wins, as in `jsonb`.
                fields.retain(|field| field.name != name);
                fields.push(JsonField {
                    name,
                    value,
                    optional: false,
                });
                if self.eat(b'}') {
                    break;
                }
                if !self.eat(b',') {
                    return None;
                }
            }
        }
        Some(JsonShape::Object(fields))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn array_of(shape: JsonShape, nullable: bool) -> JsonShape {
        JsonShape::Array(Box::new(JsonValue::new(shape, nullable)))
    }

    fn field(name: &str, shape: JsonShape, optional: bool) -> JsonField {
        JsonField {
            name: name.to_string(),
            value: JsonValue::new(shape, false),
            optional,
        }
    }

    #[test]
    fn empty_array_literal_has_no_element() {
        assert_eq!(
            JsonShape::of_literal("[]"),
            Some(array_of(JsonShape::Never, false))
        );
    }

    #[test]
    fn null_in_array_literal_makes_elements_nullable() {
        assert_eq!(
            JsonShape::of_literal("[1, null]"),
            Some(array_of(JsonShape::Integer, true))
        );
    }

    #[test]
    fn object_literal_lists_members() {
        assert_eq!(
            JsonShape::of_literal(r#"{"a": 1, "b": "x", "a": 2.5}"#),
            Some(JsonShape::Object(vec![
                field("b", JsonShape::String, false),
                field("a", JsonShape::Number, false),
            ]))
        );
    }

    #[test]
    fn invalid_literal_has_no_shape() {
        assert_eq!(JsonShape::of_literal("{"), None);
        assert_eq!(JsonShape::of_literal("[1,]"), None);
        assert_eq!(JsonShape::of_literal("1 2"), None);
    }

    #[test]
    fn deeply_nested_literal_is_not_read() {
        let text = "[".repeat(10_000);
        assert_eq!(JsonShape::of_literal(&text), None);
    }

    #[test]
    fn unify_with_empty_array_keeps_element() {
        let objects = array_of(
            JsonShape::Object(vec![field("a", JsonShape::String, false)]),
            false,
        );
        let empty = JsonShape::of_literal("[]").unwrap();
        assert_eq!(objects.unify(&empty), objects);
        assert_eq!(empty.unify(&objects), objects);
    }

    #[test]
    fn unify_objects_makes_missing_members_optional() {
        let a = JsonShape::Object(vec![field("a", JsonShape::Integer, false)]);
        let b = JsonShape::Object(vec![field("b", JsonShape::String, false)]);
        assert_eq!(
            a.unify(&b),
            JsonShape::Object(vec![
                field("a", JsonShape::Integer, true),
                field("b", JsonShape::String, true),
            ])
        );
    }

    #[test]
    fn unify_empty_object_with_map_is_map() {
        let map = JsonShape::Map(Box::new(JsonValue::new(JsonShape::Integer, false)));
        let empty = JsonShape::of_literal("{}").unwrap();
        assert_eq!(map.unify(&empty), map);
    }

    #[test]
    fn unify_different_scalars_is_any() {
        assert_eq!(JsonShape::String.unify(&JsonShape::Integer), JsonShape::Any);
        assert_eq!(
            JsonShape::Integer.unify(&JsonShape::Number),
            JsonShape::Number
        );
    }

    #[test]
    fn to_json_spells_nested_shapes() {
        let shape = JsonShape::Object(vec![field("tags", array_of(JsonShape::String, true), true)]);
        assert_eq!(
            shape.to_json(),
            r#"{"object":[{"name":"tags","value":{"shape":{"array":{"shape":"string","nullable":true}},"nullable":false},"optional":true}]}"#
        );
    }
}
