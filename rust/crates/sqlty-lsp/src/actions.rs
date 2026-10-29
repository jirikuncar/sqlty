//! Diagnostics and code actions in the terms of the protocol.

use std::collections::HashMap;

use lsp_types::{
    AnnotatedTextEdit, CodeAction, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeDescription, DiagnosticSeverity, DocumentChanges, NumberOrString, OneOf,
    OptionalVersionedTextDocumentIdentifier, PositionEncodingKind, TextDocumentEdit, TextEdit, Url,
    WorkspaceEdit,
};

use crate::text::LineIndex;
use crate::{Diagnostic, Document, Severity};

/// The rules reference; each rule is a heading of it.
const RULES: &str = "https://jirikuncar.github.io/sqlty/reference/rules/";

pub(crate) fn diagnostic(diagnostic: &Diagnostic, index: &LineIndex<'_>) -> lsp_types::Diagnostic {
    lsp_types::Diagnostic {
        range: index.range(diagnostic.start, diagnostic.end),
        severity: Some(match diagnostic.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning => DiagnosticSeverity::WARNING,
        }),
        code: Some(NumberOrString::String(diagnostic.rule.clone())),
        code_description: Url::parse(&format!("{RULES}#{}", diagnostic.rule))
            .ok()
            .map(|href| CodeDescription { href }),
        source: Some(
            diagnostic
                .source
                .clone()
                .unwrap_or_else(|| "sqlty".to_string()),
        ),
        message: diagnostic.message.clone(),
        ..lsp_types::Diagnostic::default()
    }
}

/// The fix of each diagnostic in the requested range, while the buffer is the one checked.
pub(crate) fn quick_fixes(
    documents: &HashMap<Url, Document>,
    params: &CodeActionParams,
    encoding: &PositionEncodingKind,
) -> Vec<CodeActionOrCommand> {
    let uri = &params.text_document.uri;
    let Some(document) = documents.get(uri) else {
        return Vec::new();
    };
    let Some((version, diagnostics)) = &document.diagnostics else {
        return Vec::new();
    };
    if *version != document.version {
        return Vec::new();
    }
    let index = LineIndex::new(&document.text, encoding);
    let (start, end) = (
        index.offset(params.range.start),
        index.offset(params.range.end),
    );
    diagnostics
        .iter()
        .filter(|d| !d.fix.is_empty() && d.start <= end && start <= d.end)
        .map(|d| {
            let edits = d
                .fix
                .iter()
                .map(|edit| {
                    OneOf::Left(TextEdit::new(
                        index.range(edit.start, edit.end),
                        edit.replacement.clone(),
                    ))
                })
                .collect();
            CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("Fix {}", d.rule),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diagnostic(d, &index)]),
                edit: Some(versioned(uri, *version, edits)),
                is_preferred: Some(true),
                ..CodeAction::default()
            })
        })
        .collect()
}

/// Edits of `uri` that the client drops when the buffer is no longer at `version`.
pub(crate) fn versioned(
    uri: &Url,
    version: i32,
    edits: Vec<OneOf<TextEdit, AnnotatedTextEdit>>,
) -> WorkspaceEdit {
    WorkspaceEdit {
        document_changes: Some(DocumentChanges::Edits(vec![TextDocumentEdit {
            text_document: OptionalVersionedTextDocumentIdentifier {
                uri: uri.clone(),
                version: Some(version),
            },
            edits,
        }])),
        ..WorkspaceEdit::default()
    }
}
