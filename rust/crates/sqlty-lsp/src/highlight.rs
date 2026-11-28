//! `textDocument/semanticTokens/full`: the SQL tokens of the strings `sqlty check` reads,
//! answered on the protocol thread from the buffer's text and its last check.
//!
//! Before the first check, or in a file without helper calls, the marked and wrapped
//! strings are found from the text alone. A check that finds other helper strings asks the
//! client to request the tokens again (`workspace/semanticTokens/refresh`).

use lsp_types::{
    PositionEncodingKind, SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens,
    SemanticTokensLegend,
};
use sqlty_python::locate::Queries;
use sqlty_python::rewriter::ImportStyle;

use crate::completion::moved;
use crate::text::LineIndex;
use crate::{CompletionContext, Document};

/// The token types and modifiers [`Highlight`] names, in legend order: indices already
/// sent never change.
const TYPES: [&str; 13] = [
    "keyword",
    "string",
    "number",
    "comment",
    "operator",
    "parameter",
    "function",
    "class",
    "interface",
    "namespace",
    "variable",
    "property",
    "type",
];
const MODIFIERS: [&str; 3] = ["declaration", "defaultLibrary", "deprecated"];

pub(crate) fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TYPES.map(SemanticTokenType::new).to_vec(),
        token_modifiers: MODIFIERS.map(SemanticTokenModifier::new).to_vec(),
    }
}

/// The tokens of `document`, one per line of each colored range.
pub(crate) fn tokens(document: &Document, encoding: &PositionEncodingKind) -> SemanticTokens {
    let text = &document.text;
    let (starts, dialect, style) = reading(document.completion.as_ref(), text);
    let schema = document
        .completion
        .as_ref()
        .and_then(|(_, context)| context.schema.as_deref());
    let Ok(style) = ImportStyle::parse(style.as_deref()) else {
        return SemanticTokens::default();
    };
    let index = LineIndex::new(text, encoding);
    let mut data = Vec::new();
    let mut previous = lsp_types::Position::new(0, 0);
    for (range, kind) in Queries::new(text, &style, &starts).highlights(&dialect, schema) {
        let mut start = range.start;
        for line in text[range].split_inclusive('\n') {
            let end = start + line.trim_end_matches(['\n', '\r']).len();
            let (from, to) = (index.position(start), index.position(end));
            start += line.len();
            if from == to {
                continue;
            }
            let delta_line = from.line - previous.line;
            data.push(SemanticToken {
                delta_line,
                delta_start: if delta_line == 0 {
                    from.character - previous.character
                } else {
                    from.character
                },
                length: to.character - from.character,
                token_type: position(&TYPES, kind.token_type()),
                token_modifiers_bitset: kind.modifiers().iter().fold(0, |bits, modifier| {
                    bits | 1 << position(&MODIFIERS, modifier)
                }),
            });
            previous = from;
        }
    }
    SemanticTokens {
        result_id: None,
        data,
    }
}

/// What highlighting reads of `text` from its last check: the helper strings, moved with
/// the edits since, the dialect and the import style. Before the first check: none,
/// PostgreSQL and the default.
pub(crate) fn reading(
    checked: Option<&(String, CompletionContext)>,
    text: &str,
) -> (Vec<usize>, String, Option<String>) {
    match checked {
        Some((checked, context)) => (
            moved(&context.helper_starts, checked, text),
            context.dialect.clone(),
            context.import_style.clone(),
        ),
        None => (Vec::new(), "postgres".to_string(), None),
    }
}

fn position(names: &[&str], name: &str) -> u32 {
    names
        .iter()
        .position(|known| *known == name)
        .and_then(|at| u32::try_from(at).ok())
        .unwrap_or_default()
}
