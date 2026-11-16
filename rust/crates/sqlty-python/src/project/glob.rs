//! Shell globs as Python's `fnmatch` and `glob` read them, translated to `regex`.

use regex::Regex;

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
        let regex = Regex::new(&format!("^(?s:{})$", translate(pattern, "[^/]*", "[^/]")))
            .expect("a translated glob");
        let hidden = pattern.starts_with('.');
        Self { regex, hidden }
    }

    pub fn matches(&self, name: &str) -> bool {
        (self.hidden || !name.starts_with('.')) && self.regex.is_match(name)
    }
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
    if body.is_empty() {
        return if negated { ".".into() } else { NEVER.into() };
    }
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
    if rest.is_empty() {
        chunks
            .last_mut()
            .expect("a chunk before a trailing '-'")
            .push('-');
    } else {
        chunks.push(rest);
    }
    let mut k = chunks.len() - 1;
    while k > 0 {
        let (before, after) = (&chunks[k - 1], &chunks[k]);
        if let (Some(&end), Some(&begin)) = (before.last(), after.first()) {
            if end > begin {
                let mut merged = before[..before.len() - 1].to_vec();
                merged.extend_from_slice(&after[1..]);
                chunks[k - 1] = merged;
                chunks.remove(k);
            }
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

    fn matches(pattern: &str, text: &str) -> bool {
        Regex::new(&format!("^{pattern}")).unwrap().is_match(text)
    }

    #[test]
    fn names_match_as_fnmatch_matches_them() {
        for (pattern, text, expected) in [
            ("*.py", "a.py", true),
            ("*.py", "dir/a.py", true),
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
            assert_eq!(
                matches(&fnmatch(pattern), text),
                expected,
                "{pattern} {text}"
            );
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
            assert_eq!(matches(&path(pattern), text), expected, "{pattern} {text}");
        }
    }
}
