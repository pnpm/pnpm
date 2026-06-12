//! Ported one-for-one from upstream's
//! [`hoistPeers.test.ts`](https://github.com/pnpm/pnpm/blob/097983fbca/installing/deps-resolver/test/hoistPeers.test.ts).
//! Adding a case here? Add (or mirror) the upstream case too.

use std::collections::BTreeMap;

use pacquet_resolving_resolver_base::{
    PreferredVersions, VersionSelectorEntry, VersionSelectorType, VersionSelectorWithWeight,
};
use pretty_assertions::assert_eq;

use super::{HoistPeersOptions, MissingPeerInfo, get_hoistable_optional_peers, hoist_peers};

fn preferred(entries: &[(&str, &[(&str, VersionSelectorEntry)])]) -> PreferredVersions {
    let mut map = PreferredVersions::new();
    for (name, selectors) in entries {
        let mut inner = BTreeMap::new();
        for (spec, entry) in *selectors {
            inner.insert((*spec).to_string(), entry.clone());
        }
        map.insert((*name).to_string(), inner);
    }
    map
}

fn plain(ty: VersionSelectorType) -> VersionSelectorEntry {
    VersionSelectorEntry::Plain(ty)
}

fn missing(name: &str, range: &str) -> (String, MissingPeerInfo) {
    (name.to_string(), MissingPeerInfo { range: range.to_string() })
}

fn opts(
    auto_install_peers: bool,
    all_preferred_versions: &PreferredVersions,
) -> HoistPeersOptions<'_> {
    HoistPeersOptions { auto_install_peers, all_preferred_versions, workspace_root_deps: &[] }
}

