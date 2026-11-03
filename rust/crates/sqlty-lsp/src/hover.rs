//! `textDocument/hover`, answered on the protocol thread from the buffer's text and what
//! its last check left, as completion is.

use lsp_types::{
    Hover, HoverContents, HoverParams, MarkupContent, MarkupKind, PositionEncodingKind,
};
use sqlty_python::locate::{self, Queries};
use sqlty_python::rewriter::ImportStyle;

use crate::completion::moved;
use crate::text::LineIndex;
use crate::Document;

/// What the word at the position of `params` in `document` is; `None` outside a checked
/// query, on a word that names nothing, or before the buffer's first check.
pub(crate) fn hover(
    document: &Document,
    params: &HoverParams,
    encoding: &PositionEncodingKind,
) -> Option<Hover> {
    let (checked, context) = document.completion.as_ref()?;
    let text = &document.text;
    let index = LineIndex::new(text, encoding);
    let offset = index.offset(params.text_document_position_params.position);
    let style = ImportStyle::parse(context.import_style.as_deref()).ok()?;
    let starts = moved(&context.helper_starts, checked, text);
    let queries = Queries::new(text, &style, &starts);
    let found = locate::hover(
        &queries,
        offset,
        &context.dialect,
        context.schema.as_deref(),
    )?;
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: found.contents,
        }),
        range: Some(index.range(found.range.start, found.range.end)),
    })
}
