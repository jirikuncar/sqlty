//! Which bindings of a name reach a use, and what an `isinstance` test on the way says of
//! it: one structured walk of the statements of a function (or of the module) for each
//! name, which reads only the statements that bind or test the name, and answers for any
//! number of uses.
//!
//! The walk follows the statements as they run: a binding replaces what the name held,
//! the branches of an `if`, `try` or `match` are walked apart and merged, a loop body
//! starts from what the name holds before the loop and from every binding in the loop,
//! and a branch that ends in `return`, `raise`, `continue` or `break` does not reach what
//! follows. The bindings themselves come from [`crate::scopes::Scan`], which records
//! every form that binds a name; this module adds only the order they run in.

use ruff_python_ast::{self as ast, BoolOp, Expr, Stmt, UnaryOp};
use ruff_text_size::Ranged;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use crate::scopes::{Binding, Origin};

/// What a name holds at a use.
pub struct Reach<'a> {
    /// The bindings that can be in effect; none when the use is not reached.
    pub origins: Vec<(Origin<'a>, u32)>,
    /// The class of an `isinstance` test every path to the use passed, when the name was
    /// not bound again since.
    pub narrowed: Option<&'a Expr>,
}

/// What a name holds at every place of a scope: one walk of its statements, asked about
/// any number of uses.
pub struct Flow<'a> {
    shape: Rc<Shape<'a>>,
    bindings: Vec<Binding<'a>>,
    /// Where the statements start and end.
    range: (u32, u32),
    /// The places a use can be in, in the order written: a plain statement, or the part
    /// of a compound one before its first block.
    places: Vec<Place<'a>>,
}

/// What the statements of a scope are whatever the name: one walk of them all, so that
/// the walk for a name reads only the statements that bind or test it.
#[derive(Default)]
pub struct Shape<'a> {
    /// The first statement of a block that does not run on to the next, by the start of
    /// the block: where it starts.
    stops: HashMap<u32, u32>,
    /// Where the statements nest too deep to follow.
    deep: Option<u32>,
    /// Where each name is the subject of an `isinstance` test, in order.
    tests: HashMap<&'a str, Vec<u32>>,
    /// What follows such a statement in its block, in order: it is not reached.
    dead: Vec<(u32, u32)>,
    /// The loops a `break` leaves, by their start: their `else` is then not the only
    /// way to what follows them.
    breaks: HashSet<u32>,
}

impl<'a> Shape<'a> {
    /// The shape of `body`, the statements of a scope.
    pub fn new(body: &'a [Stmt]) -> Self {
        let mut walk = Walk {
            bindings: &[],
            name: "",
            places: Vec::new(),
            quiet: true,
            closing: 0,
            looped: None,
            deep: false,
            marks: Vec::new(),
            loops: Vec::new(),
            shape: None,
            found: Some(Shape::default()),
        };
        walk.block(body, State::default(), 0);
        let mut shape = walk.found.unwrap_or_default();
        for tests in shape.tests.values_mut() {
            tests.sort_unstable();
            tests.dedup();
        }
        shape.dead.sort_unstable();
        shape.dead.dedup();
        shape
    }

    /// Whether offset `at` is not reached, or nested too deep to follow.
    fn unreached(&self, at: u32) -> bool {
        let after = self.dead.partition_point(|&(start, _)| start <= at);
        let dead = after.checked_sub(1).map(|last| self.dead[last]);
        dead.is_some_and(|(_, end)| at < end) || self.deep.is_some_and(|deep| at >= deep)
    }
}

/// A plain statement or the head of a compound one, and what the name holds before it;
/// or statements that neither bind nor test the name, which holds the same in all of them.
struct Place<'a> {
    start: u32,
    end: u32,
    before: State<'a>,
    /// The head of a `while`, read again after each turn: where its body ends.
    turns: Option<u32>,
    /// The outermost loop around it.
    looped: Option<(u32, u32)>,
}

impl<'a> Flow<'a> {
    /// The flow of `name` through `body`, the statements of the scope whose bindings of
    /// the name are `bindings` (see [`crate::scopes::Scan::bindings`]) and whose shape is
    /// `shape`.
    pub fn new(
        body: &'a [Stmt],
        shape: Rc<Shape<'a>>,
        bindings: Vec<Binding<'a>>,
        name: &str,
    ) -> Option<Self> {
        let (first, last) = (body.first()?, body.last()?);
        // The statements read: those a binding or a test of the name is in.
        let mut marks: Vec<u32> = bindings.iter().map(|binding| binding.site).collect();
        marks.extend(shape.tests.get(name).into_iter().flatten());
        marks.sort_unstable();
        let mut walk = Walk {
            bindings: &bindings,
            name,
            places: Vec::new(),
            quiet: false,
            closing: 0,
            looped: None,
            deep: false,
            marks,
            loops: Vec::new(),
            shape: Some(&shape),
            found: None,
        };
        // What is bound before the body: the parameters.
        let start = apply(
            &bindings,
            State::default(),
            0,
            first.start().to_u32(),
            u32::MAX,
        );
        walk.block(body, start, 0);
        let places = walk.places;
        Some(Flow {
            shape,
            bindings,
            range: (first.start().to_u32(), last.end().to_u32()),
            places,
        })
    }

