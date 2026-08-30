use super::{
    ManifestSpecBumps, OverriddenDeclarations, apply_manifest_spec_bumps, bumped_range,
    split_npm_alias,
};
use crate::VersionsOverrider;
use pnpm_catalogs_types::Catalogs;
use pnpm_config_parse_overrides::parse_overrides;
use pnpm_lockfile::{ImporterDepVersion, Lockfile, PkgName, ResolvedDependencyMap};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_registry::RangeSpecStyle;
use serde_json::json;
use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

fn lockfile(source: &str) -> Lockfile {
    serde_saphyr::from_str(source).expect("parse lockfile")
}

fn specifier_of(group: Option<&ResolvedDependencyMap>, alias: &str) -> String {
    let alias = PkgName::parse(alias).expect("parse the alias");
    group.expect("the group is declared")[&alias].specifier.clone()
}

fn bumps(targets: &[(&str, DependencyGroup, &str)]) -> ManifestSpecBumps {
    let targets = targets
        .iter()
        .map(|(alias, group, declared)| ((*alias).to_string(), (*group, (*declared).to_string())))
        .collect::<HashMap<_, _>>();
    ManifestSpecBumps {
        targets: BTreeMap::from([(".".to_string(), targets)]),
        range_spec_style: RangeSpecStyle::Major,
        applied: std::sync::Mutex::default(),
    }
}

fn bump(declared: &str, version: &str) -> Option<String> {
    let version = version.parse::<ImporterDepVersion>().expect("parse the resolved version");
    bumped_range(declared, &version, RangeSpecStyle::Major)
}

#[test]
fn keeps_the_declared_range_operator() {
    assert_eq!(bump("^1.0.0", "1.2.0").as_deref(), Some("^1.2.0"));
    assert_eq!(bump("~1.0.0", "1.0.3").as_deref(), Some("~1.0.3"));
    assert_eq!(bump("=1.0.0", "1.2.0").as_deref(), Some("=1.2.0"));
    assert_eq!(bump("1.0.0", "1.2.0").as_deref(), Some("1.2.0"));
}

#[test]
fn falls_back_to_the_default_operator_when_the_declaration_pins_none() {
    assert_eq!(bump(">=1.0.0", "3.1.0").as_deref(), Some("^3.1.0"));
    assert_eq!(bump("1 || 2", "1.2.0").as_deref(), Some("^1.2.0"));
    assert_eq!(bump("*", "2.1.0").as_deref(), Some("^2.1.0"));
}

#[test]
fn a_declaration_that_already_names_the_version_is_left_alone() {
    assert_eq!(bump("^1.2.0", "1.2.0"), None);
    assert_eq!(bump("1.2.0", "1.2.0"), None);
}

#[test]
fn a_dist_tag_keeps_tracking_its_tag() {
    assert_eq!(bump("latest", "3.0.1"), None);
    assert_eq!(bump("next", "3.0.1"), None);
}

#[test]
fn a_prerelease_pick_keeps_the_declared_range_operator() {
    assert_eq!(bump("^1.0.0-beta.1", "1.0.0-beta.2").as_deref(), Some("^1.0.0-beta.2"));
    assert_eq!(bump("~1.0.0-beta.1", "1.0.0-beta.2").as_deref(), Some("~1.0.0-beta.2"));
    assert_eq!(bump("1.0.0-beta.1", "1.0.0-beta.2").as_deref(), Some("1.0.0-beta.2"));
    assert_eq!(bump("=1.0.0-beta.1", "1.0.0-beta.2").as_deref(), Some("=1.0.0-beta.2"));
}

#[test]
fn a_prerelease_pick_uses_an_exact_fallback_for_an_unsupported_range() {
    assert_eq!(bump(">=1.0.0-beta.1", "2.0.0-beta.1").as_deref(), Some("2.0.0-beta.1"));
    assert_eq!(bump("1 || 2", "2.1.0-beta.1").as_deref(), Some("2.1.0-beta.1"));
}

#[test]
fn an_npm_alias_keeps_pointing_at_the_same_package() {
    assert_eq!(
        bump("npm:is-positive@^3.0.0", "is-positive@3.1.0").as_deref(),
        Some("npm:is-positive@^3.1.0"),
    );
    assert_eq!(
        bump("npm:@scope/pkg@~1.0.0", "@scope/pkg@1.0.4").as_deref(),
        Some("npm:@scope/pkg@~1.0.4"),
    );
}

#[test]
fn declarations_of_other_protocols_are_left_alone() {
    for declared in [
        "workspace:*",
        "workspace:^1.0.0",
        "link:../foo",
        "file:../foo.tgz",
        "catalog:default",
        "jsr:^1.0.0",
        "https://example.com/foo.tgz",
    ] {
        assert_eq!(bump(declared, "1.2.0"), None, "{declared} should be left alone");
    }
}

