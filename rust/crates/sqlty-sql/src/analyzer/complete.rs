//! What can be typed at a cursor in a query, possibly unfinished, for completion.
//!
//! [`complete`] decides from the tokens before the cursor what the cursor stands for: a
//! relation (`FROM`, `JOIN`, `UPDATE`, `INTO`), a qualified column (`alias.`) or an
//! unqualified one (`SELECT`, `WHERE`, `ON`, `GROUP BY`, `ORDER BY`, `HAVING`, `SET`,
//! `RETURNING`). Relations come from the catalog's name index, O(log N + k). Columns come
//! from the relations in scope only: the cursor's word is replaced by a placeholder column
//! and the analyzer, reaching it, records the columns its scope offers (aliases, CTEs,
//! subqueries, `JOIN ... USING` included). When the query does not parse that way, a
//! token scan of its `FROM` and `JOIN` items stands in. After `JOIN`, the tables a foreign
//! key links to one already in scope come first, with their `ON` clause.
//!
//! ```
//! use sqlty_sql::analyzer::{complete, load_schema_with_skipped, CandidateKind};
//!
//! let (schema, _) = load_schema_with_skipped(
//!     "CREATE TABLE users (id INTEGER NOT NULL, name TEXT); CREATE TABLE usage (n INTEGER)",
//!     "postgres",
//! );
//! let found = complete("SELECT * FROM us", 16, "postgres", Some(&schema), 50);
//! let labels: Vec<_> = found.candidates.iter().map(|c| c.label.as_str()).collect();
//! assert_eq!(labels, ["usage", "users"]);
//! assert_eq!(found.replace, 14..16);
//!
//! let found = complete("SELECT u. FROM users u", 9, "postgres", Some(&schema), 50);
//! let columns: Vec<_> = found.candidates.iter().map(|c| (c.label.as_str(), c.kind)).collect();
//! assert_eq!(columns, [("id", CandidateKind::Column), ("name", CandidateKind::Column)]);
//! ```

use std::ops::Range;

use super::nullability::Nullability;
use super::{parse, SchemaData, SqlDialect};
use crate::catalog::{Columns, RelationKind};

/// The column name the cursor's word is replaced by, which no schema defines.
pub(super) const PLACEHOLDER: &str = "__sqlty_cursor__";

/// What a completion inserts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CandidateKind {
    /// A schema table.
    Table,
    /// A schema view.
    View,
    /// A CTE of the query.
    Cte,
    /// A column of a relation in scope.
    Column,
    /// A table a foreign key links to one in scope, with the `ON` clause that joins them.
    Join,
}

/// One completion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    /// What to insert: the name as SQL writes it, quoted when it must be.
    pub label: String,
    /// What it is.
    pub kind: CandidateKind,
    /// A relation's schema, or the alias or table a column belongs to.
    pub detail: Option<String>,
    /// A column's SQL type, when known.
    pub sql_type: Option<String>,
    /// Whether a column can be NULL, when known.
    pub nullable: Option<bool>,
}

impl Candidate {
    pub(super) fn column(
        label: &str,
        source: &str,
        sql_type: Option<&str>,
        nullability: Nullability,
    ) -> Self {
        Candidate {
            label: label.to_string(),
            kind: CandidateKind::Column,
            detail: Some(source.to_string()),
            sql_type: sql_type.map(str::to_string),
            nullable: match nullability {
                Nullability::Nullable => Some(true),
                Nullability::NonNull => Some(false),
                Nullability::Unknown => None,
            },
        }
    }
}

/// What can be typed at a cursor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Completions {
    /// Candidates whose name starts with the word before the cursor, at most `limit`.
    pub candidates: Vec<Candidate>,
    /// The bytes of the query a candidate replaces: the word before the cursor.
    pub replace: Range<usize>,
    /// Whether `limit` cut the list short.
    pub incomplete: bool,
}

