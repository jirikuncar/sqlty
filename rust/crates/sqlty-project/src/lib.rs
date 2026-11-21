//! A project as `sqlty check` reads it: the files under a root, each with the settings of
//! its configuration, its schema and the helpers of the other modules, checked by the
//! engine. Hosts (the language server, the command line) bring their [`Files`].

#![cfg_attr(coverage_nightly, feature(coverage_attribute), allow(unused_features))]

mod disk;
mod flags;
mod module;
mod settings;
mod workspace;

pub use disk::Disk;
pub use flags::{parse_overrides, repr, Configs, Flags};
pub use module::{
    breaking, breaking_queries, check_module, check_queries, formatted, rewritten, skipped_strings,
    stripped, Checker,
};
pub use settings::{load_schema, FileSettings, Function, SchemaKey, Schemas};
pub use workspace::{par_map, Completion, Options, SchemaSource, Workspace};

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlty_python::project::{Files, Listing, Read};

/// A host's files, shared by the parts of a project.
#[derive(Clone)]
pub struct Shared(pub Arc<dyn Files + Send + Sync>);

impl Files for Shared {
    fn read(&self, path: &Path) -> Read {
        self.0.read(path)
    }

    fn list(&self, path: &Path) -> Listing {
        self.0.list(path)
    }

    fn canonical(&self, path: &Path) -> PathBuf {
        self.0.canonical(path)
    }

    fn is_link(&self, path: &Path) -> bool {
        self.0.is_link(path)
    }
}

impl Shared {
    /// The UTF-8 text of the file at `path`, a leading BOM dropped.
    pub fn text(&self, path: &Path) -> Option<String> {
        match self.0.read(path) {
            Read::Text(data) => {
                let text = String::from_utf8(data).ok()?;
                Some(
                    text.strip_prefix('\u{feff}')
                        .map_or(text.clone(), str::to_string),
                )
            }
            Read::Missing | Read::Unread => None,
        }
    }
}
