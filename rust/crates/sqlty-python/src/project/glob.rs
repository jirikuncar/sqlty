//! Shell globs as Python's `fnmatch` and `glob` read them, translated to `regex`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use regex::{Regex, RegexBuilder};

/// `fnmatch.translate`: `*` any run of characters, `?` one, `[...]` a set (`[!...]`
/// negated); anything else is itself. Anchored at both ends.
pub fn fnmatch(pattern: &str) -> String {
    format!("(?s:{})\\z", translate(pattern, ".*", "."))
}

/// `glob.translate(pattern, recursive=True, include_hidden=True)` on POSIX: `*` and `?`
/// stay within a segment, a `**` segment is any number of them.
pub fn path(pattern: &str) -> String {
    let parts: Vec<&str> = pattern.split('/').collect();
    let last = parts.len() - 1;
    let mut found = String::new();
    for (index, part) in parts.iter().enumerate() {
        match *part {
            "*" if index < last => found.push_str("[^/]+/"),
            "*" => found.push_str("[^/]+"),
            "**" if index < last => {
                if parts[index + 1] != "**" {
                    found.push_str("(?:.+/)?");
                }
            }
            "**" => found.push_str(".*"),
            part => {
                found.push_str(&translate(part, "[^/]*", "[^/]"));
                if index < last {
                    found.push('/');
                }
            }
        }
    }
    format!("(?s:{found})\\z")
}

/// Whether `pattern` has a wildcard, as `glob.has_magic` decides.
pub fn has_magic(pattern: &str) -> bool {
    pattern.contains(['*', '?', '['])
}

/// One segment of a glob `pattern`, as `glob.glob` without `include_hidden` matches it:
/// a name starting with `.` only when the pattern does.
pub struct Segment {
    regex: Regex,
    hidden: bool,
}

impl Segment {
    pub fn new(pattern: &str) -> Self {
        let regex = compile(&format!("^(?s:{})$", translate(pattern, "[^/]*", "[^/]")));
        let hidden = pattern.starts_with('.');
        Self { regex, hidden }
    }

    pub fn matches(&self, name: &str) -> bool {
        (self.hidden || !name.starts_with('.')) && self.regex.is_match(name)
    }
}

/// Exclude globs, as `exclude`, `include` and `overrides` read them, compiled once per set:
/// a glob without a slash matches a name at any depth; every glob matches the path
/// relative to the directory of its configuration.
#[derive(Default)]
pub struct Excludes(Mutex<HashMap<Vec<String>, Matchers>>);

/// `(name, relative path)` matchers of a set of globs.
type Matchers = Arc<(Regex, Regex)>;

impl Excludes {
    /// Whether the entry `name` of `directory`, relative to the globs' base (`None` outside
    /// it, where only globs without a slash match), matches one of `globs`.
    pub fn entry(&self, name: &str, directory: Option<&str>, globs: &[String]) -> bool {
        let compiled = self.compiled(globs);
        let (by_name, by_path) = &*compiled;
        by_name.is_match(name)
            || directory.is_some_and(|directory| match directory {
                "." => by_path.is_match(name),
                directory => by_path.is_match(&format!("{directory}/{name}")),
            })
    }

    /// Whether the resolved `path`, or a directory holding it under `base`, matches.
    pub fn path(&self, path: &Path, base: &Path, globs: &[String]) -> bool {
        let Ok(relative) = path.strip_prefix(base) else {
            return false;
        };
        let parts: Vec<String> = relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        (0..parts.len()).any(|end| {
            let directory = match end {
                0 => ".".to_string(),
                _ => parts[..end].join("/"),
            };
            self.entry(&parts[end], Some(&directory), globs)
        })
    }

    #[cfg(test)]
    pub(crate) fn sets(&self) -> usize {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).len()
    }

    fn compiled(&self, globs: &[String]) -> Matchers {
        let mut cache = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(found) = cache.get(globs) {
            return Arc::clone(found);
        }
        cache
            .entry(globs.to_vec())
            .or_insert_with(|| Arc::new(matchers(globs)))
            .clone()
    }
}

fn matchers(globs: &[String]) -> (Regex, Regex) {
    let mut names = Vec::new();
    let mut paths = Vec::new();
    for pattern in globs {
        let pattern = pattern.trim_matches('/');
        if !pattern.contains('/') {
            names.push(fnmatch(pattern));
        }
        paths.push(path(pattern));
    }
    let compile = |found: Vec<String>| match found.is_empty() {
        true => compile(NEVER),
        false => compile(&format!("^(?:{})", found.join("|"))),
    };
    (compile(names), compile(paths))
}

