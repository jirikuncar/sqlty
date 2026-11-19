//! The Python assertions of the mdtest suite and the SQL ones of `driver` sections, see
//! `rust/mdtest/python.rs`. `MDTEST_UPDATE=1` writes the actual values.

#![expect(
    clippy::unwrap_used,
    clippy::panic,
    clippy::disallowed_methods,
    reason = "a test reads the suite's files and fails on a malformed one"
)]
#![allow(dead_code, reason = "the shared runner serves every crate's kinds")]

#[path = "../../../mdtest/harness.rs"]
mod harness;
#[path = "../../../mdtest/parser.rs"]
mod parser;
#[path = "../../../mdtest/python.rs"]
mod python;
#[path = "../../../mdtest/sql.rs"]
mod sql;

#[test]
fn mdtest() {
    harness::run(&python::Python { sqruff: None });
}
