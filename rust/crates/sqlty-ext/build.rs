//! Embeds the Agent Skills of `skills/` at the repository root, so `sqlty skill` prints the
//! ones of its own version wherever the wheel is installed.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    let root = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../../skills");
    let root = root
        .canonicalize()
        .unwrap_or_else(|e| panic!("{}: {e}", root.display()));
    let mut files = Vec::new();
    collect(&root, &mut files);
    files.sort();
    let mut out = String::from("pub(crate) static SKILLS: &[(&str, &str)] = &[\n");
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
    fs::write(
        Path::new(&env::var("OUT_DIR").unwrap()).join("skills.rs"),
        out,
    )
    .unwrap();
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