    fn place(&self, at: u32) -> Option<&Place<'a>> {
        let after = self.places.partition_point(|place| place.start <= at);
        let place = self.places.get(after.checked_sub(1)?)?;
        (at < place.end).then_some(place)
    }

    /// What the name holds at offset `at`. `None` when `at` is not in the statements: a
    /// decorator or a default.
    pub fn at(&self, at: u32) -> Option<Reach<'a>> {
        if at < self.range.0 || at >= self.range.1 {
            return None;
        }
        work(1);
        // A place the walk did not reach is dead code, or nested too deep to follow.
        let reached = self.place(at).filter(|_| !self.shape.unreached(at));
        let state = reached.map_or_else(State::default, |place| {
            let bindings = &self.bindings;
            let state = apply(bindings, place.before.clone(), place.start, place.end, at);
            match place.turns {
                Some(end) => any(bindings, state, place.start, end),
                None => state,
            }
        });
        if state.lost || state.twice {
            return Some(Reach {
                origins: vec![(Origin::Unknown, at)],
                narrowed: None,
            });
        }
        Some(Reach {
            origins: state
                .live
                .iter()
                .map(|&index| (self.bindings[index].origin, self.bindings[index].site))
                .collect(),
            narrowed: state.narrowed,
        })
    }

    /// The outermost `for` or `while` around offset `at`, as its start and end.
    pub fn loop_around(&self, at: u32) -> Option<(u32, u32)> {
        self.place(at)?.looped
    }

    /// Whether a binding takes effect after `start`, or is written in `start..end`.
    pub fn binds_after(&self, start: u32, within: Option<(u32, u32)>) -> bool {
        work(1);
        let bindings = &self.bindings;
        let before = bindings.partition_point(|binding| binding.site < start);
        let pending = before
            .checked_sub(1)
            .is_some_and(|last| bindings[last].effect > start);
        before < bindings.len()
            || pending
            || within.is_some_and(|(from, to)| !written(bindings, from, to).is_empty())
    }
}

/// What `name` holds at offset `at` of `body`: [`Flow::at`] for one use.
#[cfg(test)]
fn reaching<'a>(
    body: &'a [Stmt],
    bindings: &[Binding<'a>],
    name: &str,
    at: u32,
) -> Option<Reach<'a>> {
    Flow::new(body, Rc::new(Shape::new(body)), bindings.to_vec(), name)?.at(at)
}

/// The most bindings followed at once: a name more branches than this bind is unknown.
const MAX_LIVE: usize = 64;

/// The bindings in effect, by index in the order written, and the class the name was
/// tested to be of.
#[derive(Clone, Default)]
struct State<'a> {
    live: Vec<usize>,
    narrowed: Option<&'a Expr>,
    /// The name was tested again after `narrowed`: it is of both classes, which is not
    /// followed.
    twice: bool,
    /// More than [`MAX_LIVE`] bindings reach: what the name holds is not followed.
    lost: bool,
}

impl<'a> State<'a> {
    /// The state where `class` is what a test says of the name. A second test of a name
    /// already narrowed is not followed.
    fn narrow(mut self, class: Option<&'a Expr>) -> Self {
        if class.is_some() {
            self.twice = self.narrowed.is_some();
            self.narrowed = class;
        }
        self
    }

    /// Either state: the bindings of both, and the test both passed.
    fn merge(mut self, other: &State<'a>) -> Self {
        work(self.live.len() + other.live.len());
        let mut live = Vec::with_capacity(self.live.len() + other.live.len());
        let (mut one, mut two) = (self.live.iter().peekable(), other.live.iter().peekable());
        loop {
            let next = match (one.peek(), two.peek()) {
                (Some(&&a), Some(&&b)) if a == b => {
                    two.next();
                    one.next();
                    a
                }
                (Some(&&a), Some(&&b)) if a < b => {
                    one.next();
                    a
                }
                (_, Some(&&b)) => {
                    two.next();
                    b
                }
                (Some(&&a), None) => {
                    one.next();
                    a
                }
                (None, None) => break,
            };
            live.push(next);
        }
        // What only one side tested is not known of the other: the name is then what
        // its bindings give it.
        let same = match (self.narrowed, other.narrowed) {
            (Some(one), Some(two)) => std::ptr::eq(one, two) && self.twice == other.twice,
            _ => false,
        };
        if !same {
            self.narrowed = None;
            self.twice = false;
        }
        self.lost |= other.lost || live.len() > MAX_LIVE;
        live.truncate(MAX_LIVE);
        self.live = live;
        self
    }
}

