//! Where a name of a query is defined, for go to definition: the `CREATE TABLE`, `VIEW` or
//! column definition of the schema that last defined it, or the alias, CTE or select item
//! of the query itself: a column of a subquery or a CTE goes to the select item that gives
//! it, and an `ORDER BY` or `GROUP BY` alias or position to the select item it names (the
//! first branch of a set operation), or on through a plain item to the column it reads.
//!
//! A name is read as hover reads it, through completion at the end of the word, so it costs
//! what completion does; then a lookup by key finds where the schema defines it, and only
//! the statement found is read again for the name's bytes.
//!
//! ```
//! use sqlty_sql::analyzer::{definition, load_schema_with_skipped, Definition};
//!
//! let schema_sql = "CREATE TABLE users (id INTEGER, name TEXT);";
//! let (schema, _) = load_schema_with_skipped(schema_sql, "postgres");
//! let sql = "SELECT name FROM users";
//! let found = definition(sql, sql.find("name").unwrap(), "postgres", Some(&schema), Some(schema_sql));
//! let Some(Definition::Schema(range)) = found else { panic!() };
//! assert_eq!(&schema_sql[range], "name");
//! ```

use std::ops::Range;

use super::complete::{
    complete, is_keyword, lex, CandidateKind, Tok, COLUMN_CLAUSES, RELATION_CLAUSES,
};
use super::hover::AFTER_SELECT;
use super::SchemaData;

/// Candidates the column lookup reads, as hover does.
const SHOWN: usize = 50;

/// Where a name is defined.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Definition {
    /// Bytes of the query: an alias, a CTE or a select item.
    Query(Range<usize>),
    /// Bytes of the schema text the catalog was loaded from: the name in the statement that
    /// last defined it, or the start of that statement when the name is not found in it.
    Schema(Range<usize>),
}

/// Where the name at byte `cursor` of `query` is defined; `None` outside a word, for a
/// keyword, or for a name neither the query nor the schema defines (a system catalog's
/// included). `schema_sql` is the text `schema` was loaded from, which a schema target is in;
/// without it, a schema target is the start of its statement. A query routed to another
/// database finds nothing in the schema.
pub fn definition(
    query: &str,
    cursor: usize,
    dialect: &str,
    schema: Option<&SchemaData>,
    schema_sql: Option<&str>,
) -> Option<Definition> {
    let (routed_dialect, routed) = super::route(query, dialect, schema);
    let own = match (schema, routed) {
        (Some(schema), Some(routed)) if schema.shares_data(routed) => Some(routed),
        _ => None,
    };
    let empty = crate::catalog::empty(super::SqlDialect::from_name(routed_dialect));
    let tokens = lex(query)?;
    let at = tokens
        .iter()
        .position(|tok| tok.word && tok.span.start <= cursor && cursor <= tok.span.end)?;
    let names = Names {
        query,
        tokens: &tokens,
        dialect: routed_dialect,
        schema: routed.unwrap_or(empty),
        own,
        schema_sql,
        items: items(&tokens),
        ctes: ctes(&tokens),
    };
    names.at(at)
}

struct Names<'a> {
    query: &'a str,
    tokens: &'a [Tok],
    dialect: &'a str,
    /// The schema names are looked up in.
    schema: &'a SchemaData,
    /// The schema whose text is `schema_sql`, when the query is not routed elsewhere.
    own: Option<&'a SchemaData>,
    schema_sql: Option<&'a str>,
    items: Vec<Item>,
    ctes: Vec<Cte>,
}

/// A `FROM`, `JOIN`, `UPDATE` or `INTO` item: token positions.
struct Item {
    /// The parts of its name, none for a subquery.
    name: Vec<usize>,
    alias: Option<usize>,
    /// The column aliases after its alias: they rename its first columns.
    columns: Vec<usize>,
    /// The `(` of a subquery.
    subquery: Option<usize>,
    depth: usize,
}

/// `name [(columns)] AS (` of a `WITH`.
struct Cte {
    name: usize,
    columns: Vec<usize>,
    /// The `(` of its query.
    body: usize,
}

