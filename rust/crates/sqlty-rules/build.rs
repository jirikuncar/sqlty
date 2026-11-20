//! Compiles the rule pages `docs/reference/rules/*.md` of the repository into
//! `$OUT_DIR/rules.rs`, which `src/lib.rs` includes: the `Rule` enum and its tables.
//!
//! A page's front matter is the only place a rule's metadata is written.

#![expect(
    clippy::panic,
    clippy::unwrap_used,
    reason = "reads the pages at compile time, failing the build"
)]

use std::path::PathBuf;
use std::{env, fs};

#[path = "compile.rs"]
mod compile;

fn main() {
    let dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../../docs/reference/rules");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut pages = Vec::new();
    for entry in fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "md") {
            println!("cargo:rerun-if-changed={}", path.display());
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            pages.push((name, text));
        }
    }
    let code = compile::compile(&pages).unwrap_or_else(|e| panic!("rule pages: {e}"));
    fs::write(
        PathBuf::from(env::var("OUT_DIR").unwrap()).join("rules.rs"),
        code,
    )
    .unwrap();
}