#[cfg(test)]
thread_local! {
    /// The statements walked and bindings merged by this thread, for the tests of cost.
    static WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Counts `steps` of work in tests; nothing otherwise.
#[cfg_attr(
    not(test),
    expect(clippy::missing_const_for_fn, reason = "it counts in tests")
)]
fn work(steps: usize) {
    #[cfg(test)]
    WORK.with(|work| work.set(work.get() + steps));
    #[cfg(not(test))]
    let _ = steps;
}

/// How a statement or a block ends.
enum Out<'a> {
    /// It runs on to the next statement.
    Next(State<'a>),
    /// It does not: `return`, `raise`, `continue`, `break`.
    Stop,
}

/// The branches that run on, merged; `Stop` when none does.
fn merged<'a>(outs: Vec<Out<'a>>) -> Out<'a> {
    let mut found: Option<State<'a>> = None;
    for out in outs {
        if let Out::Next(state) = out {
            found = Some(match found {
                Some(known) => known.merge(&state),
                None => state,
            });
        }
    }
    found.map_or(Out::Stop, Out::Next)
}

/// How deep statements nest before the walk gives up on what follows.
const MAX_DEPTH: u8 = 64;

/// How many `finally` blocks around one another are walked twice, once for the uses in
/// them and once for what follows them.
const MAX_CLOSING: u8 = 3;

/// The bindings written in `start..end`, by index: they are in the order written.
fn written(bindings: &[Binding<'_>], start: u32, end: u32) -> std::ops::Range<usize> {
    let from = bindings.partition_point(|binding| binding.site < start);
    let to = bindings.partition_point(|binding| binding.site < end);
    from..to.max(from)
}

/// `state` after the bindings written in `start..end` that are in effect by `limit`.
fn apply<'a>(
    bindings: &[Binding<'_>],
    state: State<'a>,
    start: u32,
    end: u32,
    limit: u32,
) -> State<'a> {
    // A binding is in effect no sooner than it is written. Those written by `limit` and
    // not yet in effect are the assignments `limit` is in the value of: more of them
    // than are followed leave the name unknown, so that no use reads every binding.
    let written = written(bindings, start, end.min(limit.saturating_add(1)));
    for (pending, index) in written.rev().enumerate() {
        work(1);
        if bindings[index].effect <= limit {
            return State {
                live: vec![index],
                ..State::default()
            };
        }
        if pending >= MAX_LIVE {
            return State {
                lost: true,
                ..State::default()
            };
        }
    }
    state
}

/// `state` or any binding written in `start..end`: what a loop body or an exception
/// handler starts from.
fn any<'a>(bindings: &[Binding<'_>], mut state: State<'a>, start: u32, end: u32) -> State<'a> {
    let written = written(bindings, start, end);
    if written.is_empty() {
        return state;
    }
    let entered = State {
        live: written.take(MAX_LIVE + 1).collect(),
        ..State::default()
    };
    state.narrowed = None;
    state.twice = false;
    state.merge(&entered)
}

struct Walk<'s, 'a> {
    bindings: &'s [Binding<'a>],
    name: &'s str,
    places: Vec<Place<'a>>,
    /// A second walk of a `finally`, for what follows it: its places are known.
    quiet: bool,
    /// The `finally` blocks around the statement.
    closing: u8,
    /// The outermost loop around the statement.
    looped: Option<(u32, u32)>,
    /// Statements nested too deep were met: what follows is not placed.
    deep: bool,
    /// Where the name is bound or tested, in order.
    marks: Vec<u32>,
    /// The loops around the statement, by their start, for the walk of no name.
    loops: Vec<u32>,
    /// The shape of the scope, for the walk of a name: the statements between two marks
    /// are not read.
    shape: Option<&'s Shape<'a>>,
    /// The shape being found, for the walk of no name.
    found: Option<Shape<'a>>,
}

