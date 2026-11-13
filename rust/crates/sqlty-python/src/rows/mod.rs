//! Row types against the columns of the queries they are named for: `TypedDict`s,
//! `NamedTuple`s, dataclasses, Pydantic models, tuples and scalars.
//!
//! [`check`] reads the `SQL[Row](...)` calls of a module, resolves each row type
//! ([`crate::types`]) and reports where the columns of its query do not fit it.

pub mod json_shape;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock};

use regex::Regex;

use ruff_python_ast::{Expr, Operator};
use sqlty_sql::analyzer::{self, Nullability};
use sqlty_sql::json::JsonShape;

use crate::checker::{wrapped_queries, Diagnostic};
use crate::options::Options;
use crate::rules::{Rule, RuleConfig};
use crate::scopes::{Key, Param};
use crate::types::compat::{
    accepts, check_type_compatibility, column_literal_values, declared_literal_values, field_type,
    set_member,
};
use crate::types::extract::extract_type_definitions;
use crate::types::generic::specialize;
use crate::types::info::{Annotation, Dict, Kind, TypeInfo};
use crate::types::pydantic::Known;
use crate::types::resolve::{slice_items, NoNames, Resolver};
use crate::types::symbols::{Index, ModuleTypes, Source};
use crate::types::ty::{builtins, Lit, Promotions, Ty};
use crate::unparse::unparse;

/// A result column with whether it can be NULL (`None` when unknown).
#[derive(Clone, Debug)]
pub struct RowColumn {
    pub name: String,
    pub python_type: Option<String>,
    pub sql_type: Option<String>,
    pub nullable: Option<bool>,
    /// The shape of the JSON the column builds.
    pub json: Option<JsonShape>,
}

impl From<analyzer::Column> for RowColumn {
    fn from(column: analyzer::Column) -> Self {
        RowColumn {
            name: column.name,
            python_type: column.python_type,
            sql_type: column.sql_type,
            nullable: match column.nullability {
                Nullability::Nullable => Some(true),
                Nullability::NonNull => Some(false),
                Nullability::Unknown => None,
            },
            json: column.json,
        }
    }
}

type Finding = (Rule, String);

/// `(module, row type, field)`.
pub type FieldKey = (Option<String>, String, String);

/// A diagnostic, and the field a `redundant-optional` one is about.
///
/// `redundant-optional` is a question about a row type, not a query: a field is redundant
/// only when no query that loads the type can return NULL for it, so a run drops the
/// reports whose field another query needs `| None` for (see [`Found::needed`]).
#[derive(Debug)]
pub struct Reported {
    pub diagnostic: Diagnostic,
    pub redundant: Option<FieldKey>,
}

/// What [`check`] found in a module.
#[derive(Debug, Default)]
pub struct Found {
    /// The diagnostics, by line.
    pub diagnostics: Vec<Reported>,
    /// The fields some query of the module needs `| None` for.
    pub needed: HashSet<FieldKey>,
}

/// What a module is checked with.
pub struct Settings<'a> {
    pub options: &'a Options<'a>,
    pub rules: &'a RuleConfig,
    /// The modules of the checked paths.
    pub index: Option<&'a Arc<dyn Index>>,
    /// The dotted name of the module.
    pub module: Option<&'a str>,
    /// The row types (`module.Name`) read as a Pydantic validator reads them.
    pub coerce: &'a HashSet<String>,
}

pub(crate) type RowQuery = (usize, String, Option<String>, (usize, usize));

pub(crate) struct Check<'a> {
    pub(crate) source: &'a Arc<Source>,
    pub(crate) settings: &'a Settings<'a>,
    pub(crate) type_defs: Dict<Arc<TypeInfo>>,
    /// The row types of the module itself, as extracted.
    pub(crate) own_defs: Dict<Arc<TypeInfo>>,
    /// The row types of the module an imported row type comes from, by local name.
    imported_defs: HashMap<String, Arc<Dict<Arc<TypeInfo>>>>,
    /// The queries whose row type is an alias of a tuple or scalar type.
    aliased: Vec<(RowQuery, &'a Expr)>,
    /// The queries whose row type is a class.
    pub(crate) user_queries: Vec<RowQuery>,
    found: Found,
}

