//! The files of the file system.

use std::path::{Path, PathBuf};

use sqlty_python::project::{normalized, Files, Kind, Listing, Read};

/// The files `std::fs` reads: a directory or an unreadable file is missing.
pub struct Disk;

impl Files for Disk {
    fn read(&self, path: &Path) -> Read {
        std::fs::read(path).map_or(Read::Missing, Read::Text)
    }

    fn list(&self, path: &Path) -> Listing {
        let Ok(entries) = std::fs::read_dir(path) else {
            return Listing::Missing;
        };
        let entries = entries.flatten().map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let linked = entry.file_type().is_ok_and(|kind| kind.is_symlink());
            let kind = match std::fs::metadata(entry.path()) {
                Ok(meta) if meta.is_dir() && linked => Kind::LinkedDir,
                Ok(meta) if meta.is_dir() => Kind::Dir,
                Ok(meta) if meta.is_file() => Kind::File,
                _ => Kind::Other,
            };
            (name, kind)
        });
        Listing::Entries(entries.collect())
    }

    /// `Path.resolve()`: the longest existing prefix resolved, the rest appended.
    fn canonical(&self, path: &Path) -> PathBuf {
        let path = normalized(path);
        let mut rest = Vec::new();
        let mut prefix = path.as_path();
        loop {
            if let Ok(resolved) = std::fs::canonicalize(prefix) {
                return rest
                    .iter()
                    .rev()
                    .fold(resolved, |path, name| path.join(name));
            }
            match (prefix.parent(), prefix.file_name()) {
                (Some(parent), Some(name)) => {
                    rest.push(name.to_os_string());
                    prefix = parent;
                }
                _ => return path,
            }
        }
    }
}
