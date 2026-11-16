//! The project under a root, as `sqlty check ROOT` reads it, kept up to date with the
//! buffers of an editor.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlty_python::calls::Helper;
use sqlty_python::checker::Diagnostic;
use sqlty_python::index::{helpers_of, Builder, HelperIndex, Roots};
use sqlty_python::project::{self, helper_roots, Discovery, Files, Packages};
use sqlty_python::queries;
use sqlty_python::suppress;
use sqlty_python::types::info::Dict;
use sqlty_python::types::stdlib::Version;
use sqlty_python::types::symbols::Source;
use sqlty_sql::analyzer::Schema;

use crate::module::{self, check_module, Checker};
use crate::settings::{Directory, FileSettings, Function, Schemas};
use crate::Shared;

/// A checked module: its text, its names and what is read of it.
struct Module {
    text: Arc<str>,
    names: Vec<String>,
    parsed: Option<Arc<Source>>,
    helpers: Dict<Helper>,
}

/// The project under a root.
pub struct Workspace {
    files: Shared,
    root: PathBuf,
    version: Version,
    extra_roots: Vec<PathBuf>,
    discovery: Discovery,
    packages: Packages,
    schemas: Schemas,
    directories: HashMap<PathBuf, Result<Arc<Directory>, String>>,
    modules: BTreeMap<PathBuf, Module>,
    index: Option<Arc<HelperIndex>>,
    configured: HashMap<(Vec<Function>, Option<PathBuf>), Arc<HelperIndex>>,
}

/// What completion reads of a buffer.
pub struct Completion {
    /// The schema the buffer is checked against.
    pub schema: Option<Schema>,
    /// The SQL dialect.
    pub dialect: &'static str,
    /// `import-style`.
    pub import_style: Option<String>,
    /// Where the helper calls of the buffer pass a query: byte offsets of the literals.
    pub helper_starts: Vec<usize>,
}

impl Workspace {
    /// The project of the `.py` files under `root`, read from `files` as Python `version`
    /// reads them; `[[tool.sqlty.functions]]` modules are also looked for under
    /// `extra_roots` (the host's `sys.path`).
    pub fn open(
        root: &Path,
        files: Arc<dyn Files + Send + Sync>,
        version: Version,
        extra_roots: Vec<PathBuf>,
    ) -> Result<Self, String> {
        let files = Shared(files);
        let root = files.canonical(root);
        let discovery = Discovery::new(Box::new(files.clone()), &root);
        let paths = discovery.walk(&root).map_err(|error| error.to_string())?;
        let mut workspace = Self {
            packages: Packages::new(Box::new(files.clone())),
            files,
            root,
            version,
            extra_roots,
            discovery,
            schemas: Schemas::default(),
            directories: HashMap::new(),
            modules: BTreeMap::new(),
            index: None,
            configured: HashMap::new(),
        };
        workspace.settings(&workspace.root.join("__init__.py"))?;
        for path in paths {
            workspace.settings(&path)?;
            if let Some(text) = workspace.files.text(&path) {
                workspace.read(&path, Arc::from(text));
            }
        }
        Ok(workspace)
    }

    /// What the schemas could not read since the last call.
    pub fn warnings(&self) -> Vec<String> {
        self.schemas.warnings()
    }

    /// Replace the text of the module at `path`, as an editor's buffer does.
    pub fn update(&mut self, path: &Path, text: &str) {
        let path = self.files.canonical(path);
        if self
            .modules
            .get(&path)
            .is_some_and(|module| *module.text == *text)
        {
            return;
        }
        self.read(&path, Arc::from(text));
    }

    fn read(&mut self, path: &Path, text: Arc<str>) {
        let names = self
            .packages
            .names(path, std::slice::from_ref(&self.root), &self.root);
        let init = path.file_name().is_some_and(|name| name == "__init__.py");
        let name = format!("{}{}", names[0], if init { ".__init__" } else { "" });
        let parsed = Source::new(&text, self.version).map(|source| {
            let source = source.named(Some(&name));
            source.targets();
            Arc::new(source)
        });
        let helpers = parsed.as_deref().map(helpers_of).unwrap_or_default();
        self.modules.insert(
            path.to_path_buf(),
            Module {
                text,
                names,
                parsed,
                helpers,
            },
        );
        self.index = None;
        self.configured.clear();
    }

