//! The `[tool.sqlty]` table for `sqlty._config`: loaded and validated by
//! `sqlty_python::config`, converted to and from Python values here.

use pyo3::create_exception;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};
use std::path::{Path, PathBuf};

use sqlty_python::config::{self, Config, LoadError};
use sqlty_python::project::{self, Discovery, Files, FindError, Kind, Listing, Read};
use toml::{Table, Value};

create_exception!(
    _core,
    ConfigSyntaxError,
    PyValueError,
    "A configuration file that is not UTF-8 or not TOML."
);

/// The sqlty table of a file called `file_name`, as `(table, skipped functions)`: TOML
/// keys, every `overrides` entry complete, incomplete `functions` entries left out and
/// rendered in the list. `None` for a `pyproject.toml` or `Cargo.toml` without one, unless
/// `required`.
///
/// Raises:
///     ConfigSyntaxError: the file is not UTF-8 or not TOML
///     ValueError: an unknown key, rule or severity, a value of the wrong type, or no
///         table in a required native file; the message starts with `source`
#[pyfunction]
#[pyo3(signature = (data, file_name, source, *, required))]
pub(crate) fn load_config<'py>(
    py: Python<'py>,
    data: &[u8],
    file_name: &str,
    source: &str,
    required: bool,
) -> PyResult<Option<(Bound<'py, PyDict>, Vec<String>)>> {
    match config::load(data, file_name, source, required) {
        Ok(found) => found.map(|found| converted(py, found)).transpose(),
        Err(LoadError::Syntax(message)) => Err(ConfigSyntaxError::new_err(message)),
        Err(LoadError::Invalid(error)) => Err(PyValueError::new_err(error.0)),
    }
}

/// `load_config` for a table already parsed, such as `tomllib` gives.
///
/// Raises:
///     ValueError: as for `load_config`
#[pyfunction]
pub(crate) fn validate_config<'py>(
    py: Python<'py>,
    table: &Bound<'py, PyDict>,
    source: &str,
) -> PyResult<(Bound<'py, PyDict>, Vec<String>)> {
    let found = config::validate(to_table(table)?, source)
        .map_err(|error| PyValueError::new_err(error.0))?;
    converted(py, found)
}

/// The files `std::fs` reads: a directory or an unreadable file is missing.
struct Disk;

impl Files for Disk {
    fn read(&self, path: &Path) -> Read {
        std::fs::read(path).map_or(Read::Missing, Read::Text)
    }

    fn list(&self, path: &Path) -> Listing {
        let Ok(entries) = std::fs::read_dir(path) else {
            return Listing::Missing;
        };
        let entries = entries.flatten().map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let linked = entry.file_type().is_ok_and(|kind| kind.is_symlink());
            let kind = match std::fs::metadata(entry.path()) {
                Ok(meta) if meta.is_dir() && linked => Kind::LinkedDir,
                Ok(meta) if meta.is_dir() => Kind::Dir,
                Ok(meta) if meta.is_file() => Kind::File,
                _ => Kind::Other,
            };
            (name, kind)
        });
        Listing::Entries(entries.collect())
    }

    /// `Path.resolve()`: the longest existing prefix resolved, the rest appended.
    fn canonical(&self, path: &Path) -> PathBuf {
        let path = project::normalized(path);
        let mut rest = Vec::new();
        let mut prefix = path.as_path();
        loop {
            if let Ok(resolved) = std::fs::canonicalize(prefix) {
                return rest
                    .iter()
                    .rev()
                    .fold(resolved, |path, name| path.join(name));
            }
            match (prefix.parent(), prefix.file_name()) {
                (Some(parent), Some(name)) => {
                    rest.push(name.to_os_string());
                    prefix = parent;
                }
                _ => return path,
            }
        }
    }
}

fn find_error(error: FindError) -> PyErr {
    match error {
        FindError::Syntax(message) => ConfigSyntaxError::new_err(message),
        error => PyValueError::new_err(error.to_string()),
    }
}

/// `(table, skipped functions, file)` of `find_config`.
type FoundConfig<'py> = (Bound<'py, PyDict>, Vec<String>, PathBuf);

