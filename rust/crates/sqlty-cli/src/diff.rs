//! `--diff`: the unified diff of Python's `difflib.unified_diff`, hunk for hunk.

use std::collections::HashMap;

/// The unified diff of `old` and `new`, both named `path`, with three lines of context.
pub fn unified(path: &str, old: &str, new: &str) -> String {
    let (a, b) = (lines(old), lines(new));
    let mut out = String::new();
    for group in grouped(&opcodes(&a, &b), 3) {
        if out.is_empty() {
            out.push_str(&format!("--- {path}\n+++ {path}\n"));
        }
        let (first, last) = (group[0], group[group.len() - 1]);
        out.push_str(&format!(
            "@@ -{} +{} @@\n",
            range(first.1, last.2),
            range(first.3, last.4)
        ));
        for (tag, i1, i2, j1, j2) in group {
            if tag == Tag::Equal {
                a[i1..i2]
                    .iter()
                    .for_each(|line| out.push_str(&format!(" {line}")));
                continue;
            }
            if matches!(tag, Tag::Replace | Tag::Delete) {
                a[i1..i2]
                    .iter()
                    .for_each(|line| out.push_str(&format!("-{line}")));
            }
            if matches!(tag, Tag::Replace | Tag::Insert) {
                b[j1..j2]
                    .iter()
                    .for_each(|line| out.push_str(&format!("+{line}")));
            }
        }
    }
    out
}

/// `text.splitlines(keepends=True)`.
fn lines(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((at, char)) = chars.next() {
        let end = match char {
            '\r' if chars.peek().is_some_and(|(_, next)| *next == '\n') => {
                chars.next();
                at + 2
            }
            '\n' | '\r' | '\x0b' | '\x0c' | '\x1c' | '\x1d' | '\x1e' | '\u{85}' | '\u{2028}'
            | '\u{2029}' => at + char.len_utf8(),
            _ => continue,
        };
        found.push(&text[start..end]);
        start = end;
    }
    if start < text.len() {
        found.push(&text[start..]);
    }
    found
}

