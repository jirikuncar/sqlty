//! The query under a cursor: which string `check` reads as a query, and where the cursor is
//! in that query.
//!
//! [`Queries::new`] finds a file's queries once, O(file size), with the recognition `check`
//! uses: a `--sql` / `-- sql` marked string, a `{wrapper}[T](...)` argument, or a string a
//! helper call passes as its query (found in Python, given as `helper_starts`).
//! [`Queries::at`] is then a binary search, O(log S) for S queries.
//!
//! ```
//! use sqlty_python::locate::{Queries, Route};
//! use sqlty_python::rewriter::ImportStyle;
//!
//! let source = "q = \"--sql\\nSELECT id FROM users\"\n";
//! let queries = Queries::new(source, &ImportStyle::parse(None).unwrap(), &[]);
//! let at = queries.at(source.find("id").unwrap()).unwrap();
//! assert_eq!((at.text.as_str(), at.offset, at.route), ("--sql\nSELECT id FROM users", 13, Route::Marker));
//! assert_eq!(at.source_offset(at.offset), Some(source.find("id").unwrap()));
//! assert!(queries.at(1).is_none());
//! ```

use std::ops::Range;

use crate::rewriter::{after_marker, escapes, extract_string_content, ImportStyle};
use crate::scan::Scan;

/// How `text` spells the marker, however the string writes it.
const MARKER: &str = "--sql";

/// How `check` recognised a query.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// A `--sql` or `-- sql` marked string.
    Marker,
    /// The argument of a `{wrapper}[T](...)` call.
    Wrapper,
    /// The query argument of a helper or a `[[tool.sqlty.functions]]` function.
    Helper,
}

impl Route {
    /// `marker`, `wrapper` or `helper`.
    pub fn name(self) -> &'static str {
        match self {
            Route::Marker => "marker",
            Route::Wrapper => "wrapper",
            Route::Helper => "helper",
        }
    }
}

/// The checked queries of a source, sorted, for finding the one at an offset.
pub struct Queries<'s> {
    source: &'s str,
    sites: Vec<(Vec<Range<usize>>, Route)>,
}

impl<'s> Queries<'s> {
    /// The queries `check` reads in `source`. `helper_starts` are the byte offsets of the
    /// string literals helper calls pass as their query; a marked string among them stays
    /// a marker query.
    pub fn new(source: &'s str, style: &ImportStyle, helper_starts: &[usize]) -> Self {
        let scan = Scan::new(source, &style.wrapper_name);
        let mut sites: Vec<_> = scan
            .marked_strings()
            .into_iter()
            .map(|(_, strings, wrapper)| {
                let route = if wrapper {
                    Route::Wrapper
                } else {
                    Route::Marker
                };
                (strings, route)
            })
            .collect();
        let groups = scan.groups();
        for &start in helper_starts {
            let at = groups.partition_point(|group| group.start < start);
            if let Some(group) = groups.get(at).filter(|group| group.start == start) {
                sites.push((scan.spans(group), Route::Helper));
            }
        }
        sites.sort_by_key(|(strings, _)| strings[0].start);
        sites.dedup_by_key(|(strings, _)| strings[0].start);
        Queries { source, sites }
    }