impl<'a> Walk<'_, 'a> {
    fn apply(&self, state: State<'a>, start: u32, end: u32) -> State<'a> {
        apply(self.bindings, state, start, end, u32::MAX)
    }

    fn any(&self, state: State<'a>, start: u32, end: u32) -> State<'a> {
        any(self.bindings, state, start, end)
    }

    /// Records that a use in `start..end` reads the name as `before` leaves it.
    fn place(&mut self, start: u32, end: u32, before: &State<'a>, turns: Option<u32>) {
        if !self.quiet && !self.deep && start < end {
            work(1 + before.live.len());
            self.places.push(Place {
                start,
                end,
                before: before.clone(),
                turns,
                looped: self.looped,
            });
        }
    }

    /// What `test` says of the name when it holds and when it does not; nothing when the
    /// test itself binds the name (`isinstance(x, C) and (x := y)`).
    fn tested(&mut self, test: &'a Expr) -> (Option<&'a Expr>, Option<&'a Expr>) {
        if let Some(found) = &mut self.found {
            let mut names = Vec::new();
            subjects(test, &mut names);
            for name in names {
                let tests = found.tests.entry(name).or_default();
                tests.push(test.start().to_u32());
            }
        }
        let written = written(self.bindings, test.start().to_u32(), test.end().to_u32());
        match written.is_empty() {
            true => narrowing(test, self.name),
            false => (None, None),
        }
    }

    /// The part of a compound statement before its first block: `state` after it.
    fn header(&mut self, state: State<'a>, start: u32, body: &[Stmt]) -> State<'a> {
        let end = body.first().map_or(start, |first| first.start().to_u32());
        self.place(start, end, &state, None);
        self.apply(state, start, end)
    }

    /// The statement of `body` from `index` on that holds the next mark; `body.len()`
    /// for none.
    fn marked(&self, body: &[Stmt], index: usize) -> usize {
        let from = body[index].start().to_u32();
        let next = self.marks.partition_point(|&mark| mark < from);
        match self.marks.get(next) {
            Some(&mark) => body.partition_point(|stmt| stmt.end().to_u32() <= mark),
            None => body.len(),
        }
    }

    /// Statements `start..end` of the block that starts at `block`, which neither bind
    /// nor test the name: it holds `state` in all of them. `false` when one of them does
    /// not run on to the next.
    fn unmarked(
        &mut self,
        shape: &Shape<'a>,
        block: u32,
        state: &State<'a>,
        run: (u32, u32),
    ) -> bool {
        work(1);
        let (start, end) = run;
        // What a statement that stops leaves unreached is in the shape, for every name.
        self.place(start, end, state, None);
        if shape.deep.is_some_and(|deep| (start..end).contains(&deep)) {
            self.deep = true;
            return false;
        }
        let stops = shape.stops.get(&block);
        !stops.is_some_and(|stop| (start..end).contains(stop))
    }

    fn block(&mut self, body: &'a [Stmt], mut state: State<'a>, depth: u8) -> Out<'a> {
        let Some(first) = body.first() else {
            return Out::Next(state);
        };
        let block = first.start().to_u32();
        if depth > MAX_DEPTH {
            self.deep = true;
            if let Some(found) = &mut self.found {
                found.deep.get_or_insert(block);
            }
            return Out::Stop;
        }
        let mut index = 0;
        while let Some(stmt) = body.get(index) {
            if let Some(shape) = self.shape {
                let next = self.marked(body, index);
                if next > index {
                    let run = (stmt.start().to_u32(), body[next - 1].end().to_u32());
                    if !self.unmarked(shape, block, &state, run) {
                        return Out::Stop;
                    }
                    index = next;
                    continue;
                }
            }
            match self.stmt(stmt, state, depth) {
                Out::Next(next) => state = next,
                // What follows in the block is not reached, and not placed.
                Out::Stop => {
                    if let Some(found) = &mut self.found {
                        found.stops.insert(block, stmt.start().to_u32());
                        let rest = body.last().map_or(0, |last| last.end().to_u32());
                        found.dead.push((stmt.end().to_u32(), rest));
                    }
                    return Out::Stop;
                }
            }
            index += 1;
        }
        Out::Next(state)
    }

    fn stmt(&mut self, stmt: &'a Stmt, state: State<'a>, depth: u8) -> Out<'a> {
        work(1);
        let (start, end) = (stmt.start().to_u32(), stmt.end().to_u32());
        match stmt {
            Stmt::If(node) => {
                let state = self.header(state, start, &node.body);
                let (yes, no) = self.tested(&node.test);
                let mut outs = vec![self.block(&node.body, state.clone().narrow(yes), depth + 1)];
                let mut rest = state.narrow(no);
                let mut closed = false;
                for clause in &node.elif_else_clauses {
                    let branch = match &clause.test {
                        Some(test) => {
                            let after = self.header(rest, clause.start().to_u32(), &clause.body);
                            let (yes, no) = self.tested(test);
                            rest = after.clone().narrow(no);
                            after.narrow(yes)
                        }
                        None => {
                            closed = true;
                            rest.clone()
                        }
                    };
                    outs.push(self.block(&clause.body, branch, depth + 1));
                }
                if !closed {
                    outs.push(Out::Next(rest));
                }
                merged(outs)
            }
            Stmt::While(ast::StmtWhile { body, orelse, .. })
            | Stmt::For(ast::StmtFor { body, orelse, .. }) => {
                let head = body.first().map_or(start, |first| first.start().to_u32());
                let body_end = body.last().map_or(end, |last| last.end().to_u32());
                let outer = self.looped;
                self.looped = outer.or(Some((start, end)));
                // The iterable of a `for` is read once, before its target is bound; the
                // test of a `while` again after each turn.
                let turns = matches!(stmt, Stmt::While(_)).then_some(body_end);
                self.place(start, head, &state, turns);
                let bound = self.apply(state.clone(), start, head);
                let entry = self.any(bound, start, body_end);
                self.loops.push(start);
                let ran = self.block(body, entry.clone(), depth + 1);
                self.loops.pop();
                // What a `break` leaves the loop with: any binding of the loop, and no
                // test, as the breaks are not followed one by one.
                let left = state.merge(&entry);
                let after = match ran {
                    Out::Next(ran) => left.clone().merge(&ran),
                    Out::Stop => left.clone(),
                };
                self.looped = outer;
                let shape = self.found.as_ref().or(self.shape);
                let broken = shape.is_some_and(|shape| shape.breaks.contains(&start));
                // The `else` runs when the loop ends without a `break`; a `break` goes
                // past it.
                match (orelse.is_empty(), broken) {
                    (true, _) => Out::Next(after),
                    (false, false) => self.block(orelse, after, depth + 1),
                    (false, true) => {
                        let ended = self.block(orelse, after, depth + 1);
                        merged(vec![ended, Out::Next(left)])
                    }
                }
            }
            Stmt::With(node) => {
                let state = self.header(state, start, &node.body);
                self.block(&node.body, state, depth + 1)
            }
            Stmt::Try(node) => {
                let body_end = node.body.last().map_or(start, |last| last.end().to_u32());
                // An exception can leave the body after any of its bindings.
                let raised = self.any(state.clone(), start, body_end);
                let ran = self.block(&node.body, state, depth + 1);
                let mut outs = Vec::new();
                for handler in &node.handlers {
                    let ast::ExceptHandler::ExceptHandler(handler) = handler;
                    let caught =
                        self.header(raised.clone(), handler.start().to_u32(), &handler.body);
                    outs.push(self.block(&handler.body, caught, depth + 1));
                }
                outs.push(match ran {
                    Out::Next(ran) => self.block(&node.orelse, ran, depth + 1),
                    Out::Stop => Out::Stop,
                });
                let Some(first) = node.finalbody.first() else {
                    return merged(outs);
                };
                // `finally` runs however the rest ended: a use in it reads any binding
                // of the statement, what follows it reads what ran to its end.
                let closing = self.any(raised, start, first.start().to_u32());
                let Out::Next(ended) = merged(outs) else {
                    self.block(&node.finalbody, closing, depth + 1);
                    return Out::Stop;
                };
                if self.quiet {
                    return self.block(&node.finalbody, ended, depth + 1);
                }
                self.closing += 1;
                let closed = self.block(&node.finalbody, closing, depth + 1);
                let out = match self.closing > MAX_CLOSING {
                    // Too many around one another to walk each twice: what follows
                    // reads any binding too.
                    true => closed,
                    false => {
                        self.quiet = true;
                        let out = self.block(&node.finalbody, ended, depth + 1);
                        self.quiet = false;
                        out
                    }
                };
                self.closing -= 1;
                out
            }
            Stmt::Match(node) => {
                let first = node.cases.first().map_or(end, |case| case.start().to_u32());
                self.place(start, first, &state, None);
                let state = self.apply(state, start, first);
                let mut outs = vec![Out::Next(state.clone())];
                for case in &node.cases {
                    let captured = self.header(state.clone(), case.start().to_u32(), &case.body);
                    outs.push(self.block(&case.body, captured, depth + 1));
                }
                merged(outs)
            }
            _ => {
                self.place(start, end, &state, None);
                let state = self.apply(state, start, end);
                if let (Stmt::Break(_), Some(found), Some(&around)) =
                    (stmt, &mut self.found, self.loops.last())
                {
                    found.breaks.insert(around);
                }
                match stmt {
                    Stmt::Return(_) | Stmt::Raise(_) | Stmt::Continue(_) | Stmt::Break(_) => {
                        Out::Stop
                    }
                    Stmt::Assert(node) => Out::Next(state.narrow(self.tested(&node.test).0)),
                    _ => Out::Next(state),
                }
            }
        }
    }
}

