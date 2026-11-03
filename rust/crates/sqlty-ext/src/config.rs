//! The `[tool.sqlty]` table for `sqlty._config`: loaded and validated by
//! `sqlty_python::config`, converted to and from Python values here.

use pyo3::create_exception;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyBool, PyDict, PyFloat, PyInt, PyList, PyString, PyTuple};
use sqlty_python::config::{self, Config, LoadError};
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
    let Value::Table(table) = to_toml(table.as_any())? else {
        unreachable!("a dict converts to a table")
    };
    let found = config::validate(table, source).map_err(|error| PyValueError::new_err(error.0))?;
    converted(py, found)
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
        Value::Array(items) => PyList::new(
            py,
            items
                .iter()
                .map(|item| to_py(py, item))
                .collect::<PyResult<Vec<_>>>()?,
        )?
        .into_any(),
        Value::Table(table) => table_to_py(py, table)?.into_any(),
    })
}

/// A value of a `tomllib` table; anything TOML has no type for, a date included, becomes
/// its `str()`, as a misplaced value is still reported by the key it sits under.
fn to_toml(value: &Bound<'_, PyAny>) -> PyResult<Value> {
    Ok(if let Ok(dict) = value.cast::<PyDict>() {
        let mut table = Table::new();
        for (key, item) in dict {
            table.insert(key.str()?.to_string(), to_toml(&item)?);
        }
        Value::Table(table)
    } else if let Ok(flag) = value.cast::<PyBool>() {
        Value::Boolean(flag.is_true())
    } else if value.is_instance_of::<PyInt>() {
        Value::Integer(value.extract()?)
    } else if let Ok(number) = value.cast::<PyFloat>() {
        Value::Float(number.value())
    } else if let Ok(text) = value.cast::<PyString>() {
        Value::String(text.to_str()?.to_string())
    } else if value.is_instance_of::<PyList>() || value.is_instance_of::<PyTuple>() {
        Value::Array(
            value
                .try_iter()?
                .map(|item| to_toml(&item?))
                .collect::<PyResult<_>>()?,
        )
    } else {
        Value::String(value.str()?.to_string())
    })
}
