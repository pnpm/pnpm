use pnpm_resolving_resolver_base::{
    EXISTING_VERSION_SELECTOR_WEIGHT, VersionSelectorEntry, VersionSelectorType,
    VersionSelectorWithWeight, VersionSelectors,
};
use pretty_assertions::assert_eq;

use super::{make_package, pick_stable_cached_range_version};

fn lockfile_selector(weight: u32) -> VersionSelectorEntry {
    VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
        selector_type: VersionSelectorType::Version,
        weight,
    })
}

fn tag_selector(weight: u32) -> VersionSelectorEntry {
    VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
        selector_type: VersionSelectorType::Tag,
        weight,
    })
}

#[test]
fn stable_cached_range_ignores_a_lockfile_version_the_winner_outranks() {
    let pkg = make_package("acme", &[("1.0.0", None), ("1.1.0", None), ("1.2.0", None)], &[]);
    let mut selectors: VersionSelectors = ["1.0.0", "1.1.0"]
        .into_iter()
        .map(|version| (version.to_string(), lockfile_selector(EXISTING_VERSION_SELECTOR_WEIGHT)))
        .collect();
    selectors.insert(
        ">=1.1.0".to_string(),
        VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
            selector_type: VersionSelectorType::Range,
            weight: EXISTING_VERSION_SELECTOR_WEIGHT,
        }),
    );

    assert_eq!(
        pick_stable_cached_range_version(&pkg, "^1.0.0", Some(&selectors)).as_deref(),
        Some("1.1.0"),
    );
}

#[test]
fn stable_cached_range_rejects_a_movable_tag_that_ties_the_winner() {
    let pkg = make_package("acme", &[("1.0.0", None), ("1.1.0", None)], &[]);
    let mut selectors: VersionSelectors =
        [("1.0.0".to_string(), lockfile_selector(EXISTING_VERSION_SELECTOR_WEIGHT))].into();
    selectors.insert("next".to_string(), tag_selector(EXISTING_VERSION_SELECTOR_WEIGHT));

    assert_eq!(pick_stable_cached_range_version(&pkg, "^1.0.0", Some(&selectors)), None);
}

#[test]
fn stable_cached_range_rejects_an_exact_selector_a_tag_could_lift_to_the_winner() {
    let pkg = make_package("acme", &[("1.0.0", None), ("1.1.0", None)], &[("next", "1.0.0")]);
    let mut selectors: VersionSelectors = [("1.0.0", 3), ("1.1.0", 2)]
        .into_iter()
        .map(|(version, weight)| (version.to_string(), lockfile_selector(weight)))
        .collect();
    selectors.insert("next".to_string(), tag_selector(1));

    assert_eq!(pick_stable_cached_range_version(&pkg, "^1.0.0", Some(&selectors)), None);
}

#[test]
fn stable_cached_range_accepts_a_lone_plain_exact_selector() {
    let pkg = make_package("acme", &[("1.0.0", None), ("1.1.0", None)], &[]);
    let selectors: VersionSelectors =
        [("1.0.0".to_string(), VersionSelectorEntry::Plain(VersionSelectorType::Version))].into();

    assert_eq!(
        pick_stable_cached_range_version(&pkg, "^1.0.0", Some(&selectors)).as_deref(),
        Some("1.0.0"),
    );
}

#[test]
fn stable_cached_range_follows_the_heavier_lockfile_version() {
    let pkg = make_package("acme", &[("1.0.0", None), ("1.1.0", None), ("1.2.0", None)], &[]);
    let selectors: VersionSelectors = [
        ("1.0.0", EXISTING_VERSION_SELECTOR_WEIGHT * 2),
        ("1.1.0", EXISTING_VERSION_SELECTOR_WEIGHT),
    ]
    .into_iter()
    .map(|(version, weight)| (version.to_string(), lockfile_selector(weight)))
    .collect();

    assert_eq!(
        pick_stable_cached_range_version(&pkg, "^1.0.0", Some(&selectors)).as_deref(),
        Some("1.0.0"),
    );
}
