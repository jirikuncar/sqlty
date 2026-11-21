//! The analyzer through its public API.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test fails on the query it cannot read"
)]

mod api;
mod complete;
mod cross;
mod databases;
mod dialect;
mod dml;
mod expr;
mod functions;
mod hover;
mod json;
mod literals;
mod names;
mod narrowing;
mod nullability;
mod operators;
mod params;
mod query;
mod resolve;
mod scope;
mod suggest;
mod support;
mod tables;