impl Names<'_> {
    fn fold(&self, tok: usize) -> String {
        self.schema.fold_typed(&self.tokens[tok].text)
    }

    fn query_at(&self, tok: usize) -> Option<Definition> {
        Some(Definition::Query(self.tokens[tok].span.clone()))
    }

    fn at(&self, at: usize) -> Option<Definition> {
        let tokens = self.tokens;
        let dot_after = tokens.get(at + 1).is_some_and(|tok| tok.text == ".");
        let dot_before = at > 0 && tokens[at - 1].text == ".";
        if let Some(cte) = self.ctes.iter().find(|cte| cte.name == at) {
            return self.query_at(cte.name);
        }
        let listed = |columns: &[usize]| columns.contains(&at);
        if self.ctes.iter().any(|cte| listed(&cte.columns))
            || self.items.iter().any(|item| listed(&item.columns))
        {
            return self.query_at(at);
        }
        for item in &self.items {
            if item.alias == Some(at) {
                return self.query_at(at);
            }
            if let Some(part) = item.name.iter().position(|&tok| tok == at) {
                // A schema name, or a function's: nothing a query names is defined there.
                if part + 1 != item.name.len() {
                    return None;
                }
                return self.relation(item);
            }
        }
        if dot_after {
            return self.qualifier(at);
        }
        if dot_before && at >= 2 && tokens[at - 2].word {
            let source = self.source(&self.fold(at - 2), tokens[at].depth)?;
            return self.column_of(source, &self.fold(at));
        }
        if let Some(item) = self.listing(at) {
            return self.column_of(item, &self.fold(at));
        }
        self.bare(at)
    }

    /// The item whose columns the parentheses around `at` list: `INSERT INTO t (a, b)`, or
    /// the item before `USING (a)`.
    fn listing(&self, at: usize) -> Option<&Item> {
        let tokens = self.tokens;
        let depth = tokens[at].depth.checked_sub(1)?;
        let open = (0..at)
            .rev()
            .find(|&tok| tokens[tok].depth == depth && tokens[tok].text == "(")?;
        let before = open.checked_sub(1)?;
        if tokens[before].text.eq_ignore_ascii_case("USING") {
            return self
                .items
                .iter()
                .rev()
                .find(|item| item.depth == depth && item.name.last().is_some_and(|&n| n < before));
        }
        self.items
            .iter()
            .find(|item| item.name.last() == Some(&before))
    }

    /// The relation an item names: a CTE, or a table or view of the schema.
    fn relation(&self, item: &Item) -> Option<Definition> {
        if let [only] = item.name.as_slice() {
            if let Some(cte) = self.cte(&self.fold(*only)) {
                return self.query_at(cte.name);
            }
        }
        let name: Vec<String> = item.name.iter().map(|&tok| self.fold(tok)).collect();
        self.in_schema(&name.join("."), None)
    }

    fn cte(&self, name: &str) -> Option<&Cte> {
        self.ctes.iter().find(|cte| self.fold(cte.name) == name)
    }

    /// What a word before `.` names: an alias, an item named without one, a CTE, or a
    /// relation of the schema.
    fn qualifier(&self, at: usize) -> Option<Definition> {
        let name = self.fold(at);
        match self.source(&name, self.tokens[at].depth) {
            Some(item) => match item.alias {
                Some(alias) => self.query_at(alias),
                None => self.relation(item),
            },
            None => match self.cte(&name) {
                Some(cte) => self.query_at(cte.name),
                None => self.in_schema(&name, None),
            },
        }
    }

    /// The item an alias, or the last part of the name of an item without one, stands
    /// for: of those, the one nearest the query level at `depth`.
    fn source(&self, name: &str, depth: usize) -> Option<&Item> {
        let names = |item: &Item| match (item.alias, item.name.last()) {
            (Some(alias), _) => self.fold(alias) == name,
            (None, Some(&last)) => self.fold(last) == name,
            (None, None) => false,
        };
        self.items
            .iter()
            .filter(|item| names(item))
            .min_by_key(|item| match item.depth <= depth {
                true => depth - item.depth,
                false => usize::MAX / 2 + item.depth - depth,
            })
    }

    /// The column `name` of the relation `item` reads: a select item of a subquery or a
    /// CTE, or a column of the schema.
    /// The column `name` of the relation `item` reads: a column alias, a select item of a
    /// subquery or a CTE, or a column of the schema.
    fn column_of(&self, item: &Item, name: &str) -> Option<Definition> {
        let renamed = |columns: &[usize]| {
            let found = columns.iter().find(|&&tok| self.fold(tok) == name);
            found.map(|&tok| self.query_at(tok))
        };
        if let Some(found) = renamed(&item.columns) {
            return found;
        }
        let skip = item.columns.len();
        if let Some(open) = item.subquery {
            return self.output(inside(self.tokens, open), name, skip);
        }
        if let [only] = item.name.as_slice() {
            if let Some(cte) = self.cte(&self.fold(*only)) {
                if let Some(found) = renamed(&cte.columns) {
                    return found;
                }
                let skip = skip.max(cte.columns.len());
                return self.output(inside(self.tokens, cte.body), name, skip);
            }
        }
        let relation: Vec<String> = item.name.iter().map(|&tok| self.fold(tok)).collect();
        self.in_schema(&relation.join("."), Some((name, skip)))
    }

    /// The bytes of the select item named `name` of the query at `level`, past the first
    /// `skip` that column aliases rename.
    fn output(&self, level: (usize, usize), name: &str, skip: usize) -> Option<Definition> {
        let found = self.named(level, name, skip)?;
        Some(Definition::Query(found.span))
    }

    fn named(&self, level: (usize, usize), name: &str, skip: usize) -> Option<SelectItem> {
        select_list(self.query, self.tokens, level)
            .into_iter()
            .skip(skip)
            .find(|item| item.name.is_some_and(|tok| self.fold(tok) == name))
    }

    /// Where the column an `ORDER BY` or `GROUP BY` names is defined: its select item, or,
    /// for one that only reads a column, where that column is.
    fn item(&self, item: SelectItem) -> Definition {
        let read = match (item.plain, item.name) {
            (true, Some(name)) => self.at(name),
            _ => None,
        };
        read.unwrap_or(Definition::Query(item.span))
    }

    /// A word that is no qualifier: a column completion offers for it, then an output
    /// column an `ORDER BY` or `GROUP BY` names by alias or position.
    fn bare(&self, at: usize) -> Option<Definition> {
        let tokens = self.tokens;
        let word = &tokens[at];
        let by = grouping(tokens, at);
        let level = || level(tokens, at);
        if by.is_some() && word.text.bytes().all(|b| b.is_ascii_digit()) {
            let position: usize = word.text.parse().ok()?;
            let items = select_list(self.query, tokens, level());
            let before = items.get(..position.checked_sub(1)?)?;
            // The columns a `*` gives, and so the item a later position names, are unknown.
            let found = items.get(position - 1).filter(|item| !item.star);
            if before.iter().any(|item| item.star) {
                return None;
            }
            return found.map(|found| self.item(found.clone()));
        }
        let name = self.fold(at);
        let output = || self.named(level(), &name, 0).map(|found| self.item(found));
        if by == Some(Grouping::Order) {
            if let Some(found) = output() {
                return Some(found);
            }
        }
        self.column(at, &name)
            .or_else(|| by.is_some().then(output).flatten())
    }

    /// The column completion offers for exactly the word at `at`, where its relation
    /// defines it.
    fn column(&self, at: usize, name: &str) -> Option<Definition> {
        let word = &self.tokens[at];
        let found = complete(
            self.query,
            word.span.end,
            self.dialect,
            Some(self.schema),
            SHOWN,
        );
        let candidate = found.candidates.iter().find(|c| {
            c.kind == CandidateKind::Column && self.schema.fold_typed(&c.label) == name
        })?;
        let source = self.source(candidate.detail.as_deref()?, word.depth)?;
        self.column_of(source, name)
    }

    /// Where the schema defines the relation `relation` or its column `column`, both
    /// folded; a column of the first `skip` is one column aliases rename.
    fn in_schema(&self, relation: &str, column: Option<(&str, usize)>) -> Option<Definition> {
        let schema = self.own?;
        let (key, source) = schema.source(relation)?;
        let (at, name) = match column {
            Some((column, skip)) => {
                let position = schema.column_position(key, column)?;
                if position < skip {
                    return None;
                }
                (source.column_at(position), column)
            }
            None => (
                source.at.clone(),
                key.split_once('.').map_or(key, |(_, name)| name),
            ),
        };
        let at = at.start as usize..at.end as usize;
        let named = self
            .schema_sql
            .and_then(|sql| sql.get(at.clone()))
            .and_then(|text| name_in(text, name, column.is_some(), schema))
            .map(|found| at.start + found.start..at.start + found.end);
        Some(Definition::Schema(named.unwrap_or(at.start..at.start)))
    }
}