/// A translated glob, however long: `translate` escapes all but the classes it writes, and
/// `fnmatch` has no size limit either.
#[expect(clippy::expect_used, reason = "a translated glob is a valid pattern")]
fn compile(pattern: &str) -> Regex {
    RegexBuilder::new(pattern)
        .size_limit(usize::MAX)
        .build()
        .expect("a translated glob")
}

/// [`path`] of `pattern` anchored at the start, compiled once per pattern.
pub(crate) fn path_prefix(pattern: &str) -> Arc<Regex> {
    static CACHE: LazyLock<Mutex<HashMap<String, Arc<Regex>>>> = LazyLock::new(Mutex::default);
    let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(found) = cache.get(pattern) {
        return Arc::clone(found);
    }
    let compiled = Arc::new(compile(&format!("^{}", path(pattern))));
    cache.insert(pattern.to_string(), Arc::clone(&compiled));
    compiled
}

/// A class that matches nothing; `regex` has no `(?!)`.
const NEVER: &str = "[^\\s\\S]";

fn translate(pattern: &str, star: &str, any: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut found = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        match c {
            '*' => {
                found.push_str(star);
                while chars.get(i) == Some(&'*') {
                    i += 1;
                }
            }
            '?' => found.push_str(any),
            '[' => {
                let mut j = i;
                if chars.get(j) == Some(&'!') {
                    j += 1;
                }
                if chars.get(j) == Some(&']') {
                    j += 1;
                }
                while j < chars.len() && chars[j] != ']' {
                    j += 1;
                }
                if j >= chars.len() {
                    found.push_str("\\[");
                    continue;
                }
                found.push_str(&set(&chars[i..j]));
                i = j + 1;
            }
            c => found.push_str(&regex::escape(&c.to_string())),
        }
    }
    found
}

/// The class of the set `stuff`, the characters between `[` and `]`, as `fnmatch` reads
/// it: `a-z` a range, a range that runs backwards dropped, an empty set matching nothing.
fn set(stuff: &[char]) -> String {
    let negated = stuff.first() == Some(&'!');
    let body = &stuff[usize::from(negated)..];
    let mut chunks: Vec<Vec<char>> = Vec::new();
    let mut start = 0;
    let mut k = 1;
    while let Some(at) = body[k.min(body.len())..]
        .iter()
        .position(|&c| c == '-')
        .map(|at| at + k)
    {
        chunks.push(body[start..at].to_vec());
        start = at + 1;
        k = at + 3;
        if k > body.len() {
            break;
        }
    }
    let rest = body[start.min(body.len())..].to_vec();
    match chunks.last_mut() {
        Some(last) if rest.is_empty() => last.push('-'),
        _ => chunks.push(rest),
    }
    let mut k = chunks.len() - 1;
    while k > 0 {
        let (before, after) = (&chunks[k - 1], &chunks[k]);
        if before.last() > after.first() {
            let mut merged = before[..before.len() - 1].to_vec();
            merged.extend_from_slice(&after[1..]);
            chunks[k - 1] = merged;
            chunks.remove(k);
        }
        k -= 1;
    }
    let escaped: Vec<String> = chunks
        .iter()
        .map(|chunk| {
            chunk
                .iter()
                .map(|&c| match c.is_ascii_punctuation() {
                    true => format!("\\{c}"),
                    false => c.to_string(),
                })
                .collect()
        })
        .collect();
    let body = escaped.join("-");
    if body.is_empty() {
        return if negated { ".".into() } else { NEVER.into() };
    }
    match negated {
        true => format!("[^{body}]"),
        false => format!("[{body}]"),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn a_path_prefix_compiles_once() {
        assert!(Arc::ptr_eq(&path_prefix("src/*"), &path_prefix("src/*")));
    }

    #[test]
    fn a_set_of_globs_compiles_once() {
        let excludes = Excludes::default();
        let globs = ["a*".to_string()];
        assert!(excludes.entry("ab", None, &globs));
        assert!(!excludes.entry("b", None, &globs));
        assert_eq!(excludes.sets(), 1);
        assert!(!excludes.entry("ab", None, &["c".to_string()]));
        assert_eq!(excludes.sets(), 2);
    }
}
