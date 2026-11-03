//! `textDocument/semanticTokens/full`: the SQL tokens of the strings `sqlty check` reads,
//! answered on the protocol thread from the buffer's text and its last check.
//!
//! Before the first check, or in a file without helper calls, the marked and wrapped
//! strings are found from the text alone. A check that finds other helper strings asks the
//! client to request the tokens again (`workspace/semanticTokens/refresh`).

use lsp_types::{
    PositionEncodingKind, SemanticToken, SemanticTokenType, SemanticTokens, SemanticTokensLegend,
};
use sqlty_python::locate::Queries;
use sqlty_python::rewriter::ImportStyle;
use sqlty_sql::highlight::Highlight;

use crate::completion::moved;
use crate::text::LineIndex;
use crate::{CompletionContext, Document};

/// The token types, in the order of [`Highlight`]'s index in the legend.
pub(crate) fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::KEYWORD,
            SemanticTokenType::STRING,
            SemanticTokenType::NUMBER,
            SemanticTokenType::COMMENT,
            SemanticTokenType::OPERATOR,
            SemanticTokenType::PARAMETER,
        ],
        token_modifiers: Vec::new(),
    }
}

/// The tokens of `document`, one per line of each colored range.
pub(crate) fn tokens(document: &Document, encoding: &PositionEncodingKind) -> SemanticTokens {
    let text = &document.text;
    let (starts, dialect, style) = reading(document.completion.as_ref(), text);
    let Ok(style) = ImportStyle::parse(style.as_deref()) else {
        return SemanticTokens::default();
    };
    let index = LineIndex::new(text, encoding);
    let mut data = Vec::new();
    let mut previous = lsp_types::Position::new(0, 0);
    for (range, kind) in Queries::new(text, &style, &starts).highlights(&dialect) {
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
                token_type: token_type(kind),
                token_modifiers_bitset: 0,
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

fn token_type(kind: Highlight) -> u32 {
    match kind {
        Highlight::Keyword => 0,
        Highlight::String => 1,
        Highlight::Number => 2,
        Highlight::Comment => 3,
        Highlight::Operator => 4,
        Highlight::Parameter => 5,
    }
}