/// The class `test` says `name` is an instance of when it holds, and when it does not:
/// `isinstance(name, Class)`, under `not`, and as the one such operand of an `and` (when
/// it holds) or of an `or` (when it does not). Two tests of the name in one `and` state an
/// intersection, which is not followed.
fn narrowing<'a>(test: &'a Expr, name: &str) -> (Option<&'a Expr>, Option<&'a Expr>) {
    match test {
        Expr::Call(call) => {
            let tested = matches!(call.func.as_ref(), Expr::Name(func) if func.id.as_str() == "isinstance")
                && call.arguments.keywords.is_empty();
            match &*call.arguments.args {
                [Expr::Name(subject), class] if tested && subject.id.as_str() == name => {
                    (Some(class), None)
                }
                _ => (None, None),
            }
        }
        Expr::UnaryOp(unary) if unary.op == UnaryOp::Not => {
            let (yes, no) = narrowing(&unary.operand, name);
            (no, yes)
        }
        Expr::BoolOp(both) => {
            let sides = both.values.iter().map(|value| narrowing(value, name));
            let only = |mut stated: Vec<&'a Expr>| match stated.len() {
                1 => stated.pop(),
                _ => None,
            };
            match both.op {
                BoolOp::And => (only(sides.filter_map(|(yes, _)| yes).collect()), None),
                BoolOp::Or => (None, only(sides.filter_map(|(_, no)| no).collect())),
            }
        }
        _ => (None, None),
    }
}

