//! Edits to a source: what `rewrite`, `strip` and the fixes of [`crate::checker`]
//! diagnostics change, applied in one pass.

use std::ops::Range;

/// Replace `range` of the source with `replacement`; an empty range inserts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    /// Byte range of the source to replace.
    pub range: Range<usize>,
    /// The text that takes its place.
    pub replacement: String,
}

impl Edit {
    /// An edit replacing `range` with `replacement`.
    pub fn new(range: Range<usize>, replacement: impl Into<String>) -> Self {
        Edit {
            range,
            replacement: replacement.into(),
        }
    }
}

/// `source` with `edits` applied in source order, and the edits that were. An edit that
/// overlaps one before it is left out, for a later pass over the edited source.
///
/// Example:
///
/// ```
/// use sqlty_python::edit::{apply_edits, Edit};
///
/// let edits = vec![Edit::new(4..5, "y"), Edit::new(0..0, "# "), Edit::new(3..5, "zz")];
/// let (edited, applied) = apply_edits("abc x", edits);
/// assert_eq!(edited, "# abczz");
/// assert_eq!(applied.len(), 2);
/// ```
pub fn apply_edits(source: &str, mut edits: Vec<Edit>) -> (String, Vec<Edit>) {
    edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
    let mut result = String::with_capacity(source.len());
    let mut applied = Vec::with_capacity(edits.len());
    let mut pos = 0;
    for edit in edits {
        if edit.range.start < pos {
            continue;
        }
        result.push_str(&source[pos..edit.range.start]);
        result.push_str(&edit.replacement);
        pos = edit.range.end;
        applied.push(edit);
    }
    result.push_str(&source[pos..]);
    (result, applied)
}