    fn index(&mut self) -> Arc<HelperIndex> {
        if let Some(index) = &self.index {
            return Arc::clone(index);
        }
        let mut builder = Builder::new(self.version);
        for module in self.modules.values() {
            let Some(parsed) = &module.parsed else {
                continue;
            };
            for name in &module.names {
                builder.add_found(name, Arc::clone(parsed), &module.helpers);
            }
        }
        let index = builder.build();
        self.index = Some(Arc::clone(&index));
        index
    }

    /// The index with the `functions` of `settings` registered.
    fn helpers(&mut self, settings: &FileSettings) -> Arc<HelperIndex> {
        let index = self.index();
        if settings.functions.is_empty() {
            return index;
        }
        let key = (settings.functions.clone(), settings.base.clone());
        if let Some(found) = self.configured.get(&key) {
            return Arc::clone(found);
        }
        let paths: Vec<PathBuf> = self.modules.keys().cloned().collect();
        let checked =
            self.packages
                .checked_roots(std::slice::from_ref(&self.root), &paths, &self.root);
        let base = settings
            .base
            .as_deref()
            .map(|base| self.files.canonical(base));
        let roots = helper_roots(base.as_deref(), &checked, &self.extra_roots);
        let files = self.files.clone();
        let read = move |path: &Path| files.text(path);
        let mut forked = index.fork();
        for function in &settings.functions {
            let roots = Roots {
                paths: &roots,
                read: &read,
            };
            let row = function.row.as_deref();
            forked.register(
                &function.import_path,
                &function.query,
                row,
                roots,
                function.database.as_deref(),
            );
        }
        let forked = forked.build();
        self.configured.insert(key, Arc::clone(&forked));
        forked
    }

    /// The settings of the file at `path`.
    pub fn settings(&mut self, path: &Path) -> Result<Arc<FileSettings>, String> {
        let path = self.files.canonical(path);
        let directory = path.parent().unwrap_or(Path::new("/")).to_path_buf();
        if !self.directories.contains_key(&directory) {
            let found = project::find(&directory, &self.files);
            let found = match found {
                Ok(found) => Ok(found.map(|found| {
                    let base = found.file.parent().unwrap_or(Path::new("/")).to_path_buf();
                    (found.config.table, base, found.file.display().to_string())
                })),
                Err(project::FindError::Syntax(_)) => Ok(None),
                Err(error) => Err(error.to_string()),
            };
            let made = found.and_then(|found| {
                Directory::new(found, &self.files, &self.schemas, self.version).map(Arc::new)
            });
            self.directories.insert(directory.clone(), made);
        }
        let directory = self.directories[&directory].clone()?;
        Ok(directory.file(&path, &self.discovery))
    }

    /// The diagnostics of `text`, the buffer of `path`: a Python module, or a `.sql` file
    /// whose project's `queries` name it (any other has none).
    pub fn check(&mut self, path: &Path, text: &str) -> Result<Vec<Diagnostic>, String> {
        let settings = self.settings(path)?;
        if path.extension().is_some_and(|extension| extension == "sql") {
            return self.check_queries(path, text, &settings);
        }
        self.update(path, text);
        let helpers = self.helpers(&settings);
        let module = &self.modules[&self.files.canonical(path)];
        let checker = Checker::of(&settings, Some(&helpers));
        let name = module
            .parsed
            .as_ref()
            .and_then(|parsed| parsed.module.clone());
        let found = check_module(text, module.parsed.as_ref(), name.as_deref(), &checker);
        Ok(found
            .diagnostics
            .into_iter()
            .map(|reported| reported.diagnostic)
            .collect())
    }