/// The nearest configuration of `directory`, an absolute path, as `(table, skipped
/// functions, file)` (see `load_config`): `sqlty.toml`, else a `pyproject.toml` or
/// `Cargo.toml` with a sqlty table, in `directory` or a directory above it, `extend`
/// applied. None when no file has one.
///
/// Raises:
///     ConfigSyntaxError: a `pyproject.toml` or `Cargo.toml` is not TOML
///     ValueError: a file is invalid, or `extend` names a missing file or a cycle
#[pyfunction]
#[expect(
    clippy::needless_pass_by_value,
    reason = "PyO3 extracts a PathBuf, not a Path"
)]
fn find_config(py: Python<'_>, directory: PathBuf) -> PyResult<Option<FoundConfig<'_>>> {
    let found = py
        .detach(|| project::find(&directory, &Disk))
        .map_err(find_error)?;
    found
        .map(|found| {
            let (table, skipped) = converted(py, found.config)?;
            Ok((table, skipped, found.file))
        })
        .transpose()
}

/// The configuration of the file at `path`, as `--config-file` names it, `extend` applied
/// with paths relative to `base`; see `find_config`.
///
/// Raises:
///     ValueError: as for `find_config`, or the file is missing or has no sqlty table
#[pyfunction]
#[expect(
    clippy::needless_pass_by_value,
    reason = "PyO3 extracts a PathBuf, not a Path"
)]
fn config_file<'py>(
    py: Python<'py>,
    path: PathBuf,
    base: PathBuf,
) -> PyResult<(Bound<'py, PyDict>, Vec<String>)> {
    let found = py
        .detach(|| project::load_file(&path, &base, &Disk))
        .map_err(find_error)?;
    converted(py, found)
}

/// The `types` entries `driver` adds in `dialect`, under the project's own.
#[pyfunction]
fn driver_types(driver: &str, dialect: &str) -> Vec<(String, String)> {
    config::driver_types(driver, dialect)
}

/// Which files `sqlty check` reads under a directory, as the configuration of each
/// directory, `--config-file` (`fixed`, its discovery keys), `--isolated` and the exclude
/// flags pick them; `cwd` is absolute. A configuration that is not TOML excludes nothing.
///
/// Raises:
///     ValueError: a configuration file is invalid
#[pyclass(frozen, name = "Discovery", module = "sqlty._core")]
struct PyDiscovery(Discovery);

#[pymethods]
impl PyDiscovery {
    #[new]
    #[pyo3(signature = (cwd, *, isolated, fixed, exclude, extend_exclude, respect_gitignore))]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "PyO3 extracts a PathBuf, not a Path"
    )]
    fn new(
        cwd: PathBuf,
        isolated: bool,
        fixed: Option<&Bound<'_, PyDict>>,
        exclude: Option<Vec<String>>,
        extend_exclude: Vec<String>,
        respect_gitignore: Option<bool>,
    ) -> PyResult<Self> {
        let mut discovery = Discovery::new(Box::new(Disk), &cwd).extend_exclude(&extend_exclude);
        if isolated {
            discovery = discovery.isolated();
        }
        if let Some(fixed) = fixed {
            discovery = discovery.fixed(to_table(fixed)?);
        }
        if let Some(exclude) = exclude {
            discovery = discovery.exclude(&exclude);
        }
        if let Some(respect) = respect_gitignore {
            discovery = discovery.respect_gitignore(respect);
        }
        Ok(Self(discovery))
    }

    /// `(base directory, globs)` pairs excluding entries of `directory`.
    fn exclusions(&self, py: Python<'_>, directory: &str) -> PyResult<Vec<(PathBuf, Vec<String>)>> {
        py.detach(|| self.0.exclusions(Path::new(directory)))
            .map_err(find_error)
    }

    /// Whether walking `directory` skips what `.gitignore` ignores.
    fn respects_gitignore(&self, py: Python<'_>, directory: &str) -> PyResult<bool> {
        py.detach(|| self.0.respects_gitignore(Path::new(directory)))
            .map_err(find_error)
    }

    /// Whether `path`, or a directory holding it up to a glob's base, is excluded.
    fn is_excluded(&self, py: Python<'_>, path: &str) -> PyResult<bool> {
        py.detach(|| self.0.is_excluded(Path::new(path)))
            .map_err(find_error)
    }

    /// The `.py` files under `root`, sorted, as joined to `root`.
    fn walk(&self, py: Python<'_>, root: &str) -> PyResult<Vec<PathBuf>> {
        py.detach(|| self.0.walk(Path::new(root)))
            .map_err(find_error)
    }
}