#[test]
fn picks_already_available_prerelease_version() {
    let preferred = preferred(&[("foo", &[("1.0.0-beta.0", plain(VersionSelectorType::Version))])]);
    let result = hoist_peers(&opts(false, &preferred), &[missing("foo", "*")]);
    let mut expected = BTreeMap::new();
    expected.insert("foo".to_string(), "1.0.0-beta.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn respects_peer_dep_range_when_preferred_versions_exist() {
    let preferred = preferred(&[(
        "chai",
        &[
            ("5.2.1", plain(VersionSelectorType::Version)),
            ("4.3.0", plain(VersionSelectorType::Version)),
        ],
    )]);
    let result = hoist_peers(&opts(true, &preferred), &[missing("chai", "4.3.0")]);
    let mut expected = BTreeMap::new();
    expected.insert("chai".to_string(), "4.3.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn falls_back_to_range_when_no_preferred_version_satisfies_it() {
    let preferred = preferred(&[("chai", &[("5.2.1", plain(VersionSelectorType::Version))])]);
    let result = hoist_peers(&opts(true, &preferred), &[missing("chai", "4.3.0")]);
    let mut expected = BTreeMap::new();
    expected.insert("chai".to_string(), "4.3.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn picks_highest_preferred_version_for_deduplication_when_range_is_not_exact() {
    let preferred = preferred(&[(
        "foo",
        &[
            ("2.0.0", plain(VersionSelectorType::Version)),
            ("2.1.0", plain(VersionSelectorType::Version)),
            ("3.0.0", plain(VersionSelectorType::Version)),
        ],
    )]);
    let result = hoist_peers(&opts(true, &preferred), &[missing("foo", "^2.0.0")]);
    let mut expected = BTreeMap::new();
    expected.insert("foo".to_string(), "3.0.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn reuses_higher_preferred_version_when_range_is_not_exact() {
    let preferred = preferred(&[("foo", &[("2.0.0", plain(VersionSelectorType::Version))])]);
    let result = hoist_peers(&opts(true, &preferred), &[missing("foo", "1")]);
    let mut expected = BTreeMap::new();
    expected.insert("foo".to_string(), "2.0.0".to_string());
    assert_eq!(result, expected);
}

/// Regression for <https://github.com/pnpm/pnpm/pull/11049>.
#[test]
fn returns_valid_specifier_when_given_only_range_preferred_version_selectors() {
    let preferred = preferred(&[("foo", &[("^2.0.0", plain(VersionSelectorType::Range))])]);
    let result = hoist_peers(&opts(true, &preferred), &[missing("foo", "2")]);
    let mut expected = BTreeMap::new();
    expected.insert("foo".to_string(), "^2.0.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn handles_workspace_protocol_range_without_panicking() {
    let preferred = preferred(&[("foo", &[("1.0.0", plain(VersionSelectorType::Version))])]);
    let result = hoist_peers(&opts(true, &preferred), &[missing("foo", "workspace:*")]);
    let mut expected = BTreeMap::new();
    expected.insert("foo".to_string(), "1.0.0".to_string());
    assert_eq!(result, expected);
}

/// Regression for <https://github.com/pnpm/pnpm/pull/11048>.
#[test]
fn handles_version_selector_with_weight() {
    let preferred = preferred(&[(
        "foo",
        &[(
            "1.0.0",
            VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
                selector_type: VersionSelectorType::Version,
                weight: 1,
            }),
        )],
    )]);
    let result = hoist_peers(&opts(true, &preferred), &[missing("foo", "1")]);
    let mut expected = BTreeMap::new();
    expected.insert("foo".to_string(), "1.0.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn get_hoistable_optional_peers_picks_a_version_that_satisfies_all_optional_ranges() {
    let preferred = preferred(&[(
        "foo",
        &[
            ("1.0.0", plain(VersionSelectorType::Version)),
            ("2.0.0", plain(VersionSelectorType::Version)),
            ("2.1.0", plain(VersionSelectorType::Version)),
            ("3.0.0", plain(VersionSelectorType::Version)),
        ],
    )]);
    let mut missing = BTreeMap::new();
    missing.insert("foo".to_string(), vec!["2".to_string(), "2.1".to_string()]);
    let result = get_hoistable_optional_peers(&missing, &preferred);
    let mut expected = BTreeMap::new();
    expected.insert("foo".to_string(), "2.1.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn get_hoistable_optional_peers_picks_the_highest_satisfying_version() {
    let preferred = preferred(&[(
        "foo",
        &[
            ("2.1.0", plain(VersionSelectorType::Version)),
            ("2.1.1", plain(VersionSelectorType::Version)),
        ],
    )]);
    let mut missing = BTreeMap::new();
    missing.insert("foo".to_string(), vec!["2".to_string(), "2.1".to_string()]);
    let result = get_hoistable_optional_peers(&missing, &preferred);
    let mut expected = BTreeMap::new();
    expected.insert("foo".to_string(), "2.1.1".to_string());
    assert_eq!(result, expected);
}

#[test]
fn get_hoistable_optional_peers_handles_version_selector_with_weight() {
    let preferred = preferred(&[(
        "jsdom",
        &[
            ("26.1.0", plain(VersionSelectorType::Version)),
            (
                "27.4.0",
                VersionSelectorEntry::Weighted(VersionSelectorWithWeight {
                    selector_type: VersionSelectorType::Version,
                    weight: 1,
                }),
            ),
        ],
    )]);
    let mut missing = BTreeMap::new();
    missing.insert("jsdom".to_string(), vec!["*".to_string()]);
    let result = get_hoistable_optional_peers(&missing, &preferred);
    let mut expected = BTreeMap::new();
    expected.insert("jsdom".to_string(), "27.4.0".to_string());
    assert_eq!(result, expected);
}

/// Mirrors upstream's `{ includePrerelease: true }` arg to
/// `semver.maxSatisfying`: a `^18.0.0`-style range must accept an
/// `18.0.0-rc.1` candidate from the preferred-versions table. The
/// default `Range::satisfies` rejects prereleases when the range has
/// none of its own; `satisfies_including_prerelease` strips the
/// prerelease tag and retries. Without that retry, `hoist_peers` and
/// `get_hoistable_optional_peers` would drop valid prerelease picks
/// even though pnpm honors them.
#[test]
fn hoist_peers_accepts_prerelease_against_non_prerelease_range() {
    let preferred =
        preferred(&[("react", &[("18.0.0-rc.1", plain(VersionSelectorType::Version))])]);
    let result = hoist_peers(&opts(true, &preferred), &[missing("react", "^18.0.0")]);
    let mut expected = BTreeMap::new();
    expected.insert("react".to_string(), "18.0.0-rc.1".to_string());
    assert_eq!(result, expected);
}

#[test]
fn get_hoistable_optional_peers_accepts_prerelease_against_non_prerelease_range() {
    let preferred =
        preferred(&[("react", &[("18.0.0-rc.1", plain(VersionSelectorType::Version))])]);
    let mut missing = BTreeMap::new();
    missing.insert("react".to_string(), vec!["^18.0.0".to_string()]);
    let result = get_hoistable_optional_peers(&missing, &preferred);
    let mut expected = BTreeMap::new();
    expected.insert("react".to_string(), "18.0.0-rc.1".to_string());
    assert_eq!(result, expected);
}
