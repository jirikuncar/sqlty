//! The SQL assertions of the mdtest suite (`-- columns:`, `-- inferred:`, ...), through the
//! analyzer's public API. `MDTEST_UPDATE=1` writes the actual values.

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
#[path = "../../../mdtest/sql.rs"]
mod sql;

use parser::{Section, Unit};

struct Sql;

impl harness::Runner for Sql {
    /// A driver's types come from sqlty-python's configuration: its sections run there.
    fn owns(&self, section: &Section, unit: &Unit, kind: &str) -> bool {
        unit.lang == "sql" && sql::KINDS.contains(&kind) && section.setting("driver").is_none()
    }

    fn compute(&self, section: &Section, unit: &Unit, kind: &str) -> Result<String, String> {
        let schema = sql::schema(section, &sql::types(section, Vec::new()))?;
        sql::compute(section, unit, kind, schema.as_ref())
    }
}

#[test]
fn mdtest() {
    harness::run(&Sql);
}