impl Check<'_> {
    /// Report at `line`, underlining `span`, else the line itself.
    pub(crate) fn report(
        &mut self,
        rule: Rule,
        message: String,
        line: usize,
        span: Option<(usize, usize)>,
    ) -> Option<&mut Reported> {
        if !self.settings.rules.is_enabled(rule) {
            return None;
        }
        let (start, end) = span.unwrap_or_else(|| self.source.line_span(line));
        self.found.diagnostics.push(Reported {
            diagnostic: Diagnostic {
                rule,
                message,
                severity: self.settings.rules.severity(rule),
                line,
                span: start..end,
                fix: Vec::new(),
            },
            redundant: None,
        });
        self.found.diagnostics.last_mut()
    }

    pub(crate) fn columns(&self, query: Option<&str>) -> Option<Vec<RowColumn>> {
        let options = self.settings.options;
        let columns = analyzer::analyze_select(query?, options.dialect.name(), options.schema);
        let columns: Vec<RowColumn> = columns?.into_iter().map(RowColumn::from).collect();
        (!columns.is_empty()).then_some(columns)
    }

    /// The fields of the composite type `sql_type` as columns; `None` when it is not one.
    fn composite(&self, sql_type: &str) -> Option<Vec<RowColumn>> {
        let options = self.settings.options;
        let fields =
            analyzer::composite_columns(sql_type, options.dialect.name(), options.schema?)?;
        Some(fields.into_iter().map(RowColumn::from).collect())
    }

    /// Add `Row[int]` to the row types when `Row` is a generic class of this source.
    fn specialize(&mut self, name: &str) {
        let Some(parsed) = crate::parse::expression(name) else {
            return;
        };
        let Expr::Subscript(row) = parsed.syntax().body.as_ref() else {
            return;
        };
        let Expr::Name(class) = row.value.as_ref() else {
            return;
        };
        let Some(info) = self.type_defs.get(class.id.as_str()) else {
            return;
        };
        // The class of this source on the line of `info`, which may be another module's.
        let source = self.source;
        let Some(node) = source.classes().into_iter().find(|node| {
            source.line(crate::types::extract::class_start(node, &source.text)) == info.lineno
        }) else {
            return;
        };
        let params = crate::types::extract::type_params(node, source);
        let names = ModuleTypes::new(
            self.source.clone(),
            self.settings.index,
            self.settings.module,
        );
        if let Some(generic) = specialize(info, &params, slice_items(&row.slice), name, &names) {
            self.type_defs.insert(name, Arc::new(generic));
        }
    }
}

/// The diagnostics of the row types `source` names in `SQL[Row](...)` calls.
pub fn check(source: &Arc<Source>, settings: &Settings<'_>) -> Found {
    if !crate::check::may_wrap(&source.text, &settings.options.style.import_line) {
        return Found::default();
    }
    let queries = row_queries(source, settings);
    if queries.is_empty() {
        return Found::default();
    }
    let mut ctx = Check::new(source, settings, queries, Vec::new(), &[]);
    ctx.typeddict_inheritance();
    ctx.structural(&HashMap::new());
    ctx.aliases();
    ctx.finish()
}

/// The `SQL[Row](...)` calls of `source` whose row type is not spelled as a tuple.
pub(crate) fn row_queries(source: &Source, settings: &Settings<'_>) -> Vec<RowQuery> {
    wrapped_queries(&source.text, &settings.options.style)
        .into_iter()
        .filter(|(_, name, _, _)| !name.starts_with("tuple["))
        .collect()
}

