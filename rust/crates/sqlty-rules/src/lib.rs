//! The lint rules of sqlty: their names, severities, groups and the rule set type.
//!
//! The metadata of a rule is written once, in the front matter of its page
//! `docs/reference/rules/<name>.md`; `build.rs` generates [`Rule`] and its tables from the pages.

include!(concat!(env!("OUT_DIR"), "/rules.rs"));

/// How loudly a rule reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// Reported as an error: the check fails.
    Error,
    /// Reported as a warning.
    Warn,
    /// Not reported.
    Ignore,
}

impl Severity {
    /// The severity called `name` (`error`, `warn` or `ignore`), if there is one.
    pub fn parse(name: &str) -> Option<Severity> {
        match name {
            "error" => Some(Severity::Error),
            "warn" => Some(Severity::Warn),
            "ignore" => Some(Severity::Ignore),
            _ => None,
        }
    }

    /// The name [`Severity::parse`] reads back: `error`, `warn` or `ignore`.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warn => "warn",
            Severity::Ignore => "ignore",
        }
    }
}

/// The group of a rule in the rules table of the documentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    /// Checks of a row type against its query.
    RowType,
    /// Checks of a query.
    Query,
    /// Names, functions and types a query cannot resolve.
    Names,
    /// Rules of `sqlty rewrite` and `sqlty check --fix`.
    Rewrite,
    /// Checks of the parameters of a query.
    Params,
    /// What a migration locks, as `sqlty breaking` reports it.
    Migration,
    /// Rules about `# sqlty: ignore` comments.
    Suppression,
    /// Failures of sqlty itself.
    Internal,
}

impl Group {
    /// The group as the documentation spells it: `Row type`, `Query`, `Names` and so on.
    pub fn name(self) -> &'static str {
        match self {
            Group::RowType => "Row type",
            Group::Query => "Query",
            Group::Names => "Names",
            Group::Rewrite => "Rewrite",
            Group::Params => "Params",
            Group::Migration => "Migration",
            Group::Suppression => "Suppression",
            Group::Internal => "Internal",
        }
    }
}

impl Rule {
    /// The position of the rule in [`Rule::ALL`], the bit it has in a [`RuleSet`].
    pub fn index(self) -> usize {
        self as usize
    }
}

/// A set of rules, one bit each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RuleSet(u128);

impl RuleSet {
    /// The set with no rule.
    pub const EMPTY: RuleSet = RuleSet(0);

    /// The set and `rule`.
    pub const fn with(self, rule: Rule) -> Self {
        RuleSet(self.0 | (1 << rule as u8))
    }

    /// Add `rule`.
    pub fn insert(&mut self, rule: Rule) {
        *self = self.with(rule);
    }

    /// Remove `rule`.
    pub fn remove(&mut self, rule: Rule) {
        self.0 &= !(1 << rule as u8);
    }

    /// Whether `rule` is in the set.
    pub fn contains(self, rule: Rule) -> bool {
        (self.0 >> rule as u8) & 1 == 1
    }

    /// The rules of both sets.
    pub fn union(self, other: RuleSet) -> RuleSet {
        RuleSet(self.0 | other.0)
    }

    /// Whether the sets share a rule.
    pub fn intersects(self, other: RuleSet) -> bool {
        self.0 & other.0 != 0
    }

    /// Whether the set has no rule.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The rules of the set, in the order of [`Rule::ALL`].
    pub fn iter(self) -> impl Iterator<Item = Rule> {
        Rule::ALL
            .into_iter()
            .filter(move |rule| self.contains(*rule))
    }
}

impl FromIterator<Rule> for RuleSet {
    fn from_iter<I: IntoIterator<Item = Rule>>(rules: I) -> Self {
        rules.into_iter().fold(RuleSet::EMPTY, RuleSet::with)
    }
}