/// What completes at byte `cursor` of `query`, which need not be finished or parse.
///
/// Costs O(length of the query) to read the context, then O(log N + k) for relations,
/// or the widths of the relations in scope for columns: never a function of how many
/// tables the schema has; after `JOIN`, also O(K + log F + k) per table in scope for the
/// tables its K foreign keys and the F of the schema link to it. Nothing completes in a string, a comment, after a name that
/// already ends an expression, or without a context this module knows.
pub fn complete(
    query: &str,
    cursor: usize,
    dialect: &str,
    schema: Option<&SchemaData>,
    limit: usize,
) -> Completions {
    let dialect = SqlDialect::from_name(dialect);
    let schema = schema.unwrap_or_else(|| crate::catalog::empty(dialect));
    let Some(before) = query.get(..cursor).and_then(lex) else {
        return Completions::default();
    };
    let Some((context, prefix)) = context(&before, cursor) else {
        return Completions::default();
    };
    let typed = &query[prefix.clone()];
    let folded = schema.fold_typed(typed);
    let mut candidates = match context {
        Context::Relation(qualifier, join) => {
            let relations = match &qualifier {
                Some(qualifier) => schema.relations_with_prefix(typed, Some(qualifier), limit + 1),
                None => schema.visible_relations_with_prefix(typed, limit + 1),
            };
            let mut found: Vec<Candidate> = relations
                .into_iter()
                .map(|relation| Candidate {
                    label: spelled(relation.name.name, dialect),
                    kind: match relation.kind {
                        RelationKind::View => CandidateKind::View,
                        RelationKind::Table | RelationKind::Unknown => CandidateKind::Table,
                    },
                    detail: relation.name.schema.map(str::to_string),
                    sql_type: None,
                    nullable: None,
                })
                .collect();
            if join {
                let scope = &before[..before.len() - usize::from(!prefix.is_empty())];
                found.extend(joins(scope, typed, schema, dialect, limit + 1));
            }
            if qualifier.is_none() {
                found.extend(
                    ctes(&before)
                        .into_iter()
                        .filter(|name| name.to_lowercase().starts_with(&folded))
                        .map(|name| Candidate {
                            label: name.to_string(),
                            kind: CandidateKind::Cte,
                            detail: None,
                            sql_type: None,
                            nullable: None,
                        }),
                );
            }
            found
        }
        Context::Column(qualifier) => {
            let mut probe = query[..prefix.start].to_string();
            probe.push_str(PLACEHOLDER);
            probe.push_str(query[cursor..].trim_start_matches(is_word));
            let found = parse(&probe, dialect.name())
                .probe(schema)
                .unwrap_or_else(|| scanned_columns(query, &before, schema));
            let qualifier = qualifier.map(|q| q.to_lowercase());
            found
                .into_iter()
                .filter(|candidate| {
                    qualifier.is_none() || candidate.detail.as_deref() == qualifier.as_deref()
                })
                .filter(|candidate| {
                    schema
                        .fold_typed(&candidate.label)
                        .starts_with(folded.as_str())
                })
                .collect()
        }
    };
    // The schema's own relations before PostgreSQL's system catalogs.
    let builtin = |c: &Candidate| {
        matches!(
            c.detail.as_deref(),
            Some("pg_catalog" | "information_schema")
        ) && c.kind != CandidateKind::Column
    };
    candidates.sort_by_cached_key(|c| {
        (
            c.kind != CandidateKind::Join,
            builtin(c),
            c.label.to_lowercase(),
            c.detail.clone(),
            c.label.clone(),
        )
    });
    // A table named twice without aliases offers each column twice, from the same source.
    candidates.dedup_by(|a, b| a.label == b.label && a.kind == b.kind && a.detail == b.detail);
    let incomplete = candidates.len() > limit;
    candidates.truncate(limit);
    Completions {
        candidates,
        replace: prefix,
        incomplete,
    }
}

/// What the cursor stands for; the qualifier is the schema of a relation, the alias or
/// table of a column.
enum Context {
    /// Also whether it follows `JOIN`.
    Relation(Option<String>, bool),
    Column(Option<String>),
}

/// Clauses whose items are relations.
const RELATION_CLAUSES: [&str; 5] = ["FROM", "JOIN", "UPDATE", "INTO", "TABLE"];
/// Clauses whose items are expressions over columns.
const COLUMN_CLAUSES: [&str; 8] = [
    "SELECT",
    "WHERE",
    "ON",
    "BY",
    "HAVING",
    "SET",
    "RETURNING",
    "DISTINCT",
];
/// Words after which an expression starts.
const OPERATOR_WORDS: [&str; 10] = [
    "AND", "OR", "NOT", "WHEN", "THEN", "ELSE", "IN", "LIKE", "ILIKE", "BETWEEN",
];