impl<'a> Check<'a> {
    /// `_Check.__init__`: the row types of the `queries` (`SQL[Row](...)` calls) and of the
    /// calls that name a row (`named`); `params` are the names of the `P` of `SQL[R, P]`.
    pub(crate) fn new(
        source: &'a Arc<Source>,
        settings: &'a Settings<'a>,
        queries: Vec<RowQuery>,
        named: Vec<RowQuery>,
        params: &[String],
    ) -> Self {
        let wanted = !(queries.is_empty() && named.is_empty() && params.is_empty());
        let mut ctx = Check {
            source,
            settings,
            type_defs: if wanted {
                extract_type_definitions(source, settings.index, settings.module)
            } else {
                Dict::new()
            },
            own_defs: Dict::new(),
            imported_defs: HashMap::new(),
            aliased: Vec::new(),
            user_queries: Vec::new(),
            found: Found::default(),
        };
        ctx.own_defs = ctx.type_defs.clone();
        let queries: Vec<RowQuery> = queries.into_iter().chain(named).collect();
        if let Some(index) = settings.index {
            let names = queries
                .iter()
                .map(|query| query.1.as_str())
                .chain(params.iter().map(String::as_str));
            for name in names {
                if !ctx.type_defs.contains(name) {
                    if let Some(imported) = index.row_type(source, name) {
                        ctx.type_defs.insert(name, imported.info);
                        ctx.imported_defs.insert(name.to_string(), imported.defs);
                    }
                }
            }
        }
        let coerced: Vec<(String, TypeInfo)> = ctx
            .type_defs
            .iter()
            .filter_map(|(name, info)| {
                let owner = info.module.as_deref().unwrap_or("");
                let owner = owner.strip_suffix(".__init__").unwrap_or(owner);
                let qualified = format!("{owner}.{}", info.name);
                let types = info.coerced.as_ref()?;
                settings
                    .coerce
                    .contains(qualified.trim_start_matches('.'))
                    .then(|| {
                        let mut out = TypeInfo::clone(info);
                        out.field_types = types.clone();
                        out.lax = info.fields.iter().map(|(name, _)| name.clone()).collect();
                        out.coerced = None;
                        (name.to_string(), out)
                    })
            })
            .collect();
        for (name, info) in coerced {
            ctx.type_defs.insert(name, Arc::new(info));
        }

        // A row type named through an alias (`type Row = User`) is checked as what it names.
        let aliases = source.type_aliases();
        let mut resolved: Vec<RowQuery> = Vec::new();
        for (line, name, query, span) in queries {
            match alias_target(&name, &aliases, source) {
                None => {
                    if !ctx.type_defs.contains(&name) {
                        ctx.specialize(&name);
                    }
                    resolved.push((line, name, query, span));
                }
                Some(target) => {
                    let base = unparse(target, &source.text);
                    if ctx.type_defs.contains(&base) {
                        resolved.push((line, base, query, span));
                    } else {
                        ctx.aliased.push(((line, name, query, span), target));
                    }
                }
            }
        }
        ctx.user_queries = resolved
            .into_iter()
            .filter(|query| ctx.type_defs.contains(&query.1))
            .collect();
        ctx
    }

    /// The diagnostics, by line.
    pub(crate) fn finish(mut self) -> Found {
        self.found
            .diagnostics
            .sort_by_key(|reported| reported.diagnostic.line);
        self.found
    }

    pub(crate) fn typeddict_inheritance(&mut self) {
        let queries = std::mem::take(&mut self.user_queries);
        check_typeddict_inheritance(self, &queries);
        self.user_queries = queries;
    }

    /// `bound` are the arguments a query is bound to, by the span of its row type.
    pub(crate) fn structural(&mut self, bound: &HashMap<(usize, usize), Vec<Param>>) {
        let queries = std::mem::take(&mut self.user_queries);
        check_structural(self, &queries, bound);
        self.user_queries = queries;
    }

    pub(crate) fn aliases(&mut self) {
        let aliased = std::mem::take(&mut self.aliased);
        check_aliases(self, &aliased);
        self.aliased = aliased;
    }
}

/// `_bind`: `query` with the placeholders bound to values that are not `None` respelled
/// as ones whose nullability is not judged.
pub(crate) fn bind(query: Option<&str>, params: Option<&[Param]>, dialect: &str) -> Option<String> {
    static PLACEHOLDER: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"%%|%\((?P<name>[^)]+)\)s|%s").expect("a valid pattern"));
    let query = query?;
    let non_null: HashSet<&Key> = params
        .unwrap_or_default()
        .iter()
        .filter(|param| param.kind.as_deref().is_some_and(|kind| kind != "None"))
        .map(|param| &param.key)
        .collect();
    if non_null.is_empty() {
        return Some(query.to_string());
    }
    let dialect = dialect.to_lowercase();
    let marker = if dialect == "postgres" || dialect == "postgresql" {
        "$0"
    } else {
        "?"
    };
    let mut position = 0;
    let bound = PLACEHOLDER.replace_all(query, |found: &regex::Captures<'_>| {
        if &found[0] == "%%" {
            return found[0].to_string();
        }
        let key = match found.name("name") {
            Some(name) => Key::Name(name.as_str().to_string()),
            None => {
                position += 1;
                Key::Index(position)
            }
        };
        if non_null.contains(&key) {
            marker.to_string()
        } else {
            found[0].to_string()
        }
    });
    Some(bound.into_owned())
}

