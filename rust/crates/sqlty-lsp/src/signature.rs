//! `textDocument/signatureHelp`, answered on the protocol thread from the buffer's text and
//! what its last check left, as hover is.

use lsp_types::{
    Documentation, MarkupContent, MarkupKind, ParameterInformation, ParameterLabel,
    PositionEncodingKind, SignatureHelp, SignatureHelpParams, SignatureInformation,
};
use sqlty_python::locate::{self, Queries};
use sqlty_python::rewriter::ImportStyle;

use crate::completion::moved;
use crate::text::LineIndex;
use crate::Document;

/// The overloads of the built-in function whose arguments hold the position of `params`;
/// `None` outside a call of one in a checked query, or before the buffer's first check.
pub(crate) fn signature_help(
    document: &Document,
    params: &SignatureHelpParams,
    encoding: &PositionEncodingKind,
) -> Option<SignatureHelp> {
    let (checked, context) = document.completion.as_ref()?;
    let text = &document.text;
    let index = LineIndex::new(text, encoding);
    let offset = index.offset(params.text_document_position_params.position);
    let style = ImportStyle::parse(context.import_style.as_deref()).ok()?;
    let starts = moved(&context.helper_starts, checked, text);
    let queries = Queries::new(text, &style, &starts);
    let found = locate::signature_help(
        &queries,
        offset,
        &context.dialect,
        context.schema.as_deref(),
    )?;
    let signatures = found
        .signatures
        .iter()
        .map(|signature| SignatureInformation {
            label: signature.label.to_string(),
            documentation: Some(Documentation::MarkupContent(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("[Documentation]({})", signature.url()),
            })),
            // Labels are ASCII: a byte offset is a UTF-16 one.
            parameters: Some(
                signature
                    .parameters
                    .iter()
                    .map(|parameter| ParameterInformation {
                        label: ParameterLabel::LabelOffsets([
                            count(parameter.label.0),
                            count(parameter.label.1),
                        ]),
                        documentation: None,
                    })
                    .collect(),
            ),
            active_parameter: None,
        })
        .collect();
    let active = &found.signatures[found.active_signature];
    // Past the last parameter, no parameter is highlighted.
    let parameter = found.active_parameter.unwrap_or(active.parameters.len());
    Some(SignatureHelp {
        signatures,
        active_signature: Some(count(found.active_signature)),
        active_parameter: Some(count(parameter)),
    })
}

fn count(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}