/// The context of the cursor at the end of `tokens`, and the bytes of the word it is in.
fn context(tokens: &[Tok], cursor: usize) -> Option<(Context, Range<usize>)> {
    let mut end = tokens.len();
    let prefix = match tokens.last() {
        Some(last) if last.word && last.span.end == cursor => {
            end -= 1;
            last.span.clone()
        }
        _ => cursor..cursor,
    };
    let qualifier = match end.checked_sub(2).map(|i| (&tokens[i], &tokens[i + 1])) {
        Some((name, dot)) if name.word && dot.text == "." => {
            end -= 2;
            Some(name.text.trim_matches(['"', '`']).to_string())
        }
        _ => None,
    };
    let depth = cursor_depth(tokens);
    let prev = end.checked_sub(1).map(|i| &tokens[i]);
    let (clause, crossed) = clause(&tokens[..end], depth)?;
    let is = |tok: Option<&Tok>, words: &[&str]| {
        tok.is_some_and(|tok| tok.word && words.iter().any(|w| tok.text.eq_ignore_ascii_case(w)))
    };
    let context = if RELATION_CLAUSES
        .iter()
        .any(|w| clause.eq_ignore_ascii_case(w))
    {
        if crossed || !(is(prev, &RELATION_CLAUSES) || prev.is_some_and(|tok| tok.text == ",")) {
            return None;
        }
        let join = qualifier.is_none() && is(prev, &["JOIN"]);
        Context::Relation(qualifier, join)
    } else {
        let after_name = prev.is_some_and(|tok| {
            (tok.word && !is(Some(tok), &COLUMN_CLAUSES) && !is(Some(tok), &OPERATOR_WORDS))
                || tok.text == ")"
        });
        if after_name {
            return None;
        }
        Context::Column(qualifier)
    };
    Some((context, prefix))
}

/// The nearest clause keyword before the end of `tokens` at `depth` or around it, and
/// whether a `(` lies between.
fn clause(tokens: &[Tok], mut depth: usize) -> Option<(&str, bool)> {
    let mut crossed = false;
    for tok in tokens.iter().rev() {
        if tok.depth > depth || (tok.depth == depth && tok.text == "(") {
            continue;
        }
        if tok.text == "(" {
            crossed = true;
            depth = tok.depth;
            continue;
        }
        if tok.word && tok.text.eq_ignore_ascii_case("VALUES") {
            return None;
        }
        if tok.word
            && RELATION_CLAUSES
                .iter()
                .chain(&COLUMN_CLAUSES)
                .any(|w| tok.text.eq_ignore_ascii_case(w))
        {
            return Some((&tok.text, crossed));
        }
    }
    None
}

/// The names of the CTEs `WITH name AS (` and `, name AS (` declare.
fn ctes(tokens: &[Tok]) -> Vec<&str> {
    tokens
        .windows(4)
        .filter(|w| {
            (w[0].text.eq_ignore_ascii_case("WITH") || w[0].text == ",")
                && w[1].word
                && w[2].text.eq_ignore_ascii_case("AS")
                && w[3].text == "("
        })
        .map(|w| w[1].text.as_str())
        .collect()
}

