//! UTF-16 code unit offsets of byte offsets, the positions editors and browsers count in.

/// The UTF-16 offset of any byte of a text in logarithmic time: a long line with many
/// diagnostics or highlights is not counted again for each.
///
/// ```
/// use sqlty_python::utf16::Utf16;
///
/// let units = Utf16::new("é = '𝄞' + x");
/// assert_eq!([units.at(2), units.at(10), units.at(15)], [1, 7, 12]);
/// ```
pub struct Utf16 {
    /// The byte after each non-ASCII character, and the bytes saved up to it: a character
    /// takes fewer UTF-16 units than UTF-8 bytes.
    saved: Vec<(usize, usize)>,
}

impl Utf16 {
    /// The offsets of `text`.
    pub fn new(text: &str) -> Self {
        let mut total = 0;
        let saved = text
            .char_indices()
            .filter(|(_, char)| !char.is_ascii())
            .map(|(at, char)| {
                total += char.len_utf8() - char.len_utf16();
                (at + char.len_utf8(), total)
            })
            .collect();
        Self { saved }
    }

    /// The UTF-16 offset of the character boundary `byte`.
    pub fn at(&self, byte: usize) -> usize {
        let before = self.saved.partition_point(|&(end, _)| end <= byte);
        byte - before.checked_sub(1).map_or(0, |i| self.saved[i].1)
    }
}
