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
//! - [`edit`]: the [`edit::Edit`]s that rewriting, stripping and fixing make, applied in
//!   one pass by [`edit::apply_edits`].
//! - [`rules`]: the lint [`rules::Rule`]s, their severities and the user's overrides.
//! - [`options`]: the settings every entry point shares.
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

pub mod checker;
pub mod edit;
pub mod fixer;
pub mod options;
pub mod rewriter;
pub mod rules;
pub(crate) mod scan;
pub(crate) mod template;
pub(crate) mod tokens;