/// The tables a foreign key links to those the `FROM` and `JOIN` items of `scope` name,
/// whose name starts with `typed`, each with the `ON` clause joining it: at most `limit`.
/// A table whose name is already a qualifier in scope is left out, since its `ON` clause
/// would be ambiguous.
fn joins(
    scope: &[Tok],
    typed: &str,
    schema: &SchemaData,
    dialect: SqlDialect,
    limit: usize,
) -> Vec<Candidate> {
    let in_scope = named_relations(scope, cursor_depth(scope), schema);
    let qualifiers: Vec<String> = in_scope
        .iter()
        .map(|(_, alias, name)| alias.unwrap_or(name).to_lowercase())
        .collect();
    let mut found = Vec::new();
    for (name, alias, written) in &in_scope {
        let qualifier = alias.unwrap_or(written);
        for join in schema.joins_with_prefix(name, typed, limit - found.len()) {
            let table = match join.table.schema {
                Some(owner) if schema.table_id(join.table.name) != Some(join.table.key) => {
                    format!(
                        "{}.{}",
                        spelled(owner, dialect),
                        spelled(join.table.name, dialect)
                    )
                }
                _ => spelled(join.table.name, dialect),
            };
            if qualifiers.contains(&table.to_lowercase()) {
                continue;
            }
            let on: Vec<String> = join
                .pairs
                .iter()
                .map(|(theirs, ours)| {
                    format!(
                        "{table}.{} = {qualifier}.{}",
                        spelled(theirs, dialect),
                        spelled(ours, dialect)
                    )
                })
                .collect();
            found.push(Candidate {
                label: format!("{table} ON {}", on.join(" AND ")),
                kind: CandidateKind::Join,
                detail: join.table.schema.map(str::to_string),
                sql_type: None,
                nullable: None,
            });
        }
    }
    found
}

/// The columns of the schema relations the `FROM` and `JOIN` items around the cursor name,
/// read from the tokens: what stands in when the query does not parse.
fn scanned_columns(query: &str, before: &[Tok], schema: &SchemaData) -> Vec<Candidate> {
    let Some(tokens) = lex(query) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for (name, alias, written) in named_relations(&tokens, cursor_depth(before), schema) {
        let alias = match alias {
            Some(alias) => alias.to_lowercase(),
            None => schema.fold_typed(written),
        };
        match schema.columns_of(&name) {
            Some(Columns::Table(columns)) => found.extend(columns.iter().map(|col| {
                Candidate::column(
                    &col.ident,
                    &alias,
                    Some(&col.sql_type),
                    Nullability::declared(col.not_null),
                )
            })),
            Some(Columns::View(columns)) => found.extend(columns.iter().map(|col| {
                Candidate::column(&col.name, &alias, col.sql_type.as_deref(), col.nullability)
            })),
            None => {}
        }
    }
    found
}

/// The relations the `FROM` and `JOIN` items of `tokens` at `depth` or around it name: the
/// name, folded and dotted, then the alias and the last part of the name as written.
fn named_relations<'t>(
    tokens: &'t [Tok],
    depth: usize,
    schema: &SchemaData,
) -> Vec<(String, Option<&'t str>, &'t str)> {
    let mut found = Vec::new();
    let mut i = 0;
    let mut in_relations = false;
    while i < tokens.len() {
        let tok = &tokens[i];
        i += 1;
        if tok.depth > depth {
            continue;
        }
        let keyword =
            |words: &[&str]| tok.word && words.iter().any(|w| tok.text.eq_ignore_ascii_case(w));
        if keyword(&RELATION_CLAUSES) {
            in_relations = true;
        } else if !(in_relations && tok.text == ",") {
            in_relations &= !(keyword(&COLUMN_CLAUSES) || is_keyword(&tok.text));
            continue;
        }
        let mut name = Vec::new();
        let mut written = "";
        while let Some(part) = tokens
            .get(i)
            .filter(|part| part.word && !is_keyword(&part.text))
        {
            name.push(schema.fold_typed(&part.text));
            written = &part.text;
            i += 1;
            if tokens.get(i).is_none_or(|dot| dot.text != ".") {
                break;
            }
            i += 1;
        }
        if name.is_empty() {
            continue;
        }
        if tokens
            .get(i)
            .is_some_and(|tok| tok.text.eq_ignore_ascii_case("AS"))
        {
            i += 1;
        }
        let alias = tokens
            .get(i)
            .filter(|tok| tok.word && !is_keyword(&tok.text))
            .map(|tok| tok.text.as_str());
        i += usize::from(alias.is_some());
        found.push((name.join("."), alias, written));
    }
    found
}

