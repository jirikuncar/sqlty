use sqlty_python::project::*;

/// A pattern without a slash matches the name of an entry at any depth.
fn name_matches(pattern: &str, text: &str) -> bool {
    Excludes::default().entry(text, None, &[pattern.to_string()])
}

/// A pattern with a slash matches the path of an entry, relative to the globs' base.
fn path_matches(pattern: &str, text: &str) -> bool {
    let (directory, name) = text.rsplit_once('/').unwrap_or((".", text));
    Excludes::default().entry(name, Some(directory), &[pattern.to_string()])
}

#[test]
fn a_glob_past_the_regex_size_limit_compiles() {
    let long = "[!a]".repeat(20_000);
    assert!(Excludes::default().entry(&"b".repeat(20_000), None, std::slice::from_ref(&long)));
    assert!(!Excludes::default().entry("a", None, &[long]));
}

#[test]
fn names_match_as_fnmatch_matches_them() {
    for (pattern, text, expected) in [
        ("*.py", "a.py", true),
        ("*.py", "dir/a.py", true),
        ("a**.py", "ab.py", true),
        ("?.py", "ab.py", false),
        ("[ab].py", "b.py", true),
        ("[!ab].py", "b.py", false),
        ("[a-c]", "b", true),
        ("[c-a]", "b", false),
        ("[a-]", "-", true),
        ("[!]", "x", false),
        ("[]]", "]", true),
        ("[", "[", true),
        ("a.b", "axb", false),
        ("[&&]", "&", true),
        ("[a-c-e]", "d", false),
    ] {
        assert_eq!(name_matches(pattern, text), expected, "{pattern} {text}");
    }
}

#[test]
fn paths_match_as_glob_translate_matches_them() {
    for (pattern, text, expected) in [
        ("gen/*.py", "gen/a.py", true),
        ("gen/*.py", "gen/sub/a.py", false),
        ("gen/**", "gen/sub/a.py", true),
        ("**/a.py", "a.py", true),
        ("**/a.py", "x/y/a.py", true),
        ("**/**/a.py", "x/a.py", true),
        ("*", ".hidden", true),
        ("*/b", "a/b", true),
        ("a?", "a/", false),
    ] {
        assert_eq!(path_matches(pattern, text), expected, "{pattern} {text}");
    }
}
