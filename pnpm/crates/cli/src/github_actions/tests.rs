use super::{filter_package_selectors, opted_in, selects_actions_only};
use pnpm_config::Config;

fn strings(items: &[&str]) -> Vec<String> {
    items
        .iter()
        .map(|item| (*item).to_string())
        .collect()
}

#[test]
fn workflow_files_are_read_only_when_opted_in() {
    let mut config = Config::new();
    assert!(!opted_in(false, &config));
    assert!(opted_in(true, &config));

    config.update_config.github_actions = Some(false);
    assert!(!opted_in(false, &config));
    assert!(opted_in(true, &config));

    config.update_config.github_actions = Some(true);
    assert!(opted_in(false, &config));
}

#[test]
fn keeps_purl_selectors_as_package_selectors_when_github_actions_are_enabled() {
    let selectors = strings(&[
        "pkg:npm/pkg-a",
        "!pkg:npm/pkg-b",
        "pkg:cargo/serde",
        "pkg:pypi/requests",
        "actions/checkout",
        "!actions/cache/save",
    ]);
    assert_eq!(
        filter_package_selectors(&selectors, true),
        strings(&["pkg:npm/pkg-a", "!pkg:npm/pkg-b", "pkg:cargo/serde", "pkg:pypi/requests"]),
    );
}

#[test]
fn negated_action_selectors_do_not_skip_package_updates() {
    for (raw, actions_only) in [
        (&[][..], false),
        (&["actions/checkout"][..], true),
        (&["actions/cache", "!actions/cache/save"][..], true),
        (&["!actions/checkout"][..], false),
        (&["!actions/cache/save"][..], false),
        (&["pkg:npm/pkg-a"][..], false),
        (&["actions/checkout", "lodash"][..], false),
    ] {
        let raw = strings(raw);
        let packages = filter_package_selectors(&raw, true);
        assert_eq!(selects_actions_only(&raw, &packages), actions_only, "raw: {raw:?}");
    }
}
