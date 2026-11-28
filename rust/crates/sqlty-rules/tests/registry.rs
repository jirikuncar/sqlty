//! The generated `Rule` registry and the `RuleSet` and `Severity` types around it.

use sqlty_rules::{Group, Rule, RuleSet, Severity, PAGES};

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
    assert_eq!(Rule::SQL.len(), 20);
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