/// `_alias_target`: what the alias `name` stands for, through aliases of aliases.
pub(crate) fn alias_target<'a>(
    name: &str,
    aliases: &HashMap<&str, &'a Expr>,
    source: &Source,
) -> Option<&'a Expr> {
    let mut name = name.to_string();
    let mut target = None;
    for _ in 0..16 {
        let Some(found) = aliases.get(name.as_str()) else {
            break;
        };
        target = Some(*found);
        name = unparse(found, &source.text);
    }
    target
}

fn check_typeddict_inheritance(ctx: &mut Check<'_>, user_queries: &[RowQuery]) {
    if !ctx.settings.rules.is_enabled(Rule::TypeddictInheritance) {
        return;
    }
    // The named types and their same-source TypedDict bases, bases first, each once.
    fn visit<'d>(
        name: &'d str,
        defs: &'d Dict<Arc<TypeInfo>>,
        seen: &mut Dict<Arc<TypeInfo>>,
        active: &mut HashSet<&'d str>,
    ) {
        if seen.contains(name) {
            return;
        }
        let Some(info) = defs.get(name) else {
            return;
        };
        // A class that redefines its own base (`class Row(Row)`) names itself.
        if !active.insert(name) {
            return;
        }
        for base in &info.bases {
            visit(base, defs, seen, active);
        }
        seen.insert(name, info.clone());
    }
    let mut seen = Dict::new();
    let mut active = HashSet::new();
    for (_, name, _, _) in user_queries {
        if !ctx.imported_defs.contains_key(name) {
            visit(name, &ctx.type_defs, &mut seen, &mut active);
        }
    }
    for info in seen.values() {
        for message in &info.errors {
            ctx.report(
                Rule::TypeddictInheritance,
                message.clone(),
                info.lineno,
                None,
            );
        }
    }
}

/// Rows named by a type defined in this source (or an imported one).
fn check_structural(
    ctx: &mut Check<'_>,
    user_queries: &[RowQuery],
    bound: &HashMap<(usize, usize), Vec<Param>>,
) {
    for (line, name, query, span) in user_queries {
        let dialect = ctx.settings.options.dialect.name();
        let query = match bound.get(span) {
            Some(params) if !params.is_empty() => bind(query.as_deref(), Some(params), dialect),
            _ => query.clone(),
        };
        let Some(columns) = ctx.columns(query.as_deref()) else {
            continue;
        };
        let Some(info) = ctx.type_defs.get(name).cloned() else {
            continue;
        };
        // A field this query can fill with NULL needs `| None`, whatever other queries do.
        for (index, column) in columns.iter().enumerate() {
            let field = if info.kind == Kind::NamedTuple {
                info.fields.get(index).map(|(field, _)| field.as_str())
            } else {
                info.field_for(&column.name)
            };
            if let Some(field) = field.filter(|_| column.nullable != Some(false)) {
                let key = (info.module.clone(), info.name.clone(), field.to_string());
                ctx.found.needed.insert(key);
            }
        }
        let found = {
            let defs = ctx
                .imported_defs
                .get(name)
                .map_or(&ctx.type_defs, |defs| defs);
            let structural = Structural {
                rules: ctx.settings.rules,
                defs,
                composite: &|sql_type: &str| ctx.composite(sql_type),
            };
            structural.diagnostics(name, &info, &columns, "query")
        };
        for (rule, message) in found {
            let field = message
                .split_once(" is ")
                .map_or(message.as_str(), |(field, _)| field);
            let field = field.strip_prefix(&format!("{name}.")).unwrap_or(field);
            let key = (info.module.clone(), info.name.clone(), field.to_string());
            if let Some(reported) = ctx.report(rule, message, *line, Some(*span)) {
                if rule == Rule::RedundantOptional {
                    reported.redundant = Some(key);
                }
            }
        }
    }
}

