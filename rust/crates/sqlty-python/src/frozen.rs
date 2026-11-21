//! Maps read after they are built, kept as sorted slices: every module holds its tables for
//! the whole run, and a hash table would hold its spare capacity for each of them.

use std::collections::HashMap;

/// Entries sorted by key, one per key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frozen<V>(Box<[(String, V)]>);

/// Local names to dotted names.
pub type Table = Frozen<String>;

impl<V> Default for Frozen<V> {
    fn default() -> Self {
        Self(Box::default())
    }
}

impl<V> Frozen<V> {
    /// The value of `key`.
    pub fn get(&self, key: &str) -> Option<&V> {
        let at = self.0.binary_search_by(|(k, _)| k.as_str().cmp(key)).ok()?;
        Some(&self.0[at].1)
    }

    /// Whether there is no entry.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The entries, by key.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&String, &V)> {
        self.0.iter().map(|(key, value)| (key, value))
    }

    /// The keys, in order.
    pub fn keys(&self) -> impl ExactSizeIterator<Item = &String> {
        self.0.iter().map(|(key, _)| key)
    }

    /// The values, by key.
    pub fn values(&self) -> impl ExactSizeIterator<Item = &V> {
        self.0.iter().map(|(_, value)| value)
    }
}

impl<V, S> From<HashMap<String, V, S>> for Frozen<V> {
    fn from(map: HashMap<String, V, S>) -> Self {
        let mut entries: Vec<(String, V)> = map.into_iter().collect();
        entries.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));
        Self(entries.into_boxed_slice())
    }
}

/// A later pair wins, as in a `HashMap`.
impl<V> FromIterator<(String, V)> for Frozen<V> {
    fn from_iter<I: IntoIterator<Item = (String, V)>>(pairs: I) -> Self {
        let mut entries: Vec<(String, V)> = pairs.into_iter().collect();
        entries.reverse();
        entries.sort_by(|(a, _), (b, _)| a.cmp(b));
        entries.dedup_by(|earlier, later| earlier.0 == later.0);
        Self(entries.into_boxed_slice())
    }
}