    fn check_queries(
        &self,
        path: &Path,
        text: &str,
        settings: &FileSettings,
    ) -> Result<Vec<Diagnostic>, String> {
        let resolved = self.files.canonical(path);
        let named = settings.base.as_ref().is_some_and(|base| {
            project::names_query_file(
                path,
                &resolved,
                &self.files.canonical(base),
                &settings.queries,
            )
        });
        if !named {
            return Ok(Vec::new());
        }
        let format = settings
            .query_format
            .parse::<queries::Format>()
            .expect("the configuration checks its query-format");
        let checker = Checker::of(settings, None);
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_default();
        let found = queries::check_queries(text, format, &name, &checker.options, &checker.rules);
        let ignores = suppress::sql_ignores(text);
        Ok(suppress::apply_sql(&ignores, found, &checker.rules))
    }

    /// `text` as `sqlty check --fix` writes it.
    pub fn fix(&mut self, path: &Path, text: &str) -> Result<String, String> {
        let settings = self.settings(path)?;
        self.update(path, text);
        let helpers = self.helpers(&settings);
        let module = &self.modules[&self.files.canonical(path)];
        let checker = Checker::of(&settings, Some(&helpers));
        let name = module
            .parsed
            .as_ref()
            .and_then(|parsed| parsed.module.clone());
        Ok(module::fix(
            text,
            module.parsed.as_deref(),
            name.as_deref(),
            &checker,
            &settings,
        ))
    }

    /// `text` as `sqlty rewrite` writes it.
    pub fn rewrite(&mut self, path: &Path, text: &str) -> Result<String, String> {
        let settings = self.settings(path)?;
        Ok(module::rewrite(
            text,
            &Checker::of(&settings, None),
            &settings,
        ))
    }

    /// `text` as `sqlty strip` writes it.
    pub fn strip(&mut self, path: &Path, text: &str) -> Result<String, String> {
        let settings = self.settings(path)?;
        Ok(module::strip(text, &Checker::of(&settings, None)))
    }

    /// `text` as `sqlty format` writes it.
    pub fn format(&mut self, path: &Path, text: &str) -> Result<String, String> {
        let settings = self.settings(path)?;
        Ok(module::format(text, &Checker::of(&settings, None)))
    }