/// Rows named by a module-level alias of a tuple or scalar type (`type Row = tuple[int, str]`).
fn check_aliases(ctx: &mut Check<'_>, aliased: &[(RowQuery, &Expr)]) {
    let mut shapes: HashMap<&str, Option<RowShape>> = HashMap::new();
    for ((line, name, query, span), target) in aliased {
        let Some(columns) = ctx.columns(query.as_deref()) else {
            continue;
        };
        let shape = shapes
            .entry(name.as_str())
            .or_insert_with(|| row_shape(target, name, ctx.source));
        let found = value_diagnostics(shape.as_ref(), &columns, ctx.settings.rules, &ctx.type_defs);
        for (rule, message) in found {
            ctx.report(rule, message, *line, Some(*span));
        }
    }
}

/// `_nullability_diagnostics`: `nullable-mismatch` and `redundant-optional` for one field
/// and its column.
///
/// `label` reads `<field>` or `<row> item <i>`. A column of unknown nullability, or an
/// annotation that can't be resolved, is never reported.
fn nullability_diagnostics(
    label: &str,
    declared: Option<&Ty>,
    column: &RowColumn,
    rules: &RuleConfig,
    item: &str,
) -> Vec<Finding> {
    let (Some(nullable), Some(declared)) = (column.nullable, declared) else {
        return Vec::new();
    };
    let message = |tail: &str| {
        format!(
            "{label} is {} but {item} '{}' {tail}",
            declared.display(),
            column.name
        )
    };
    if nullable && rules.is_enabled(Rule::NullableMismatch) && declared.admits_none() == Some(false)
    {
        return vec![(Rule::NullableMismatch, message("can be NULL"))];
    }
    if !nullable && rules.is_enabled(Rule::RedundantOptional) && declared.is_optional() {
        return vec![(Rule::RedundantOptional, message("is never NULL"))];
    }
    Vec::new()
}

/// `repr` of a literal value.
fn lit_repr(value: &Lit) -> String {
    match value {
        Lit::Str(text) => crate::unparse::repr_str(text),
        Lit::Int(digits) => digits.clone(),
        Lit::Bool(true) => "True".to_string(),
        Lit::Bool(false) => "False".to_string(),
    }
}

/// `_literal_diagnostics`: `literal-mismatch` and `redundant-literal` for one field and its
/// column.
///
/// Reported only when the annotation is made of `Literal` members (and `None`) and the
/// column's values are known.
fn literal_diagnostics(
    label: &str,
    declared: Option<&Ty>,
    column: &RowColumn,
    rules: &RuleConfig,
    item: &str,
) -> Vec<Finding> {
    let (Some(declared), Some(python_type)) = (
        declared,
        column.python_type.as_deref().filter(|tp| !tp.is_empty()),
    ) else {
        return Vec::new();
    };
    let Some(inferred) = column_literal_values(python_type) else {
        return Vec::new();
    };
    let Some(admitted) = declared_literal_values(declared, &inferred) else {
        return Vec::new();
    };
    let holds =
        |values: &[Lit], value: &Lit| values.iter().any(|v| set_member(v) == set_member(value));
    let message = |tail: &str, values: &[&Lit]| {
        let values: Vec<String> = values.iter().map(|value| lit_repr(value)).collect();
        format!(
            "{label} is {} but {item} '{}' {tail} {}",
            declared.display(),
            column.name,
            values.join(", ")
        )
    };
    let missing: Vec<&Lit> = inferred.iter().filter(|v| !holds(&admitted, v)).collect();
    if !missing.is_empty() && rules.is_enabled(Rule::LiteralMismatch) {
        return vec![(Rule::LiteralMismatch, message("can be", &missing))];
    }
    let mut extra: Vec<&Lit> = admitted.iter().filter(|v| !holds(&inferred, v)).collect();
    extra.sort_by_key(|value| lit_repr(value));
    if missing.is_empty() && !extra.is_empty() && rules.is_enabled(Rule::RedundantLiteral) {
        return vec![(Rule::RedundantLiteral, message("is never", &extra))];
    }
    Vec::new()
}

/// `_field_diagnostics`: nullability and value-set diagnostics for one field and its column.
fn field_diagnostics(
    label: &str,
    declared: Option<&Ty>,
    column: &RowColumn,
    rules: &RuleConfig,
    item: &str,
) -> Vec<Finding> {
    let mut found = nullability_diagnostics(label, declared, column, rules, item);
    found.extend(literal_diagnostics(label, declared, column, rules, item));
    found
}

