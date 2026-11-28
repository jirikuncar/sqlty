//! What a word of a query is, for hover: the result column a select item gives, or the
//! column, table, view or CTE a name stands for.
//!
//! A word at the top level of a select item (`total` in `SUM(x) AS total`, `SUM`) shows the
//! result column: its Python and SQL types and the inference rules behind them, as
//! `sqlty check --explain` reads them. Any other name is looked up as completion would
//! offer it at the end of the word, so it costs what completion does; a word before `.`
//! lists the columns of that relation.
//!
//! ```
//! use sqlty_sql::analyzer::{hover, load_schema_with_skipped};
//!
//! let (schema, _) =
//!     load_schema_with_skipped("CREATE TABLE users (id INTEGER NOT NULL, name TEXT)", "postgres");
//! let sql = "SELECT name FROM users WHERE id = 1";
//! let found = hover(sql, sql.find("id").unwrap(), "postgres", Some(&schema)).unwrap();
//! assert_eq!(found.contents, "Column `id` of `users`: `INT NOT NULL`");
//! assert_eq!(&sql[found.range], "id");
//! ```

use std::fmt::Write as _;
use std::ops::Range;

use super::complete::{complete, lex, Candidate, CandidateKind, Tok};
use super::{analyze_select, explain_select, SchemaData, SqlDialect};
use crate::catalog::Columns;

/// What a word of a query is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hover {
    /// The bytes of the word.
    pub range: Range<usize>,
    /// What it is, in Markdown.
    pub contents: String,
}

/// Columns a relation lists at most.
const SHOWN: usize = 50;

/// What the word at byte `cursor` of `query` is; `None` outside a word, or for a word that
/// names nothing the schema or the query defines.
pub fn hover(
    query: &str,
    cursor: usize,
    dialect: &str,
    schema: Option<&SchemaData>,
) -> Option<Hover> {
    let (dialect, schema) = super::route(query, dialect, schema);
    let schema = schema.unwrap_or_else(|| crate::catalog::empty(SqlDialect::from_name(dialect)));
    let tokens = lex(query)?;
    let at = tokens
        .iter()
        .position(|tok| tok.word && tok.span.start <= cursor && cursor <= tok.span.end)?;
    let word = &tokens[at];
    let contents = if tokens.get(at + 1).is_some_and(|tok| tok.text == ".") {
        qualifier(query, &tokens[at + 1], dialect, schema)
    } else {
        match select_item(&tokens, at) {
            Some(item) if word.depth == item.depth => {
                match result_column(query, &item, dialect, schema) {
                    Some(text) if reads_only(&tokens, at) => {
                        Some(match candidate(query, word, dialect, schema) {
                            Some(Candidate {
                                kind: CandidateKind::Column,
                                description: Some(description),
                                ..
                            }) => format!("{text}\n\n{description}"),
                            _ => text,
                        })
                    }
                    Some(text) => Some(text),
                    None => named(query, word, dialect, schema),
                }
            }
            Some(item) => named(query, word, dialect, schema)
                .or_else(|| result_column(query, &item, dialect, schema)),
            None => named(query, word, dialect, schema),
        }
    }?;
    Some(Hover {
        range: word.span.clone(),
        contents,
    })
}

/// Whether the select item of the word at `at` is that column alone: `name` or `t.name`,
/// then an alias or nothing.
fn reads_only(tokens: &[Tok], at: usize) -> bool {
    let is = |at: usize, words: &[&str]| {
        tokens
            .get(at)
            .is_some_and(|tok| words.iter().any(|w| tok.text.eq_ignore_ascii_case(w)))
    };
    let start = match at.checked_sub(2) {
        Some(qualifier) if is(at - 1, &["."]) => qualifier,
        _ => at,
    };
    let starts = start == 0 || is(start - 1, &[",", "SELECT", "DISTINCT", "ALL"]);
    let ends =
        at + 1 == tokens.len() || is(at + 1, &[",", "AS", ")", ";"]) || is(at + 1, &AFTER_SELECT);
    starts && ends
}