/// The names `test` may say the class of: the subjects [`narrowing`] reads.
fn subjects<'a>(test: &'a Expr, names: &mut Vec<&'a str>) {
    match test {
        Expr::Call(call) => {
            if let [Expr::Name(subject), _] = &*call.arguments.args {
                names.push(subject.id.as_str());
            }
        }
        Expr::UnaryOp(unary) => subjects(&unary.operand, names),
        Expr::BoolOp(both) => both.values.iter().for_each(|value| subjects(value, names)),
        _ => {}
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::scopes::{Node, Scan};
    use crate::types::symbols::Source;

    /// What `name` holds at each `name.use()` of the first function of `text`: the lines
    /// of the bindings in effect, then `narrowed` when a test narrows it.
    fn held(text: &str, name: &str) -> Vec<String> {
        let source = Source::new(text, (3, 14)).expect("parses");
        let scan = Scan::new(source.body(), |_| false);
        let scope = (0..scan.scopes.len())
            .find(|&scope| matches!(scan.scopes[scope], Node::Function(_)))
            .expect("a function");
        let Node::Function(function) = scan.scopes[scope] else {
            unreachable!()
        };
        let bindings = scan.bindings(scope, name).unwrap_or(&[]);
        let needle = format!("{name}.use()");
        let shape = Rc::new(Shape::new(&function.body));
        let flow = Flow::new(&function.body, shape, bindings.to_vec(), name).expect("a body");
        text.match_indices(&needle)
            .map(|(at, _)| {
                let at = u32::try_from(at).unwrap();
                // As the resolver asks of a use in a lambda or a generator expression.
                if let Some(start) = scan.deferred(scope, at) {
                    let _ = flow.binds_after(start, flow.loop_around(at));
                }
                let reach = flow.at(at).expect("in the body");
                let mut lines: Vec<String> = reach
                    .origins
                    .iter()
                    .map(|(_, site)| (text[..*site as usize].matches('\n').count() + 1).to_string())
                    .collect();
                lines.extend(reach.narrowed.map(|_| "narrowed".to_string()));
                lines.join(" ")
            })
            .collect()
    }

    #[test]
    fn a_binding_replaces_what_the_name_held() {
        let text = "def f(x):\n    x.use()\n    x = a\n    x.use()\n    x = b\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1", "3", "5"]);
    }

    #[test]
    fn branches_merge_and_a_branch_that_ends_does_not() {
        let text = "def f(x, c):\n    if c:\n        x = a\n    else:\n        x = b\n    x.use()\n    if c:\n        x = d\n        return\n    x.use()\n";
        assert_eq!(held(text, "x"), ["3 5", "3 5"]);
        let text = "def f(x, c):\n    if c:\n        x = a\n        raise E\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1"]);
    }

    #[test]
    fn a_loop_body_sees_the_bindings_of_the_loop() {
        let text = "def f(x, xs):\n    while xs:\n        x.use()\n        x = a\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1 4", "1 4"]);
        let text = "def f(xs):\n    for x in xs:\n        x.use()\n    x.use()\n";
        assert_eq!(held(text, "x"), ["2", "2"]);
    }

    #[test]
    fn a_break_goes_past_the_else_of_its_loop() {
        let text = "def f(x, xs):\n    for i in xs:\n        if i:\n            break\n    else:\n        x = a\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1 6"]);
        // The `else` ends the function: what follows is reached from the `break`.
        let text = "def f(x, xs):\n    while xs:\n        if x:\n            break\n    else:\n        return\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1"]);
        // A `break` of a loop in the loop leaves that one only.
        let text = "def f(x, xs):\n    for i in xs:\n        for j in i:\n            break\n    else:\n        return\n    x.use()\n";
        assert_eq!(held(text, "x"), [""]);
        let text = "def f(x, xs):\n    for i in xs:\n        if i:\n            break\n    else:\n        assert isinstance(x, C)\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1"]);
    }

    #[test]
    fn a_handler_sees_every_binding_of_its_try() {
        let text = "def f(x):\n    try:\n        x = a\n        g()\n        x = b\n    except E:\n        x.use()\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1 3 5", "1 3 5"]);
    }

    #[test]
    fn isinstance_narrows_until_the_name_is_bound_again() {
        let text = "def f(x, y):\n    if isinstance(x, C):\n        x.use()\n        x = y\n        x.use()\n    else:\n        x.use()\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1 narrowed", "4", "1", "1 4"]);
    }

    #[test]
    fn a_test_that_ends_the_other_branch_narrows_what_follows() {
        let text = "def f(x):\n    if not isinstance(x, C):\n        return\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1 narrowed"]);
        let text = "def f(x):\n    assert isinstance(x, C)\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1 narrowed"]);
        let text = "def f(x, c):\n    if c and isinstance(x, C):\n        x.use()\n    if c or isinstance(x, C):\n        x.use()\n";
        assert_eq!(held(text, "x"), ["1 narrowed", "1"]);
    }

    #[test]
    fn two_tests_of_the_name_or_a_binding_in_the_test_do_not_narrow() {
        let text = "def f(x, y):\n    if isinstance(x, C) and isinstance(x, D):\n        x.use()\n    if isinstance(x, C) and (x := y):\n        x.use()\n    if not (isinstance(x, C) or isinstance(x, D)):\n        return\n    x.use()\n    if isinstance(y, C) or isinstance(x):\n        x.use()\n";
        // `:=` is taken to have run: it leaves the name unknown either way.
        assert_eq!(held(text, "x"), ["1", "4", "4", "4"]);
    }

    #[test]
    fn an_elif_is_tested_after_the_tests_before_it_failed() {
        let text = "def f(x):\n    if not isinstance(x, C):\n        x.use()\n    elif c:\n        x.use()\n    else:\n        x.use()\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1", "1 narrowed", "1 narrowed", "1"]);
        let text = "def f(x, c):\n    if c:\n        return\n    elif isinstance(x, C):\n        pass\n    else:\n        raise E\n    x.use()\n    if c:\n        pass\n    elif (x := c):\n        x.use()\n";
        assert_eq!(held(text, "x"), ["1 narrowed", "11"]);
    }

    #[test]
    fn finally_runs_however_the_try_ended() {
        let text = "def f(x):\n    try:\n        x = a\n    except E:\n        x = b\n        return\n    else:\n        x = c\n    finally:\n        x.use()\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1 3 5 8", "8"]);
        let text =
            "def f(x):\n    try:\n        return\n    finally:\n        x.use()\n    x.use()\n";
        assert_eq!(held(text, "x"), ["1", ""]);
        let text = "def f(x):\n    try:\n        x.use()\n    except E as x:\n        x.use()\n";
        assert_eq!(held(text, "x"), ["1", "4"]);
    }

    #[test]
    fn a_case_binds_its_captures_and_a_with_its_target() {
        let text = "def f(x, v):\n    match x.use():\n        case [x]:\n            x.use()\n        case _ if x.use():\n            return\n    x.use()\n    with x.use() as x:\n        x.use()\n";
        assert_eq!(held(text, "x"), ["1", "3", "1", "1 3", "1 3", "8"]);
    }

    #[test]
    fn a_header_reads_the_name_before_it_binds_it() {
        let text = "def f(x, xs):\n    for x in x.use():\n        pass\n    else:\n        x.use()\n    while x.use():\n        x = a\n    if x.use():\n        pass\n    elif x.use():\n        pass\n";
        assert_eq!(held(text, "x"), ["1", "1 2", "1 2 7", "1 2 7", "1 2 7"]);
    }

    #[test]
    fn what_follows_a_statement_that_ends_the_block_is_not_reached() {
        let text =
            "def f(x):\n    while x:\n        break\n        x.use()\n    return\n    x.use()\n";
        assert_eq!(held(text, "x"), ["", ""]);
    }

    #[test]
    fn statements_nested_too_deep_are_not_followed() {
        let depth = usize::from(MAX_DEPTH) + 2;
        let mut text = "def f(x):\n".to_string();
        for level in 0..depth {
            text.push_str(&format!("{}if c:\n", "    ".repeat(level + 1)));
        }
        text.push_str(&format!("{}x.use()\n", "    ".repeat(depth + 1)));
        assert_eq!(held(&text, "x"), [""]);
    }

    #[test]
    fn many_bindings_are_walked_once() {
        let mut text = "def f(x):\n".to_string();
        for _ in 0..3000 {
            text.push_str("    x = a\n    x.use()\n");
        }
        let found = held(&text, "x");
        assert_eq!(found.len(), 3000);
        assert_eq!(found[2999], "6000");
    }

    #[test]
    fn a_name_tested_twice_is_not_followed() {
        let text = "def f(x):\n    if isinstance(x, C):\n        if isinstance(x, D):\n            x.use()\n        x.use()\n    assert isinstance(x, C)\n    if not isinstance(x, D):\n        return\n    x.use()\n    x = a\n    x.use()\n";
        // The use stands for a name that is not followed. After the inner `if`, only one
        // side tested the name twice: it is what its binding gives it.
        assert_eq!(held(text, "x"), ["4", "1", "9", "10"]);
    }

    #[test]
    fn more_bindings_than_are_followed_leave_the_name_unknown() {
        let mut text = "def f(x, c):\n".to_string();
        for _ in 0..MAX_LIVE {
            text.push_str("    if c:\n        x = a\n");
        }
        text.push_str("    x.use()\n    x = b\n    x.use()\n");
        let found = held(&text, "x");
        let unknown = (2 * MAX_LIVE + 2).to_string();
        let last = (2 * MAX_LIVE + 3).to_string();
        assert_eq!(
            found,
            [unknown, last],
            "the use itself stands for the unknown"
        );
    }

    /// The work of placing every use of `text`.
    fn cost(text: &str) -> usize {
        WORK.with(|work| work.set(0));
        let _ = held(text, "x");
        WORK.with(std::cell::Cell::get)
    }

    #[test]
    fn a_scope_is_walked_once_whatever_the_number_of_uses() {
        // Twice the statements and twice the uses: twice the work, whether the name is
        // bound in every statement or under a condition.
        let plain =
            |count: usize| "def f(x):\n".to_string() + &"    x = a\n    x.use()\n".repeat(count);
        let branched = |count: usize| {
            "def f(x, c):\n".to_string() + &"    if c:\n        x = a\n    x.use()\n".repeat(count)
        };
        let (small, large) = (cost(&plain(400)), cost(&plain(800)));
        assert!(small <= 11 * 400, "{small}");
        assert!(large <= 2 * small + 16, "{small} then {large}");
        // More branches than are followed bind the name: each place keeps MAX_LIVE at most.
        // The first MAX_LIVE branches cost less than the rest, hence the margin.
        let (small, large) = (cost(&branched(1500)), cost(&branched(3000)));
        assert!(large <= 2 * small + small / 10, "{small} then {large}");
        assert!(large <= 3000 * (10 + 4 * MAX_LIVE), "{large}");
    }

    #[test]
    fn a_use_reads_a_bounded_number_of_bindings() {
        // Twice the uses in a lambda in a loop, each after a binding: twice the work.
        let later = |count: usize| {
            "def f(x, xs):\n    for x in xs:\n".to_string()
                + &"        x = a\n        g(lambda: x.use())\n".repeat(count)
        };
        let (small, large) = (cost(&later(1500)), cost(&later(3000)));
        assert!(small <= 12 * 1500, "{small}");
        assert!(large <= 2 * small + 16, "{small} then {large}");
        // One statement that binds the name before every use, and one whose value holds
        // every use: a use reads the bindings around it, not all of the statement.
        let walrus = |count: usize| {
            format!(
                "def f(x):\n    y = ({})\n",
                "(x := a), x.use(), ".repeat(count)
            )
        };
        let (small, large) = (cost(&walrus(1500)), cost(&walrus(3000)));
        assert!(small <= 6 * 1500, "{small}");
        assert!(large <= 2 * small + 16, "{small} then {large}");
        let chained = |count: usize| {
            format!(
                "def f(x):\n    {}({})\n",
                "x = ".repeat(count),
                "x.use(), ".repeat(count)
            )
        };
        let (small, large) = (cost(&chained(1500)), cost(&chained(3000)));
        assert!(large <= 2 * small + 16, "{small} then {large}");
        assert!(large <= 3000 * (4 + MAX_LIVE), "{large}");
        let unknown = held(&chained(MAX_LIVE + 2), "x");
        assert_eq!(unknown[0], "2", "the use itself stands for the unknown");
    }

    #[test]
    fn a_name_is_walked_through_the_statements_that_bind_or_test_it() {
        // Twice the names, each bound and used once among the statements of the others:
        // twice the work for them all, not four times.
        let all = |count: usize| {
            let mut text = "def f(c):\n".to_string();
            for at in 0..count {
                text.push_str(&format!("    if c:\n        x{at} = a\n    x{at}.use()\n"));
            }
            WORK.with(|work| work.set(0));
            let source = Source::new(text.as_str(), (3, 14)).expect("parses");
            let scan = Scan::new(source.body(), |_| false);
            let Node::Function(function) = scan.scopes[1] else {
                panic!("a function")
            };
            let shape = Rc::new(Shape::new(&function.body));
            for at in 0..count {
                let name = format!("x{at}");
                let bindings = scan.bindings(1, &name).expect("bound").to_vec();
                let flow = Flow::new(&function.body, Rc::clone(&shape), bindings, &name);
                let used = text.find(&format!("{name}.use()")).expect("used");
                let reach = flow.expect("a body").at(u32::try_from(used).unwrap());
                assert_eq!(reach.expect("in the body").origins.len(), 1);
            }
            WORK.with(std::cell::Cell::get)
        };
        let (small, large) = (all(400), all(800));
        assert!(small <= 16 * 400, "{small}");
        assert!(large <= 2 * small + 16, "{small} then {large}");
    }

    #[test]
    fn a_finally_in_a_finally_is_not_walked_twice_at_every_depth() {
        let nested = |depth: usize| {
            let mut text = "def f(x):\n".to_string();
            for level in 0..depth {
                let indent = "    ".repeat(level + 1);
                text.push_str(&format!(
                    "{indent}try:\n{indent}    x = a\n{indent}finally:\n"
                ));
            }
            text.push_str(&format!("{}x.use()\n", "    ".repeat(depth + 1)));
            text
        };
        let (small, large) = (cost(&nested(10)), cost(&nested(20)));
        assert!(large <= 4 * small, "{small} then {large}");
    }

    #[test]
    fn a_use_outside_the_body_is_not_placed() {
        let source = Source::new("def f(x=y.use()):\n    pass\n", (3, 14)).unwrap();
        let scan = Scan::new(source.body(), |_| false);
        let Node::Function(function) = scan.scopes[1] else {
            panic!("a function")
        };
        assert!(reaching(&function.body, &[], "y", 9).is_none());
    }
}
