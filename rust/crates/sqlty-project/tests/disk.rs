//! What `Disk` reads of the file system: links, file contents and paths that do not exist.

use std::path::Path;

use sqlty_project::Disk;
use sqlty_python::project::{Files, Kind, Listing, Read};

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

#[test]
fn reads_files_and_resolves_the_part_that_exists() {
    let root = std::env::temp_dir().join(format!("sqlty-disk-read-{}", std::process::id()));
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