/// `_json_diagnostics`: `json-shape-mismatch` for one field and a JSON column of known
/// shape.
fn json_diagnostics(
    label: &str,
    annotation: Option<&Annotation>,
    column: &RowColumn,
    defs: &Dict<Arc<TypeInfo>>,
    aliases: &HashMap<String, String>,
    rules: &RuleConfig,
) -> Vec<Finding> {
    let (Some(shape), Some(annotation)) = (&column.json, annotation) else {
        return Vec::new();
    };
    if !rules.is_enabled(Rule::JsonShapeMismatch) {
        return Vec::new();
    }
    let problems = json_shape::json_problems(shape, annotation, defs, aliases, &column.name);
    if problems.is_empty() {
        return Vec::new();
    }
    vec![(
        Rule::JsonShapeMismatch,
        format!(
            "{label} is {} but query column '{}' builds JSON that does not fit: {}",
            annotation.unparse(),
            column.name,
            json_shape::summarize(&problems)
        ),
    )]
}

/// `_row_type_of`: the row type an annotation names, through `X | None` and `Optional[X]`.
fn row_type_of<'d>(
    annotation: Option<&Annotation>,
    defs: &'d Dict<Arc<TypeInfo>>,
) -> Option<&'d Arc<TypeInfo>> {
    fn of<'d>(
        expr: &Expr,
        source: &str,
        defs: &'d Dict<Arc<TypeInfo>>,
    ) -> Option<&'d Arc<TypeInfo>> {
        match expr {
            Expr::BinOp(op) if op.op == Operator::BitOr && op.right.is_none_literal_expr() => {
                of(&op.left, source, defs)
            }
            Expr::BinOp(op) if op.op == Operator::BitOr && op.left.is_none_literal_expr() => {
                of(&op.right, source, defs)
            }
            Expr::Subscript(sub) if unparse(&sub.value, source).ends_with("Optional") => {
                of(&sub.slice, source, defs)
            }
            Expr::Name(_) | Expr::Attribute(_) => defs.get(&unparse(expr, source)),
            _ => None,
        }
    }
    let annotation = annotation?;
    of(&annotation.expr, &annotation.source, defs)
}

fn truthy(text: Option<&str>) -> bool {
    text.is_some_and(|text| !text.is_empty())
}

fn plural(kind: Kind) -> &'static str {
    match kind {
        Kind::TypedDict => "keys",
        Kind::Protocol => "attributes",
        _ => "fields",
    }
}

fn singular(kind: Kind) -> &'static str {
    match kind {
        Kind::TypedDict => "key",
        Kind::Protocol => "attribute",
        _ => "field",
    }
}

struct Structural<'a> {
    rules: &'a RuleConfig,
    defs: &'a Dict<Arc<TypeInfo>>,
    composite: &'a dyn Fn(&str) -> Option<Vec<RowColumn>>,
}