/// The column, table, view or CTE completion offers for exactly `word`.
fn candidate(query: &str, word: &Tok, dialect: &str, schema: &SchemaData) -> Option<Candidate> {
    let found = complete(query, word.span.end, dialect, Some(schema), SHOWN);
    let typed = schema.fold_typed(&word.text);
    found
        .candidates
        .into_iter()
        .find(|c| schema.fold_typed(&c.label) == typed)
}

/// What [`candidate`] is, in Markdown.
fn named(query: &str, word: &Tok, dialect: &str, schema: &SchemaData) -> Option<String> {
    let candidate = &candidate(query, word, dialect, schema)?;
    let mut text = match candidate.kind {
        CandidateKind::Column => {
            let source = candidate.detail.as_deref().unwrap_or("?");
            match &candidate.sql_type {
                Some(_) => format!(
                    "Column `{}` of `{source}`: `{}`",
                    candidate.label,
                    typed_as(candidate)
                ),
                None => format!("Column `{}` of `{source}`", candidate.label),
            }
        }
        CandidateKind::Cte => format!("CTE `{}` of the query", candidate.label),
        // No word is a join, whose label holds its `ON`.
        CandidateKind::Table | CandidateKind::View | CandidateKind::Join => {
            let name = match &candidate.detail {
                Some(schema) => format!("{schema}.{}", candidate.label),
                None => candidate.label.clone(),
            };
            let kind = if candidate.kind == CandidateKind::View {
                "View"
            } else {
                "Table"
            };
            let columns: Vec<Candidate> =
                match schema.columns_of(&name).unwrap_or(Columns::Table(&[])) {
                    Columns::Table(columns) => columns
                        .iter()
                        .map(|c| {
                            Candidate::column(
                                &c.ident,
                                &name,
                                Some(&c.sql_type),
                                super::Nullability::declared(c.not_null),
                            )
                        })
                        .collect(),
                    Columns::View(columns) => columns
                        .iter()
                        .map(|c| {
                            Candidate::column(&c.name, &name, c.sql_type.as_deref(), c.nullability)
                        })
                        .collect(),
                };
            let title = match &candidate.description {
                Some(text) => format!("{kind} `{name}`\n\n{text}\n"),
                None => format!("{kind} `{name}`"),
            };
            return Some(listed(title, &columns, columns.len() > SHOWN));
        }
    };
    if let Some(description) = &candidate.description {
        text.push_str("\n\n");
        text.push_str(description);
    }
    Some(text)
}

/// The columns of the relation a word before `dot` names: an alias, a table or a CTE.
fn qualifier(query: &str, dot: &Tok, dialect: &str, schema: &SchemaData) -> Option<String> {
    let found = complete(query, dot.span.end, dialect, Some(schema), SHOWN);
    let source = found.candidates.first()?.detail.clone()?;
    if found.candidates[0].kind != CandidateKind::Column {
        return None;
    }
    Some(listed(
        format!("Relation `{source}`"),
        &found.candidates,
        found.incomplete,
    ))
}

/// `title`, then a line per column, at most [`SHOWN`].
fn listed(title: String, columns: &[Candidate], more: bool) -> String {
    let mut text = title;
    for column in columns.iter().take(SHOWN) {
        let _ = match &column.sql_type {
            Some(_) => write!(text, "\n- `{}` `{}`", column.label, typed_as(column)),
            None => write!(text, "\n- `{}`", column.label),
        };
    }
    if more || columns.len() > SHOWN {
        text.push_str("\n- ...");
    }
    text
}

/// `TEXT NOT NULL`, `TEXT NULL`, or `TEXT` when nullability is unknown.
fn typed_as(column: &Candidate) -> String {
    let sql_type = column.sql_type.as_deref().unwrap_or("?");
    match column.nullable {
        Some(true) => format!("{sql_type} NULL"),
        Some(false) => format!("{sql_type} NOT NULL"),
        None => sql_type.to_string(),
    }
}

