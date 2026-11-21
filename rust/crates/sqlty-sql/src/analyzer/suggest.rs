//! The column a misspelled name most likely meant, for `unknown-column`'s hint and fix.

/// The candidate closest to `name`, ignoring case, when it is close: an edit distance of at
/// most a third of the longer name, or one name inside the other (`username`, `name`) when
/// the shorter has four characters. `None` on a tie, the same name of two tables included,
/// so a guess is never offered as a fix.
pub(super) fn closest<'c>(name: &str, candidates: &[&'c str]) -> Option<&'c str> {
    let name = name.to_lowercase();
    let mut best: Option<(usize, &str)> = None;
    let mut tied = false;
    for &candidate in candidates {
        let lower = candidate.to_lowercase();
        let distance = distance(&name, &lower);
        let (short, long) = if name.len() < lower.len() {
            (&name, &lower)
        } else {
            (&lower, &name)
        };
        let contained = short.chars().count() >= 4 && long.contains(short.as_str());
        if 3 * distance > long.chars().count() && !contained {
            continue;
        }
        match best {
            Some((least, _)) if distance == least => tied = true,
            Some((least, _)) if distance > least => {}
            _ => (best, tied) = (Some((distance, candidate)), false),
        }
    }
    best.filter(|_| !tied).map(|(_, candidate)| candidate)
}

/// The optimal string alignment distance: insertions, deletions, substitutions and swaps of
/// adjacent characters, the usual typos.
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut rows = vec![(0..=b.len()).collect::<Vec<usize>>()];
    for i in 1..=a.len() {
        let mut row = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            row[j] = (rows[i - 1][j] + 1)
                .min(row[j - 1] + 1)
                .min(rows[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                row[j] = row[j].min(rows[i - 2][j - 2] + 1);
            }
        }
        rows.push(row);
    }
    rows[a.len()][b.len()]
}
