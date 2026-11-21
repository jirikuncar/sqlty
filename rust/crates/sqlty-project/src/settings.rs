//! The settings a file is checked with: those of its configuration, of the database of
//! `[tool.sqlty.databases]` it belongs to and of the overrides that match it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use sqlty_python::config;
use sqlty_python::project::{self, Discovery, Files, Route};
use sqlty_python::queries;
use sqlty_python::rewriter::ImportStyle;
use sqlty_python::types::stdlib::Version;
use sqlty_sql::analyzer::{self, Databases, Schema, SchemaData, TypeOverrides};
use toml::{Table, Value};

use crate::flags::{Flags, Loaded};
use crate::Shared;

/// `max-query-variants` when `preview` is on and it is not set.
pub const MAX_QUERY_VARIANTS: usize = 64;

type LoadKey = (String, &'static str, Vec<(String, String)>);

/// A database: its name, settings, schema text and `types`.
type Named = (
    String,
    FileSettings,
    Option<SchemaText>,
    Vec<(String, String)>,
);

/// The schema files a configuration names, read: their paths and their text, joined.
#[derive(Clone)]
struct SchemaText {
    text: String,
    files: Vec<PathBuf>,
}

/// A `[[tool.sqlty.functions]]` entry.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Function {
    /// `pkg.mod.func` or `pkg.mod.Class.method`.
    pub import_path: String,
    /// The parameter taking the query.
    pub query: String,
    /// The parameter taking the row type.
    pub row: Option<String>,
    /// The database a `[tool.sqlty.databases.NAME]` entry reads its queries against.
    pub database: Option<String>,
}

/// What a file is checked with.
#[derive(Clone)]
pub struct FileSettings {
    /// The SQL dialect.
    pub dialect: &'static str,
    /// The schema, with the other databases of the project.
    pub schema: Option<Schema>,
    /// The SQL `schema` was loaded from.
    pub schema_sql: Option<Arc<str>>,
    /// The files `schema_sql` was read from, in order.
    pub schema_files: Vec<PathBuf>,
    /// `types`, with those of `driver`, sorted.
    pub types: Vec<(String, String)>,
    /// `import-style`.
    pub import_style: Option<String>,
    /// `untyped = "sentinel"`.
    pub untyped: bool,
    /// `json-types = "typing.TypedDict"`.
    pub json_typed_dicts: bool,
    /// `param-types`, on unless set to `false`.
    pub param_types: bool,
    /// Whether `requires-python` rules out every Python before 3.15.
    pub closed_params: bool,
    /// The severity of each rule the configuration sets.
    pub rules: HashMap<String, String>,
    /// `max-query-variants`, with `preview`.
    pub variants: Option<usize>,
    /// `coerce-row-types`.
    pub coerce: HashSet<String>,
    /// `functions`, with those of every database.
    pub functions: Vec<Function>,
    /// `format-command`.
    pub format_command: Vec<String>,
    /// `queries`, relative to `base`.
    pub queries: Vec<String>,
    /// `query-format`.
    pub query_format: queries::Format,
    /// The directory of the configuration, which its paths are relative to.
    pub base: Option<PathBuf>,
    /// The Python the sources are read as.
    pub version: Version,
}

/// The schemas of a run, each loaded once, and what could not be read of them.
#[derive(Default)]
pub struct Schemas {
    loaded: Mutex<HashMap<LoadKey, Schema>>,
    /// Every warning, and how many [`Schemas::warnings`] has returned.
    warnings: Mutex<(Vec<String>, usize)>,
}

impl Schemas {
    /// What the schemas could not read, each once, since the last call.
    pub fn warnings(&self) -> Vec<String> {
        let mut warnings = self.warnings.lock().unwrap_or_else(PoisonError::into_inner);
        let (all, read) = &mut *warnings;
        let new = all[*read..].to_vec();
        *read = all.len();
        new
    }

    pub(crate) fn warn(&self, warning: String) {
        let mut warnings = self.warnings.lock().unwrap_or_else(PoisonError::into_inner);
        if !warnings.0.contains(&warning) {
            warnings.0.push(warning);
        }
    }

