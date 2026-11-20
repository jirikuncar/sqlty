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
        let mut items = self.shuffle(items.to_vec());
        items.truncate(self.int(low, high));
        items
    }

    pub fn shuffle<T>(&mut self, mut items: Vec<T>) -> Vec<T> {
        for i in (1..items.len()).rev() {
            items.swap(i, self.int(0, i));
        }
        items
    }
}

fn env(name: &str) -> Option<u64> {
    let value = std::env::var(name).ok().filter(|value| !value.is_empty())?;
    Some(
        value
            .parse()
            .unwrap_or_else(|_| panic!("{name} is a number")),
    )
}

/// Run `property` on `SQLTY_FUZZ_EXAMPLES` (100) distinct cases built by `case`, seeded by
/// `PROPTEST_RNG_SEED`, on a thread with a 64 MiB stack as the parsers recurse. A failure is
/// shrunk and printed with its case and seed.
pub fn check<T: Debug>(
    case: impl Fn(&mut Draw<'_>) -> T + Send,
    property: impl Fn(T) -> Result<(), TestCaseError> + Send,
) {
    let name = std::thread::current().name().unwrap_or("?").to_string();
    std::thread::scope(|scope| {
        let thread = std::thread::Builder::new().name(name).stack_size(64 << 20);
        if let Err(panic) = thread
            .spawn_scoped(scope, || run(case, property))
            .expect("a thread")
            .join()
        {
            std::panic::resume_unwind(panic);
        }
    });
}

#[derive(Default)]
struct Seen {
    passed: HashSet<String>,
    duplicates: usize,
    rejected: usize,
    failed: bool,
}

fn run<T: Debug>(
    case: impl Fn(&mut Draw<'_>) -> T,
    property: impl Fn(T) -> Result<(), TestCaseError>,
) {
    let seed = env("PROPTEST_RNG_SEED").unwrap_or_else(|| {
        std::time::UNIX_EPOCH
            .elapsed()
            .map_or(0, |t| t.as_nanos() as u64)
    });
    let cases = env("SQLTY_FUZZ_EXAMPLES").unwrap_or(100) as u32;
    let config = Config {
        cases,
        rng_seed: RngSeed::Fixed(seed),
        failure_persistence: None,
        max_shrink_iters: 4096,
        // Duplicates are rejected too: a space smaller than `cases` ends the run.
        max_global_rejects: cases.saturating_mul(20),
        ..Config::default()
    };
    let seen = RefCell::new(Seen::default());
    let started = Instant::now();
    let bytes = proptest::collection::vec(any::<u8>(), 0..1024);
    let result = TestRunner::new(config).run(&bytes, |bytes| {
        let case = case(&mut Draw(Unstructured::new(&bytes)));
        let shown = format!("{case:#?}");
        let mut seen = seen.borrow_mut();
        // Like Hypothesis, run each case once; shrinking may revisit them.
        if !seen.failed && seen.passed.contains(&shown) {
            seen.duplicates += 1;
            return Err(TestCaseError::reject("seen"));
        }
        match property(case) {
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
    let Seen {
        passed,
        duplicates,
        rejected,
        failed,
    } = seen.into_inner();
    let name = std::thread::current().name().unwrap_or("?").to_string();
    let seconds = started.elapsed().as_secs_f64();
    eprintln!(
        "{name}: {} distinct cases, {duplicates} duplicates, {rejected} rejected, {seconds:.2}s",
        passed.len()
    );
    match result {
        Err(TestError::Abort(_)) if !failed && duplicates > rejected => {}
        Err(TestError::Fail(reason, _)) => panic!("{reason}\nreplay with PROPTEST_RNG_SEED={seed}"),
        Err(error) => panic!("{error}\nreplay with PROPTEST_RNG_SEED={seed}"),
        Ok(()) => {}
    }
}