    /// The query whose string content holds byte `offset` (between the quotes, either end
    /// included); `None` outside a query, in its marker, or in an f-, t- or bytes string.
    pub fn at(&self, offset: usize) -> Option<QueryAt> {
        let next = self
            .sites
            .partition_point(|(strings, _)| strings[0].start <= offset);
        let (strings, route) = &self.sites[next.checked_sub(1)?];
        if offset > strings[strings.len() - 1].end {
            return None;
        }
        let mut pieces = Vec::new();
        let mut value = String::new();
        let mut cursor = None;
        for span in strings {
            let literal = &self.source[span.clone()];
            let quoted = literal.trim_start_matches(|c: char| c.is_ascii_alphabetic());
            let prefix = literal[..literal.len() - quoted.len()].to_ascii_lowercase();
            if prefix.contains(['b', 'f', 't']) {
                return None;
            }
            let inner = extract_string_content(quoted);
            let start = span.start + prefix.len() + (quoted.len() - inner.len()) / 2;
            let first = pieces.len();
            if prefix.contains('r') {
                pieces.push(Piece {
                    value: value.len(),
                    source: start..start + inner.len(),
                    verbatim: true,
                });
                value.push_str(inner);
            } else {
                escapes(inner, |piece, written| {
                    pieces.push(Piece {
                        value: value.len(),
                        source: start + written.start..start + written.end,
                        verbatim: piece.len() == written.len(),
                    });
                    value.push_str(piece);
                });
            }
            if (start..=start + inner.len()).contains(&offset) {
                cursor = Some(
                    pieces[first..]
                        .iter()
                        .find(|piece| offset < piece.source.end)
                        .map_or(value.len(), |piece| {
                            piece.value
                                + if piece.verbatim {
                                    offset - piece.source.start
                                } else {
                                    0
                                }
                        }),
                );
            }
        }
        let cursor = cursor?;
        let (shift, text) = match route {
            Route::Helper => (0, value),
            Route::Marker | Route::Wrapper => {
                let rest = after_marker(&value)?;
                let marker_end = value.len() - rest.len();
                if cursor < marker_end {
                    return None;
                }
                (marker_end - MARKER.len(), format!("{MARKER}{rest}"))
            }
        };
        Some(QueryAt {
            text,
            offset: cursor - shift,
            route: *route,
            span: strings[0].start..strings[strings.len() - 1].end,
            shift,
            pieces,
        })
    }
}

/// The query under a cursor.
#[derive(Clone, Debug)]
pub struct QueryAt {
    /// The query as the analyzer reads it: the string's value, escapes decoded, from the
    /// marker on (`--sql` however it was written).
    pub text: String,
    /// The cursor's byte offset in `text`.
    pub offset: usize,
    /// How `check` recognised it.
    pub route: Route,
    /// The source span of its string literals, prefixes and quotes included.
    pub span: Range<usize>,
    /// Bytes of the string's value before `text`, less the `--sql` it starts with.
    shift: usize,
    pieces: Vec<Piece>,
}

impl QueryAt {
    /// Where byte `offset` of `text` is written in the source; an offset in an escape
    /// sequence maps to its start. `None` past the end of `text`, or in the marker.
    pub fn source_offset(&self, offset: usize) -> Option<usize> {
        if offset > self.text.len() || (self.route != Route::Helper && offset < MARKER.len()) {
            return None;
        }
        let at = offset + self.shift;
        let piece = &self.pieces[self.pieces.partition_point(|piece| piece.value <= at) - 1];
        Some(if piece.verbatim {
            piece.source.start + (at - piece.value).min(piece.source.len())
        } else if at == piece.value {
            piece.source.start
        } else {
            piece.source.end
        })
    }
}

/// A run of a string's value and the source bytes it is decoded from.
#[derive(Clone, Debug)]
struct Piece {
    /// Its offset in the value.
    value: usize,
    source: Range<usize>,
    /// Written as is: no escape sequence.
    verbatim: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The query at the first `|` of `source` (removed), with `helpers` the starts of
    /// strings helper calls pass, as `needle` positions in the source without the `|`.
    fn at(source: &str, helpers: &[&str]) -> Option<(String, String, Route)> {
        let cursor = source.find('|').expect("a cursor");
        let source = source.replacen('|', "", 1);
        let starts: Vec<usize> = helpers.iter().map(|h| source.find(h).unwrap()).collect();
        let found =
            Queries::new(&source, &ImportStyle::parse(None).unwrap(), &starts).at(cursor)?;
        assert_eq!(found.source_offset(found.offset), Some(cursor), "{source}");
        let marked = format!(
            "{}|{}",
            &found.text[..found.offset],
            &found.text[found.offset..]
        );
        Some((marked, found.text, found.route))
    }

    fn marked(source: &str, helpers: &[&str]) -> Option<(String, Route)> {
        at(source, helpers).map(|(marked, _, route)| (marked, route))
    }