    fn load(
        &self,
        text: &str,
        dialect: &'static str,
        types: &[(String, String)],
    ) -> Result<Schema, String> {
        let key = (text.to_string(), dialect, types.to_vec());
        if let Some(found) = self.lock().get(&key) {
            return Ok(Arc::clone(found));
        }
        let (schema, skipped) = loaded_schema(text, dialect, types)?;
        if !dialect
            .parse::<sqlty_sql::Dialect>()
            .is_ok_and(sqlty_sql::Dialect::checks_names)
        {
            self.warn(format!(
                "Table and column names are not checked for {dialect}"
            ));
        }
        for (kind, name) in skipped {
            self.warn(match kind {
                "statement" => {
                    format!("Schema statement could not be parsed and is ignored: {name}")
                }
                "limit" => format!("Schema is too large to load; ignored: {name}"),
                kind => {
                    format!("Schema {kind} {name} could not be parsed; its columns are unknown")
                }
            });
        }
        self.lock().insert(key, Arc::clone(&schema));
        Ok(schema)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<LoadKey, Schema>> {
        self.loaded.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn loaded_schema(
    text: &str,
    dialect: &'static str,
    types: &[(String, String)],
) -> Result<(Schema, Vec<analyzer::SkippedObject>), String> {
    let overrides = TypeOverrides::new(types, dialect)
        .map_err(|error| format!("[tool.sqlty.types]: {error}"))?;
    Ok(match overrides.is_empty() {
        true => analyzer::load_schema_with_skipped(text, dialect),
        false => analyzer::load_schema_with_overrides(text, dialect, overrides),
    })
}

/// The settings of the files of one directory: its project's, and those of each database.
pub struct Directory {
    own: Arc<FileSettings>,
    databases: Vec<(String, Arc<FileSettings>)>,
    routing: Option<(Table, PathBuf)>,
    cli_rules: Vec<(String, String)>,
}

impl Directory {
    /// The settings of the configuration `loaded`, with the command line's `flags`.
    pub(crate) fn new(
        config: Loaded,
        flags: &Flags,
        files: &Shared,
        schemas: &Schemas,
        version: Version,
    ) -> Result<Self, String> {
        let Loaded { table, base, label } = config;
        let load = loaded_schema;
        let read = |table: &Table| {
            schema_text(&strings(table, "schema"), base.as_deref()?, files, schemas)
        };
        let schema_sql = match &flags.schema {
            Some(entries) => schema_text(&entries_of(entries), &flags.cwd, files, schemas),
            None => read(&table),
        };
        let closed = closed_params(base.as_deref().unwrap_or(&flags.cwd), files);
        let own_dialect = flags.dialect.as_deref();
        let mut own = settings(&table, &label, flags, own_dialect, closed, &base, version)?;
        let databases = table
            .get("databases")
            .and_then(Value::as_table)
            .cloned()
            .unwrap_or_default();
        if databases.is_empty() {
            own.types = sorted(own_types(&table, own.dialect));
            own.schema = schema_sql
                .as_ref()
                .map(|schema| schemas.load(&schema.text, own.dialect, &own.types))
                .transpose()?;
            if let Some(SchemaText { text, files }) = schema_sql {
                own.schema_sql = Some(Arc::from(text));
                own.schema_files = files;
            }
            return Ok(Self {
                routing: base
                    .clone()
                    .filter(|_| table.contains_key("overrides"))
                    .map(|base| (table.clone(), base)),
                own: Arc::new(own),
                databases: Vec::new(),
                cli_rules: flags.rules(),
            });
        }
        let own_dialect_name = own.dialect;
        let mut named: Vec<Named> = Vec::new();
        named.push((
            analyzer::DEFAULT.to_string(),
            own,
            schema_sql.clone(),
            own_types(&table, own_dialect_name),
        ));
        let mut functions = Vec::new();
        for (name, database) in &databases {
            let database = database.as_table().cloned().unwrap_or_default();
            let merged = merged(&table, &database);
            let schema = match database.contains_key("schema") {
                true => read(&database),
                false => schema_sql.clone(),
            };
            let label = format!("{label} [tool.sqlty.databases.{name}]");
            let dialect = flags
                .dialect
                .as_deref()
                .filter(|_| !database.contains_key("dialect"));
            let settings = settings(&merged, &label, flags, dialect, closed, &base, version)?;
            functions.extend(entries(&database, Some(name)));
            let types = own_types(&merged, settings.dialect);
            named.push((name.clone(), settings, schema, types));
        }
        let plain: Vec<(String, &'static str, Option<Schema>)> = named
            .iter()
            .map(|(name, settings, schema, types)| {
                let schema = schema
                    .as_ref()
                    .map(|schema| schemas.load(&schema.text, settings.dialect, types))
                    .transpose()?;
                Ok((name.clone(), settings.dialect, schema))
            })
            .collect::<Result<_, String>>()?;
        let mut routed = Vec::new();
        for (name, mut settings, schema, types) in named {
            let text = schema.as_ref().map_or("", |schema| schema.text.as_str());
            let (loaded, _) = load(text, settings.dialect, &types)?;
            let others = plain
                .iter()
                .filter(|(other, ..)| *other != name)
                .cloned()
                .collect();
            let databases = Databases {
                name: name.clone(),
                absent: schema.is_none(),
                others,
            };
            settings.schema = SchemaData::with_databases(loaded, databases);
            if let Some(SchemaText { text, files }) = schema {
                settings.schema_sql = Some(Arc::from(text));
                settings.schema_files = files;
            }
            settings.types = sorted(types);
            settings.functions.extend(functions.iter().cloned());
            routed.push((name, Arc::new(settings)));
        }
        let own = routed.remove(0).1;
        Ok(Self {
            own,
            databases: routed,
            routing: base.map(|base| (table, base)),
            cli_rules: flags.rules(),
        })
    }

    /// The settings of the project under [`analyzer::DEFAULT`], then of each database.
    pub(crate) fn named(&self) -> impl Iterator<Item = (&str, &Arc<FileSettings>)> {
        std::iter::once((analyzer::DEFAULT, &self.own)).chain(
            self.databases
                .iter()
                .map(|(name, settings)| (name.as_str(), settings)),
        )
    }

    /// The settings of the file at `path`.
    pub fn file(&self, path: &Path, discovery: &Discovery) -> Arc<FileSettings> {
        let Some((table, base)) = &self.routing else {
            return Arc::clone(&self.own);
        };
        let route = discovery.route(table, base, path);
        let settings = route
            .database
            .as_ref()
            .and_then(|name| self.databases.iter().find(|(database, _)| database == name))
            .map_or(&self.own, |(_, settings)| settings);
        self.overridden(settings, &route)
    }

    /// `settings` with the rules of the overrides that match the schema file at `path`,
    /// in the database it is given: a database's `include` does not route a migration.
    pub(crate) fn schema_file(
        &self,
        settings: &Arc<FileSettings>,
        path: &Path,
        discovery: &Discovery,
    ) -> Arc<FileSettings> {
        match &self.routing {
            Some((table, base)) => self.overridden(settings, &discovery.route(table, base, path)),
            None => Arc::clone(settings),
        }
    }

    /// `settings` with the rules of the overrides `route` matched, the command line's on top.
    fn overridden(&self, settings: &Arc<FileSettings>, route: &Route) -> Arc<FileSettings> {
        let overrides = self.routing.as_ref().is_some_and(|(table, _)| {
            table
                .get("overrides")
                .and_then(Value::as_array)
                .is_some_and(|all| !all.is_empty())
        });
        if !overrides {
            return Arc::clone(settings);
        }
        let mut rules = settings.rules.clone();
        rules.extend(
            route.rules.iter().filter_map(|(rule, severity)| {
                Some((rule.clone(), severity.as_str()?.to_string()))
            }),
        );
        rules.extend(self.cli_rules.iter().cloned());
        Arc::new(FileSettings {
            rules,
            ..(**settings).clone()
        })
    }
}

/// What a schema is loaded from: two settings with one key check queries alike.
pub type SchemaKey<'a> = (Option<&'a str>, &'static str, &'a [(String, String)]);

impl FileSettings {
    /// What [`FileSettings::schema`] is loaded from.
    pub fn schema_key(&self) -> SchemaKey<'_> {
        (self.schema_sql.as_deref(), self.dialect, &self.types)
    }
}

fn sorted(mut types: Vec<(String, String)>) -> Vec<(String, String)> {
    types.sort();
    types
}

/// The schema `text` defines in `dialect`, with the column types `types` overrides.
pub fn load_schema(
    text: &str,
    dialect: &'static str,
    types: &[(String, String)],
) -> Result<Schema, String> {
    loaded_schema(text, dialect, types).map(|(schema, _)| schema)
}

/// `table` with the keys of `database` on top, `rules` and `types` merged.
fn merged(table: &Table, database: &Table) -> Table {
    let mut merged = table.clone();
    merged.remove("databases");
    for (key, value) in database {
        match (merged.get_mut(key), value) {
            (Some(Value::Table(own)), Value::Table(value)) if key == "rules" || key == "types" => {
                own.extend(value.clone());
            }
            _ => {
                merged.insert(key.clone(), value.clone());
            }
        }
    }
    merged
}

fn settings_dialect(table: &Table) -> &'static str {
    dialect(table).unwrap_or("postgres")
}

fn dialect(table: &Table) -> Option<&'static str> {
    let name = table.get("dialect")?.as_str()?;
    name.parse::<sqlty_sql::Dialect>()
        .ok()
        .map(sqlty_sql::Dialect::name)
}

