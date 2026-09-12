//! Unit tests for reading a walk's subtree missing-peer summaries.

use super::{MissingSummary, index_missing_names};
use rustc_hash::FxHashSet as HashSet;
use std::sync::Arc;

fn summary(
    own: Option<(&str, &[&str])>,
    children: Vec<Arc<MissingSummary>>,
) -> Arc<MissingSummary> {
    Arc::new(MissingSummary {
        own: own.map(|(pkg_id, names)| {
            (pkg_id.to_string(), names.iter().map(|name| (*name).to_string()).collect())
        }),
        children,
    })
}

fn names_of<'a>(
    index: &'a rustc_hash::FxHashMap<&str, super::MissingNames<'_>>,
    pkg_id: &str,
) -> HashSet<&'a str> {
    index.get(pkg_id).expect("package reported missing peers").iter().collect()
}

#[test]
fn descendants_of_every_root_are_indexed() {
    let shared = summary(Some(("deep@1.0.0", &["deep-peer"])), Vec::new());
    let roots = vec![
        summary(Some(("a@1.0.0", &["a-peer"])), vec![Arc::clone(&shared)]),
        summary(Some(("b@1.0.0", &["b-peer"])), vec![shared]),
    ];

    let index = index_missing_names(&roots);

    assert_eq!(index.len(), 3);
    assert_eq!(names_of(&index, "a@1.0.0"), HashSet::from_iter(["a-peer"]));
    assert_eq!(names_of(&index, "b@1.0.0"), HashSet::from_iter(["b-peer"]));
    assert_eq!(names_of(&index, "deep@1.0.0"), HashSet::from_iter(["deep-peer"]));
}

#[test]
fn occurrences_of_one_package_report_the_union_of_their_missing_peers() {
    let roots = vec![
        summary(Some(("pkg@1.0.0", &["first"])), Vec::new()),
        summary(Some(("pkg@1.0.0", &["second"])), Vec::new()),
        summary(Some(("pkg@1.0.0", &["third", "first"])), Vec::new()),
    ];

    let index = index_missing_names(&roots);

    assert_eq!(names_of(&index, "pkg@1.0.0"), HashSet::from_iter(["first", "second", "third"]));
}
