//! Standard-library classes, as nominal classes built from typeshed's class graph.

use std::collections::HashMap;
use std::ops::Not;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use super::stdlib_table::{CLASSES, MODULES};
use super::ty::{builtins, Class};

/// A Python version, `(major, minor)`.
pub type Version = (u8, u8);

/// `stdlib_class`: the class `module.Name` of the standard library of Python `version`.
///
/// A column type is itself; any other class is a stand-in deriving from the column type
/// the real one does, and from the stand-in of `enum.Enum` when it is an enum.
pub fn stdlib_class(qualname: &str, version: Version) -> Option<Arc<Class>> {
    static BUILT: LazyLock<Mutex<HashMap<&'static str, Arc<Class>>>> =
        LazyLock::new(Mutex::default);
    let b = builtins();
    if qualname.contains('.') {
        if let Some(column) = b.column(qualname) {
            return Some(column.clone());
        }
    }
    let index = CLASSES
        .binary_search_by_key(&qualname, |(name, _)| name)
        .ok()?;
    let (key, code) = CLASSES[index];
    let (module, name) = qualname.rsplit_once('.')?;
    if !available(module, version) {
        return None;
    }
    if let Some(found) = BUILT
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(key)
    {
        return Some(found.clone());
    }
    let mut bases = Vec::new();
    match code & 7 {
        1 => bases.push(b.int.clone()),
        2 => bases.push(b.str.clone()),
        3 => bases.push(b.dict.clone()),
        4 => bases.push(b.any.clone()),
        5 => bases.push(b.list.clone()),
        _ => {}
    }
    if code & 8 != 0 {
        bases.extend(stdlib_class("enum.Enum", version));
    }
    let class = Class::new(name, module, &bases)?;
    Some(
        BUILT
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(key)
            .or_insert(class)
            .clone(),
    )
}

fn available(module: &str, version: Version) -> bool {
    module
        .match_indices('.')
        .map(|(end, _)| &module[..end])
        .chain([module])
        .all(|prefix| {
            MODULES
                .binary_search_by_key(&prefix, |(name, _, _)| name)
                .is_ok_and(|index| {
                    let (_, low, high) = MODULES[index];
                    low > version || high.is_some_and(|high| version > high)
                })
                .not()
        })
}