/// The result column a top-level select item gives, with the rules that typed it.
fn result_column(
    query: &str,
    item: &SelectItem,
    dialect: &str,
    schema: &SchemaData,
) -> Option<String> {
    let columns = analyze_select(query, dialect, Some(schema))?;
    let fits = match item.star {
        None => columns.len() == item.count,
        Some(star) => star > item.index && columns.len() >= item.count,
    };
    let column = columns.get(item.index).filter(|_| fits)?;
    let mut text = format!("Result column `{}`", column.name);
    let _ = match (column.display_type(), &column.sql_type) {
        (Some(python), Some(sql)) => write!(text, ": `{python}` (`{sql}`)"),
        (Some(python), None) => write!(text, ": `{python}`"),
        (None, Some(sql)) => write!(text, ": `{sql}`"),
        (None, None) => Ok(()),
    };
    // Each rule decides both type and nullability.
    let rule = explain_select(query, dialect, Some(schema))
        .and_then(|rules| rules.into_iter().nth(item.index))
        .and_then(|rules| rules.type_rule);
    if let Some(rule) = rule {
        let _ = write!(
            text,
            "\n\nTyped by `{}`: {}\n\n{}",
            rule.id, rule.reasoning, rule.reference
        );
    }
    Some(text)
}

/// A top-level select item of the query's first `SELECT`.
struct SelectItem {
    /// Its position among the select items.
    index: usize,
    /// How many select items there are.
    count: usize,
    /// The position of the first `*` or `alias.*` item, which expands to many columns.
    star: Option<usize>,
    /// The depth of its top level.
    depth: usize,
}

/// Words that end a select list.
pub(super) const AFTER_SELECT: [&str; 14] = [
    "FROM",
    "INTO",
    "WHERE",
    "GROUP",
    "HAVING",
    "WINDOW",
    "ORDER",
    "LIMIT",
    "OFFSET",
    "FETCH",
    "FOR",
    "UNION",
    "INTERSECT",
    "EXCEPT",
];

