//! Embeds the Agent Skills of `skills/`, so `sqlty skill` prints those of its own version
//! wherever sqlty is installed.

#![expect(
    clippy::panic,
    clippy::unwrap_used,
    reason = "a failing build script panics"
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("embedded.rs"),
        embed("skills", "SKILLS"),
    )
    .unwrap();
}

/// `pub static NAME: &[(&str, &str)]`: the files under `dir` of the repository, by path.
fn embed(dir: &str, name: &str) -> String {
    let root = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("../../..")
        .join(dir);
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
