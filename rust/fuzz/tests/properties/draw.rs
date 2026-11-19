//! Choices drawn from a byte string, so a failing case shrinks with its bytes: proptest
//! drops and lowers bytes, and every draw falls back to its first choice once they run out.

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt::Debug;
use std::time::Instant;

use arbitrary::Unstructured;
use proptest::prelude::*;
use proptest::test_runner::{Config, RngSeed, TestCaseError, TestError, TestRunner};

pub struct Draw<'a>(Unstructured<'a>);

impl Draw<'_> {
    pub fn int(&mut self, low: usize, high: usize) -> usize {
        self.0.int_in_range(low..=high).unwrap_or(low)
    }

    pub fn bool(&mut self) -> bool {
        self.int(0, 1) == 1
    }

    pub fn pick<T: Clone>(&mut self, items: &[T]) -> T {
        items[self.int(0, items.len() - 1)].clone()
    }

    pub fn list<T>(
        &mut self,
        low: usize,
        high: usize,
        mut item: impl FnMut(&mut Self) -> T,
    ) -> Vec<T> {
        (0..self.int(low, high)).map(|_| item(self)).collect()
    }

    /// `low` to `high` distinct items of `items`, in random order.
    pub fn subset<T: Clone>(&mut self, items: &[T], low: usize, high: usize) -> Vec<T> {
        let mut left: Vec<T> = items.to_vec();
        let count = self.int(low, high.min(items.len()));
        (0..count)
            .map(|_| left.remove(self.int(0, left.len() - 1)))
            .collect()
    }

    pub fn shuffle<T>(&mut self, mut items: Vec<T>) -> Vec<T> {
        for i in (1..items.len()).rev() {
            items.swap(i, self.int(0, i));
        }
        items
    }
}

/// How many cases each property runs: `SQLTY_FUZZ_EXAMPLES`, 100 by default.
fn examples() -> u32 {
    match std::env::var("SQLTY_FUZZ_EXAMPLES")
        .unwrap_or_default()
        .as_str()
    {
        "" => 100,
        n => n.parse().expect("SQLTY_FUZZ_EXAMPLES is a number"),
    }
}

/// The seed of the run: `PROPTEST_RNG_SEED`, else a new one.
fn seed() -> u64 {
    match std::env::var("PROPTEST_RNG_SEED")
        .unwrap_or_default()
        .as_str()
    {
        "" => std::time::SystemTime::UNIX_EPOCH
            .elapsed()
            .map_or(0, |time| time.as_nanos() as u64),
        seed => seed.parse().expect("PROPTEST_RNG_SEED is a number"),
    }
}

/// Run `property` on `examples()` cases built by `case`; a failure is shrunk and printed with
/// its case and the seed that replays it.
///
/// The cases run on a thread with a 64 MiB stack, as the CLI parses: the parsers recurse.
pub fn check<T: Debug>(
    case: impl Fn(&mut Draw<'_>) -> T + Send,
    property: impl Fn(T) -> Result<(), TestCaseError> + Send,
) {
    let name = std::thread::current().name().unwrap_or("?").to_string();
    std::thread::scope(|scope| {
        let runner = std::thread::Builder::new().name(name).stack_size(64 << 20);
        let cases = runner
            .spawn_scoped(scope, || run(case, property))
            .expect("a thread");
        if let Err(panic) = cases.join() {
            std::panic::resume_unwind(panic);
        }
    });
}

fn run<T: Debug>(
    case: impl Fn(&mut Draw<'_>) -> T,
    property: impl Fn(T) -> Result<(), TestCaseError>,
) {
    let seed = seed();
    let cases = examples();
    let config = Config {
        cases,
        rng_seed: RngSeed::Fixed(seed),
        failure_persistence: None,
        max_shrink_iters: 4096,
        // Duplicates are rejected too: a space smaller than `cases` ends the run.
        max_global_rejects: cases.saturating_mul(20),
        ..Config::default()
    };
    let mut runner = TestRunner::new(config);
    let bytes = proptest::collection::vec(any::<u8>(), 0..1024);
    let seen = RefCell::new(Seen::default());
    let started = Instant::now();
    let result = runner.run(&bytes, |bytes| {
        let case = case(&mut Draw(Unstructured::new(&bytes)));
        let shown = format!("{case:#?}");
        // Like Hypothesis, run each case once; shrinking may revisit them.
        if !seen.borrow().failed && seen.borrow().passed.contains(&shown) {
            seen.borrow_mut().duplicates += 1;
            return Err(TestCaseError::reject("seen"));
        }
        let outcome = property(case);
        let mut seen = seen.borrow_mut();
        match outcome {
            Ok(()) => {
                seen.passed.insert(shown);
                Ok(())
            }
            Err(TestCaseError::Fail(reason)) => {
                seen.failed = true;
                Err(TestCaseError::fail(format!("{reason}\ncase: {shown}")))
            }
            Err(reject) => {
                seen.rejected += 1;
                Err(reject)
            }
        }
    });
    let seen = seen.into_inner();
    if std::env::var_os("SQLTY_FUZZ_STATS").is_some() {
        let name = std::thread::current().name().unwrap_or("?").to_string();
        eprintln!(
            "stats {name}: {} distinct passed, {} duplicates, {} rejected, {:.2}s",
            seen.passed.len(),
            seen.duplicates,
            seen.rejected,
            started.elapsed().as_secs_f64()
        );
    }
    match result {
        Err(TestError::Fail(reason, _)) => panic!("{reason}\nreplay with PROPTEST_RNG_SEED={seed}"),
        Err(TestError::Abort(_)) if !seen.failed && seen.duplicates > seen.rejected => {}
        Err(error) => panic!("{error}\nreplay with PROPTEST_RNG_SEED={seed}"),
        Ok(()) => {}
    }
}

#[derive(Default)]
struct Seen {
    passed: HashSet<String>,
    duplicates: usize,
    rejected: usize,
    failed: bool,
}
