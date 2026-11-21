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
            let own = entry.file_type();
            let linked = own.as_ref().is_ok_and(std::fs::FileType::is_symlink);
            let target = match own {
                Ok(_) if linked => std::fs::metadata(entry.path()).map(|meta| meta.file_type()),
                own => own,
            };
            let kind = match target {
                Ok(kind) if kind.is_dir() && linked => Kind::LinkedDir,
                Ok(kind) if kind.is_dir() => Kind::Dir,
                Ok(kind) if kind.is_file() => Kind::File,
                _ => Kind::Other,
            };
            (name, kind)
        });
        Listing::Entries(entries.collect())
    }

    fn size(&self, path: &Path) -> Option<u64> {
        let found = std::fs::metadata(path).ok()?;
        found.is_file().then_some(found.len())
    }

    /// Only a regular file is read, and no further than `limit + 1` bytes: a link to a
    /// device or a pipe would give bytes without end, or none ever.
    fn read_within(&self, path: &Path, limit: u64) -> Read {
        use std::io::Read as _;

        // Opening a pipe waits for its writer: the open does not wait, and what was
        // opened is asked what it is, so that the file read is the one asked about.
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            // Nor does a terminal become the one this process is controlled from.
            options.custom_flags(libc::O_NONBLOCK | libc::O_NOCTTY);
        }
        // No such flag elsewhere: the path is asked first.
        #[cfg(not(unix))]
        if !std::fs::metadata(path).is_ok_and(|found| found.is_file()) {
            return Read::Missing;
        }
        let Ok(file) = options.open(path) else {
            return Read::Missing;
        };
        if !file.metadata().is_ok_and(|found| found.is_file()) {
            return Read::Missing;
        }
        let mut data = Vec::new();
        match file.take(limit.saturating_add(1)).read_to_end(&mut data) {
            Ok(_) => Read::Text(data),
            Err(_) => Read::Missing,
        }
    }

    fn is_link(&self, path: &Path) -> bool {
        std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn links_are_listed_as_what_they_point_to() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("sqlty-disk-{}", std::process::id()));
        std::fs::create_dir_all(root.join("dir")).unwrap();
        std::fs::write(root.join("a.py"), "").unwrap();
        symlink(root.join("a.py"), root.join("to-file")).unwrap();
        symlink(root.join("dir"), root.join("to-dir")).unwrap();
        symlink(root.join("missing"), root.join("broken")).unwrap();
        let Listing::Entries(mut entries) = Disk.list(&root) else {
            panic!("lists");
        };
        entries.sort();
        let link = Disk.is_link(&root.join("to-file"));
        let plain = Disk.is_link(&root.join("a.py"));
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(
            entries,
            [
                ("a.py".to_string(), Kind::File),
                ("broken".to_string(), Kind::Other),
                ("dir".to_string(), Kind::Dir),
                ("to-dir".to_string(), Kind::LinkedDir),
                ("to-file".to_string(), Kind::File),
            ]
        );
        assert!(link && !plain);
    }

    #[cfg(unix)]
    #[test]
    fn a_read_is_bounded_and_of_a_regular_file_only() {
        use std::os::unix::fs::symlink;

        let root = std::env::temp_dir().join(format!("sqlty-disk-read-{}", std::process::id()));
        std::fs::create_dir_all(root.join("dir")).unwrap();
        std::fs::write(root.join("long.py"), "x = 1\n".repeat(100)).unwrap();
        // A device that gives bytes without end, and a pipe nothing writes to: reading
        // either would not return.
        symlink("/dev/zero", root.join("endless.py")).unwrap();
        let pipe = root.join("pipe.py");
        let made = std::process::Command::new("mkfifo").arg(&pipe).status();
        let read = |name: &str| match Disk.read_within(&root.join(name), 10) {
            Read::Text(data) => Some(data.len()),
            _ => None,
        };
        let found = [
            read("long.py"),
            read("endless.py"),
            read("dir"),
            read("missing.py"),
        ];
        let piped = made
            .is_ok_and(|status| status.success())
            .then(|| read("pipe.py"));
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(found, [Some(11), None, None, None]);
        assert!(piped.is_none_or(|read| read.is_none()), "a pipe is no file");
    }

    #[test]
    fn only_a_file_has_a_size() {
        let root = std::env::temp_dir().join(format!("sqlty-disk-size-{}", std::process::id()));
        std::fs::create_dir_all(root.join("dir")).unwrap();
        std::fs::write(root.join("a.py"), "x = 1\n").unwrap();
        let sizes = ["a.py", "dir", "missing.py"].map(|name| Disk.size(&root.join(name)));
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(sizes, [Some(6), None, None]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_whose_read_fails_is_missing() {
        // A regular file by what it says of itself, whose first bytes cannot be read.
        let memory = Path::new("/proc/self/mem");
        assert!(std::fs::metadata(memory).is_ok_and(|found| found.is_file()));
        assert!(matches!(Disk.read_within(memory, 10), Read::Missing));
    }

    #[test]
    fn reads_files_and_resolves_the_part_that_exists() {
        let root = std::env::temp_dir().join(format!("sqlty-disk-resolve-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.py"), "x = 1\n").unwrap();
        let read = Disk.read(&root.join("a.py"));
        let missing = Disk.read(&root.join("missing.py"));
        let unlisted = Disk.list(&root.join("missing"));
        let resolved = Disk.canonical(&root.join("missing/b.py"));
        let canonical = std::fs::canonicalize(&root).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert!(matches!(read, Read::Text(text) if text == b"x = 1\n"));
        assert!(matches!(missing, Read::Missing));
        assert!(matches!(unlisted, Listing::Missing));
        assert_eq!(resolved, canonical.join("missing/b.py"));
    }

    #[test]
    fn a_relative_path_that_does_not_exist_stays() {
        let missing = Path::new("no-such-dir/x.py");
        assert_eq!(Disk.canonical(missing), missing);
    }
}