/// The bytes of `name`, folded, in the statement `text` that defines it: the first
/// spelling after a word that introduces a name of its kind, else the first spelling.
fn name_in(text: &str, name: &str, column: bool, schema: &SchemaData) -> Option<Range<usize>> {
    const RELATION: [&str; 5] = ["TABLE", "VIEW", "EXISTS", ".", "TO"];
    const COLUMN: [&str; 9] = [
        "(", ",", "COLUMN", "ADD", "EXISTS", "TO", "AS", "SELECT", ".",
    ];
    let after: &[&str] = if column { &COLUMN } else { &RELATION };
    let tokens = lex(text)?;
    let spelled: Vec<usize> = (0..tokens.len())
        .filter(|&at| tokens[at].word && schema.fold_typed(&tokens[at].text) == name)
        .collect();
    let introduced = spelled.iter().find(|&&at| {
        at > 0
            && after
                .iter()
                .any(|word| tokens[at - 1].text.eq_ignore_ascii_case(word))
    });
    let at = *introduced.or(spelled.first())?;
    Some(tokens[at].span.clone())
}

fn is_clause(word: &str) -> bool {
    RELATION_CLAUSES
        .iter()
        .chain(&COLUMN_CLAUSES)
        .chain(&AFTER_SELECT)
        .any(|w| word.eq_ignore_ascii_case(w))
}

