//! File paths of `lsp_types::Uri`, which since lsp-types 0.97 has no file-path conversions.

use std::path::{Path, PathBuf};
use std::str::FromStr;

use lsp_types::Uri;

/// The local path of a `file:` URI.
pub(crate) fn to_path(uri: &Uri) -> Option<PathBuf> {
    url::Url::parse(uri.as_str()).ok()?.to_file_path().ok()
}

/// The `file:` URI of an absolute path.
pub(crate) fn from_path(path: &Path) -> Option<Uri> {
    Uri::from_str(url::Url::from_file_path(path).ok()?.as_str()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_round_trip_through_uris() {
        let path = std::env::temp_dir().join("a dir").join("ü.py");
        let uri = from_path(&path).unwrap();
        assert!(uri.as_str().starts_with("file://"), "{}", uri.as_str());
        assert_eq!(to_path(&uri), Some(path));
        assert_eq!(
            to_path(&Uri::from_str("untitled:Untitled-1").unwrap()),
            None
        );
    }
}
