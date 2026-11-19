//! The `[tool.sqlty]` table for `sqlty._config`: loaded and validated by
//! `sqlty_python::config`, converted to and from Python values here.

use pyo3::create_exception;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};
use std::path::PathBuf;

use sqlty_project::Disk;
use sqlty_python::config::{self, Config};
use sqlty_python::project::{self, FindError};
use toml::{Table, Value};

create_exception!(
    _core,
    ConfigSyntaxError,
    PyValueError,
    "A configuration file that is not UTF-8 or not TOML."
);

/// The sqlty table `table`, as `(table, skipped functions)`: TOML keys, every `overrides`
/// entry complete, incomplete `functions` entries left out and rendered in the list.
///
/// Raises:
///     ValueError: an unknown key, rule or severity, or a value of the wrong type
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

fn find_error(error: FindError) -> PyErr {
    match error {
        FindError::Syntax(message) => ConfigSyntaxError::new_err(message),
        error => PyValueError::new_err(error.to_string()),
    }
}

/// `(table, skipped functions, file)` of `find_config`.
type FoundConfig<'py> = (Bound<'py, PyDict>, Vec<String>, PathBuf);

/// The nearest configuration of `directory`, an absolute path, as `(table, skipped
/// functions, file)` (see `validate_config`): `sqlty.toml`, else a `pyproject.toml` or
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

/// The `types` entries `driver` adds in `dialect`, under the project's own.
#[pyfunction]
fn driver_types(driver: &str, dialect: &str) -> Vec<(String, String)> {
    config::driver_types(driver, dialect)
}

/// Add the key lists and `ConfigSyntaxError` to the module.
pub(crate) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("ConfigSyntaxError", m.py().get_type::<ConfigSyntaxError>())?;
    m.add("TOOL_KEYS", config::TOOL_KEYS)?;
    m.add("DISCOVERY_KEYS", config::DISCOVERY_KEYS)?;
    m.add("FUNCTION_KEYS", config::FUNCTION_KEYS)?;
    m.add_function(wrap_pyfunction!(validate_config, m)?)?;
    m.add_function(wrap_pyfunction!(driver_types, m)?)?;
    m.add_function(wrap_pyfunction!(find_config, m)?)?;
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
