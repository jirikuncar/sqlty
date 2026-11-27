//! Runs the assertions of the mdtest suite a crate owns, every file on its own thread.
//!
//! `MDTEST_UPDATE=1` writes the actual values into the Markdown instead of failing, as
//! `pytest --update-mdtest` does; `MDTEST_FILTER=text` runs only the files whose path holds it.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::parser::{self, Assertion, Section, Unit};

/// A diagnostic as the `error` assertions see it: its Markdown line, rule, message and
/// `[start:end]` columns.
pub type Found = (usize, String, String, String);

pub trait Runner: Sync {
    /// Whether this crate checks `kind` assertions of `unit`.
    fn owns(&self, section: &Section, unit: &Unit, kind: &str) -> bool;

    /// The actual value of a `kind` assertion.
    fn compute(&self, section: &Section, unit: &Unit, kind: &str) -> Result<String, String>;

    /// The diagnostics `error` assertions are matched against.
    fn diagnostics(&self, _section: &Section, _unit: &Unit) -> Result<Vec<Found>, String> {
        Ok(Vec::new())
    }
}

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .unwrap()
}

/// Runs every owned assertion; panics with every failure.
pub fn run(runner: &dyn Runner) {
    let root = root();
    let filter = std::env::var("MDTEST_FILTER").unwrap_or_default();
    let update = std::env::var_os("MDTEST_UPDATE").is_some();
    let files: Vec<PathBuf> = parser::files(&root)
        .into_iter()
        .filter(|path| path.to_string_lossy().contains(&filter))
        .collect();
    let failures = Mutex::new(Vec::new());
    let next = Mutex::new(files.iter());
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| loop {
                let Some(path) = next.lock().unwrap().next() else {
                    break;
                };
                let found = run_file(runner, &root, path, update);
                failures.lock().unwrap().extend(found);
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    assert!(
        failures.is_empty(),
        "{} mdtest failure(s); MDTEST_UPDATE=1 writes the actual values\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// The failures of the file at `path`, reported relative to `root`; with `update`, none: the
/// actual values are written instead.
pub fn run_file(runner: &dyn Runner, root: &Path, path: &Path, update: bool) -> Vec<String> {
    let text = std::fs::read_to_string(path).unwrap();
    let relative = path.strip_prefix(root).unwrap_or(path);
    let mut updater = update.then(|| Updater::new(path));
    let mut failures = Vec::new();
    for section in parser::parse(relative, &text) {
        if section.directives.contains_key("skip") {
            continue;
        }
        let found = run_section(runner, &section, updater.as_mut());
        if !section.directives.contains_key("xfail") {
            failures.extend(found);
        } else if found.is_empty() && owns_any(runner, &section) {
            failures.push(format!(
                "{}:{}: `{}` passes: drop its mdtest:xfail",
                relative.display(),
                section.line,
                section.name()
            ));
        }
    }
    failures
}

fn owns_any(runner: &dyn Runner, section: &Section) -> bool {
    section.units.iter().any(|unit| {
        unit.assertions
            .iter()
            .any(|assertion| runner.owns(section, unit, &assertion.kind))
    })
}

fn run_section(
    runner: &dyn Runner,
    section: &Section,
    mut updater: Option<&mut Updater>,
) -> Vec<String> {
    let mut failures = Vec::new();
    let head = |assertion: &Assertion| {
        format!(
            "{}:{}: `{} {}:` failed",
            section.path.display(),
            assertion.line,
            assertion.comment,
            assertion.kind
        )
    };
    for unit in &section.units {
        let mut errors = Vec::new();
        for assertion in &unit.assertions {
            if !runner.owns(section, unit, &assertion.kind) {
                continue;
            }
            if assertion.kind == "error" {
                errors.push(assertion);
                continue;
            }
            match runner.compute(section, unit, &assertion.kind) {
                Ok(actual) if actual == assertion.expected => {}
                Ok(actual) => match updater.as_deref_mut() {
                    Some(updater) => updater.replace(assertion, &actual),
                    None => failures.push(format!(
                        "{}\n  expected: {}\n  actual:   {}",
                        head(assertion),
                        assertion.expected.replace('\n', "\n            "),
                        actual.replace('\n', "\n            ")
                    )),
                },
                Err(error) => failures.push(format!("{}\n  {error}", head(assertion))),
            }
        }
        if !errors.is_empty() {
            match runner.diagnostics(section, unit) {
                Ok(found) => failures.extend(match_errors(
                    section,
                    &errors,
                    &found,
                    updater.as_deref_mut(),
                    unit.lang,
                )),
                Err(error) => failures.push(format!("{}\n  {error}", head(errors[0]))),
            }
        }
    }
    failures
}

fn match_errors(
    section: &Section,
    assertions: &[&Assertion],
    found: &[Found],
    updater: Option<&mut Updater>,
    lang: &str,
) -> Vec<String> {
    let mut unmatched: Vec<&Assertion> = assertions
        .iter()
        .copied()
        .filter(|assertion| !assertion.expected.is_empty())
        .collect();
    let mut unexpected = Vec::new();
    for diagnostic in found {
        match unmatched
            .iter()
            .position(|assertion| matches(assertion, diagnostic))
        {
            Some(index) => {
                unmatched.remove(index);
            }
            None => unexpected.push(diagnostic),
        }
    }
    if unmatched.is_empty() && unexpected.is_empty() {
        return Vec::new();
    }
    if let Some(updater) = updater {
        updater.replace_errors(assertions, found, lang);
        return Vec::new();
    }
    let path = section.path.display();
    unmatched
        .iter()
        .map(|a| {
            format!(
                "{path}:{}: unmatched assertion `{} error{}: {}`",
                a.target.unwrap_or(a.line),
                a.comment,
                a.range.as_deref().unwrap_or(""),
                a.expected
            )
        })
        .chain(unexpected.iter().map(|(line, rule, message, columns)| {
            format!("{path}:{line}: unexpected diagnostic [{rule}] \"{message}\" at {columns}")
        }))
        .collect()
}

/// `[rule] "message"`, either part optional; the message is matched as a substring.
fn matches(assertion: &Assertion, (line, rule, message, columns): &Found) -> bool {
    if assertion.target != Some(*line)
        || assertion
            .range
            .as_ref()
            .is_some_and(|range| range != columns)
    {
        return false;
    }
    let expected = assertion.expected.as_str();
    let (want_rule, rest) = match expected.strip_prefix('[') {
        Some(rest) => match rest.split_once(']') {
            Some((name, rest)) => (Some(name), rest.trim_start()),
            None => return false,
        },
        None => (None, expected),
    };
    let want_message = match rest {
        "" => None,
        quoted if quoted.len() >= 2 && quoted.starts_with('"') && quoted.ends_with('"') => {
            Some(&quoted[1..quoted.len() - 1])
        }
        _ => return false,
    };
    want_rule.is_none_or(|want| want == rule)
        && want_message.is_none_or(|want| message.contains(want))
}

/// Rewrites assertion comments in place; edits shift the lines after them.
struct Updater {
    path: PathBuf,
    edits: Vec<(usize, usize, isize)>,
    done: std::collections::HashSet<(bool, usize)>,
}

impl Updater {
    fn new(path: &Path) -> Self {
        Updater {
            path: path.to_path_buf(),
            edits: Vec::new(),
            done: Default::default(),
        }
    }

    fn replace(&mut self, assertion: &Assertion, actual: &str) {
        if !self.done.insert((false, assertion.line)) {
            return;
        }
        let prefix = format!(
            "{}{} {}:",
            assertion.indent, assertion.comment, assertion.kind
        );
        let lines = actual
            .split('\n')
            .map(|value| join(&prefix, value))
            .collect();
        let start = assertion.line - 1;
        self.splice(start, start + assertion.span, lines);
    }

    fn replace_errors(&mut self, assertions: &[&Assertion], found: &[Found], lang: &str) {
        let comment = if lang == "sql" { "--" } else { "#" };
        if !self.done.insert((true, assertions[0].line)) {
            return;
        }
        for assertion in assertions {
            let start = assertion.line - 1;
            if assertion.trailing {
                let line = self.lines()[self.at(start)].clone();
                let code = line[..line.find(" # error").unwrap_or(line.len())]
                    .trim_end()
                    .to_string();
                self.splice(start, start + 1, vec![code]);
            } else {
                self.splice(start, start + 1, Vec::new());
            }
        }
        if found.is_empty() {
            let first = assertions.iter().min_by_key(|a| a.line).unwrap();
            self.splice(
                first.line - 1,
                first.line - 1,
                vec![format!("{}{comment} error:", first.indent)],
            );
        }
        let mut sorted = found.to_vec();
        sorted.sort();
        for (line, rule, message, _) in sorted {
            let target = self.lines()[self.at(line - 1)].clone();
            let indent = &target[..target.len() - target.trim_start().len()];
            self.splice(
                line - 1,
                line - 1,
                vec![format!("{indent}{comment} error: [{rule}] \"{message}\"")],
            );
        }
    }

    fn lines(&self) -> Vec<String> {
        // Split on `\n` only: `str::lines` would turn a CRLF into LF in lines it does not edit.
        let text = std::fs::read_to_string(&self.path).unwrap();
        text.strip_suffix('\n')
            .unwrap_or(&text)
            .split('\n')
            .map(str::to_string)
            .collect()
    }

    fn at(&self, line: usize) -> usize {
        let shift: isize = self
            .edits
            .iter()
            .filter(|(_, end, _)| *end <= line)
            .map(|(_, _, d)| d)
            .sum();
        (line as isize + shift) as usize
    }

    fn splice(&mut self, start: usize, end: usize, new: Vec<String>) {
        let mut lines = self.lines();
        let at = self.at(start);
        let delta = new.len() as isize - (end - start) as isize;
        // A CRLF file keeps CRLF on the lines written too.
        let eol = if lines.iter().any(|line| line.ends_with('\r')) {
            "\r"
        } else {
            ""
        };
        let new = new
            .into_iter()
            .map(|line| format!("{}{eol}", line.trim_end_matches('\r')));
        lines.splice(at..at + end - start, new);
        std::fs::write(
            &self.path,
            lines
                .iter()
                .map(|line| format!("{line}\n"))
                .collect::<String>(),
        )
        .unwrap();
        self.edits.push((start, end, delta));
    }
}

fn join(prefix: &str, value: &str) -> String {
    if value.is_empty() {
        prefix.to_string()
    } else {
        format!("{prefix} {value}")
    }
}
