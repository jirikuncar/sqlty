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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_holds_what_was_added() {
        let mut set = RuleSet::EMPTY;
        assert!(set.is_empty());
        set.insert(Rule::ChasmJoin);
        set.insert(Rule::InternalError);
        assert!(set.contains(Rule::ChasmJoin));
        assert!(!set.contains(Rule::SelectStar));
        assert!(!set.is_empty());
        set.remove(Rule::ChasmJoin);
        assert!(!set.contains(Rule::ChasmJoin));
        assert!(set.contains(Rule::InternalError));
    }

    #[test]
    fn sets_combine() {
        let a = RuleSet::EMPTY.with(Rule::SelectStar);
        let b = RuleSet::EMPTY.with(Rule::ChasmJoin);
        assert!(!a.intersects(b));
        let both = a.union(b);
        assert!(both.intersects(a) && both.intersects(b));
        assert_eq!(
            both.iter().collect::<Vec<_>>(),
            [Rule::SelectStar, Rule::ChasmJoin]
        );
        assert_eq!(
            both,
            [Rule::ChasmJoin, Rule::SelectStar].into_iter().collect()
        );
    }

    #[test]
    fn every_rule_has_its_own_bit() {
        let all: RuleSet = Rule::ALL.into_iter().collect();
        assert_eq!(all.iter().count(), Rule::COUNT);
        for (i, rule) in Rule::ALL.into_iter().enumerate() {
            assert_eq!(rule.index(), i);
        }
    }

    #[test]
    fn names_parse_back() {
        for rule in Rule::ALL {
            assert_eq!(Rule::parse(rule.name()), Some(rule));
        }
        assert_eq!(Rule::parse("no-such-rule"), None);
    }

    #[test]
    fn a_rule_page_exists_for_every_rule() {
        assert_eq!(PAGES.len(), Rule::COUNT);
        for rule in Rule::ALL {
            let (_, text) = PAGES
                .iter()
                .find(|(file, _)| *file == format!("{}.md", rule.name()))
                .expect("a page");
            assert!(text.starts_with(&format!("# {}\n", rule.name())));
        }
        assert!(PAGES.iter().all(|(_, text)| !text.starts_with("---")));
    }

    #[test]
    fn the_registry_reads_the_pages() {
        assert_eq!(Rule::ChasmJoin.default_severity(), Severity::Warn);
        assert_eq!(Rule::ChasmJoin.group(), Group::Names);
        assert!(Rule::ChasmJoin.is_sql());
        assert!(!Rule::ChasmJoin.fixable());
        assert!(Rule::SelectStar.fixable());
        assert!(!Rule::SelectStar.is_sql());
        assert!(Rule::SQL.iter().all(|rule| rule.is_sql()));
        assert_eq!(Rule::SQL.len(), 21);
        assert!(Rule::ALL.iter().all(|rule| !rule.summary().is_empty()));
    }

    #[test]
    fn severities_and_groups_have_names() {
        for (name, severity) in [
            ("error", Severity::Error),
            ("warn", Severity::Warn),
            ("ignore", Severity::Ignore),
        ] {
            assert_eq!(Severity::parse(name), Some(severity));
            assert_eq!(severity.as_str(), name);
        }
        assert_eq!(Severity::parse("loud"), None);
        let names: Vec<_> = Rule::ALL.iter().map(|rule| rule.group().name()).collect();
        for group in [
            "Row type",
            "Query",
            "Names",
            "Rewrite",
            "Params",
            "Migration",
            "Suppression",
            "Internal",
        ] {
            assert!(names.contains(&group));
        }
    }
}
