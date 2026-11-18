//! Embeds the Agent Skills of `skills/` and the rule pages of `src/sqlty/rules/`, so
//! `sqlty skill` and `sqlty rule` print those of their own version wherever sqlty is
//! installed.

#![expect(
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failing build script panics"
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    let mut out = embed("skills", "SKILLS");
    out.push_str(&embed("src/sqlty/rules", "RULE_PAGES"));
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("embedded.rs"),
        out,
    )
    .unwrap();
}

/// `pub static NAME: &[(&str, &str)]`: the files under `dir` of the repository, by path;
/// of `embedded/`, where `make crates` copies them, in a package built outside it.
fn embed(dir: &str, name: &str) -> String {
    let manifest = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).to_path_buf();
    let repository = manifest.join("../../..");
    let root = match repository.join("rust/crates/sqlty-cli/build.rs").exists() {
        true => repository.join(dir),
        false => manifest.join("embedded").join(dir),
    };
    let root = root
        .canonicalize()
        .unwrap_or_else(|e| panic!("{}: {e}", root.display()));
    let mut files = Vec::new();
    collect(&root, &mut files);
    files.sort();
    let mut out =
        format!("/// The files of `{dir}/`, by path.\npub static {name}: &[(&str, &str)] = &[\n");
    for file in &files {
        let relative = file
            .strip_prefix(&root)
            .unwrap()
            .to_str()
            .unwrap()
            .replace('\\', "/");
        writeln!(
            out,
            "    ({relative:?}, include_str!({:?})),",
            file.to_str().unwrap()
        )
        .unwrap();
    }
    out.push_str("];\n");
    out
}

/// Every file under `dir`, and every directory for Cargo to watch.
fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
    println!("cargo:rerun-if-changed={}", dir.display());
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect(&path, files);
        } else {
            println!("cargo:rerun-if-changed={}", path.display());
            files.push(path);
        }
    }
}