/// Whether `word` is a keyword that can follow a `FROM` item, so it is no alias.
fn is_keyword(word: &str) -> bool {
    [
        "WHERE",
        "JOIN",
        "INNER",
        "LEFT",
        "RIGHT",
        "FULL",
        "CROSS",
        "ON",
        "USING",
        "GROUP",
        "ORDER",
        "HAVING",
        "LIMIT",
        "SET",
        "RETURNING",
        "UNION",
        "NATURAL",
        "WINDOW",
        "VALUES",
        "SELECT",
        "DEFAULT",
        "OFFSET",
        "FETCH",
        "FOR",
        "LATERAL",
    ]
    .iter()
    .any(|w| word.eq_ignore_ascii_case(w))
}

/// `name` as a query writes it: double-quoted in PostgreSQL when folding would change it.
fn spelled(name: &str, dialect: SqlDialect) -> String {
    let plain = name.starts_with(|c: char| c.is_ascii_lowercase() || c == '_')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '$');
    if dialect == SqlDialect::Postgres && !plain {
        format!("\"{}\"", name.replace('"', "\"\""))
    } else {
        name.to_string()
    }
}

/// Open parentheses before the end of `tokens`.
fn cursor_depth(tokens: &[Tok]) -> usize {
    tokens
        .last()
        .map_or(0, |tok| tok.depth + usize::from(tok.text == "("))
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

/// A token of the query: a word (name or keyword, quoted or not) or one punctuation char.
pub(super) struct Tok {
    pub(super) text: String,
    pub(super) word: bool,
    pub(super) span: Range<usize>,
    /// Open parentheses before it; a `(` and its `)` count the outer level.
    pub(super) depth: usize,
}

/// The tokens of `text`, without strings and comments; `None` when `text` ends inside a
/// string or a comment. A quoted name left open at the end is a word.
pub(super) fn lex(text: &str) -> Option<Vec<Tok>> {
    let mut tokens = Vec::new();
    let mut depth = 0usize;
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        let rest = &text[start..];
        let skip_to = |end: Option<usize>| end.map(|end| start + end);
        let end = if c.is_whitespace() {
            continue;
        } else if rest.starts_with("--") {
            skip_to(rest.find('\n').map(|n| n + 1))
        } else if rest.starts_with("/*") {
            skip_to(rest.find("*/").map(|n| n + 2))
        } else if c == '\'' {
            skip_to(rest[1..].find('\'').map(|n| n + 2))
        } else if c == '"' || c == '`' {
            let end = rest[1..].find(c).map_or(text.len(), |n| start + n + 2);
            tokens.push(Tok {
                text: text[start..end].to_string(),
                word: true,
                span: start..end,
                depth,
            });
            Some(end)
        } else if is_word(c) {
            let end = rest.find(|c| !is_word(c)).map_or(text.len(), |n| start + n);
            tokens.push(Tok {
                text: text[start..end].to_string(),
                word: true,
                span: start..end,
                depth,
            });
            Some(end)
        } else {
            if c == ')' {
                depth = depth.saturating_sub(1);
            }
            tokens.push(Tok {
                text: c.to_string(),
                word: false,
                span: start..start + c.len_utf8(),
                depth,
            });
            if c == '(' {
                depth += 1;
            }
            Some(start + c.len_utf8())
        };
        let end = end?;
        while chars.peek().is_some_and(|&(at, _)| at < end) {
            chars.next();
        }
    }
    Some(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{load, Catalog};

    const SCHEMA: &str =
        "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, email TEXT NOT NULL);
        CREATE TABLE orders (id INTEGER PRIMARY KEY, user_id INTEGER, total NUMERIC);
        CREATE VIEW user_names AS SELECT id, name FROM users;
        CREATE TABLE legacy (LIKE missing);";

    /// The labels completing at the `|` of `sql`, as `label:detail`.
    fn labels(sql: &str, catalog: &Catalog, dialect: &str) -> Vec<String> {
        let cursor = sql.find('|').expect("a cursor");
        let sql = sql.replacen('|', "", 1);
        let found = complete(&sql, cursor, dialect, Some(catalog), 50);
        assert!(
            found.candidates.is_empty() || found.replace.end == cursor,
            "{sql}"
        );
        found
            .candidates
            .iter()
            .map(|c| format!("{}:{}", c.label, c.detail.as_deref().unwrap_or("")))
            .collect()
    }

    fn pg(sql: &str) -> Vec<String> {
        labels(sql, &load(SCHEMA, "postgres").0, "postgres")
    }

    #[test]
    fn relations() {
        assert_eq!(
            pg("SELECT * FROM us|"),
            ["user_names:public", "users:public"]
        );
        assert_eq!(pg("SELECT * FROM users u JOIN o|"), ["orders:public"]);
        assert_eq!(pg("SELECT * FROM users, ord|"), ["orders:public"]);
        assert_eq!(pg("SELECT * FROM public.or|"), ["orders:public"]);
        assert_eq!(pg("UPDATE ord| SET total = 1"), ["orders:public"]);
        assert_eq!(pg("INSERT INTO leg| VALUES (1)"), ["legacy:public"]);
        assert_eq!(
            pg("WITH recent AS (SELECT 1) SELECT * FROM re|"),
            ["recent:"]
        );
    }

    #[test]
    fn joins_on_foreign_keys_come_first() {
        let catalog = load(
            "CREATE TABLE users (id INT PRIMARY KEY, manager_id INT REFERENCES users);
            CREATE TABLE orders (id INT PRIMARY KEY, user_id INT REFERENCES users (id));
            CREATE TABLE items (order_id INT, line INT, PRIMARY KEY (order_id, line),
                FOREIGN KEY (order_id) REFERENCES orders);
            CREATE TABLE notes (order_id INT, line INT,
                FOREIGN KEY (order_id, line) REFERENCES items (order_id, line));
            CREATE SCHEMA billing;
            CREATE TABLE billing.invoices (id INT, order_id INT REFERENCES orders);",
            "postgres",
        )
        .0;
        let pg = |sql: &str| labels(sql, &catalog, "postgres");
        assert_eq!(
            pg("SELECT * FROM orders o JOIN |")[..4],
            [
                "billing.invoices ON billing.invoices.order_id = o.id:billing",
                "items ON items.order_id = o.id:public",
                "users ON users.id = o.user_id:public",
                "items:public",
            ]
        );
        assert_eq!(
            pg("SELECT * FROM orders JOIN us|"),
            ["users ON users.id = orders.user_id:public", "users:public"]
        );
        assert_eq!(
            pg("SELECT * FROM items i JOIN no|"),
            [
                "notes ON notes.order_id = i.order_id AND notes.line = i.line:public",
                "notes:public"
            ]
        );
        assert_eq!(
            pg("SELECT * FROM users JOIN orders o ON o.user_id = users.id JOIN us|"),
            ["users:public"],
            "a name already in scope makes its ON clause ambiguous"
        );
        assert_eq!(
            pg("SELECT * FROM users u JOIN us|"),
            [
                "users ON users.id = u.manager_id:public",
                "users ON users.manager_id = u.id:public",
                "users:public"
            ]
        );
        assert_eq!(pg("SELECT * FROM orders o, us|"), ["users:public"]);
        assert_eq!(
            pg("SELECT * FROM orders o JOIN public.us|"),
            ["users:public"]
        );
    }

    #[test]
    fn schema_relations_come_before_system_catalogs() {
        let found = pg("SELECT * FROM |");
        assert_eq!(
            found[..4],
            [
                "legacy:public",
                "orders:public",
                "user_names:public",
                "users:public"
            ]
        );
        assert!(found[4..]
            .iter()
            .all(|label| label.ends_with(":pg_catalog")));
    }

    #[test]
    fn qualified_columns() {
        assert_eq!(pg("SELECT u.| FROM users u"), ["email:u", "id:u", "name:u"]);
        assert_eq!(pg("SELECT u.na| FROM users u"), ["name:u"]);
        assert_eq!(pg("SELECT users.e| FROM users"), ["email:users"]);
        assert_eq!(
            pg("SELECT o.| FROM users u JOIN orders o ON u.id = o.user_id"),
            ["id:o", "total:o", "user_id:o"]
        );
        assert_eq!(pg("SELECT v.| FROM user_names v"), ["id:v", "name:v"]);
    }

    #[test]
    fn unqualified_columns_in_each_clause() {
        let both = ["id:o", "id:u", "total:o", "user_id:o"];
        let from = "FROM users u JOIN orders o ON u.id = o.user_id";
        assert_eq!(pg(&format!("SELECT u.name, | {from}"))[..1], ["email:u"]);
        assert_eq!(pg(&format!("SELECT * {from} WHERE i|")), ["id:o", "id:u"]);
        assert_eq!(
            pg(&format!("SELECT * {from} WHERE u.id = 1 AND t|")),
            ["total:o"]
        );
        assert_eq!(pg(&format!("SELECT * {from} ORDER BY |"))[..1], ["email:u"]);
        assert_eq!(
            pg(&format!("SELECT count(*) {from} GROUP BY u|")),
            ["user_id:o"]
        );
        assert_eq!(
            pg(&format!("SELECT * {from} WHERE |")),
            ["email:u", "id:o", "id:u", "name:u", "total:o", "user_id:o"]
        );
        assert_eq!(
            pg("SELECT * FROM users u JOIN orders o ON |")
                .into_iter()
                .filter(|l| !l.contains("name") && !l.contains("email"))
                .collect::<Vec<_>>(),
            both
        );
        assert_eq!(pg("UPDATE users SET na| = 'x'"), ["name:users"]);
        assert_eq!(
            pg("INSERT INTO orders (total) VALUES (1) RETURNING us|"),
            ["user_id:orders"]
        );
    }

    #[test]
    fn half_written_queries() {
        assert_eq!(
            pg("SELECT o.| FROM users u JOIN orders o ON u.id ="),
            ["id:o", "total:o", "user_id:o"]
        );
        assert_eq!(pg("SELECT u.na| FROM users u WHERE"), ["name:u"]);
        assert_eq!(pg("SELECT id, | FROM users WHERE id IN (").len(), 3);
        assert!(pg("SELECT |").is_empty(), "no FROM: no columns");
        assert!(pg("SELECT l.| FROM legacy l").is_empty(), "unknown shape");
    }

    #[test]
    fn cte_and_subquery_scope() {
        assert_eq!(
            pg("WITH recent AS (SELECT id AS rid FROM orders) SELECT r.| FROM recent r"),
            ["rid:r"]
        );
        assert_eq!(
            pg("SELECT s.| FROM (SELECT id, total AS amount FROM orders) s"),
            ["amount:s", "id:s"]
        );
        assert_eq!(
            pg("SELECT * FROM users u WHERE EXISTS (SELECT 1 FROM orders o WHERE o.user_id = u.|)"),
            ["email:u", "id:u", "name:u"]
        );
        assert_eq!(
            pg("SELECT * FROM users JOIN orders USING (id) WHERE tot|"),
            ["total:orders"]
        );
    }

    #[test]
    fn nothing_completes() {
        for sql in [
            "SELECT 'us|' FROM users",
            "SELECT 1 -- us|",
            "SELECT * FROM users u|",
            "SELECT * FROM users AS |",
            "SELECT name | FROM users",
            "SELECT count(*) | FROM users",
            "SELECT * FROM (|",
            "INSERT INTO users VALUES (|",
            "us|",
        ] {
            assert!(pg(sql).is_empty(), "{sql}");
        }
    }

    #[test]
    fn candidates_carry_type_and_nullability() {
        let catalog = load(SCHEMA, "postgres").0;
        let found = complete("SELECT u. FROM users u", 9, "postgres", Some(&catalog), 50);
        let email = &found.candidates[0];
        assert_eq!(
            (email.sql_type.as_deref(), email.nullable, email.kind),
            (Some("TEXT"), Some(false), CandidateKind::Column)
        );
        assert_eq!(found.candidates[2].nullable, Some(true));
        let views = complete("SELECT * FROM user_n", 20, "postgres", Some(&catalog), 50);
        assert_eq!(views.candidates[0].kind, CandidateKind::View);
    }

    #[test]
    fn limit_marks_the_list_incomplete() {
        let catalog = load(SCHEMA, "postgres").0;
        let found = complete("SELECT u. FROM users u", 9, "postgres", Some(&catalog), 2);
        assert_eq!((found.candidates.len(), found.incomplete), (2, true));
        let found = complete("SELECT u. FROM users u", 9, "postgres", Some(&catalog), 3);
        assert!(!found.incomplete);
    }

    #[test]
    fn quoted_names() {
        let catalog = load(
            r#"CREATE TABLE "UserLog" ("Kind" TEXT, at INT);"#,
            "postgres",
        )
        .0;
        assert_eq!(
            labels(r#"SELECT * FROM "Us|"#, &catalog, "postgres"),
            [r#""UserLog":public"#]
        );
        assert_eq!(
            labels(r#"SELECT l.| FROM "UserLog" l"#, &catalog, "postgres"),
            [r#""Kind":l"#, "at:l"]
        );
    }

    #[test]
    fn mysql_and_sqlite() {
        for dialect in ["mysql", "sqlite"] {
            let catalog = load(
                "CREATE TABLE Users (id INTEGER PRIMARY KEY, Name TEXT); CREATE TABLE orders (id INTEGER, user_id INTEGER);",
                dialect,
            )
            .0;
            let complete = |sql: &str| labels(sql, &catalog, dialect);
            assert_eq!(complete("SELECT * FROM US|"), ["users:"], "{dialect}");
            assert_eq!(
                complete("SELECT u.| FROM Users u"),
                ["id:u", "Name:u"],
                "{dialect}"
            );
            assert_eq!(
                complete("SELECT * FROM Users u JOIN orders o ON o.user_id = u.|"),
                ["id:u", "Name:u"],
                "{dialect}"
            );
            assert_eq!(
                complete("SELECT n| FROM Users"),
                ["Name:users"],
                "{dialect}"
            );
        }
    }

    /// Completion looks up the relations in scope by name and walks only the matches of a
    /// prefix: the catalog entries it reaches are as many at 50, 540 and 5000 tables.
    #[test]
    fn work_does_not_depend_on_the_number_of_tables() {
        let queries = [
            ("SELECT * FROM t1|", 5),
            ("SELECT * FROM app.t1|", 5),
            ("SELECT u.| FROM users u JOIN t7 x ON x.id = u.id", 50),
            (
                "SELECT * FROM users u JOIN t7 x ON x.id = u.id WHERE n|",
                50,
            ),
            ("SELECT o.| FROM users u JOIN t7 o ON", 50),
            ("SELECT * FROM users u JOIN t1|", 5),
            ("SELECT * FROM t7 x JOIN |", 5),
        ];
        let runs: Vec<Vec<(usize, Vec<String>)>> = [50, 540, 5000]
            .into_iter()
            .map(|tables| {
                let mut sql = String::from("CREATE SCHEMA app; SET search_path TO app, public;\n");
                sql += "CREATE TABLE users (id INT PRIMARY KEY, name TEXT);\n";
                for i in 0..tables {
                    sql += &format!(
                        "CREATE TABLE t{i} (id INT, note TEXT, user_id INT REFERENCES users);\n"
                    );
                }
                let catalog = load(&sql, "postgres").0;
                queries
                    .iter()
                    .map(|(query, limit)| {
                        let cursor = query.find('|').unwrap();
                        let query = query.replacen('|', "", 1);
                        let mut found = Completions::default();
                        let visited = crate::catalog::visited(|| {
                            found = complete(&query, cursor, "postgres", Some(&catalog), *limit);
                        });
                        let labels = found.candidates.into_iter().map(|c| c.label).collect();
                        (visited, labels)
                    })
                    .collect()
            })
            .collect();
        let counts = |run: &[(usize, Vec<String>)]| -> Vec<(usize, usize)> {
            run.iter()
                .map(|(visited, labels)| (*visited, labels.len()))
                .collect()
        };
        assert_eq!(counts(&runs[0]), counts(&runs[1]));
        assert_eq!(counts(&runs[1]), counts(&runs[2]));
        assert_eq!(runs[2][0].1.len(), 5);
        assert_eq!(runs[2][2].1, ["id", "name"]);
        assert_eq!(
            runs[2][5].1[..2],
            ["t1 ON t1.user_id = u.id", "t10 ON t10.user_id = u.id"]
        );
        assert_eq!(runs[2][6].1[0], "users ON users.id = x.user_id");
        assert!(
            runs[2].iter().all(|(visited, _)| *visited <= 64),
            "{:?}",
            runs[2]
        );
    }
}