    #[test]
    fn each_route() {
        assert_eq!(
            marked(r#"q = "--sql\nSELECT | FROM users""#, &[]),
            Some(("--sql\nSELECT | FROM users".into(), Route::Marker))
        );
        assert_eq!(
            marked(
                r#"q = SQL[tuple[int]]("--sql\nSELECT count(*) FROM |")"#,
                &[]
            ),
            Some(("--sql\nSELECT count(*) FROM |".into(), Route::Wrapper))
        );
        assert_eq!(
            marked(
                r#"fetch_all("SELECT | FROM users", tuple[int])"#,
                &["\"SELECT"]
            ),
            Some(("SELECT | FROM users".into(), Route::Helper))
        );
        assert_eq!(
            marked(
                r#"db.fetch_one("--sql\nSELECT u.| FROM users u", str)"#,
                &["\"--sql"]
            ),
            Some(("--sql\nSELECT u.| FROM users u".into(), Route::Marker))
        );
    }

    #[test]
    fn string_forms() {
        assert_eq!(
            marked(
                "q = (\n  \"--sql\\n\"\n  \"SELECT id \"\n  'FROM |users'\n)",
                &[]
            ),
            Some(("--sql\nSELECT id FROM |users".into(), Route::Marker))
        );
        assert_eq!(
            marked("q = \"\"\"--sql\nSELECT id\nFROM u|\"\"\"", &[]),
            Some(("--sql\nSELECT id\nFROM u|".into(), Route::Marker))
        );
        assert_eq!(
            marked("q = \"  -- sql\\nSELECT |\"", &[]),
            Some(("--sql\nSELECT |".into(), Route::Marker))
        );
        assert_eq!(
            marked(r#"run(r"SELECT '\d' AS d, |")"#, &["r\""]),
            Some((r"SELECT '\d' AS d, |".into(), Route::Helper))
        );
        assert_eq!(
            marked(
                r#"q = "--sql\nSELECT \"Name\", \x41|, 'é', \u00e9 FROM t""#,
                &[]
            ),
            Some((
                "--sql\nSELECT \"Name\", A|, 'é', é FROM t".into(),
                Route::Marker
            ))
        );
        assert_eq!(
            marked(r#"q = "--sql\nSELECT 'é', 'ü', i| FROM t""#, &[]),
            Some(("--sql\nSELECT 'é', 'ü', i| FROM t".into(), Route::Marker))
        );
    }

    #[test]
    fn cursor_at_either_end_of_the_content() {
        let source = r#"q = "--sql\nSELECT 1""#;
        let queries = Queries::new(source, &ImportStyle::parse(None).unwrap(), &[]);
        let end = queries.at(source.len() - 1).unwrap();
        assert_eq!(end.offset, end.text.len());
        assert_eq!(end.span, 4..source.len());
        assert!(queries.at(source.len()).is_none());
        assert!(queries.at(5).is_none(), "in the marker");
        let at = queries.at(source.find("SELECT").unwrap()).unwrap();
        assert_eq!(at.source_offset(0), None);
        assert_eq!(at.source_offset(at.text.len() + 1), None);
        assert_eq!(at.source_offset(5), Some(source.find("\\n").unwrap()));
    }

    #[test]
    fn does_not_complete() {
        for source in [
            r#"q = f"--sql\nSELECT {x}, | FROM t""#,
            r#"q = t"--sql\nSELECT {x}, | FROM t""#,
            r#"q = rb"--sql\nSELECT | FROM t""#,
            r#"run("SELECT | FROM users", int)"#,
            r#"q = "SELECT | FROM users""#,
            r#"q = query  # "--sql\nSELECT |"#,
            "q = (\"--sql\\nSELECT 1\" |  \"FROM t\")",
            r#"q = "--sql\nSELECT 1" + |x"#,
        ] {
            assert_eq!(marked(source, &[]), None, "{source}");
        }
        assert_eq!(marked(r#"run(f"SELECT {x} |", int)"#, &["f\""]), None);
    }

    #[test]
    fn locates_among_many_queries() {
        let source: String = (0..50)
            .map(|i| format!("q{i} = \"--sql\\nSELECT {i} FROM t\"\nx = 'plain'\n"))
            .collect();
        let queries = Queries::new(&source, &ImportStyle::parse(None).unwrap(), &[]);
        let at = queries.at(source.find("SELECT 37").unwrap() + 7).unwrap();
        assert_eq!(
            (at.text.as_str(), at.offset),
            ("--sql\nSELECT 37 FROM t", 13)
        );
        assert!(queries.at(source.find("plain").unwrap()).is_none());
    }
}