impl Structural<'_> {
    /// `_structural_diagnostics`: a user type against the columns of `source` (`query`, or
    /// a composite type).
    ///
    /// `TypedDict`s, dataclasses and Pydantic models match columns to fields by name, as
    /// `dict_row` and `class_row` pass them: fields with a default (`NotRequired` keys) may
    /// be absent, and a column without a field is a violation, unless a Pydantic model
    /// allows it or `extra_items` admits it. `NamedTuple`s match by position. A reported
    /// count mismatch suppresses the per-column checks.
    fn diagnostics(
        &self,
        type_name: &str,
        info: &TypeInfo,
        columns: &[RowColumn],
        source: &str,
    ) -> Vec<Finding> {
        let fields = &info.fields;
        let query = source == "query";
        let item = if query { "column" } else { "field" };
        let by_name = info.kind != Kind::NamedTuple;
        let by_alias = info.kind == Kind::BaseModel;
        let extra_items = truthy(info.extra_items.as_deref());
        let open_ended = extra_items || by_alias || info.allows_extra;
        let lo = if by_name {
            info.required.difference(&info.opaque).count()
        } else {
            fields.len()
        };
        let hi = if open_ended {
            columns.len()
        } else {
            fields.len()
        };
        if !(lo <= columns.len() && columns.len() <= hi)
            && self.rules.is_enabled(Rule::ColumnCountMismatch)
        {
            let unit = plural(info.kind);
            let expected = if open_ended {
                format!("{lo} required {unit}")
            } else if lo < hi {
                format!("{lo} required and {hi} total {unit}")
            } else {
                format!("{} fields", fields.len())
            };
            let counted = if query {
                format!("{source} returns {} columns", columns.len())
            } else {
                format!("{source} has {} fields", columns.len())
            };
            return vec![(
                Rule::ColumnCountMismatch,
                format!("{type_name} has {expected} but {counted}"),
            )];
        }

        if !by_name {
            let pairs = || fields.iter().zip(columns);
            let mut found: Vec<Finding> = pairs()
                .flat_map(|((field, _), column)| {
                    self.type_mismatch(type_name, info, field, column, source, item)
                })
                .collect();
            found.extend(
                pairs()
                    .filter(|((field, _), column)| *field != column.name)
                    .map(|((field, _), column)| {
                        (
                            Rule::ColumnNameMismatch,
                            format!(
                                "{type_name}.{field} doesn't match {source} {item} '{}'",
                                column.name
                            ),
                        )
                    }),
            );
            return found;
        }

        let admits_extra = extra_items || info.allows_extra || !info.opaque.is_empty();
        let mut found = Vec::new();
        let mut unknown = Vec::new();
        for column in columns {
            match info.field_for(&column.name) {
                Some(field) if !field.is_empty() => {
                    found.extend(self.type_mismatch(type_name, info, field, column, source, item));
                }
                owner => {
                    if extra_items {
                        found.extend(self.type_mismatch(
                            type_name,
                            info,
                            &column.name,
                            column,
                            source,
                            item,
                        ));
                    }
                    if owner.is_none() && !admits_extra {
                        unknown.push(column);
                    }
                }
            }
        }
        let noun = singular(info.kind);
        found.extend(unknown.into_iter().map(|column| {
            (
                Rule::TypeddictKeyMismatch,
                format!(
                    "{type_name} has no {noun} for {source} {item} '{}'",
                    column.name
                ),
            )
        }));
        for (key, _) in fields {
            let names = info.column_names(key);
            if info.required.contains(key)
                && !info.opaque.contains(key)
                && !names
                    .iter()
                    .any(|name| columns.iter().any(|c| c.name == *name))
            {
                if let Some(first) = names.first() {
                    found.push((
                        Rule::TypeddictKeyMismatch,
                        format!("{type_name}.{first} is required but missing from {source}"),
                    ));
                }
            }
        }
        found
    }

    fn type_mismatch(
        &self,
        type_name: &str,
        info: &TypeInfo,
        field: &str,
        column: &RowColumn,
        source: &str,
        item: &str,
    ) -> Vec<Finding> {
        let python_type = column.python_type.as_deref().filter(|tp| !tp.is_empty());
        if python_type.is_some() && !check_type_compatibility(info, field, python_type) {
            let declared = info
                .fields
                .iter()
                .find(|(name, _)| name == field)
                .map(|(_, annotation)| annotation.as_str())
                .filter(|annotation| !annotation.is_empty())
                .map_or_else(
                    || {
                        format!(
                            "extra_items {}",
                            info.extra_items.as_deref().unwrap_or("None")
                        )
                    },
                    str::to_string,
                );
            return vec![(
                Rule::ColumnTypeMismatch,
                format!(
                    "{type_name}.{field} is {declared} but {source} {item} '{}' infers {}",
                    column.name,
                    python_type.unwrap_or_default()
                ),
            )];
        }
        let label = format!("{type_name}.{field}");
        let annotation = info.annotations.get(field);
        let mut found = field_diagnostics(
            &label,
            field_type(info, field),
            column,
            self.rules,
            &format!("{source} {item}"),
        );
        found.extend(json_diagnostics(
            &label,
            annotation,
            column,
            self.defs,
            &info.aliases,
            self.rules,
        ));
        if let (None, Some(sql_type), Some(nested)) = (
            &column.python_type,
            column.sql_type.as_deref().filter(|tp| !tp.is_empty()),
            row_type_of(annotation, self.defs),
        ) {
            if let Some(fields) = (self.composite)(sql_type) {
                let composite = format!("composite {}", sql_type.to_lowercase());
                found.extend(self.diagnostics(&label, nested, &fields, &composite));
            }
        }
        found
    }
}

