//! Tests for [`super::hoist_peers`] and
//! [`super::get_hoistable_optional_peers`].

use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use std::{collections::BTreeMap, path::Path};

use pnpm_resolving_resolver_base::{
    PreferredVersions, VersionSelectorEntry, VersionSelectorType, VersionSelectorWithWeight,
};
use pretty_assertions::assert_eq;

use super::{
    HoistPeersOptions, MissingPeerInfo, WorkspaceRootDep, get_hoistable_optional_peers,
    get_hoistable_optional_peers_with_locked_versions, hoist_peers,
};

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
    HoistPeersOptions {
        auto_install_peers,
        all_preferred_versions,
        workspace_root_deps: &[],
        override_bare_specifier: None,
        project_dir: Path::new("/workspace"),
    }
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
fn picks_highest_preferred_version_satisfying_range_for_deduplication() {
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
    expected.insert("foo".to_string(), "2.1.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn does_not_reuse_preferred_version_that_the_peer_range_rejects() {
    let preferred = preferred(&[("foo", &[("2.0.0", plain(VersionSelectorType::Version))])]);
    let result = hoist_peers(&opts(true, &preferred), &[missing("foo", "1")]);
    let mut expected = BTreeMap::new();
    expected.insert("foo".to_string(), "1".to_string());
    assert_eq!(result, expected);
}

/// A peer declared as `^1.0.0` must not be handed a foreign `2.x`
/// contributed by another importer when a satisfying `1.x` exists.
#[test]
fn prefers_preferred_version_satisfying_non_exact_range() {
    let preferred = preferred(&[(
        "foo",
        &[
            ("1.0.0", plain(VersionSelectorType::Version)),
            ("2.0.0", plain(VersionSelectorType::Version)),
        ],
    )]);
    let result = hoist_peers(&opts(true, &preferred), &[missing("foo", "^1.0.0")]);
    let mut expected = BTreeMap::new();
    expected.insert("foo".to_string(), "1.0.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn does_not_treat_prerelease_of_next_major_as_satisfying_caret_range() {
    let preferred = preferred(&[(
        "foo",
        &[
            ("1.0.0", plain(VersionSelectorType::Version)),
            ("2.0.0-beta.1", plain(VersionSelectorType::Version)),
        ],
    )]);
    let result = hoist_peers(&opts(true, &preferred), &[missing("foo", "^1.0.0")]);
    let mut expected = BTreeMap::new();
    expected.insert("foo".to_string(), "1.0.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn falls_back_to_range_when_no_preferred_version_satisfies_non_exact_range() {
    let preferred = preferred(&[("foo", &[("2.0.0", plain(VersionSelectorType::Version))])]);
    let result = hoist_peers(&opts(true, &preferred), &[missing("foo", "^1.0.0")]);
    let mut expected = BTreeMap::new();
    expected.insert("foo".to_string(), "^1.0.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn hoists_nothing_when_no_preferred_version_satisfies_range_without_auto_install() {
    let preferred = preferred(&[("foo", &[("2.0.0", plain(VersionSelectorType::Version))])]);
    let result = hoist_peers(&opts(false, &preferred), &[missing("foo", "^1.0.0")]);
    assert_eq!(result, BTreeMap::new());
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

#[test]
fn dedupes_named_registry_peer_onto_a_preferred_version_that_satisfies_its_extracted_range() {
    let preferred = preferred(&[(
        "foo",
        &[
            ("1.0.0", plain(VersionSelectorType::Version)),
            ("2.0.0", plain(VersionSelectorType::Version)),
        ],
    )]);
    let result = hoist_peers(&opts(true, &preferred), &[missing("foo", "work:^1.0.0")]);
    let mut expected = BTreeMap::new();
    expected.insert("foo".to_string(), "1.0.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn falls_back_to_raw_scheme_specifier_when_no_preferred_version_satisfies_its_extracted_range() {
    let preferred = preferred(&[("foo", &[("2.0.0", plain(VersionSelectorType::Version))])]);
    let result = hoist_peers(&opts(true, &preferred), &[missing("foo", "work:^1.0.0")]);
    let mut expected = BTreeMap::new();
    expected.insert("foo".to_string(), "work:^1.0.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn respects_a_merged_union_of_scheme_specifiers_instead_of_picking_the_highest() {
    // `4.0.0` is the highest but satisfies neither `^2.0.0` nor `^3.0.0`, so a
    // blind highest-version pick would be wrong; `3.0.0` is the highest match.
    let preferred = preferred(&[(
        "foo",
        &[
            ("2.1.0", plain(VersionSelectorType::Version)),
            ("3.0.0", plain(VersionSelectorType::Version)),
            ("4.0.0", plain(VersionSelectorType::Version)),
        ],
    )]);
    let result =
        hoist_peers(&opts(true, &preferred), &[missing("foo", "work:^2.0.0 || work:^3.0.0")]);
    let mut expected = BTreeMap::new();
    expected.insert("foo".to_string(), "3.0.0".to_string());
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
    let result = get_hoistable_optional_peers(&missing, &preferred, &[]);
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
    let result = get_hoistable_optional_peers(&missing, &preferred, &[]);
    let mut expected = BTreeMap::new();
    expected.insert("foo".to_string(), "2.1.1".to_string());
    assert_eq!(result, expected);
}

#[test]
fn get_hoistable_optional_peers_preserves_the_importers_locked_version() {
    let missing = BTreeMap::from([("peer".to_string(), vec!["*".to_string()])]);
    let preferred = PreferredVersions::from([(
        "peer".to_string(),
        BTreeMap::from([
            ("1.0.0".to_string(), VersionSelectorEntry::Plain(VersionSelectorType::Version)),
            ("2.0.0".to_string(), VersionSelectorEntry::Plain(VersionSelectorType::Version)),
        ]),
    )]);
    let locked =
        HashMap::from_iter([("peer".to_string(), HashSet::from_iter(["1.0.0".to_string()]))]);

    assert_eq!(
        get_hoistable_optional_peers_with_locked_versions(&missing, &preferred, &[], &locked,),
        BTreeMap::from([("peer".to_string(), "1.0.0".to_string())]),
    );
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
    let result = get_hoistable_optional_peers(&missing, &preferred, &[]);
    let mut expected = BTreeMap::new();
    expected.insert("jsdom".to_string(), "27.4.0".to_string());
    assert_eq!(result, expected);
}

/// `^18.0.0` spells its lower bound out, so `includePrerelease` leaves
/// it at `18.0.0` and the prerelease stays below the range. Hoisting the
/// range instead lets the registry supply a stable `18.x`.
#[test]
fn hoist_peers_rejects_a_prerelease_below_a_spelled_out_lower_bound() {
    let preferred =
        preferred(&[("react", &[("18.0.0-rc.1", plain(VersionSelectorType::Version))])]);
    let result = hoist_peers(&opts(true, &preferred), &[missing("react", "^18.0.0")]);
    let mut expected = BTreeMap::new();
    expected.insert("react".to_string(), "^18.0.0".to_string());
    assert_eq!(result, expected);
}

/// The same prerelease against `^18.x`, whose lower bound npm
/// synthesized from the omitted patch and therefore lowers to
/// `18.0.0-0`.
#[test]
fn hoist_peers_accepts_a_prerelease_below_a_synthesized_lower_bound() {
    let preferred =
        preferred(&[("react", &[("18.0.0-rc.1", plain(VersionSelectorType::Version))])]);
    let result = hoist_peers(&opts(true, &preferred), &[missing("react", "^18.x")]);
    let mut expected = BTreeMap::new();
    expected.insert("react".to_string(), "18.0.0-rc.1".to_string());
    assert_eq!(result, expected);
}

#[test]
fn hoist_peers_accepts_a_prerelease_inside_the_span_of_a_union() {
    let preferred = preferred(&[(
        "react",
        &[("19.3.0-canary-28cd4bb0-20260723", plain(VersionSelectorType::Version))],
    )]);
    let result = hoist_peers(&opts(true, &preferred), &[missing("react", "^18.x || ^19.x")]);
    let mut expected = BTreeMap::new();
    expected.insert("react".to_string(), "19.3.0-canary-28cd4bb0-20260723".to_string());
    assert_eq!(result, expected);
}

/// A prerelease sitting at the lower bound of a union's second
/// alternative is rejected by both alternatives, so the peer resolves
/// from the registry rather than auto-installing the alpha.
#[test]
fn hoist_peers_rejects_a_prerelease_at_the_lower_bound_of_a_union() {
    let preferred =
        preferred(&[("jest-util", &[("30.0.0-alpha.6", plain(VersionSelectorType::Version))])]);
    let result =
        hoist_peers(&opts(true, &preferred), &[missing("jest-util", "^29.0.0 || ^30.0.0")]);
    let mut expected = BTreeMap::new();
    expected.insert("jest-util".to_string(), "^29.0.0 || ^30.0.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn get_hoistable_optional_peers_rejects_prerelease_against_non_prerelease_range() {
    let preferred =
        preferred(&[("react", &[("18.0.0-rc.1", plain(VersionSelectorType::Version))])]);
    let mut missing = BTreeMap::new();
    missing.insert("react".to_string(), vec!["^18.0.0".to_string()]);
    let result = get_hoistable_optional_peers(&missing, &preferred, &[]);
    assert_eq!(result, BTreeMap::new());
}

#[test]
fn get_hoistable_optional_peers_rejects_prerelease_within_the_range_span() {
    let preferred = preferred(&[(
        "jest-util",
        &[
            ("29.7.0", plain(VersionSelectorType::Version)),
            ("30.0.0-alpha.6", plain(VersionSelectorType::Version)),
        ],
    )]);
    let mut missing = BTreeMap::new();
    missing.insert("jest-util".to_string(), vec!["^29.0.0 || ^30.0.0".to_string()]);
    let result = get_hoistable_optional_peers(&missing, &preferred, &[]);
    let mut expected = BTreeMap::new();
    expected.insert("jest-util".to_string(), "29.7.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn installs_auto_installed_peer_at_the_overridden_specifier() {
    let preferred = preferred(&[("react", &[("18.3.1", plain(VersionSelectorType::Version))])]);
    let root_deps = [WorkspaceRootDep {
        alias: "react".to_string(),
        pkg_name: "react".to_string(),
        normalized_bare_specifier: Some("18.3.1".to_string()),
    }];
    let overrider = |name: &str, _range: &str, _pkg_dir: &Path| {
        (name == "react").then(|| "npm:react@19.2.0".to_string())
    };
    let opts = HoistPeersOptions {
        auto_install_peers: true,
        all_preferred_versions: &preferred,
        workspace_root_deps: &root_deps,
        override_bare_specifier: Some(&overrider),
        project_dir: Path::new("/workspace"),
    };
    let result = hoist_peers(&opts, &[missing("react", "^16.5.1 || ^17.0.0 || ^18.0.0")]);
    let mut expected = BTreeMap::new();
    expected.insert("react".to_string(), "npm:react@19.2.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn override_does_not_install_a_peer_nothing_provides_without_auto_install_peers() {
    let preferred = PreferredVersions::new();
    let overrider =
        |_name: &str, _range: &str, _pkg_dir: &Path| Some("npm:react@19.2.0".to_string());
    let opts = HoistPeersOptions {
        auto_install_peers: false,
        all_preferred_versions: &preferred,
        workspace_root_deps: &[],
        override_bare_specifier: Some(&overrider),
        project_dir: Path::new("/workspace"),
    };
    let result = hoist_peers(&opts, &[missing("react", "^18.0.0")]);
    assert_eq!(result, BTreeMap::new());
}

#[test]
fn leaves_a_deduplicating_hoist_to_the_graph_without_auto_install_peers() {
    let preferred = preferred(&[("react", &[("18.3.1", plain(VersionSelectorType::Version))])]);
    let overrider =
        |_name: &str, _range: &str, _pkg_dir: &Path| Some("npm:react@19.2.0".to_string());
    let opts = HoistPeersOptions {
        auto_install_peers: false,
        all_preferred_versions: &preferred,
        workspace_root_deps: &[],
        override_bare_specifier: Some(&overrider),
        project_dir: Path::new("/workspace"),
    };
    let result = hoist_peers(&opts, &[missing("react", "^18.0.0")]);
    let mut expected = BTreeMap::new();
    expected.insert("react".to_string(), "18.3.1".to_string());
    assert_eq!(result, expected);
}

#[test]
fn override_redirects_the_workspace_roots_hoist_without_auto_install_peers() {
    let empty = PreferredVersions::new();
    let root_deps = [WorkspaceRootDep {
        alias: "react".to_string(),
        pkg_name: "react".to_string(),
        normalized_bare_specifier: Some("18.3.1".to_string()),
    }];
    let overrider =
        |_name: &str, _range: &str, _pkg_dir: &Path| Some("npm:react@19.2.0".to_string());
    let opts = HoistPeersOptions {
        auto_install_peers: false,
        all_preferred_versions: &empty,
        workspace_root_deps: &root_deps,
        override_bare_specifier: Some(&overrider),
        project_dir: Path::new("/workspace"),
    };
    let result = hoist_peers(&opts, &[missing("react", "^18.0.0")]);
    let mut expected = BTreeMap::new();
    expected.insert("react".to_string(), "npm:react@19.2.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn passes_the_importer_directory_to_the_overrider() {
    let preferred = preferred(&[("react", &[("18.3.1", plain(VersionSelectorType::Version))])]);
    let overrider =
        |_name: &str, _range: &str, pkg_dir: &Path| Some(format!("link:{}", pkg_dir.display()));
    let opts = HoistPeersOptions {
        auto_install_peers: true,
        all_preferred_versions: &preferred,
        workspace_root_deps: &[],
        override_bare_specifier: Some(&overrider),
        project_dir: Path::new("/workspace/packages/app"),
    };
    let result = hoist_peers(&opts, &[missing("react", "^18.0.0")]);
    let mut expected = BTreeMap::new();
    expected.insert("react".to_string(), "link:/workspace/packages/app".to_string());
    assert_eq!(result, expected);
}

#[test]
fn leaves_peer_removed_by_an_override_uninstalled() {
    let preferred = preferred(&[("react", &[("18.3.1", plain(VersionSelectorType::Version))])]);
    let overrider = |_name: &str, _range: &str, _pkg_dir: &Path| Some("-".to_string());
    let opts = HoistPeersOptions {
        auto_install_peers: true,
        all_preferred_versions: &preferred,
        workspace_root_deps: &[],
        override_bare_specifier: Some(&overrider),
        project_dir: Path::new("/workspace"),
    };
    let result = hoist_peers(&opts, &[missing("react", "^18.0.0")]);
    assert_eq!(result, BTreeMap::new());
}

#[test]
fn get_hoistable_optional_peers_stays_within_the_workspace_roots_range() {
    let preferred = preferred(&[(
        "postcss",
        &[
            ("8.5.10", plain(VersionSelectorType::Version)),
            ("8.5.22", plain(VersionSelectorType::Version)),
        ],
    )]);
    let mut missing = BTreeMap::new();
    missing.insert("postcss".to_string(), vec!["*".to_string()]);
    let root_deps = [WorkspaceRootDep {
        alias: "postcss".to_string(),
        pkg_name: "postcss".to_string(),
        normalized_bare_specifier: Some("8.5.10".to_string()),
    }];

    let result = get_hoistable_optional_peers(&missing, &preferred, &root_deps);
    let mut expected = BTreeMap::new();
    expected.insert("postcss".to_string(), "8.5.10".to_string());
    assert_eq!(result, expected);

    let unbounded = get_hoistable_optional_peers(&missing, &preferred, &[]);
    let mut expected_unbounded = BTreeMap::new();
    expected_unbounded.insert("postcss".to_string(), "8.5.22".to_string());
    assert_eq!(unbounded, expected_unbounded);
}

#[test]
fn get_hoistable_optional_peers_ignores_a_root_specifier_a_wanted_range_rejects() {
    let preferred = preferred(&[(
        "date-fns",
        &[
            ("2.30.0", plain(VersionSelectorType::Version)),
            ("4.4.0", plain(VersionSelectorType::Version)),
        ],
    )]);
    let mut missing = BTreeMap::new();
    missing.insert("date-fns".to_string(), vec!["^4.0.0".to_string()]);
    let root_deps = [WorkspaceRootDep {
        alias: "date-fns-v2".to_string(),
        pkg_name: "date-fns".to_string(),
        normalized_bare_specifier: Some("npm:date-fns@2.30.0".to_string()),
    }];

    let result = get_hoistable_optional_peers(&missing, &preferred, &root_deps);
    let mut expected = BTreeMap::new();
    expected.insert("date-fns".to_string(), "4.4.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn get_hoistable_optional_peers_ignores_a_root_specifier_only_one_wanted_range_accepts() {
    let preferred = preferred(&[("foo", &[("2.0.0", plain(VersionSelectorType::Version))])]);
    let mut missing = BTreeMap::new();
    missing.insert(
        "foo".to_string(),
        vec![">=1.0.0 <3.0.0".to_string(), ">=2.0.0 <4.0.0".to_string()],
    );
    let root_deps = [WorkspaceRootDep {
        alias: "foo".to_string(),
        pkg_name: "foo".to_string(),
        normalized_bare_specifier: Some("1.0.0".to_string()),
    }];

    let result = get_hoistable_optional_peers(&missing, &preferred, &root_deps);
    let mut expected = BTreeMap::new();
    expected.insert("foo".to_string(), "2.0.0".to_string());
    assert_eq!(result, expected);
}

#[test]
fn skips_a_workspace_root_dep_without_a_specifier_in_favor_of_one_with() {
    let root_deps = [
        WorkspaceRootDep {
            alias: "postcss".to_string(),
            pkg_name: "postcss".to_string(),
            normalized_bare_specifier: None,
        },
        WorkspaceRootDep {
            alias: "zz-postcss".to_string(),
            pkg_name: "postcss".to_string(),
            normalized_bare_specifier: Some("8.5.10".to_string()),
        },
    ];
    let empty = PreferredVersions::new();
    let opts = HoistPeersOptions {
        auto_install_peers: true,
        all_preferred_versions: &empty,
        workspace_root_deps: &root_deps,
        override_bare_specifier: None,
        project_dir: Path::new("/workspace"),
    };
    let result = hoist_peers(&opts, &[missing("postcss", "^8.0.0")]);
    let mut expected = BTreeMap::new();
    expected.insert("postcss".to_string(), "8.5.10".to_string());
    assert_eq!(result, expected);

    let preferred = preferred(&[(
        "postcss",
        &[
            ("8.5.10", plain(VersionSelectorType::Version)),
            ("9.0.0", plain(VersionSelectorType::Version)),
        ],
    )]);
    let mut missing_optional = BTreeMap::new();
    missing_optional.insert("postcss".to_string(), vec!["*".to_string()]);
    let optional = get_hoistable_optional_peers(&missing_optional, &preferred, &root_deps);
    assert_eq!(optional, expected);
}

#[test]
fn get_hoistable_optional_peers_bounds_by_a_scheme_prefixed_workspace_root_specifier() {
    let preferred = preferred(&[(
        "postcss",
        &[
            ("8.5.10", plain(VersionSelectorType::Version)),
            ("9.0.0", plain(VersionSelectorType::Version)),
        ],
    )]);
    let mut missing = BTreeMap::new();
    missing.insert("postcss".to_string(), vec!["*".to_string()]);
    for spec in ["workspace:^8.5.10", "npm:postcss@^8.5.10", "work:^8.5.10"] {
        let root_deps = [WorkspaceRootDep {
            alias: "postcss".to_string(),
            pkg_name: "postcss".to_string(),
            normalized_bare_specifier: Some(spec.to_string()),
        }];
        let result = get_hoistable_optional_peers(&missing, &preferred, &root_deps);
        let mut expected = BTreeMap::new();
        expected.insert("postcss".to_string(), "8.5.10".to_string());
        dbg!(spec);
        assert_eq!(result, expected);
    }
}

#[test]
fn get_hoistable_optional_peers_stays_unbounded_when_the_root_specifier_has_no_version() {
    let preferred = preferred(&[(
        "postcss",
        &[
            ("8.5.10", plain(VersionSelectorType::Version)),
            ("9.0.0", plain(VersionSelectorType::Version)),
        ],
    )]);
    let mut missing = BTreeMap::new();
    missing.insert("postcss".to_string(), vec!["*".to_string()]);
    let root_deps = [WorkspaceRootDep {
        alias: "postcss".to_string(),
        pkg_name: "postcss".to_string(),
        normalized_bare_specifier: Some("file:../postcss".to_string()),
    }];

    let result = get_hoistable_optional_peers(&missing, &preferred, &root_deps);
    let mut expected = BTreeMap::new();
    expected.insert("postcss".to_string(), "9.0.0".to_string());
    assert_eq!(result, expected);
}
