//! Python source handling: tokenizing, rewriting, linting and fixing.
//!
//! The crate finds the SQL strings in Python source (`"--sql\nSELECT ..."` and
//! `SQL[T]("--sql\n...")`), asks `sqlty-sql` for their types and edits or reports on the
//! source. Every entry point takes the source and an [`options::Options`] (dialect, schema,
//! import style).
//!
//! - [`rewriter`]: [`rewriter::rewrite_source_with`] wraps strings in `SQL[T](...)` and
//!   [`rewriter::strip_source`] takes the wrappers off again.
//! - [`checker`]: [`checker::check_source`] lints the source into [`checker::Diagnostic`]s.
//! - [`fixer`]: [`fixer::fix_source`] applies the fixes the checker's diagnostics carry.
//! - [`locate`]: [`locate::Queries`] finds the query under a cursor.
//! - [`edit`]: the [`edit::Edit`]s that rewriting, stripping and fixing make, applied in
//!   one pass by [`edit::apply_edits`].
//! - [`rules`]: the lint [`rules::Rule`]s, their severities and the user's overrides.
//! - [`config`]: the `[tool.sqlty]` table, loaded and validated for every host.
//! - [`options`]: the settings every entry point shares.
//! - [`params`]: [`params::add_param_types`] writes the `P` of `SQL[R, P]`.
//! - [`module`]: [`module::facts`] reads what a module imports and calls.
//! - [`queries`]: [`queries::check_queries`] checks the named queries of a `.sql` file.
//! - [`migrations`]: [`migrations::check_migrations`] checks what new migration statements lock.
//!
//! The tokenizer and scanner are crate-internal.
//!
//! # Example
//!
//! ```
//! use sqlty_python::options::Options;
//! use sqlty_python::rewriter::{rewrite_source_with, strip_source};
//!
//! let options = Options::new("postgres", None, None, false).unwrap();
//! let source = "q = \"--sql\\nSELECT id::INTEGER FROM users\"\n";
//!
//! let rewritten = rewrite_source_with(source, &options, false);
//! assert!(rewritten.contains("SQL[tuple[int]]("));
//! assert_eq!(strip_source(&rewritten, &options.style), source);
//! ```

#![cfg_attr(coverage_nightly, feature(coverage_attribute), allow(unused_features))]

pub(crate) mod analyses;
pub mod arg_types;
pub mod breaking;
#[allow(missing_docs)]
pub mod calls;
#[allow(missing_docs)]
pub mod check;
pub mod checker;
#[allow(missing_docs)]
pub mod compose;
pub mod config;
pub mod drivers;
pub mod edit;
pub mod facts;
pub mod fixer;
pub(crate) mod flow;
pub mod frozen;
pub mod helpers;
#[allow(missing_docs)]
pub mod index;
pub mod json_types;
pub mod locate;
pub mod migrations;
pub mod module;
pub mod modules;
pub mod options;
pub mod params;
pub mod parse;
pub mod project;
pub mod queries;
#[allow(missing_docs)]
pub mod receivers;
pub mod report;
pub mod rewriter;
#[allow(missing_docs)]
pub mod rows;
pub mod rules;
pub(crate) mod scan;
pub mod scopes;
pub mod suppress;
pub(crate) mod template;
pub mod templates;
pub(crate) mod tokens;
#[allow(missing_docs)]
pub mod types;
pub mod unparse;
pub mod utf16;
pub mod variants;
