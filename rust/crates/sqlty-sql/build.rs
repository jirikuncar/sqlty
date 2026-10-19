//! Compiles the inference rules in `rules/**/*.inference.toml` into
//! `$OUT_DIR/inference_rules.rs`, which `src/analyzer/inference.rs` includes.
//!
//! The rules become static data and one `match` on the operator, so the analyzer reads
//! no file and parses nothing at run time. See `rules/README.md` for the format.

use std::path::{Path, PathBuf};
use std::{env, fs};

#[path = "rules/compile.rs"]
mod inference;

fn main() {
    let crate_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = crate_dir.join("rules");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut paths = Vec::new();
    collect(&root, &mut paths);
    let files: Vec<(String, String)> = paths
        .into_iter()
        .map(|path| {
            println!("cargo:rerun-if-changed={}", path.display());
            let name = path.strip_prefix(&crate_dir).unwrap().display().to_string();
            let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
            (name.replace('\\', "/"), text)
        })
        .collect();
    let code = inference::compile(&files).unwrap_or_else(|e| panic!("inference rules: {e}"));
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("inference_rules.rs");
    fs::write(out, code).unwrap();
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, files);
        } else if path.to_string_lossy().ends_with(".inference.toml")
            || path.ends_with("rules/dialects.toml")
        {
            files.push(path);
        }
    }
}
