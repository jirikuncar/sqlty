//! Byte offsets of the checker against the positions of the protocol.

use lsp_types::{Position, PositionEncodingKind, Range, TextEdit};

/// Where each line of a text starts, to turn byte offsets into positions and back.
pub(crate) struct LineIndex<'a> {
    text: &'a str,
    starts: Vec<usize>,
    utf8: bool,
}

impl<'a> LineIndex<'a> {
    pub(crate) fn new(text: &'a str, encoding: &PositionEncodingKind) -> Self {
        let starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(at, _)| at + 1))
            .collect();
        Self {
            text,
            starts,
            utf8: *encoding == PositionEncodingKind::UTF8,
        }
    }

    pub(crate) fn position(&self, offset: usize) -> Position {
        let offset = self.boundary(offset.min(self.text.len()));
        let line = self.starts.partition_point(|&start| start <= offset) - 1;
        let before = &self.text[self.starts[line]..offset];
        let character = if self.utf8 {
            before.len()
        } else {
            before.encode_utf16().count()
        };
        Position::new(count(line), count(character))
    }

    pub(crate) fn range(&self, start: usize, end: usize) -> Range {
        Range::new(self.position(start), self.position(end))
    }

    pub(crate) fn offset(&self, position: Position) -> usize {
        let Some(&start) = self.starts.get(position.line as usize) else {
            return self.text.len();
        };
        let end = self
            .starts
            .get(position.line as usize + 1)
            .map_or(self.text.len(), |&next| next);
        let mut units = 0;
        for (at, ch) in self.text[start..end].char_indices() {
            if units >= position.character as usize {
                return start + at;
            }
            units += if self.utf8 {
                ch.len_utf8()
            } else {
                ch.len_utf16()
            };
        }
        end
    }

    fn boundary(&self, mut offset: usize) -> usize {
        while !self.text.is_char_boundary(offset) {
            offset -= 1;
        }
        offset
    }
}

/// One edit that turns `old` into `new`: the span between their common prefix and suffix.
pub(crate) fn diff(old: &str, new: &str, index: &LineIndex<'_>) -> Option<TextEdit> {
    if old == new {
        return None;
    }
    let prefix = old
        .char_indices()
        .zip(new.chars())
        .find(|((_, a), b)| a != b)
        .map_or(old.len().min(new.len()), |((at, _), _)| at);
    let suffix = old[prefix..]
        .chars()
        .rev()
        .zip(new[prefix..].chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(ch, _)| ch.len_utf8())
        .sum::<usize>();
    Some(TextEdit::new(
        index.range(prefix, old.len() - suffix),
        new[prefix..new.len() - suffix].to_string(),
    ))
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_count_utf16_units_unless_utf8_is_agreed() {
        let text = "é = 1\nx = '𝄞' + y\n";
        let utf16 = LineIndex::new(text, &PositionEncodingKind::UTF16);
        let y = text.find('y').unwrap();
        assert_eq!(utf16.position(y), Position::new(1, 11));
        assert_eq!(utf16.offset(Position::new(1, 11)), y);
        let utf8 = LineIndex::new(text, &PositionEncodingKind::UTF8);
        assert_eq!(utf8.position(y), Position::new(1, 13));
        assert_eq!(utf8.offset(Position::new(1, 13)), y);
        assert_eq!(utf16.position(1), Position::new(0, 0), "inside `é`");
        assert_eq!(utf16.offset(Position::new(9, 0)), text.len());
    }

    #[test]
    fn diff_replaces_the_changed_middle() {
        let old = "a = SQL[tuple[int]](q)\n";
        let new = "a = SQL[tuple[float]](q)\n";
        let index = LineIndex::new(old, &PositionEncodingKind::UTF16);
        let edit = diff(old, new, &index).unwrap();
        assert_eq!(
            edit.range,
            Range::new(Position::new(0, 14), Position::new(0, 16))
        );
        assert_eq!(edit.new_text, "floa", "`t]` is common");
        assert_eq!(diff(old, old, &index), None);
        let longer = diff(
            "ab",
            "abc",
            &LineIndex::new("ab", &PositionEncodingKind::UTF16),
        );
        assert_eq!(longer.unwrap().new_text, "c");
    }
}