/// The `FROM`, `JOIN`, `UPDATE` and `INTO` items of every level of `tokens`.
fn items(tokens: &[Tok]) -> Vec<Item> {
    let mut found = Vec::new();
    let mut listing: Vec<bool> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let tok = &tokens[i];
        let depth = tok.depth;
        if listing.len() <= depth {
            listing.resize(depth + 1, false);
        }
        i += 1;
        let opens = tok.word
            && RELATION_CLAUSES
                .iter()
                .any(|w| tok.text.eq_ignore_ascii_case(w));
        if opens {
            listing[depth] = true;
        } else if !(listing[depth] && tok.text == ",") {
            if tok.word && (is_clause(&tok.text) || is_keyword(&tok.text)) {
                listing[depth] = false;
            }
            continue;
        }
        if tokens
            .get(i)
            .is_some_and(|tok| tok.text.eq_ignore_ascii_case("LATERAL"))
        {
            i += 1;
        }
        let mut item = Item {
            name: Vec::new(),
            alias: None,
            columns: Vec::new(),
            subquery: None,
            depth,
        };
        let mut resume = None;
        if tokens.get(i).is_some_and(|tok| tok.text == "(") {
            let Some(close) = (i + 1..tokens.len())
                .find(|&at| tokens[at].depth == depth && tokens[at].text == ")")
            else {
                break;
            };
            let reads = tokens.get(i + 1).is_some_and(|tok| {
                ["SELECT", "WITH", "VALUES"]
                    .iter()
                    .any(|w| tok.text.eq_ignore_ascii_case(w))
            });
            if !reads {
                continue;
            }
            item.subquery = Some(i);
            // Its own items follow, read once its alias is.
            resume = Some(i + 1);
            i = close + 1;
        } else {
            while tokens
                .get(i)
                .is_some_and(|part| part.word && !is_keyword(&part.text))
            {
                item.name.push(i);
                i += 1;
                if tokens.get(i).is_none_or(|dot| dot.text != ".") {
                    break;
                }
                i += 1;
            }
            if item.name.is_empty() {
                continue;
            }
            // A function's rows: its name defines nothing a query reads. After `INTO`, a
            // column list follows.
            if !tok.text.eq_ignore_ascii_case("INTO")
                && tokens.get(i).is_some_and(|tok| tok.text == "(")
            {
                item.name.clear();
                let close = (i + 1..tokens.len())
                    .find(|&at| tokens[at].depth == depth && tokens[at].text == ")");
                i = close.map_or(tokens.len(), |close| close + 1);
            }
        }
        if tokens
            .get(i)
            .is_some_and(|tok| tok.text.eq_ignore_ascii_case("AS"))
        {
            i += 1;
        }
        if tokens
            .get(i)
            .is_some_and(|tok| tok.word && !is_keyword(&tok.text) && !is_clause(&tok.text))
        {
            item.alias = Some(i);
            i += 1;
            if !tok.text.eq_ignore_ascii_case("INTO")
                && tokens.get(i).is_some_and(|t| t.text == "(")
            {
                let close = closing(tokens, i);
                item.columns.extend((i + 1..close).filter(|&at| {
                    tokens[at].word && [",", "("].contains(&tokens[at - 1].text.as_str())
                }));
                i = close + 1;
            }
        }
        found.push(item);
        i = resume.unwrap_or(i);
    }
    found
}

