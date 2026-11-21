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

use sqlty_sql::analyzer::{self, Completions, Hover, SchemaData};
use sqlty_sql::highlight::{highlight, Highlight};

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

    /// The string literals of each query, as source byte ranges, in source order.
    pub fn literals(&self) -> impl Iterator<Item = &[Range<usize>]> {
        self.sites.iter().map(|(strings, _)| strings.as_slice())
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
        let decoded = self.decode(strings)?;
        let cursor = decoded.literals.iter().find_map(|literal| {
            if !literal.content.contains(&offset) && literal.content.end != offset {
                return None;
            }
            let pieces = &decoded.pieces[literal.pieces.clone()];
            Some(
                pieces
                    .iter()
                    .find(|piece| offset < piece.source.end)
                    .map_or(literal.value_end, |piece| {
                        piece.value
                            + if piece.verbatim {
                                offset - piece.source.start
                            } else {
                                0
                            }
                    }),
            )
        })?;
        let (shift, text) = text(decoded.value, *route)?;
        if cursor < shift + MARKER.len() && *route != Route::Helper {
            return None;
        }
        Some(QueryAt {
            text,
            offset: cursor - shift,
            route: *route,
            span: strings[0].start..strings[strings.len() - 1].end,
            shift,
            pieces: decoded.pieces,
        })
    }

    /// What to color in every query, as source byte ranges in order: a token written
    /// across string literals or escape sequences is split at them.
    pub fn highlights(&self, dialect: &str) -> Vec<(Range<usize>, Highlight)> {
        let mut found = Vec::new();
        for (strings, route) in &self.sites {
            let Some(decoded) = self.decode(strings) else {
                continue;
            };
            let value = decoded.value.as_str();
            let (marker, sql) = match route {
                Route::Helper => (None, 0),
                Route::Marker | Route::Wrapper => {
                    let Some(rest) = after_marker(value) else {
                        continue;
                    };
                    let start = value.len() - value.trim_start().len();
                    let end = value.len() - rest.len();
                    (Some((start..end, Highlight::Comment)), end)
                }
            };
            let tokens = highlight(&value[sql..], dialect)
                .into_iter()
                .map(|(range, kind)| (range.start + sql..range.end + sql, kind));
            for (range, kind) in marker.into_iter().chain(tokens) {
                for source in source_ranges(&decoded.pieces, range) {
                    found.push((source, kind));
                }
            }
        }
        found
    }

    /// The value of `strings`, one query's literals; `None` with an f-, t- or bytes string.
    fn decode(&self, strings: &[Range<usize>]) -> Option<Decoded> {
        let mut decoded = Decoded::default();
        for span in strings {
            let literal = &self.source[span.clone()];
            let quoted = literal.trim_start_matches(|c: char| c.is_ascii_alphabetic());
            let prefix = literal[..literal.len() - quoted.len()].to_ascii_lowercase();
            if prefix.contains(['b', 'f', 't']) {
                return None;
            }
            let inner = extract_string_content(quoted);
            let start = span.start + prefix.len() + (quoted.len() - inner.len()) / 2;
            let first = decoded.pieces.len();
            let (pieces, value) = (&mut decoded.pieces, &mut decoded.value);
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
            decoded.literals.push(Literal {
                content: start..start + inner.len(),
                pieces: first..decoded.pieces.len(),
                value_end: decoded.value.len(),
            });
        }
        Some(decoded)
    }
}

/// The query `check` reads in `value` and how many bytes of `value` precede it, less the
/// `--sql` it starts with; `None` for a marker route without a marker.
fn text(value: String, route: Route) -> Option<(usize, String)> {
    match route {
        Route::Helper => Some((0, value)),
        Route::Marker | Route::Wrapper => {
            let rest = after_marker(&value)?;
            let marker_end = value.len() - rest.len();
            Some((marker_end - MARKER.len(), format!("{MARKER}{rest}")))
        }
    }
}

/// The source bytes `value` of a decoded string is written in: a range per piece it covers,
/// a whole escape sequence for any of its bytes.
fn source_ranges(pieces: &[Piece], value: Range<usize>) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for (i, piece) in pieces.iter().enumerate() {
        let piece_end = pieces.get(i + 1).map_or(usize::MAX, |next| next.value);
        let (start, end) = (value.start.max(piece.value), value.end.min(piece_end));
        if start >= end {
            continue;
        }
        let source = if piece.verbatim {
            piece.source.start + start - piece.value..piece.source.start + end - piece.value
        } else {
            piece.source.clone()
        };
        match ranges.last_mut() {
            Some(last) if last.end == source.start => last.end = source.end,
            _ => ranges.push(source),
        }
    }
    ranges
}

#[derive(Default)]
struct Decoded {
    value: String,
    pieces: Vec<Piece>,
    literals: Vec<Literal>,
}

/// One string literal of a query.
struct Literal {
    /// Its content in the source, between the quotes.
    content: Range<usize>,
    /// Its pieces in [`Decoded::pieces`].
    pieces: Range<usize>,
    /// The length of the value up to its end.
    value_end: usize,
}

/// What completes at byte `offset` of `source`, `None` outside a query `check` reads:
/// [`Queries::at`], then [`complete`](sqlty_sql::analyzer::complete) on the query, with
/// [`Completions::replace`] mapped back to the bytes of the source.
pub fn complete(
    queries: &Queries<'_>,
    offset: usize,
    dialect: &str,
    schema: Option<&SchemaData>,
    limit: usize,
) -> Option<Completions> {
    let at = queries.at(offset)?;
    let mut found = analyzer::complete(&at.text, at.offset, dialect, schema, limit);
    let start = at.source_offset(found.replace.start).unwrap_or(offset);
    found.replace = start.min(offset)..offset;
    Some(found)
}

/// What the word at byte `offset` of `source` is, `None` outside a query `check` reads:
/// [`Queries::at`], then [`hover`](sqlty_sql::analyzer::hover) on the query, with
/// [`Hover::range`] mapped back to the bytes of the source.
pub fn hover(
    queries: &Queries<'_>,
    offset: usize,
    dialect: &str,
    schema: Option<&SchemaData>,
) -> Option<Hover> {
    let at = queries.at(offset)?;
    let mut found = analyzer::hover(&at.text, at.offset, dialect, schema)?;
    found.range = at.source_offset(found.range.start)?..at.source_offset(found.range.end)?;
    Some(found)
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
