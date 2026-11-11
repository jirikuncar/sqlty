//! User-defined row types, read from source and checked against SQL column types.
//!
//! Nothing of the checked code is imported, executed or evaluated. [`extract`] finds the
//! recognized classes (`TypedDict`, `NamedTuple`, dataclass, `BaseModel`, `Protocol`) and
//! [`resolve`] reads each field's annotation as a [`ty::Ty`], going through:
//!
//! - the module's own names, [`symbols::ModuleTypes`]: aliases, `NewType`s and classes;
//! - the modules of the checked paths, read from their source ([`symbols::Index`]);
//! - the standard library, as nominal classes built from typeshed's class graph ([`stdlib`]);
//! - Pydantic's types, from a table of what each takes ([`pydantic`]).
//!
//! Any other name is unknown, and a field of unknown type is never reported.

pub mod compat;
pub mod info;
pub mod pydantic;
pub mod resolve;
pub mod stdlib;
mod stdlib_table;
pub mod ty;
