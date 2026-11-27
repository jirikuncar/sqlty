//! `textDocument/definition`, answered on the protocol thread from the buffer's text and
//! what its last check left, as hover is: the schema statement that last defined a table,
//! view or column, or the alias, CTE or select item of the query.

use lsp_types::{GotoDefinitionParams, GotoDefinitionResponse, Location, PositionEncodingKind};
use sqlty_python::locate::{self, Queries};
use sqlty_python::queries;
use sqlty_python::rewriter::ImportStyle;
use sqlty_sql::analyzer::Definition;

use crate::completion::moved;
use crate::text::LineIndex;
use crate::{CompletionContext, Document};

/// Where the name at the position of `params` in `document` is defined; `None` outside a
/// checked query, on a name nothing defines, or before the buffer's first check.
pub(crate) fn definition(
    document: &Document,
    params: &GotoDefinitionParams,
    encoding: &PositionEncodingKind,
) -> Option<GotoDefinitionResponse> {
    let (checked, context) = document.completion.as_ref()?;
    let position = &params.text_document_position_params;
    let text = &document.text;
    let index = LineIndex::new(text, encoding);
    let offset = index.offset(position.position);
    let schema = context.schema.as_deref();
    let schema_sql = context.schema_sql.as_deref();
    let found = match context.query_format {
        Some(format) => {
            queries::definition(text, format, offset, &context.dialect, schema, schema_sql)
        }
        None => {
            let style = ImportStyle::parse(context.import_style.as_deref()).ok()?;
            let starts = moved(&context.helper_starts, checked, text);
            let queries = Queries::new(text, &style, &starts);
            locate::definition(&queries, offset, &context.dialect, schema, schema_sql)
        }
    }?;
    let location = match found {
        Definition::Query(range) => Location {
            uri: position.text_document.uri.clone(),
            range: index.range(range.start, range.end),
        },
        Definition::Schema(range) => in_schema(context, range, encoding)?,
    };
    Some(GotoDefinitionResponse::Scalar(location))
}

/// `range` of the schema text as a place in the file it was read from. Only the lines it
/// spans are indexed: the file may be long.
fn in_schema(
    context: &CompletionContext,
    range: std::ops::Range<usize>,
    encoding: &PositionEncodingKind,
) -> Option<Location> {
    let sql = context.schema_sql.as_deref()?;
    let file = context
        .schema_files
        .partition_point(|(_, start)| *start <= range.start)
        .checked_sub(1)?;
    let (path, start) = &context.schema_files[file];
    let end = context
        .schema_files
        .get(file + 1)
        .map_or(sql.len(), |(_, next)| next - 1);
    let text = sql.get(*start..end)?;
    let (from, to) = (range.start - start, (range.end - start).min(text.len()));
    let first = text[..from].rfind('\n').map_or(0, |at| at + 1);
    let last = text[to..].find('\n').map_or(text.len(), |at| to + at);
    let lines = u32::try_from(text[..first].matches('\n').count()).unwrap_or(u32::MAX);
    let mut range = LineIndex::new(&text[first..last], encoding).range(from - first, to - first);
    range.start.line += lines;
    range.end.line += lines;
    Some(Location {
        uri: crate::uri::from_path(path)?,
        range,
    })
}
