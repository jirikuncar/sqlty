//! Snapshots for `sqlty breaking`: what `--save` writes and `--against` reads.
//!
//! A snapshot holds what checking a revision needs, so a release is a baseline without its
//! git history: its Python files and `.sql` query files, the settings each is checked
//! with, and the schemas they name. A file the snapshot lacks takes the schema of its
//! nearest directory.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Map, Value};
use sqlty_project::{FileSettings, Function, Workspace};
use sqlty_python::index::HelperIndex;
use sqlty_python::queries::Format;
use sqlty_python::rules::RuleConfig;
use sqlty_python::types::stdlib::Version;
use sqlty_sql::analyzer::Schema;

use super::{configured, index_of, Against, File, Queries, Revision, Schemas, StubText};

const FORMAT: u64 = 1;

/// A revision read back from its snapshot, limited to some paths.
pub struct Snapshot {
    /// How findings name it.
    pub label: String,
    schemas: Vec<(Arc<str>, &'static str, super::Types)>,
    directories: HashMap<String, Option<usize>>,
    files: Vec<File>,
    /// The index in `files` of each relative path.
    paths: HashMap<PathBuf, usize>,
    loaded: HashMap<usize, Schema>,
    index: Option<Arc<HelperIndex>>,
    /// The stubs the files of some settings were checked with.
    stubs: Vec<(Arc<FileSettings>, Vec<StubText>)>,
    version: Version,
}

/// The snapshot of `revision`, as JSON.
pub fn save<W: std::borrow::BorrowMut<Workspace>>(
    revision: &mut Revision<W>,
) -> Result<String, String> {
    let files = revision.files()?;
    let mut schemas: Vec<Value> = Vec::new();
    let mut settings: Vec<Value> = Vec::new();
    let mut schema = |own: &FileSettings| {
        own.schema_sql.as_ref().map(|sql| {
            let entry = json!({
                "sql": &**sql,
                "dialect": own.dialect,
                "types": types(&own.types),
            });
            index(&mut schemas, entry)
        })
    };
    let mut entries = Vec::new();
    // The stubs each settings read helpers from, found once per settings.
    let mut stubs: Vec<(Arc<FileSettings>, Vec<Value>)> = Vec::new();
    for file in &files {
        let own = &file.settings;
        let known = stubs.iter().position(|(of, _)| Arc::ptr_eq(of, own));
        let read = match known {
            Some(at) => stubs[at].1.clone(),
            None => {
                let read: Vec<Value> = revision
                    .helpers(own)
                    .stubs()
                    .into_iter()
                    .map(|(module, read_as, text)| json!([module, read_as, &*text]))
                    .collect();
                stubs.push((Arc::clone(own), read.clone()));
                read
            }
        };
        let mut rules: Vec<_> = own.rules.iter().collect();
        rules.sort();
        let functions: Vec<Value> = own
            .functions
            .iter()
            .map(|function| {
                json!([
                    function.import_path,
                    function.query,
                    function.row,
                    function.database
                ])
            })
            .collect();
        let entry = json!({
            "dialect": own.dialect,
            "schema": schema(own),
            "import_style": own.import_style,
            "untyped": own.untyped,
            "rules": match rules.is_empty() {
                true => Value::Null,
                false => Value::Object(rules.into_iter().map(|(rule, severity)| {
                    (rule.clone(), Value::from(severity.as_str()))
                }).collect()),
            },
            "functions": functions,
            "query_format": format_name(own.query_format),
        });
        let mut entry = entry;
        // Written only when there are some: a snapshot of a project without stubs is
        // what it was before stubs were read.
        if !read.is_empty() {
            entry["stubs"] = Value::Array(read);
        }
        entries.push(index(&mut settings, entry));
    }
    let mut directories: BTreeSet<PathBuf> = BTreeSet::from([PathBuf::from(".")]);
    for file in &files {
        directories.extend(super::parents(&file.relative).map(Path::to_path_buf));
    }
    let mut by_directory = Map::new();
    for directory in directories {
        let path = revision.root.join(&directory).join("__init__.py");
        let own = revision.workspace.borrow_mut().settings(&path)?;
        by_directory.insert(posix(&directory), schema(&own).into());
    }
    let files: Map<String, Value> = files
        .iter()
        .zip(entries)
        .map(|(file, settings)| {
            let entry = json!({
                "source": &*file.text,
                "module": file.module.as_deref().unwrap_or(""),
                "names": file.names,
                "settings": settings,
            });
            (posix(&file.relative), entry)
        })
        .collect();
    let data = json!({
        "sqlty": FORMAT,
        "schemas": schemas,
        "settings": settings,
        "directories": by_directory,
        "files": files,
    });
    // A `Value` always serializes: its keys are strings.
    Ok(serde_json::to_string_pretty(&data).unwrap_or_default() + "\n")
}

fn index(items: &mut Vec<Value>, item: Value) -> usize {
    match items.iter().position(|known| *known == item) {
        Some(at) => at,
        None => {
            items.push(item);
            items.len() - 1
        }
    }
}

fn types(types: &[(String, String)]) -> Value {
    match types.is_empty() {
        true => Value::Null,
        false => Value::Object(
            types
                .iter()
                .map(|(name, sql)| (name.clone(), Value::from(sql.as_str())))
                .collect(),
        ),
    }
}

fn posix(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

pub(crate) fn format_name(format: Format) -> &'static str {
    match format {
        Format::Auto => "auto",
        Format::Aiosql => "aiosql",
        Format::Sqlc => "sqlc",
        Format::Pugsql => "pugsql",
        Format::File => "file",
    }
}

impl Snapshot {
    /// The snapshot at `path`, shown as `shown`, limited to `paths`, named `label`.
    pub fn load(
        path: &Path,
        shown: &Path,
        label: &str,
        paths: &[PathBuf],
        version: Version,
    ) -> Result<Self, String> {
        let not = |why: &str| format!("{} is not a sqlty snapshot: {why}", shown.display());
        let text = std::fs::read_to_string(path).map_err(|error| not(&error.to_string()))?;
        let data: Value = serde_json::from_str(&text).map_err(|error| not(&error.to_string()))?;
        if data.get("sqlty").and_then(Value::as_u64) != Some(FORMAT) {
            return Err(format!(
                "{} is not a sqlty snapshot of format {FORMAT}",
                shown.display()
            ));
        }
        let invalid = |what: &str| not(&format!("invalid {what}"));
        let get = |key: &str| data.get(key).ok_or_else(|| invalid(key));
        let dialect = |value: &Value| {
            text_of(value, "dialect")
                .and_then(|name| name.parse::<sqlty_sql::Dialect>().ok())
                .map(sqlty_sql::Dialect::name)
                .ok_or_else(|| invalid("dialect"))
        };
        let mut snapshot = Self {
            label: label.to_string(),
            schemas: Vec::new(),
            directories: HashMap::new(),
            files: Vec::new(),
            paths: HashMap::new(),
            loaded: HashMap::new(),
            index: None,
            stubs: Vec::new(),
            version,
        };
        for entry in get("schemas")?
            .as_array()
            .ok_or_else(|| invalid("schemas"))?
        {
            let sql = text_of(entry, "sql").ok_or_else(|| invalid("schema"))?;
            let types = pairs(entry.get("types"));
            snapshot
                .schemas
                .push((Arc::from(sql), dialect(entry)?, types));
        }
        let count = snapshot.schemas.len();
        let schema_index = |value: Option<&Value>| match value {
            None | Some(Value::Null) => Ok(None),
            Some(value) => position(value, count)
                .map(Some)
                .ok_or_else(|| invalid("schema index")),
        };
        let mut settings = Vec::new();
        for entry in get("settings")?
            .as_array()
            .ok_or_else(|| invalid("settings"))?
        {
            let rules: HashMap<String, String> = pairs(entry.get("rules")).into_iter().collect();
            RuleConfig::from_overrides(Some(rules.clone()))
                .map_err(|error| not(&error.to_string()))?;
            let functions = entry
                .get("functions")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|function| {
                    let field = |at: usize| function.get(at).and_then(Value::as_str);
                    Some(Function {
                        import_path: field(0)?.to_string(),
                        query: field(1)?.to_string(),
                        row: field(2).map(str::to_string),
                        database: field(3).map(str::to_string),
                    })
                })
                .collect();
            let schema = schema_index(entry.get("schema"))?;
            let (schema_sql, types) = match schema {
                Some(at) => {
                    let (sql, _, types) = &snapshot.schemas[at];
                    (Some(Arc::clone(sql)), types.clone())
                }
                None => (None, Vec::new()),
            };
            let own = Arc::new(FileSettings {
                dialect: dialect(entry)?,
                schema: schema.map(|at| snapshot.schema(at)).transpose()?,
                schema_sql,
                schema_files: Vec::new(),
                types,
                import_style: text_of(entry, "import_style").map(str::to_string),
                untyped: entry.get("untyped").and_then(Value::as_bool) == Some(true),
                json_typed_dicts: false,
                param_types: true,
                closed_params: false,
                rules,
                variants: None,
                coerce: HashSet::new(),
                functions,
                stub_paths: Vec::new(),
                format_command: Vec::new(),
                queries: Vec::new(),
                query_format: text_of(entry, "query_format")
                    .and_then(|name| name.parse().ok())
                    .unwrap_or(Format::Auto),
                base: None,
                version,
            });
            let stubs: Vec<StubText> = entry
                .get("stubs")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|stub| {
                    let field = |at: usize| stub.get(at).and_then(Value::as_str);
                    Some((
                        field(0)?.to_string(),
                        field(1)?.to_string(),
                        Arc::from(field(2)?),
                    ))
                })
                .collect();
            if !stubs.is_empty() {
                snapshot.stubs.push((Arc::clone(&own), stubs));
            }
            settings.push(own);
        }
        let directories = get("directories")?
            .as_object()
            .ok_or_else(|| invalid("directories"))?;
        for (name, schema) in directories {
            let schema = schema_index(Some(schema))?;
            snapshot.directories.insert(name.clone(), schema);
        }
        for (name, file) in get("files")?.as_object().ok_or_else(|| invalid("files"))? {
            let relative = PathBuf::from(name);
            let wanted = paths.iter().any(|path| {
                path == Path::new(".") || *path == relative || relative.starts_with(path)
            });
            if !wanted {
                continue;
            }
            let at = file
                .get("settings")
                .and_then(|at| position(at, settings.len()))
                .ok_or_else(|| invalid("settings index"))?;
            let module = text_of(file, "module").unwrap_or("");
            let names = file
                .get("names")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|name| name.as_str().map(str::to_string))
                .collect();
            snapshot
                .paths
                .insert(relative.clone(), snapshot.files.len());
            snapshot.files.push(File {
                relative,
                text: Arc::from(text_of(file, "source").unwrap_or("")),
                module: (!module.is_empty()).then(|| module.to_string()),
                names,
                settings: Arc::clone(&settings[at]),
            });
        }
        Ok(snapshot)
    }

    fn schema(&mut self, index: usize) -> Result<Schema, String> {
        if let Some(schema) = self.loaded.get(&index) {
            return Ok(schema.clone());
        }
        let (sql, dialect, types) = &self.schemas[index];
        let schema = sqlty_project::load_schema(sql, dialect, types)?;
        self.loaded.insert(index, schema.clone());
        Ok(schema)
    }

    /// The schema index of the file `relative`, or of its nearest directory.
    fn schema_index(&self, relative: &Path) -> Option<usize> {
        if let Some(file) = self.paths.get(relative).map(|&at| &self.files[at]) {
            let sql = file.settings.schema_sql.as_ref()?;
            return self
                .schemas
                .iter()
                .position(|(text, ..)| Arc::ptr_eq(text, sql));
        }
        super::parents(relative)
            .find_map(|directory| self.directories.get(&posix(directory)))
            .copied()
            .flatten()
    }
}

