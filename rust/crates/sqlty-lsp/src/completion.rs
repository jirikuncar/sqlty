//! `textDocument/completion` and `completionItem/resolve`, answered on the protocol thread.
//!
//! The check worker leaves with each buffer the [`CompletionContext`] its last check read
//! (schema, dialect, helper strings), the [`CompletionContext`](crate::CompletionContext); a request reads it and the current text, so it never
//! waits behind a check. A schema reload swaps the context; a request holds its own `Arc`.

use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionItemLabelDetails, CompletionList,
    CompletionParams, CompletionResponse, CompletionTextEdit, Documentation, MarkupContent,
    MarkupKind, PositionEncodingKind, TextEdit,
};
use serde_json::json;
use sqlty_python::locate::{self, Queries};
use sqlty_python::rewriter::ImportStyle;
use sqlty_sql::analyzer::{Candidate, CandidateKind};

use crate::text::LineIndex;
use crate::Document;

/// Items a response holds at most; with more, the list is incomplete and the client asks
/// again as the word grows.
const LIMIT: usize = 200;

/// The completions at the cursor of `params` in `document`; `None` outside a checked query,
/// or before the buffer's first check.
pub(crate) fn complete(
    document: &Document,
    params: &CompletionParams,
    encoding: &PositionEncodingKind,
) -> Option<CompletionResponse> {
    let (checked, context) = document.completion.as_ref()?;
    let text = &document.text;
    let index = LineIndex::new(text, encoding);
    let offset = index.offset(params.text_document_position.position);
    let style = ImportStyle::parse(context.import_style.as_deref()).ok()?;
    let starts = moved(&context.helper_starts, checked, text);
    let queries = Queries::new(text, &style, &starts);
    let schema = context.schema.as_deref();
    let found = locate::complete(&queries, offset, &context.dialect, schema, LIMIT)?;
    let range = index.range(found.replace.start, found.replace.end);
    let items = found
        .candidates
        .into_iter()
        .enumerate()
        .map(|(rank, candidate)| item(candidate, rank, range))
        .collect();
    Some(CompletionResponse::List(CompletionList {
        is_incomplete: found.incomplete,
        items,
    }))
}

/// `item` with the detail and documentation `complete` left in its data.
pub(crate) fn resolve(mut item: CompletionItem) -> CompletionItem {
    if let Some(data) = item.data.take() {
        item.detail = data["detail"].as_str().map(str::to_string);
        item.documentation = data["documentation"].as_str().map(|value| {
            Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: value.to_string(),
            })
        });
    }
    item
}

fn item(candidate: Candidate, rank: usize, range: lsp_types::Range) -> CompletionItem {
    let (kind, documentation) = match candidate.kind {
        CandidateKind::Table => (CompletionItemKind::STRUCT, "table"),
        CandidateKind::View => (CompletionItemKind::STRUCT, "view"),
        CandidateKind::Cte => (CompletionItemKind::MODULE, "CTE of the query"),
        CandidateKind::Column => (CompletionItemKind::FIELD, "column"),
        CandidateKind::Join => (CompletionItemKind::STRUCT, "join on a foreign key"),
    };
    let nullability = match candidate.nullable {
        Some(true) => " NULL",
        Some(false) => " NOT NULL",
        None => "",
    };
    let detail = match (&candidate.sql_type, &candidate.detail) {
        (Some(sql_type), _) => format!("{sql_type}{nullability}"),
        (None, Some(schema)) => format!("{documentation} in {schema}"),
        (None, None) => documentation.to_string(),
    };
    let documentation = match &candidate.detail {
        Some(source) if candidate.kind == CandidateKind::Column => {
            format!("Column of `{source}`: `{detail}`")
        }
        _ => format!("`{}`: {detail}", candidate.label),
    };
    CompletionItem {
        kind: Some(kind),
        label_details: candidate
            .detail
            .clone()
            .map(|source| CompletionItemLabelDetails {
                detail: None,
                description: Some(source),
            }),
        sort_text: Some(format!("{rank:04}")),
        text_edit: Some(CompletionTextEdit::Edit(TextEdit::new(
            range,
            candidate.label.clone(),
        ))),
        data: Some(json!({"detail": detail, "documentation": documentation})),
        label: candidate.label,
        ..CompletionItem::default()
    }
}

/// `starts`, offsets into `old`, as offsets into `new`: those before the first changed
/// byte stay, those after the last move with it, and those in between are dropped.
pub(crate) fn moved(starts: &[usize], old: &str, new: &str) -> Vec<usize> {
    let (old, new) = (old.as_bytes(), new.as_bytes());
    let prefix = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let room = old.len().min(new.len()) - prefix;
    let suffix = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    starts
        .iter()
        .filter_map(|&start| {
            if start < prefix {
                Some(start)
            } else if start >= old.len() - suffix {
                Some(start + new.len() - old.len())
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::moved;

    #[test]
    fn helper_strings_follow_an_edit() {
        let old = "a(\"x\")\nb(\"y\")\n";
        let new = "a(\"x\")\n# note\nb(\"y\")\n";
        assert_eq!(moved(&[2, 9], old, new), [2, 16]);
        assert_eq!(moved(&[2, 9], old, old), [2, 9]);
        assert_eq!(
            moved(&[2, 9], old, "a(\"x\")\nc(1)\n"),
            [2],
            "the string is gone"
        );
    }
}