/// The clause a word at the top of an `ORDER BY` or `GROUP BY` item is in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Grouping {
    Order,
    Group,
}

/// Whether the word at `at` is a whole `ORDER BY` or `GROUP BY` item at its level.
fn grouping(tokens: &[Tok], at: usize) -> Option<Grouping> {
    let depth = tokens[at].depth;
    let prev = tokens[..at].iter().rposition(|tok| tok.depth == depth)?;
    if !(tokens[prev].text == "," || tokens[prev].text.eq_ignore_ascii_case("BY")) {
        return None;
    }
    let clause = tokens[..at]
        .iter()
        .rposition(|tok| tok.depth == depth && tok.word && tok.text.eq_ignore_ascii_case("BY"))?;
    let between = &tokens[clause + 1..at];
    if between
        .iter()
        .any(|tok| tok.depth == depth && tok.word && is_clause(&tok.text))
    {
        return None;
    }
    let keyword = tokens[..clause]
        .iter()
        .rev()
        .find(|tok| tok.depth == depth)?;
    if keyword.text.eq_ignore_ascii_case("ORDER") {
        Some(Grouping::Order)
    } else if keyword.text.eq_ignore_ascii_case("GROUP") {
        Some(Grouping::Group)
    } else {
        None
    }
}

/// The query level of the token at `at`, as `(depth, first token)`: inside the parentheses
/// around it, or the statement it is in.
fn level(tokens: &[Tok], at: usize) -> (usize, usize) {
    let depth = tokens[at].depth;
    let Some(outer) = depth.checked_sub(1) else {
        let start = tokens[..at].iter().rposition(|tok| tok.text == ";");
        return (0, start.map_or(0, |end| end + 1));
    };
    let open = tokens[..at]
        .iter()
        .rposition(|tok| tok.depth == outer && tok.text == "(");
    open.map_or((depth, 0), |open| inside(tokens, open))
}

/// The query level that opens after the `(` at `open`.
fn inside(tokens: &[Tok], open: usize) -> (usize, usize) {
    (tokens[open].depth + 1, open + 1)
}

/// A select item: its bytes, and the token of the name it gives its column.
#[derive(Clone)]
struct SelectItem {
    span: Range<usize>,
    name: Option<usize>,
    /// It only reads a column: `name` or `alias.name`.
    plain: bool,
    /// `*` or `alias.*`.
    star: bool,
}

