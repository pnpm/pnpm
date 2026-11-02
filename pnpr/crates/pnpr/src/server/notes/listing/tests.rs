use super::{Candidates, MAX_CANDIDATES};
use std::collections::{BTreeMap, BTreeSet};

fn candidates(noted: &[&str], published: &[&str]) -> Candidates {
    Candidates {
        noted: noted
            .iter()
            .map(ToString::to_string)
            .collect::<BTreeSet<_>>(),
        published: published
            .iter()
            .map(|version| ((*version).to_string(), 1))
            .collect::<BTreeMap<_, _>>(),
    }
}

#[test]
fn candidates_are_ordered_newest_first_with_non_semver_last() {
    let candidates = candidates(&["1.0.0", "next", "10.0.0"], &["2.0.0", "1.0.0"]);
    let (newest, more) = candidates.newest();
    assert_eq!(newest, ["10.0.0", "2.0.0", "1.0.0", "next"]);
    assert!(!more);
}

#[test]
fn only_the_newest_candidates_are_kept() {
    let versions: Vec<String> = (0..MAX_CANDIDATES + 50)
        .map(|minor| format!("1.{minor}.0"))
        .collect();
    let noted: Vec<&str> = versions
        .iter()
        .map(String::as_str)
        .collect();
    let candidates = candidates(&noted, &[]);
    let (newest, more) = candidates.newest();
    assert!(more);
    assert_eq!(newest.len(), MAX_CANDIDATES);
    assert_eq!(newest[0], &format!("1.{}.0", MAX_CANDIDATES + 49));
    assert_eq!(newest[MAX_CANDIDATES - 1], "1.50.0");
}