/// `types`, with the defaults of `driver` under them.
fn own_types(table: &Table, dialect: &str) -> Vec<(String, String)> {
    let mut types = match table.get("driver").and_then(Value::as_str) {
        Some(driver) if dialect == "postgres" => config::driver_types(driver, dialect),
        _ => Vec::new(),
    };
    let own = table
        .get("types")
        .and_then(Value::as_table)
        .into_iter()
        .flatten();
    for (key, value) in own.filter_map(|(key, value)| Some((key, value.as_str()?))) {
        types.retain(|(known, _)| known != key);
        types.push((key.clone(), value.to_string()));
    }
    types
}

fn entries(table: &Table, database: Option<&str>) -> Vec<Function> {
    let functions = table
        .get("functions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    functions
        .filter_map(Value::as_table)
        .filter_map(|entry| {
            Some(Function {
                import_path: entry.get("import-path")?.as_str()?.to_string(),
                query: entry.get("query-param")?.as_str()?.to_string(),
                row: entry
                    .get("return-type-param")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                database: database.map(str::to_string),
            })
        })
        .collect()
}

fn strings(table: &Table, key: &str) -> Vec<String> {
    match table.get(key) {
        Some(Value::String(text)) => vec![text.clone()],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

/// The settings of `table`, whose errors name `label`; no schema yet.
fn settings(
    table: &Table,
    label: &str,
    flags: &Flags,
    dialect_flag: Option<&str>,
    closed_params: bool,
    base: &Option<PathBuf>,
    version: Version,
) -> Result<FileSettings, String> {
    let text = |key: &str| table.get(key).and_then(Value::as_str);
    let choice = |key: &str| table.get(key).map(|value| value.as_str().unwrap_or(""));
    let flag = |key: &str| table.get(key).and_then(Value::as_bool).unwrap_or(false);
    let origin = |key: &str| flags.origin(key, label);
    let untyped = match flags
        .untyped
        .as_deref()
        .or(choice("untyped"))
        .unwrap_or("skip")
    {
        "skip" => false,
        "sentinel" => true,
        _ => {
            return Err(format!(
                "{}: untyped must be 'skip' or 'sentinel'",
                origin("untyped")
            ))
        }
    };
    let json_typed_dicts = match choice("json-types").unwrap_or("typing.Any") {
        "typing.Any" => false,
        "typing.TypedDict" => true,
        _ => {
            return Err(format!(
                "{}: json-types must be 'typing.Any' or 'typing.TypedDict'",
                origin("json-types")
            ))
        }
    };
    let import_style = text("import-style").map(str::to_string);
    if let Err(error) = ImportStyle::parse(import_style.as_deref()) {
        return Err(format!(
            "{}: {error}; use 'from sqlty import SQL [as Q]' or 'import sqlty [as sy]'",
            origin("import-style")
        ));
    }
    let preview = flag("preview");
    #[expect(clippy::expect_used, reason = "the configuration checked query-format")]
    let query_format = text("query-format")
        .unwrap_or("auto")
        .parse()
        .expect("a known query-format");
    let variants = table.get("max-query-variants").and_then(Value::as_integer);
    Ok(FileSettings {
        dialect: dialect_flag
            .and_then(|name| name.parse::<sqlty_sql::Dialect>().ok())
            .map_or_else(|| settings_dialect(table), sqlty_sql::Dialect::name),
        schema: None,
        schema_sql: None,
        schema_files: Vec::new(),
        types: Vec::new(),
        import_style,
        untyped,
        json_typed_dicts,
        param_types: table
            .get("param-types")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        closed_params,
        rules: table
            .get("rules")
            .and_then(Value::as_table)
            .into_iter()
            .flatten()
            .filter_map(|(rule, severity)| Some((rule.clone(), severity.as_str()?.to_string())))
            .collect(),
        variants: preview.then(|| {
            variants.map_or(MAX_QUERY_VARIANTS, |most| {
                usize::try_from(most.max(1)).unwrap_or(usize::MAX)
            })
        }),
        coerce: strings(table, "coerce-row-types").into_iter().collect(),
        functions: entries(table, None),
        format_command: strings(table, "format-command"),
        queries: strings(table, "queries"),
        query_format,
        base: base.clone(),
        version,
    })
}

/// The schema files `entries` names relative to `base`, read; `None` when none can be.
fn schema_text(
    entries: &[String],
    base: &Path,
    files: &Shared,
    schemas: &Schemas,
) -> Option<SchemaText> {
    let (found, empty) = project::schema_files(entries, base, files).ok()?;
    for entry in empty {
        schemas.warn(format!("Schema pattern matches no files: {entry}"));
    }
    let (mut parts, mut read) = (Vec::new(), Vec::new());
    for path in found {
        match files.text(&path) {
            Some(text) => {
                parts.push(text);
                read.push(path);
            }
            None => schemas.warn(format!("Schema file not found: {}", path.display())),
        }
    }
    (!parts.is_empty()).then(|| SchemaText {
        text: parts.join("\n"),
        files: read,
    })
}

/// A path or list of paths, as `schema` and `queries` hold them.
pub(crate) fn entries_of(entries: &Value) -> Vec<String> {
    match entries {
        Value::Array(items) => items
            .iter()
            .filter_map(|item| item.as_str().map(str::to_string))
            .collect(),
        entry => entry.as_str().map(str::to_string).into_iter().collect(),
    }
}

/// Whether the `requires-python` of the nearest `pyproject.toml` holding one, from
/// `directory` up, rules out every Python before 3.15.
fn closed_params(directory: &Path, files: &Shared) -> bool {
    let mut current = Some(files.canonical(directory));
    while let Some(folder) = current {
        let found = files
            .text(&folder.join("pyproject.toml"))
            .and_then(|text| text.parse::<Table>().ok())
            .and_then(|table| {
                let project = table.get("project")?.as_table()?;
                Some(project.get("requires-python")?.as_str()?.to_string())
            });
        if let Some(found) = found {
            return requires_closed(&found);
        }
        current = folder.parent().map(Path::to_path_buf);
    }
    false
}

/// Whether a lower bound of `requires_python` is 3.15 or later.
fn requires_closed(requires_python: &str) -> bool {
    let bounds = sqlty_sql::regex(r"(?:>=|~=|==)\s*(\d+)\.(\d+)");
    let closed = bounds.captures_iter(requires_python).any(|found| {
        let (major, minor) = (found[1].parse::<u32>(), found[2].parse::<u32>());
        matches!((major, minor), (Ok(major), Ok(minor)) if (major, minor) >= (3, 15))
    });
    closed
}
