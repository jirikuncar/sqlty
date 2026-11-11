//! Row-type description ([`TypeInfo`]).

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use ruff_python_ast::Expr;

use super::ty::Ty;

/// A Python `dict` with `str` keys: insertion order, a key set again keeps its place.
#[derive(Clone, Debug)]
pub struct Dict<V>(Vec<(String, V)>);

impl<V> Default for Dict<V> {
    fn default() -> Self {
        Dict(Vec::new())
    }
}

impl<V> Dict<V> {
    pub fn new() -> Self {
        Dict(Vec::new())
    }

    pub fn insert(&mut self, key: impl Into<String> + AsRef<str>, value: V) {
        match self.0.iter_mut().find(|(k, _)| k == key.as_ref()) {
            Some(entry) => entry.1 = value,
            None => self.0.push((key.into(), value)),
        }
    }

    pub fn get(&self, key: &str) -> Option<&V> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &V)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(|(k, _)| k.as_str())
    }

    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.0.iter().map(|(_, v)| v)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// `self | other`.
    #[must_use]
    pub fn merged(&self, other: &Dict<V>) -> Dict<V>
    where
        V: Clone,
    {
        let mut out = self.clone();
        out.update(other);
        out
    }

    /// `self |= other`.
    pub fn update(&mut self, other: &Dict<V>)
    where
        V: Clone,
    {
        for (key, value) in other.iter() {
            self.insert(key, value.clone());
        }
    }
}

impl<K: Into<String> + AsRef<str>, V> FromIterator<(K, V)> for Dict<V> {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut out = Dict::new();
        for (key, value) in iter {
            out.insert(key, value);
        }
        out
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    TypedDict,
    NamedTuple,
    Dataclass,
    BaseModel,
    Protocol,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::TypedDict => "TypedDict",
            Kind::NamedTuple => "NamedTuple",
            Kind::Dataclass => "dataclass",
            Kind::BaseModel => "BaseModel",
            Kind::Protocol => "Protocol",
        }
    }
}

/// An annotation as written, with the source it is an expression of.
#[derive(Clone, Debug)]
pub struct Annotation {
    pub expr: Expr,
    pub source: Arc<str>,
}

impl Annotation {
    pub fn unparse(&self) -> String {
        crate::unparse::unparse(&self.expr, &self.source)
    }
}

/// A user-defined row type read from source.
///
/// `required`, `closed` and `extra_items` describe `TypedDict` keys (PEP 655, PEP 728);
/// other kinds have every field required and are open. `field_types` holds each field's
/// resolved type (`None` when unknown), `owner` names the class whose header set the
/// openness, `bases` its `TypedDict` bases and `errors` its PEP 728 inheritance violations.
/// For Pydantic models, `columns` maps each accepted column name (field name or validation
/// alias) to its field, `opaque` names the fields whose validation names are not statically
/// known and `allows_extra` whether it keeps undeclared columns. `lax` names the fields
/// that accept lax-mode conversions. `annotations` holds each field's annotation,
/// qualifiers stripped and `| None` kept. `coerced` is `field_types` as a Pydantic
/// validator reads a row that is not Pydantic's (`coerce-row-types`).
#[derive(Clone, Debug)]
pub struct TypeInfo {
    pub name: String,
    pub fields: Vec<(String, String)>,
    pub kind: Kind,
    pub required: BTreeSet<String>,
    pub closed: bool,
    pub extra_items: Option<String>,
    pub extra_items_readonly: bool,
    pub extra_items_type: Option<Ty>,
    pub field_types: Dict<Option<Ty>>,
    pub owner: String,
    pub lineno: usize,
    pub bases: Vec<String>,
    pub errors: Vec<String>,
    pub columns: Dict<String>,
    pub opaque: BTreeSet<String>,
    pub allows_extra: bool,
    pub lax: BTreeSet<String>,
    pub annotations: Dict<Annotation>,
    pub aliases: Arc<HashMap<String, String>>,
    pub module: Option<String>,
    pub coerced: Option<Dict<Option<Ty>>>,
    /// `T` of `class Row[T]`, or of a `Generic[T]` base.
    pub type_params: Vec<String>,
}

impl TypeInfo {
    pub fn has_field(&self, name: &str) -> bool {
        self.fields.iter().any(|(field, _)| field == name)
    }

    /// The field a query column fills, by name (a Pydantic alias counts).
    pub fn field_for(&self, column: &str) -> Option<&str> {
        if self.kind == Kind::BaseModel {
            return self.columns.get(column).map(String::as_str);
        }
        self.fields
            .iter()
            .find(|(field, _)| field == column)
            .map(|(field, _)| field.as_str())
    }

    /// The query column names that fill `field`, the Pydantic alias first.
    pub fn column_names<'a>(&'a self, field: &'a str) -> Vec<&'a str> {
        if self.kind == Kind::BaseModel {
            return self
                .columns
                .iter()
                .filter(|(_, f)| f.as_str() == field)
                .map(|(column, _)| column)
                .collect();
        }
        vec![field]
    }

    /// Whether columns match keys by name (closed or `extra_items` `TypedDict`).
    pub fn keyed(&self) -> bool {
        self.closed || self.extra_items.is_some()
    }
}
