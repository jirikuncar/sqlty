//! Property tests of the engine against independent oracles: SQLite, PostgreSQL, Python and
//! models of the rules. Each property runs `SQLTY_FUZZ_EXAMPLES` cases (100 by default):
//!
//!     SQLTY_FUZZ_EXAMPLES=5000 cargo test --release --manifest-path rust/fuzz/Cargo.toml

mod analyzer;
mod draw;
mod engine;
mod foreign_keys;
mod params;
mod python;
mod variants;