/// The select item of the first `SELECT` at the top level that holds `tokens[at]`; `None`
/// when the statement is no `SELECT` or the token is outside its select list.
fn select_item(tokens: &[Tok], at: usize) -> Option<SelectItem> {
    let is = |tok: &Tok, words: &[&str]| {
        tok.word && words.iter().any(|w| tok.text.eq_ignore_ascii_case(w))
    };
    let depth = tokens
        .iter()
        .filter(|tok| is(tok, &["SELECT"]))
        .map(|tok| tok.depth)
        .min()?;
    let first = tokens
        .iter()
        .position(|tok| tok.depth == depth && is(tok, &["SELECT"]))?;
    if tokens[..first]
        .iter()
        .any(|tok| tok.depth == depth && is(tok, &["INSERT", "UPDATE", "DELETE", "MERGE"]))
    {
        return None;
    }
    let mut i = first + 1;
    if tokens
        .get(i)
        .is_some_and(|tok| is(tok, &["DISTINCT", "ALL"]))
    {
        i += 1;
        if tokens.get(i).is_some_and(|tok| is(tok, &["ON"])) {
            i += 1;
            while tokens
                .get(i)
                .is_some_and(|tok| tok.depth > depth || tok.text == "(")
            {
                i += 1;
            }
            i += 1;
        }
    }
    let (mut index, mut held, mut star) = (0, None, None);
    let mut starts = true;
    for (n, tok) in tokens.iter().enumerate().skip(i) {
        if tok.depth < depth || (tok.depth == depth && (tok.text == ";" || is(tok, &AFTER_SELECT)))
        {
            break;
        }
        if tok.depth == depth && tok.text == "," {
            index += 1;
            starts = true;
            continue;
        }
        let after_dot = n > 0 && tokens[n - 1].text == ".";
        if tok.depth == depth && tok.text == "*" && (starts || after_dot) && star.is_none() {
            star = Some(index);
        }
        starts = false;
        if n == at {
            held = Some(index);
        }
    }
    Some(SelectItem {
        index: held?,
        count: index + 1,
        star,
        depth,
    })
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::catalog::load;

    const SCHEMA: &str = "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, \"Nick\" TEXT);
        CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, total INTEGER);
        CREATE VIEW user_names AS SELECT id, name FROM users;";

    /// The hover at the `|` of `sql`, with the word it covers.
    fn at(sql: &str) -> Option<(String, String)> {
        let cursor = sql.find('|').expect("a cursor");
        let sql = sql.replacen('|', "", 1);
        let schema = load(SCHEMA, "postgres").0;
        let found = hover(&sql, cursor, "postgres", Some(&schema))?;
        Some((sql[found.range].to_string(), found.contents))
    }

    fn contents(sql: &str) -> String {
        at(sql).map(|(_, contents)| contents).unwrap_or_default()
    }

    #[test]
    fn a_column_shows_its_type_and_source() {
        assert_eq!(
            at("SELECT 1 FROM users u WHERE u.na|me = ''"),
            Some((
                "name".to_string(),
                "Column `name` of `u`: `TEXT NULL`".to_string()
            ))
        );
        assert_eq!(
            contents("SELECT 1 FROM users WHERE |id = 1"),
            "Column `id` of `users`: `INT NOT NULL`"
        );
        assert_eq!(
            contents("SELECT 1 FROM users WHERE \"Nick|\" = ''"),
            "Column `\"Nick\"` of `users`: `TEXT NULL`"
        );
    }

    #[test]
    fn a_table_lists_its_columns_in_order() {
        assert_eq!(
            contents("SELECT 1 FROM ord|ers"),
            "Table `public.orders`\n- `id` `INT NOT NULL`\n- `user_id` `INT NOT NULL`\n- `total` `INT NULL`"
        );
        assert_eq!(
            contents("SELECT 1 FROM user_names|"),
            "View `public.user_names`\n- `id` `INT NOT NULL`\n- `name` `TEXT NULL`"
        );
        assert_eq!(
            contents("WITH recent AS (SELECT 1) SELECT * FROM rec|ent"),
            "CTE `recent` of the query"
        );
    }

    #[test]
    fn a_qualifier_lists_the_columns_of_its_relation() {
        assert_eq!(
            contents("SELECT 1 FROM orders o WHERE |o.total > 1"),
            "Relation `o`\n- `id` `INT NOT NULL`\n- `total` `INT NULL`\n- `user_id` `INT NOT NULL`"
        );
    }

    #[test]
    fn a_select_item_shows_its_result_column_and_rules() {
        let text = contents("SELECT SUM(total) AS tot|al FROM orders");
        assert!(
            text.starts_with(
                "Result column `total`: `int | None` (`BIGINT`)\n\nTyped by `postgres.aggregate.sum-integers`: "
            ),
            "{text}"
        );
        assert_eq!(
            contents("SELECT id, na|me FROM users"),
            "Result column `name`: `str | None` (`TEXT`)"
        );
        let text = contents("WITH x AS (SELECT 1 AS a) SELECT a, a + 1 AS |b FROM x");
        assert!(
            text.starts_with("Result column `b`: `int` (`INT`)\n\nTyped by `common.arithmetic."),
            "{text}"
        );
    }

    #[test]
    fn a_name_inside_a_select_item_is_its_column() {
        assert_eq!(
            contents("SELECT SUM(tot|al) AS n FROM orders"),
            "Column `total` of `orders`: `INT NULL`"
        );
    }

    #[test]
    fn a_star_before_the_item_hides_its_result_column() {
        assert_eq!(
            contents("SELECT *, na|me FROM users"),
            "Column `name` of `users`: `TEXT NULL`"
        );
        assert_eq!(
            contents("SELECT na|me, * FROM users"),
            "Result column `name`: `str | None` (`TEXT`)"
        );
        assert_eq!(
            contents("SELECT DISTINCT ON (id) na|me FROM users"),
            "Result column `name`: `str | None` (`TEXT`)"
        );
    }

    #[test]
    fn nothing_for_keywords_unknown_names_or_dml() {
        assert_eq!(at("SEL|ECT 1"), None);
        assert_eq!(at("SELECT 1 FROM nowh|ere"), None);
        assert_eq!(at("SELECT 1 FROM users u WHERE u.nope| = 1"), None);
        assert_eq!(at("SELECT 1 FROM users WHERE id = 'te|xt'"), None);
        assert_eq!(
            contents("INSERT INTO orders (user_id) SELECT id FROM users RETURNING to|tal"),
            "Column `total` of `orders`: `INT NULL`"
        );
    }

    fn contents_in(schema: &str, dialect: &str, sql: &str) -> String {
        let cursor = sql.find('|').expect("a cursor");
        let sql = sql.replacen('|', "", 1);
        let schema = load(schema, dialect).0;
        hover(&sql, cursor, dialect, Some(&schema)).map_or_else(String::new, |found| found.contents)
    }

    #[test]
    fn rarer_shapes() {
        let wide = format!(
            "CREATE TABLE wide ({});",
            (0..51)
                .map(|n| format!("c{n} INT"))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let views = "CREATE TABLE t (a INT, b TEXT);
            CREATE VIEW v AS SELECT NULL AS x, a FROM t;";
        let found = [
            contents_in(&wide, "postgres", "SELECT 1 FROM wi|de")
                .lines()
                .last()
                .unwrap()
                .to_string(),
            contents_in(&wide, "postgres", "SELECT 1 FROM wide w WHERE |w.c0 = 1")
                .lines()
                .last()
                .unwrap()
                .to_string(),
            contents_in(views, "sqlite", "SELECT 1 FROM |t"),
            contents_in(views, "sqlite", "SELECT 1 FROM |v"),
            contents_in(views, "sqlite", "SELECT 1 FROM v WHERE |x IS NULL"),
            contents_in(views, "sqlite", "SELECT (zz|z) AS q FROM t"),
            contents_in(views, "sqlite", "SELECT NULL AS |q FROM t"),
            contents_in(
                views,
                "sqlite",
                "SELECT u.a AS |q FROM t LEFT JOIN t u ON true",
            )
            .lines()
            .take(3)
            .collect::<Vec<_>>()
            .join("|"),
            contents_in(views, "sqlite", "UPDATE t SET |a = 1"),
            contents_in(views, "sqlite", "SELECT DISTINCT ON ((a), b) |b FROM t"),
            contents_in(
                "CREATE SCHEMA s; CREATE TABLE s.t (a INT);",
                "postgres",
                "SELECT 1 FROM |s.t",
            ),
            contents_in(views, "postgres", "SELECT a::geometry AS |q FROM t"),
            contents_in(views, "sqlite", "SELECT json_extract(b, '$') AS |q FROM t")
                .lines()
                .next()
                .unwrap()
                .to_string(),
            contents_in(
                "CREATE VIEW p AS SELECT $1::int AS p;",
                "postgres",
                "SELECT 1 FROM |p",
            ),
            contents_in(views, "postgres", "SELECT DISTINCT |a FROM t"),
        ];
        assert_eq!(
            found,
            [
                "- ...",
                "- ...",
                "Table `t`\n- `a` `INT NULL`\n- `b` `TEXT NULL`",
                "View `v`\n- `x`\n- `a` `INT NULL`",
                "Column `x` of `v`",
                "Result column `q`",
                "Result column `q`",
                "Result column `q`: `int | None` (`INT`)",
                "Column `a` of `t`: `INT NULL`",
                "Result column `b`: `str | None` (`TEXT`)",
                "",
                "Result column `q`: `GEOMETRY`",
                "Result column `q`: `typing.Any`",
                "View `public.p`\n- `p` `INT`",
                "Result column `a`: `int | None` (`INT`)",
            ]
        );
    }
}