    /// What completion reads of the buffer [`Workspace::check`] last read for `path`.
    pub fn completion(&mut self, path: &Path) -> Option<Completion> {
        let settings = self.settings(path).ok()?;
        let helpers = self.helpers(&settings);
        let module = self.modules.get(&self.files.canonical(path))?;
        let checker = Checker::of(&settings, Some(&helpers));
        let helper_starts = match &module.parsed {
            Some(parsed) => module::helper_starts(parsed, parsed.module.as_deref(), &checker),
            None => Vec::new(),
        };
        Some(Completion {
            schema: settings.schema.clone(),
            dialect: settings.dialect,
            import_style: settings.import_style.clone(),
            helper_starts,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlty_python::project::{normalized, Kind, Listing, Read};

    struct Tree(BTreeMap<PathBuf, &'static str>);

    impl Files for Tree {
        fn read(&self, path: &Path) -> Read {
            self.0
                .get(path)
                .filter(|text| **text != "<unreadable>")
                .map_or(Read::Missing, |text| Read::Text(text.as_bytes().to_vec()))
        }

        fn list(&self, path: &Path) -> Listing {
            let path = &normalized(path);
            let mut entries: Vec<(String, Kind)> = Vec::new();
            for file in self.0.keys() {
                let Ok(rest) = file.strip_prefix(path) else {
                    continue;
                };
                let mut parts = rest.components();
                let Some(name) = parts.next() else { continue };
                let name = name.as_os_str().to_string_lossy().into_owned();
                let kind = if parts.next().is_some() {
                    Kind::Dir
                } else {
                    Kind::File
                };
                if !entries.iter().any(|(seen, _)| *seen == name) {
                    entries.push((name, kind));
                }
            }
            if entries.is_empty() {
                Listing::Missing
            } else {
                Listing::Entries(entries)
            }
        }
    }

    const CONFIG: &str = "[tool.sqlty]\nschema = \"schema.sql\"\n\n[[tool.sqlty.functions]]\nimport-path = \"app.db.fetch\"\nquery-param = \"query\"\n";

    fn opened(files: &[(&str, &'static str)]) -> Workspace {
        open(files).expect("opens")
    }

    fn open(files: &[(&str, &'static str)]) -> Result<Workspace, String> {
        let tree = Tree(
            files
                .iter()
                .map(|(path, text)| (PathBuf::from(path), *text))
                .collect(),
        );
        Workspace::open(Path::new("/repo"), Arc::new(tree), (3, 14), Vec::new())
    }

    fn project() -> Workspace {
        opened(&[
            ("/repo/pyproject.toml", CONFIG),
            ("/repo/schema.sql", "CREATE TABLE users (id INT NOT NULL);"),
            ("/repo/app/__init__.py", ""),
            ("/repo/app/db.py", "def fetch(query: str) -> list: ...\n"),
            (
                "/repo/queries.sql",
                "-- name: one\nSELECT nope FROM users;\n",
            ),
        ])
    }

    fn rules(found: &[Diagnostic]) -> Vec<&'static str> {
        found.iter().map(|found| found.rule.name()).collect()
    }

    #[test]
    fn checks_a_buffer_against_the_schema_and_its_ignores() {
        let mut workspace = project();
        let path = Path::new("/repo/app/q.py");
        let found = workspace
            .check(path, "q = \"--sql\\nSELECT nope FROM users\"\n")
            .expect("checks");
        assert_eq!(rules(&found), ["unknown-column"]);
        let ignored = "q = \"--sql\\nSELECT nope FROM users\"  # sqlty: ignore[unknown-column]\n";
        assert_eq!(
            rules(&workspace.check(path, ignored).expect("checks")),
            Vec::<&str>::new()
        );
    }

    #[test]
    fn checks_the_calls_of_configured_and_project_helpers() {
        let mut workspace = project();
        let text = "from app.db import fetch\n\nfetch(\"SELECT nope FROM users\")\n";
        let found = workspace
            .check(Path::new("/repo/app/use.py"), text)
            .expect("checks");
        assert_eq!(rules(&found), ["unknown-column"]);
        assert_eq!(
            workspace
                .completion(Path::new("/repo/app/use.py"))
                .expect("completes")
                .helper_starts,
            [32]
        );
    }

    #[test]
    fn checks_only_the_sql_files_queries_names() {
        let mut workspace = project();
        let text = "-- name: one\nSELECT nope FROM users;\n";
        assert_eq!(
            rules(
                &workspace
                    .check(Path::new("/repo/queries.sql"), text)
                    .expect("checks")
            ),
            Vec::<&str>::new()
        );
        let mut named = opened(&[
            (
                "/repo/pyproject.toml",
                "[tool.sqlty]\nschema = \"schema.sql\"\nqueries = [\"queries.sql\"]\n",
            ),
            ("/repo/schema.sql", "CREATE TABLE users (id INT NOT NULL);"),
        ]);
        let found = named
            .check(Path::new("/repo/queries.sql"), text)
            .expect("checks");
        assert_eq!(rules(&found), ["unknown-column"]);
    }

    #[test]
    fn fixes_rewrites_and_strips() {
        let mut workspace = project();
        let path = Path::new("/repo/app/q.py");
        let text = "q = \"--sql\\nSELECT id FROM users\"\n";
        let fixed = workspace.fix(path, text).expect("fixes");
        assert!(
            fixed.ends_with("q = SQL[tuple[int]](\"--sql\\nSELECT id FROM users\")\n"),
            "{fixed}"
        );
        assert_eq!(workspace.rewrite(path, text).expect("rewrites"), fixed);
        assert_eq!(workspace.strip(path, &fixed).expect("strips"), text);
    }

    #[test]
    fn reports_an_invalid_config() {
        let error = open(&[("/repo/pyproject.toml", "[tool.sqlty]\ndialect = \"nope\"\n")]).err();
        assert!(error.is_some_and(|error| error.contains("unknown dialect 'nope'")));
        let error = open(&[("/repo/pyproject.toml", "[tool.sqlty]\nuntyped = \"nope\"\n")]).err();
        assert!(error.is_some_and(|error| error.ends_with("untyped must be 'skip' or 'sentinel'")));
        let mut excluded = opened(&[("/repo/.venv/sqlty.toml", "dialect = \"nope\"\n")]);
        let error = excluded
            .check(Path::new("/repo/.venv/q.py"), "x = 1\n")
            .err();
        assert!(error.is_some_and(|error| error.contains("unknown dialect 'nope'")));
    }

    #[test]
    fn routes_files_to_databases_and_overrides() {
        const CONFIG: &str = "[project]\nrequires-python = \">=3.15\"\n\n[tool.sqlty]\nschema = \"app.sql\"\n\n[tool.sqlty.databases.analytics]\ndialect = \"sqlite\"\nschema = \"analytics.sql\"\ninclude = [\"jobs/**\"]\n\n[tool.sqlty.databases.lake]\ndialect = \"duckdb\"\n\n[[tool.sqlty.overrides]]\ninclude = [\"legacy/**\"]\nrules = { unknown-column = \"ignore\" }\n";
        let mut workspace = opened(&[
            ("/repo/pyproject.toml", CONFIG),
            ("/repo/app.sql", "CREATE TABLE users (id TEXT NOT NULL);"),
            (
                "/repo/analytics.sql",
                "CREATE TABLE events (n INTEGER NOT NULL);",
            ),
        ]);
        let events = "q = \"--sql\\nSELECT nope FROM events\"\n";
        let found = workspace
            .check(Path::new("/repo/jobs/q.py"), events)
            .expect("checks");
        assert_eq!(rules(&found), ["unknown-column"]);
        assert_eq!(
            workspace
                .settings(Path::new("/repo/jobs/q.py"))
                .expect("settings")
                .dialect,
            "sqlite"
        );
        let users = "q = \"--sql\\nSELECT nope FROM users\"\n";
        assert_eq!(
            rules(
                &workspace
                    .check(Path::new("/repo/app/q.py"), users)
                    .expect("checks")
            ),
            ["unknown-column"]
        );
        let ignored = workspace
            .check(Path::new("/repo/legacy/q.py"), users)
            .expect("checks");
        assert_eq!(rules(&ignored), Vec::<&str>::new());
        assert!(
            workspace
                .settings(Path::new("/repo/q.py"))
                .expect("settings")
                .closed_params
        );
        assert_eq!(
            workspace.warnings(),
            ["Table and column names are not checked for duckdb"]
        );
    }

    #[test]
    fn warns_once_about_what_the_schemas_do_not_read() {
        let workspace = opened(&[
            (
                "/repo/pyproject.toml",
                "[tool.sqlty]\nschema = [\"schema.sql\", \"gone.sql\", \"none/*.sql\"]\n\
                 [tool.sqlty.databases.logs]\ntypes = { \"t.c\" = \"int\" }\n",
            ),
            (
                "/repo/schema.sql",
                "CREATE VIEW bad (a) AS SELECT id FROM (;\nCREATE TYPE mood AS ENUM ('a') FOO;",
            ),
        ]);
        assert_eq!(
            workspace.warnings(),
            [
                "Schema pattern matches no files: none/*.sql",
                "Schema file not found: /repo/gone.sql",
                "Schema view bad could not be parsed; its columns are unknown",
                "Schema statement could not be parsed and is ignored: CREATE TYPE mood AS ENUM ('a') FOO",
            ]
        );
    }

    #[test]
    fn reads_the_project_without_a_configuration() {
        let mut workspace = opened(&[
            ("/repo/pyproject.toml", "[tool"),
            ("/repo/broken.py", "x = (\n"),
            ("/repo/gone.py", "<unreadable>"),
        ]);
        let path = Path::new("/repo/broken.py");
        let found = workspace
            .check(path, "q = \"--sql\\nSELECT 1\"  # sqlty: ignore\nx = (\n")
            .expect("checks");
        assert_eq!(rules(&found), Vec::<&str>::new());
        let completion = workspace.completion(path).expect("completes");
        assert_eq!(completion.helper_starts, Vec::<usize>::new());
        assert_eq!(completion.dialect, "postgres");
    }

    #[test]
    fn writes_the_types_the_settings_ask_for() {
        let mut workspace = opened(&[
            (
                "/repo/pyproject.toml",
                "[tool.sqlty]\nschema = \"schema.sql\"\nuntyped = \"sentinel\"\n\
                 json-types = \"typing.TypedDict\"\nparam-types = true\ndriver = \"psycopg\"\n\
                 types = { \"users.id\" = \"app.UserId\" }\nrules = { sqruff = \"warn\" }\n",
            ),
            (
                "/repo/schema.sql",
                "CREATE TABLE users (id INT NOT NULL, meta JSON NOT NULL);",
            ),
        ]);
        let path = Path::new("/repo/q.py");
        let text = "q = \"--sql\\nSELECT id, json_build_object('a', 1) AS m FROM users WHERE id = %(id)s\"\n";
        let rewritten = workspace.rewrite(path, text).expect("rewrites");
        assert!(rewritten.contains("app.UserId"), "{rewritten}");
        assert!(rewritten.contains("TypedDict"), "{rewritten}");
        let found = workspace
            .check(path, "q = \"--sql\\nselect distinct(id) from users\"\n")
            .expect("checks");
        assert_eq!(rules(&found), ["stale-rewrite", "sqruff"]);
    }

    #[test]
    fn reports_invalid_settings() {
        for (config, error) in [
            ("json-types = \"nope\"", "json-types must be 'typing.Any' or 'typing.TypedDict'"),
            ("import-style = \"nope\"", "Unsupported import-style: 'nope'"),
            ("schema = \"schema.sql\"\ntypes = { \"(\" = \"int\" }", "[tool.sqlty.types]: "),
            ("[tool.sqlty.databases.logs]\nschema = \"schema.sql\"\n[tool.sqlty.types]\n\"(\" = \"int\"", "[tool.sqlty.types]: "),
        ] {
            let text: &'static str = Box::leak(format!("[tool.sqlty]\n{config}\n").into_boxed_str());
            let found = open(&[("/repo/pyproject.toml", text), ("/repo/schema.sql", "CREATE TABLE t (a INT);")]);
            let found = found.err().unwrap_or_default();
            assert!(found.contains(error), "{config}: {found}");
        }
    }

    #[test]
    fn merges_the_rules_of_databases_and_overrides() {
        let mut workspace = opened(&[
            (
                "/repo/pyproject.toml",
                "[tool.sqlty]\nrules = { select-star = \"error\" }\n\
                 [tool.sqlty.databases.logs]\ndialect = \"sqlite\"\ninclude = [\"logs/**\"]\n\
                 rules = { select-star = \"ignore\" }\n",
            ),
            (
                "/repo/sub/pyproject.toml",
                "[tool.sqlty]\n[[tool.sqlty.overrides]]\ninclude = [\"old/**\"]\n",
            ),
        ]);
        let text = "q = \"--sql\\nSELECT * FROM t\"\n";
        let logs = workspace
            .check(Path::new("/repo/logs/q.py"), text)
            .expect("checks");
        assert_eq!(rules(&logs), Vec::<&str>::new());
        let old = workspace
            .check(Path::new("/repo/sub/old/q.py"), text)
            .expect("checks");
        assert_eq!(rules(&old), Vec::<&str>::new());
    }

    #[test]
    fn registers_helpers_found_on_the_extra_roots() {
        let tree = Tree(BTreeMap::from([
            (
                PathBuf::from("/repo/pyproject.toml"),
                "[tool.sqlty]\n[[tool.sqlty.functions]]\nimport-path = \"lib.fetch\"\nquery-param = \"query\"\n",
            ),
            (PathBuf::from("/site/lib.py"), "def fetch(query: str) -> None: ...\n"),
        ]));
        let mut workspace = Workspace::open(
            Path::new("/repo"),
            Arc::new(tree),
            (3, 14),
            vec![PathBuf::from("/site")],
        )
        .expect("opens");
        let text = "from lib import fetch\n\nfetch(\"SELEC 1\")\n";
        let found = workspace
            .check(Path::new("/repo/q.py"), text)
            .expect("checks");
        assert_eq!(rules(&found), ["unparsed-query"]);
    }
}