/// The files a `schema` or `queries` key names relative to `base`, and its globs that
/// match none; see `sqlty_python::project::schema_files`.
#[pyfunction]
#[expect(
    clippy::needless_pass_by_value,
    reason = "PyO3 extracts a Vec, not a slice"
)]
fn schema_files(
    py: Python<'_>,
    entries: Vec<String>,
    base: &str,
) -> PyResult<(Vec<PathBuf>, Vec<String>)> {
    py.detach(|| project::schema_files(&entries, Path::new(base), &Disk))
        .map_err(find_error)
}

/// Add the key lists and `ConfigSyntaxError` to the module.
pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("ConfigSyntaxError", m.py().get_type::<ConfigSyntaxError>())?;
    m.add("CONFIG_FILES", config::CONFIG_FILES)?;
    m.add("TOOL_KEYS", config::TOOL_KEYS)?;
    m.add("DISCOVERY_KEYS", config::DISCOVERY_KEYS)?;
    m.add("FUNCTION_KEYS", config::FUNCTION_KEYS)?;
    m.add_function(wrap_pyfunction!(load_config, m)?)?;
    m.add_function(wrap_pyfunction!(validate_config, m)?)?;
    m.add_function(wrap_pyfunction!(driver_types, m)?)?;
    m.add_function(wrap_pyfunction!(find_config, m)?)?;
    m.add_function(wrap_pyfunction!(config_file, m)?)?;
    m.add_function(wrap_pyfunction!(schema_files, m)?)?;
    m.add_class::<PyDiscovery>()?;
    m.add("DEFAULT_EXCLUDE", project::DEFAULT_EXCLUDE)?;
    Ok(())
}

fn converted(py: Python<'_>, found: Config) -> PyResult<(Bound<'_, PyDict>, Vec<String>)> {
    Ok((table_to_py(py, &found.table)?, found.skipped_functions))
}

fn table_to_py<'py>(py: Python<'py>, table: &Table) -> PyResult<Bound<'py, PyDict>> {
    let dict = PyDict::new(py);
    for (key, value) in table {
        dict.set_item(key, to_py(py, value)?)?;
    }
    Ok(dict)
}

/// Validated tables hold strings, booleans and integers in their values; a date can only
/// sit under a key the host ignores, so it is handed over as its TOML text.
fn to_py<'py>(py: Python<'py>, value: &Value) -> PyResult<Bound<'py, PyAny>> {
    Ok(match value {
        Value::String(text) => text.into_pyobject(py)?.into_any(),
        Value::Integer(number) => number.into_pyobject(py)?.into_any(),
        Value::Float(number) => number.into_pyobject(py)?.into_any(),
        Value::Boolean(flag) => PyBool::new(py, *flag).to_owned().into_any(),
        Value::Datetime(datetime) => datetime.to_string().into_pyobject(py)?.into_any(),
        Value::Array(items) => {
            let items = items.iter().map(|item| to_py(py, item));
            PyList::new(py, items.collect::<PyResult<Vec<_>>>()?)?.into_any()
        }
        Value::Table(table) => table_to_py(py, table)?.into_any(),
    })
}

/// A value of a `tomllib` table; anything TOML has no type for, a date included, becomes
/// its `str()`, as a misplaced value is still reported by the key it sits under.
fn to_toml(value: &Bound<'_, PyAny>) -> PyResult<Value> {
    Ok(if let Ok(dict) = value.cast::<PyDict>() {
        Value::Table(to_table(dict)?)
    } else if let Ok(flag) = value.cast::<PyBool>() {
        Value::Boolean(flag.is_true())
    } else if value.is_instance_of::<PyInt>() {
        Value::Integer(value.extract()?)
    } else if let Ok(number) = value.cast::<PyFloat>() {
        Value::Float(number.value())
    } else if let Ok(text) = value.cast::<PyString>() {
        Value::String(text.to_str()?.to_string())
    } else if value.is_instance_of::<PyList>() || value.is_instance_of::<PyTuple>() {
        let items = value.try_iter()?.map(|item| to_toml(&item?));
        Value::Array(items.collect::<PyResult<_>>()?)
    } else {
        Value::String(value.str()?.to_string())
    })
}

fn to_table(dict: &Bound<'_, PyDict>) -> PyResult<Table> {
    let mut table = Table::new();
    for (key, item) in dict {
        table.insert(key.str()?.to_string(), to_toml(&item)?);
    }
    Ok(table)
}