#[test]
fn a_version_with_no_semver_to_pin_is_left_alone() {
    assert_eq!(bump("^1.0.0", "link:../foo"), None);
    assert_eq!(bump("^1.0.0", "file:../foo(react@18.0.0)"), None);
}

#[test]
fn a_peer_suffix_is_dropped_from_the_written_range() {
    assert_eq!(bump("^17.0.0", "17.0.2(react@17.0.2)").as_deref(), Some("^17.0.2"));
}

#[test]
fn npm_aliases_split_into_the_prefix_they_keep() {
    assert_eq!(split_npm_alias("^1.0.0"), Some(("", "^1.0.0")));
    assert_eq!(split_npm_alias("npm:foo@^1.0.0"), Some(("npm:foo@", "^1.0.0")));
    assert_eq!(split_npm_alias("npm:@scope/foo@^1.0.0"), Some(("npm:@scope/foo@", "^1.0.0")));
    assert_eq!(split_npm_alias("npm:^1.0.0"), Some(("npm:", "^1.0.0")));
    assert_eq!(split_npm_alias("workspace:^1.0.0"), None);
}

/// A package declared in more than one direct group has one entry per group,
/// each with its own range. The bump has to read and rewrite the entry under
/// the group the declaration came from.
#[test]
fn a_bump_moves_the_entry_of_the_group_the_manifest_declared() {
    let mut lockfile = lockfile(
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.0.0
    devDependencies:
      foo:
        specifier: ^2.0.0
        version: 2.1.0
",
    );

    let bumps = bumps(&[("foo", DependencyGroup::Dev, "^2.0.0")]);
    apply_manifest_spec_bumps(&mut lockfile, &bumps, None);

    let importer = &lockfile.importers["."];
    assert_eq!(specifier_of(importer.dev_dependencies.as_ref(), "foo"), "^2.1.0");
    assert_eq!(specifier_of(importer.dependencies.as_ref(), "foo"), "^1.0.0");
    let applied = bumps.applied.into_inner().expect("never poisoned");
    let expected = (DependencyGroup::Dev, "^2.1.0".to_string());
    assert_eq!(applied.manifests["."]["foo"], expected);
}

/// The declared text is what the resolver read. When the lockfile entry
/// carries something else — an override replaced the declaration before
/// resolution — the entry is not the update's to move (pnpm/pnpm#12115).
#[test]
fn a_declaration_an_override_replaced_is_left_alone() {
    let mut lockfile = lockfile(
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      foo:
        specifier: ^1.0.0
        version: 1.2.0
",
    );

    let bumps = bumps(&[("foo", DependencyGroup::Prod, "catalog:")]);
    apply_manifest_spec_bumps(&mut lockfile, &bumps, None);

    assert_eq!(specifier_of(lockfile.importers["."].dependencies.as_ref(), "foo"), "^1.0.0");
    assert!(bumps.applied.into_inner().expect("never poisoned").is_empty());
}

/// Mirrors `update moves a declaration a range-scoped override does not claim`
/// on the TypeScript side. A range-scoped override claims one declaration of
/// an alias and not another, so ownership is decided per declared range: the
/// `devDependencies` entry the override matches stands, and the
/// `dependencies` entry it does not match is still the update's to move.
#[test]
fn a_range_scoped_override_claims_only_the_declaration_it_matches() {
    let overrides = parse_overrides(
        &HashMap::from([("foo@^1.0.0".to_string(), "1.0.0".to_string())]),
        &Catalogs::new(),
    )
    .expect("parse the overrides");
    let overrider = VersionsOverrider::new(&overrides, Path::new("/workspace"));
    let manifest = PackageManifest::from_value(
        PathBuf::from("package.json"),
        json!({
            "name": "my-app",
            "version": "1.0.0",
            "dependencies": { "foo": "^100.0.0" },
            "devDependencies": { "foo": "^1.0.0" },
        }),
    );
    let importer_manifests = BTreeMap::from([(".".to_string(), &manifest)]);
    let overridden =
        OverriddenDeclarations { overrider: &overrider, importer_manifests: &importer_manifests };

    let mut lockfile = lockfile(
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      foo:
        specifier: ^100.0.0
        version: 100.1.0
    devDependencies:
      foo:
        specifier: ^1.0.0
        version: 1.1.0
",
    );

    let unclaimed = bumps(&[("foo", DependencyGroup::Prod, "^100.0.0")]);
    apply_manifest_spec_bumps(&mut lockfile, &unclaimed, Some(&overridden));
    assert_eq!(specifier_of(lockfile.importers["."].dependencies.as_ref(), "foo"), "^100.1.0");

    let claimed = bumps(&[("foo", DependencyGroup::Dev, "^1.0.0")]);
    apply_manifest_spec_bumps(&mut lockfile, &claimed, Some(&overridden));
    assert_eq!(specifier_of(lockfile.importers["."].dev_dependencies.as_ref(), "foo"), "^1.0.0");
    assert!(claimed.applied.into_inner().expect("never poisoned").is_empty());
}