/// The row type of a query that is not a class: what each column must be.
pub(crate) struct RowShape {
    text: String,
    is_tuple: bool,
    /// `(label, resolved type, annotation)` of each column.
    targets: Vec<(String, Option<Ty>, Annotation)>,
    aliases: Arc<HashMap<String, String>>,
}

/// `_row_shape`: what each column of the row type `text` must be; `None` when unchecked.
///
/// A scalar type takes one column, `tuple[A, B]` one column per item. Other row types (an
/// unknown class, `dict`, `tuple[int, ...]`) are not checked.
pub(crate) fn row_shape(row: &Expr, text: &str, source: &Source) -> Option<RowShape> {
    let b = builtins();
    let resolver = Resolver {
        aliases: &source.aliases,
        imports: None,
        known: Known::Imported,
        version: source.version,
    };
    let resolve = |expr: &Expr| resolver.resolve(expr, &source.text, &NoNames);
    let annotation = |expr: &Expr| Annotation {
        expr: expr.clone(),
        source: source.text.clone(),
    };
    if let Expr::Subscript(sub) = row {
        if resolve(&sub.value).is_some_and(|tp| tp.is(&b.tuple)) {
            let items = slice_items(&sub.slice);
            if items.is_empty() || items.iter().any(Expr::is_ellipsis_literal_expr) {
                return None;
            }
            let targets = items
                .iter()
                .enumerate()
                .map(|(i, item)| {
                    let spelled = unparse(item, &source.text);
                    (
                        format!("{text} item {i} is {spelled}"),
                        resolve(item),
                        annotation(item),
                    )
                })
                .collect();
            return Some(RowShape {
                text: text.to_string(),
                is_tuple: true,
                targets,
                aliases: source.aliases.clone(),
            });
        }
    }
    let resolved = resolve(row)?;
    let container = [&b.dict, &b.list, &b.set, &b.frozenset, &b.tuple]
        .iter()
        .any(|class| resolved.is(class));
    let has_args = match &resolved {
        Ty::Generic(_, args) => !args.is_empty(),
        Ty::Literal(values) => !values.is_empty(),
        Ty::Union(_) => true,
        Ty::Never | Ty::Class(_) | Ty::NewType(_) => false,
    };
    if container || has_args {
        return None;
    }
    Some(RowShape {
        text: text.to_string(),
        is_tuple: false,
        targets: vec![(
            format!("{text} is expected"),
            Some(resolved),
            annotation(row),
        )],
        aliases: source.aliases.clone(),
    })
}

/// `_value_diagnostics`: a tuple or scalar row type against the query columns.
pub(crate) fn value_diagnostics(
    shape: Option<&RowShape>,
    columns: &[RowColumn],
    rules: &RuleConfig,
    defs: &Dict<Arc<TypeInfo>>,
) -> Vec<Finding> {
    let Some(shape) = shape else {
        return Vec::new();
    };
    if shape.targets.len() != columns.len() {
        let takes = if shape.is_tuple {
            format!("has {} items", shape.targets.len())
        } else {
            "takes 1 column".to_string()
        };
        return vec![(
            Rule::ColumnCountMismatch,
            format!(
                "{} {takes} but query returns {} columns",
                shape.text,
                columns.len()
            ),
        )];
    }
    let mut found = Vec::new();
    for (i, ((label, resolved, node), column)) in shape.targets.iter().zip(columns).enumerate() {
        let item = if shape.is_tuple {
            format!("{} item {i}", shape.text)
        } else {
            "row type".to_string()
        };
        found.extend(json_diagnostics(
            &item,
            Some(node),
            column,
            defs,
            &shape.aliases,
            rules,
        ));
        let (Some(resolved), Some(python_type)) = (
            resolved,
            column.python_type.as_deref().filter(|tp| !tp.is_empty()),
        ) else {
            continue;
        };
        if accepts(Some(resolved), Some(python_type), Promotions::Numeric) {
            found.extend(field_diagnostics(
                &item,
                Some(resolved),
                column,
                rules,
                "query column",
            ));
        } else {
            found.push((
                Rule::ColumnTypeMismatch,
                format!(
                    "{label} but query column '{}' infers {python_type}",
                    column.name
                ),
            ));
        }
    }
    found
}
