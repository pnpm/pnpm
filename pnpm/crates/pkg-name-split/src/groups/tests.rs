use super::{
    Group,
    dep_path_suffix_start,
    split_trailing_groups,
};
use pretty_assertions::assert_eq;

/// The text in front of the groups that end `text`, and each group as
/// written and its body.
fn groups_of(text: &str) -> Option<(&str, Vec<(&str, &str)>)> {
    let (front, groups) = split_trailing_groups(text)?;
    let groups = groups
        .into_iter()
        .map(|group: Group<'_>| (group.as_str(), group.body()))
        .collect();
    Some((front, groups))
}

#[test]
fn splits_the_groups_that_end_a_text() {
    assert_eq!(
        groups_of("a@1(b@2)(c@3(d@4))"),
        Some(("a@1", vec![("(b@2)", "b@2"), ("(c@3(d@4))", "c@3(d@4)")])),
    );
    assert_eq!(groups_of("é(日本)"), Some(("é", vec![("(日本)", "日本")])));
    assert_eq!(groups_of("(a)(b)"), Some(("", vec![("(a)", "a"), ("(b)", "b")])));
    for text in ["", "a", "(", "a(b)c"] {
        assert_eq!(groups_of(text), Some((text, vec![])), "{text:?}");
    }
}

#[test]
fn rejects_a_group_without_its_open_parenthesis() {
    for text in [")", "a)(b)", "a(b))"] {
        assert_eq!(groups_of(text), None, "{text:?}");
    }
}

#[test]
fn finds_where_a_dep_path_suffix_starts() {
    assert_eq!(dep_path_suffix_start("foo@1.0.0(react@18.2.0)"), Some(9));
    assert_eq!(dep_path_suffix_start("foo@1.0.0(patch_hash=abc)(a@1)"), Some(9));
    assert_eq!(dep_path_suffix_start("a()"), Some(1));
    for dep_path in ["", "a", "(a)", "a)", "a)()", "a(b"] {
        assert_eq!(dep_path_suffix_start(dep_path), None, "{dep_path:?}");
    }
}

/// Read right to left, the count drops below zero at `((` and the `)` in
/// front of them brings it back, so the scan stops at the `x` and the
/// suffix it reports starts with `)`.
#[test]
fn reports_a_suffix_after_unbalanced_parentheses() {
    assert_eq!(dep_path_suffix_start("x)((a)"), Some(1));
}