fn text_of<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// The index `value` holds, when below `count`.
fn position(value: &Value, count: usize) -> Option<usize> {
    let at = usize::try_from(value.as_u64()?).ok()?;
    (at < count).then_some(at)
}

/// A JSON object of strings as pairs; anything else is none.
fn pairs(value: Option<&Value>) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = value
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_string())))
        .collect();
    found.sort();
    found
}

impl Queries for Snapshot {
    fn files(&mut self) -> Result<Vec<File>, String> {
        Ok(self
            .files
            .iter()
            .map(|file| File {
                relative: file.relative.clone(),
                text: Arc::clone(&file.text),
                module: file.module.clone(),
                names: file.names.clone(),
                settings: Arc::clone(&file.settings),
            })
            .collect())
    }

    /// A snapshot keeps the checked files and the stubs they were checked with, not the
    /// directories they were read from: its `functions` and the classes of its receivers
    /// are found among them only.
    fn helpers(&mut self, settings: &FileSettings) -> Arc<HelperIndex> {
        let index = match &self.index {
            Some(index) => Arc::clone(index),
            None => {
                let files: Vec<File> = self.files().unwrap_or_default();
                let index = index_of(&files, self.version);
                self.index = Some(Arc::clone(&index));
                index
            }
        };
        let stubs = self
            .stubs
            .iter()
            .find(|(of, _)| std::ptr::eq(&raw const **of, settings))
            .map_or(&[][..], |(_, stubs)| stubs);
        match settings.functions.is_empty() && stubs.is_empty() {
            true => index,
            false => configured(&index, settings, stubs),
        }
    }
}

impl Schemas for Snapshot {
    fn against(&mut self, relative: &Path, own: &FileSettings) -> Result<Against, String> {
        let Some(index) = self.schema_index(relative) else {
            return Ok(Against {
                key: (None, own.dialect, own.types.clone()),
                schema: None,
            });
        };
        let schema = self.schema(index)?;
        let (sql, dialect, types) = &self.schemas[index];
        Ok(Against {
            key: (Some(Arc::clone(sql)), dialect, types.clone()),
            schema: Some(schema),
        })
    }
}
