//! `sqlty complete FILE:LINE:COL`: the names that can be typed at a cursor in a query string.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use sqlty_project::Shared;
use sqlty_python::locate;
use sqlty_python::project::Files;
use sqlty_python::rewriter::ImportStyle;
use sqlty_sql::analyzer::CandidateKind;

use crate::{workspace, Common, Host, Lines};

/// The most names listed.
const LIMIT: usize = 200;

/// The cursor of `sqlty complete`: a file, a 1-based line and a 1-based column in characters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    path: PathBuf,
    line: usize,
    column: usize,
}

/// Parse `FILE:LINE:COL`.
pub fn location(value: &str) -> Result<Location, String> {
    let invalid = || format!("expected FILE:LINE:COL, got {}", sqlty_project::repr(value));
    let (rest, column) = value.rsplit_once(':').ok_or_else(invalid)?;
    let (path, line) = rest.rsplit_once(':').ok_or_else(invalid)?;
    let number = |text: &str| {
        text.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| text.parse::<usize>().ok())
            .flatten()
    };
    match (path.is_empty(), number(line), number(column)) {
        (false, Some(line), Some(column)) => Ok(Location {
            path: PathBuf::from(path),
            line,
            column,
        }),
        _ => Err(invalid()),
    }
}

/// Print what completes at `location`, read with the settings `check` gives its file and
/// the helpers of `common.paths` (default: the file's directory).
pub fn run(
    host: &mut dyn Host,
    location: &Location,
    mut common: Common,
    severities: Vec<(String, String)>,
    json: bool,
) -> Result<i32, String> {
    let path = &location.path;
    let files = Shared(host.files());
    let absolute = host.cwd().join(path);
    if files.text(&absolute).is_none() || files.canonical(&absolute).is_dir() {
        return Err(format!("{}: no such file", path.display()));
    }
    if common.paths.is_empty() {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty());
        common
            .paths
            .push(parent.unwrap_or(Path::new(".")).to_path_buf());
    }
    common.paths.insert(0, path.clone());
    let mut workspace = workspace(host, &common, severities, None, true)?;
    let canonical = files.canonical(&absolute);
    let path = workspace
        .files()
        .iter()
        .find(|file| files.canonical(&host.cwd().join(file)) == canonical)
        .cloned()
        .ok_or_else(|| format!("{}: not a Python file sqlty reads", path.display()))?;
    let text = workspace.text(&path).unwrap_or("").to_string();
    let context = workspace.completion(&path).ok_or("no completion context")?;
    let style = ImportStyle::parse(context.import_style.as_deref()).map_err(|e| e.to_string())?;
    let queries = locate::Queries::new(&text, &style, &context.helper_starts);
    let offset = offset(&text, location.line, location.column);
    let schema = context.schema.as_deref();
    let found = locate::complete(&queries, offset, context.dialect, schema, LIMIT);
    if json {
        let result = found.map_or(Value::Null, |found| {
            let items: Vec<Value> = found
                .candidates
                .into_iter()
                .map(|item| {
                    json!({
                        "label": item.label,
                        "kind": kind(item.kind),
                        "detail": item.detail,
                        "sql_type": item.sql_type,
                        "nullable": item.nullable,
                    })
                })
                .collect();
            let lines = Lines::new(&text);
            json!({
                "start": lines.position(found.replace.start),
                "end": lines.position(found.replace.end),
                "incomplete": found.incomplete,
                "items": items,
            })
        });
        host.out(&format!("{result}\n"));
    } else if let Some(found) = found {
        for item in found.candidates {
            let fields = [
                item.label.as_str(),
                kind(item.kind),
                item.detail.as_deref().unwrap_or(""),
                item.sql_type.as_deref().unwrap_or(""),
            ];
            host.out(&format!("{}\n", fields.join("\t")));
        }
    }
    Ok(0)
}

fn kind(kind: CandidateKind) -> &'static str {
    match kind {
        CandidateKind::Table => "table",
        CandidateKind::View => "view",
        CandidateKind::Cte => "cte",
        CandidateKind::Column => "column",
        CandidateKind::Join => "join",
    }
}

/// The byte offset of the 1-based `line` and `column` (in characters) of `text`; a column
/// past the end of its line stops at the line break, a line past the last is the last.
fn offset(text: &str, line: usize, column: usize) -> usize {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let Some(last) = lines.len().checked_sub(1) else {
        return 0;
    };
    let index = line.clamp(1, lines.len()) - 1;
    let start: usize = lines[..index.min(last)].iter().map(|line| line.len()).sum();
    let line = lines[index].trim_end_matches(['\n', '\r']);
    let width: usize = line
        .chars()
        .take(column.saturating_sub(1))
        .map(char::len_utf8)
        .sum();
    start + width
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn reads_file_line_and_column() {
        let found = location("C:/app.py:12:30").map(|at| (at.path, at.line, at.column));
        assert_eq!(found, Ok((PathBuf::from("C:/app.py"), 12, 30)));
        for wrong in ["app.py:12", ":1:2", "app.py:a:2", "app.py:1:+2"] {
            assert!(location(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn names_each_kind() {
        let kinds = [
            CandidateKind::Table,
            CandidateKind::View,
            CandidateKind::Cte,
            CandidateKind::Column,
            CandidateKind::Join,
        ];
        assert_eq!(kinds.map(kind), ["table", "view", "cte", "column", "join"]);
    }

    #[test]
    fn counts_columns_in_characters_and_stops_at_the_line_break() {
        let text = "x = 1\nf('é', g)";
        assert_eq!(offset(text, 2, 8), 14);
        assert_eq!(offset(text, 1, 99), 5);
        assert_eq!(offset(text, 9, 1), 6);
        assert_eq!(offset(text, 0, 0), 0);
        assert_eq!(offset("", 1, 1), 0);
    }
}
