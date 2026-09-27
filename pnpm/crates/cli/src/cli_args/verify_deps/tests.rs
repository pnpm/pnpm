use super::filter_selector_args;

fn selectors(selectors: &[&str]) -> Vec<String> {
    selectors
        .iter()
        .map(ToString::to_string)
        .collect()
}

#[test]
fn filter_selector_args_forward_the_selection_with_dependencies() {
    assert!(filter_selector_args(&[], &[]).is_empty());
    assert_eq!(
        filter_selector_args(&selectors(&["foo", "bar"]), &[]),
        ["--filter=foo...", "--filter=bar..."],
    );
    assert_eq!(filter_selector_args(&[], &selectors(&["foo"])), ["--filter-prod=foo..."]);
}

#[test]
fn filter_selector_args_keep_exclusions_and_dependency_selectors() {
    assert_eq!(
        filter_selector_args(&selectors(&["foo...", "...bar", "baz^...", "!qux"]), &[]),
        ["--filter=foo...", "--filter=...bar...", "--filter=baz^...", "--filter=!qux"],
    );
}