fn range(start: usize, stop: usize) -> String {
    let length = stop - start;
    match length {
        1 => format!("{}", start + 1),
        0 => format!("{start},0"),
        _ => format!("{},{length}", start + 1),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tag {
    Replace,
    Delete,
    Insert,
    Equal,
}

type Opcode = (Tag, usize, usize, usize, usize);

/// `SequenceMatcher(None, a, b).get_opcodes()`, its autojunk heuristic included.
fn opcodes(a: &[&str], b: &[&str]) -> Vec<Opcode> {
    let mut b2j: HashMap<&str, Vec<usize>> = HashMap::new();
    for (j, line) in b.iter().enumerate() {
        b2j.entry(line).or_default().push(j);
    }
    if b.len() >= 200 {
        let most = b.len() / 100 + 1;
        b2j.retain(|_, found| found.len() <= most);
    }
    let longest = |alo: usize, ahi: usize, blo: usize, bhi: usize| {
        let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0);
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        for (i, line) in a.iter().enumerate().take(ahi).skip(alo) {
            let mut next = HashMap::new();
            for &j in b2j.get(line).map_or(&[][..], Vec::as_slice) {
                if j < blo {
                    continue;
                }
                if j >= bhi {
                    break;
                }
                let k = j
                    .checked_sub(1)
                    .and_then(|before| j2len.get(&before))
                    .copied()
                    .unwrap_or(0)
                    + 1;
                next.insert(j, k);
                if k > bestsize {
                    (besti, bestj, bestsize) = (i + 1 - k, j + 1 - k, k);
                }
            }
            j2len = next;
        }
        while besti > alo && bestj > blo && a[besti - 1] == b[bestj - 1] {
            (besti, bestj, bestsize) = (besti - 1, bestj - 1, bestsize + 1);
        }
        while besti + bestsize < ahi
            && bestj + bestsize < bhi
            && a[besti + bestsize] == b[bestj + bestsize]
        {
            bestsize += 1;
        }
        (besti, bestj, bestsize)
    };
    let mut queue = vec![(0, a.len(), 0, b.len())];
    let mut blocks = Vec::new();
    while let Some((alo, ahi, blo, bhi)) = queue.pop() {
        let (i, j, k) = longest(alo, ahi, blo, bhi);
        if k > 0 {
            blocks.push((i, j, k));
            if alo < i && blo < j {
                queue.push((alo, i, blo, j));
            }
            if i + k < ahi && j + k < bhi {
                queue.push((i + k, ahi, j + k, bhi));
            }
        }
    }
    blocks.sort_unstable();
    let mut merged = Vec::new();
    let (mut i1, mut j1, mut k1) = (0, 0, 0);
    for (i2, j2, k2) in blocks {
        if i1 + k1 == i2 && j1 + k1 == j2 {
            k1 += k2;
        } else {
            if k1 > 0 {
                merged.push((i1, j1, k1));
            }
            (i1, j1, k1) = (i2, j2, k2);
        }
    }
    if k1 > 0 {
        merged.push((i1, j1, k1));
    }
    merged.push((a.len(), b.len(), 0));
    let (mut i, mut j) = (0, 0);
    let mut codes = Vec::new();
    for (ai, bj, size) in merged {
        let tag = match (i < ai, j < bj) {
            (true, true) => Some(Tag::Replace),
            (true, false) => Some(Tag::Delete),
            (false, true) => Some(Tag::Insert),
            (false, false) => None,
        };
        if let Some(tag) = tag {
            codes.push((tag, i, ai, j, bj));
        }
        (i, j) = (ai + size, bj + size);
        if size > 0 {
            codes.push((Tag::Equal, ai, i, bj, j));
        }
    }
    codes
}

/// `get_grouped_opcodes(n)`: the hunks, each with `n` lines of context.
fn grouped(codes: &[Opcode], n: usize) -> Vec<Vec<Opcode>> {
    let mut codes = codes.to_vec();
    if codes.is_empty() {
        codes.push((Tag::Equal, 0, 1, 0, 1));
    }
    if let Some((Tag::Equal, i1, i2, j1, j2)) = codes.first().copied() {
        codes[0] = (
            Tag::Equal,
            i1.max(i2.saturating_sub(n)),
            i2,
            j1.max(j2.saturating_sub(n)),
            j2,
        );
    }
    if let Some((Tag::Equal, i1, i2, j1, j2)) = codes.last().copied() {
        let last = codes.len() - 1;
        codes[last] = (Tag::Equal, i1, i2.min(i1 + n), j1, j2.min(j1 + n));
    }
    let mut groups = Vec::new();
    let mut group = Vec::new();
    for (tag, mut i1, i2, mut j1, j2) in codes {
        if tag == Tag::Equal && i2 - i1 > 2 * n {
            group.push((tag, i1, i2.min(i1 + n), j1, j2.min(j1 + n)));
            groups.push(std::mem::take(&mut group));
            (i1, j1) = (i1.max(i2.saturating_sub(n)), j1.max(j2.saturating_sub(n)));
        }
        group.push((tag, i1, i2, j1, j2));
    }
    if !group.is_empty() && !(group.len() == 1 && group[0].0 == Tag::Equal) {
        groups.push(group);
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hunks_as_difflib_writes_them() {
        let old = "a\nb\nc\nd\ne\nf\ng\nh\ni\nj\n";
        let new = "a\nB\nc\nd\ne\nf\ng\nh\ni\nJ\nk";
        let expected = "--- q.py\n+++ q.py\n@@ -1,5 +1,5 @@\n a\n-b\n+B\n c\n d\n e\n@@ -7,4 +7,5 @@\n g\n h\n i\n-j\n+J\n+k";
        assert_eq!(unified("q.py", old, new), expected);
        assert_eq!(
            unified("q.py", "x\n", ""),
            "--- q.py\n+++ q.py\n@@ -1 +0,0 @@\n-x\n"
        );
        assert_eq!(unified("q.py", "same\n", "same\n"), "");
    }
}