/// The select items of the first `SELECT` of the query at `level` (the first branch of a set
/// operation), each from the `SELECT` or `,` before it, as the lexer drops string literals.
fn select_list(query: &str, tokens: &[Tok], (depth, from): (usize, usize)) -> Vec<SelectItem> {
    let is = |tok: &Tok, word: &str| tok.word && tok.text.eq_ignore_ascii_case(word);
    let first = tokens.get(from).filter(|tok| tok.depth == depth);
    if first.is_some_and(|tok| tok.text == "(") {
        return select_list(query, tokens, inside(tokens, from));
    }
    let Some(select) = tokens[from..]
        .iter()
        .take_while(|tok| tok.depth >= depth && !(tok.depth == depth && tok.text == ";"))
        .position(|tok| tok.depth == depth && is(tok, "SELECT"))
    else {
        return Vec::new();
    };
    let mut i = from + select + 1;
    if tokens
        .get(i)
        .is_some_and(|tok| is(tok, "DISTINCT") || is(tok, "ALL"))
    {
        i += 1;
        if tokens.get(i).is_some_and(|tok| is(tok, "ON")) {
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
    let ends = |tok: &Tok| {
        tok.depth < depth
            || (tok.depth == depth && (tok.text == ";" || AFTER_SELECT.iter().any(|w| is(tok, w))))
    };
    let end = (i..tokens.len())
        .find(|&at| ends(&tokens[at]))
        .unwrap_or(tokens.len());
    let mut bounds = vec![i - 1];
    bounds.extend((i..end).filter(|&at| tokens[at].depth == depth && tokens[at].text == ","));
    bounds.push(end);
    bounds
        .windows(2)
        .filter_map(|pair| {
            let (start, stop) = (pair[0] + 1, pair[1]);
            let after = tokens[pair[0]].span.end;
            let before = tokens.get(stop).map_or(query.len(), |tok| tok.span.start);
            let span = trimmed(query, after..before)?;
            let item = &tokens[start..stop];
            Some(SelectItem {
                span,
                name: item_name(item).map(|at| start + at),
                plain: item.len() == 1 || (item.len() == 3 && item[1].text == "."),
                star: item.last().is_some_and(|tok| tok.text == "*"),
            })
        })
        .collect()
}

/// `range` of `query` without the whitespace and comments around it; `None` when that
/// leaves nothing.
fn trimmed(query: &str, range: Range<usize>) -> Option<Range<usize>> {
    let text = &query[range.clone()];
    let mut start = 0;
    loop {
        let rest = text[start..].trim_start();
        start = text.len() - rest.len();
        let comment = match rest {
            _ if rest.starts_with("--") => rest.find('\n').map_or(rest.len(), |n| n + 1),
            _ if rest.starts_with("/*") => rest.find("*/").map_or(rest.len(), |n| n + 2),
            _ => break,
        };
        start += comment;
    }
    let end = text.trim_end().len();
    (start < end).then(|| range.start + start..range.start + end)
}

/// Words that join or end an expression: no alias follows them, nor are they one.
const OPERATORS: [&str; 18] = [
    "AND", "OR", "NOT", "IS", "IN", "LIKE", "ILIKE", "BETWEEN", "SIMILAR", "ESCAPE", "CASE",
    "WHEN", "THEN", "ELSE", "END", "NULL", "TRUE", "FALSE",
];

/// The token that names a select item's column: its alias, or the column it reads.
fn item_name(item: &[Tok]) -> Option<usize> {
    let last = item.len().checked_sub(1)?;
    let tok = &item[last];
    let operator = |tok: &Tok| OPERATORS.iter().any(|w| tok.text.eq_ignore_ascii_case(w));
    if !tok.word
        || is_keyword(&tok.text)
        || operator(tok)
        || tok.text.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let named = match last.checked_sub(1).map(|at| &item[at]) {
        None => true,
        Some(prev) => {
            prev.text == "."
                || prev.text.eq_ignore_ascii_case("AS")
                || (prev.word
                    && !is_clause(&prev.text)
                    && (!operator(prev) || prev.text.eq_ignore_ascii_case("END")))
                || prev.text == ")"
        }
    };
    named.then_some(last)
}

/// The CTEs of every `WITH` of `tokens`: `name [(columns)] AS [NOT] [MATERIALIZED] (...)`,
/// separated by commas.
fn ctes(tokens: &[Tok]) -> Vec<Cte> {
    let word = |at: usize, text: &str| {
        tokens
            .get(at)
            .is_some_and(|tok| tok.text.eq_ignore_ascii_case(text))
    };
    let mut found = Vec::new();
    for with in (0..tokens.len()).filter(|&at| word(at, "WITH")) {
        let mut at = with + 1 + usize::from(word(with + 1, "RECURSIVE"));
        while tokens.get(at).is_some_and(|tok| tok.word) {
            let name = at;
            let mut columns = Vec::new();
            at += 1;
            if word(at, "(") {
                let close = closing(tokens, at);
                columns.extend((at + 1..close).filter(|&column| tokens[column].word));
                at = close + 1;
            }
            if !word(at, "AS") {
                break;
            }
            at += 1;
            at += usize::from(word(at, "NOT"));
            at += usize::from(word(at, "MATERIALIZED"));
            if !word(at, "(") {
                break;
            }
            found.push(Cte {
                name,
                columns,
                body: at,
            });
            at = closing(tokens, at) + 1;
            if !word(at, ",") {
                break;
            }
            at += 1;
        }
    }
    found
}

/// The `)` that closes the `(` at `open`, or the end of `tokens`.
fn closing(tokens: &[Tok], open: usize) -> usize {
    let depth = tokens[open].depth;
    (open + 1..tokens.len())
        .find(|&at| tokens[at].depth == depth && tokens[at].text == ")")
        .unwrap_or(tokens.len())
}
