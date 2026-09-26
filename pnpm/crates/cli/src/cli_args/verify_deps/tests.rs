use super::filter_selector_args;

#[test]
fn filter_selector_args_forward_the_selection() {
    assert!(filter_selector_args(&[], &[]).is_empty());
    assert_eq!(
        filter_selector_args(&["foo".to_string(), "bar".to_string()], &[]),
        ["--filter=foo", "--filter=bar"],
    );
    assert_eq!(filter_selector_args(&[], &["foo".to_string()]), ["--filter-prod=foo"]);
}
