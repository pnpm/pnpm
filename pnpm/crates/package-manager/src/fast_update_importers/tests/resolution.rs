use super::{
    WITH_NESTED_PEER_ON_REMOVABLE_DEP, WITH_NON_ASCII_PEER_SUFFIX, WITH_PEER_ON_ANOTHER_VERSION,
    WITH_PEER_ON_REMOVABLE_DEP, WITH_PEER_ON_REMOVABLE_LINK, WITH_REMOVABLE_DEP,
    WITH_REMOVABLE_PEER_PAIR, WITH_TWO_LOCKED_VERSIONS, lockfile, manifest, manifest_from,
    parsed_lockfile, sorted_snapshot_keys, try_fast_update_importers, with_a_direct_foo_at,
    with_a_lower_peerless_foo,
};
use pnpm_config::ResolutionMode::LowestDirect as LOWEST_DIRECT;
use pnpm_lockfile::{PackageKey, PkgName};
use serde_json::json;

#[test]
fn updates_a_compatible_dependency_range() {
    let manifest = manifest(">=1 <2");
    let updated = try_fast_update_importers(&lockfile(), &[(".".to_string(), &manifest)])
        .expect("compatible range should update");
    assert_eq!(
        updated.importers["."].dependencies.as_ref().expect("dependencies")
            [&"foo".parse().expect("package name")]
            .specifier,
        ">=1 <2",
    );
}
#[test]
fn rejects_an_incompatible_dependency_range() {
    let manifest = manifest("^2");
    assert!(try_fast_update_importers(&lockfile(), &[(".".to_string(), &manifest)]).is_none());
}
#[test]
fn rejects_a_non_semver_dependency_specifier() {
    let manifest = manifest("latest");
    assert!(try_fast_update_importers(&lockfile(), &[(".".to_string(), &manifest)]).is_none());
}
#[test]
fn rejects_dropping_a_dependency_another_package_resolves_as_a_peer() {
    let manifest = manifest_from(json!({ "dependencies": { "baz": "^4.0.0" } }));

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_PEER_ON_REMOVABLE_DEP),
            &[(".".to_string(), &manifest)],
        )
        .is_none(),
        "baz's key embeds foo, so dropping foo rekeys baz rather than only pruning",
    );
}
#[test]
fn drops_a_peer_pair_removed_together() {
    let manifest = manifest_from(json!({ "dependencies": { "bar": "^2.0.0" } }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_REMOVABLE_PEER_PAIR),
        &[(".".to_string(), &manifest)],
    )
    .expect("the peer-dependent snapshot is unreachable after the removal, so nothing rekeys");

    let mut packages: Vec<_> =
        updated.packages.as_ref().expect("packages").keys().map(ToString::to_string).collect();
    packages.sort();
    assert_eq!(packages, vec!["bar@2.0.0".to_string()]);
}
#[test]
fn moves_a_group_alongside_a_satisfied_range_change() {
    let manifest = manifest_from(json!({
        "dependencies": { "foo": "^1.0.0" },
        "devDependencies": { "bar": ">=2 <3" },
    }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_REMOVABLE_DEP),
        &[(".".to_string(), &manifest)],
    )
    .expect("both edits stay within the importer");

    let moved = &updated.importers["."].dev_dependencies.as_ref().expect("devDependencies")
        [&"bar".parse::<PkgName>().expect("alias")];
    assert_eq!(moved.specifier, ">=2 <3");
}
#[test]
fn rejects_a_widened_range_when_resolution_would_pick_its_lowest_locked_version() {
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^1.0.0" } }));
    let other = manifest_from(json!({ "dependencies": { "foo": "1.2.0" } }));
    let config = pnpm_config::Config { resolution_mode: LOWEST_DIRECT, ..Default::default() };

    assert!(
        crate::fast_update_compose::try_compose_fast_updates(
            &parsed_lockfile(WITH_TWO_LOCKED_VERSIONS),
            &[(".".to_string(), &manifest), ("pkg-a".to_string(), &other)],
            &[],
            &config,
            None,
            false,
        )
        .is_none(),
        "which end of the range applies is not a property of the lockfile",
    );
}
#[test]
fn rejects_a_range_no_locked_version_satisfies() {
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^2.0.0" } }));
    let other = manifest_from(json!({ "dependencies": { "foo": "1.2.0" } }));

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_TWO_LOCKED_VERSIONS),
            &[(".".to_string(), &manifest), ("pkg-a".to_string(), &other)],
        )
        .is_none(),
        "only the resolver can fetch a version the lockfile does not hold",
    );
}
#[test]
fn drops_a_dependency_whose_version_no_surviving_peer_suffix_names() {
    let manifest = manifest_from(json!({ "dependencies": { "qux": "^5.0.0" } }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_PEER_ON_ANOTHER_VERSION),
        &[(".".to_string(), &manifest)],
    )
    .expect("the surviving suffix names the version qux provides, not the dropped one");

    assert_eq!(
        sorted_snapshot_keys(&updated),
        vec!["baz@4.0.0(foo@1.2.0)".to_string(), "foo@1.2.0".to_string(), "qux@5.0.0".to_string()],
    );
}
#[test]
fn rejects_dropping_a_dependency_a_nested_peer_suffix_segment_names() {
    let manifest = manifest_from(json!({ "dependencies": { "baz": "^4.0.0" } }));

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_NESTED_PEER_ON_REMOVABLE_DEP),
            &[(".".to_string(), &manifest)],
        )
        .is_none(),
        "the peers of a peer are as much a part of baz's key as the top-level ones",
    );
}
#[test]
fn reads_a_peer_suffix_segment_that_starts_with_a_multi_byte_character() {
    let manifest = manifest_from(json!({ "dependencies": { "baz": "^4.0.0" } }));

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_NON_ASCII_PEER_SUFFIX),
            &[(".".to_string(), &manifest)],
        )
        .is_some(),
        "the segment names neither foo nor anything else dropped",
    );
}
#[test]
fn rejects_dropping_a_linked_dependency_a_surviving_peer_suffix_names() {
    let manifest = manifest_from(json!({ "dependencies": { "baz": "^4.0.0" } }));

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_PEER_ON_REMOVABLE_LINK),
            &[(".".to_string(), &manifest)],
        )
        .is_none(),
        "nothing pins the link to a version, so every suffix naming it stays suspect",
    );
}
#[test]
fn moves_a_range_past_a_peer_suffix_naming_the_version_it_moves_to() {
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^1.1.0", "qux": "^5.0.0" } }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_PEER_ON_ANOTHER_VERSION),
        &[(".".to_string(), &manifest)],
    )
    .expect("baz already resolves the peer to the version the importer moves to");

    let alias: PkgName = "foo".parse().expect("alias");
    assert_eq!(
        updated.importers["."].dependencies.as_ref().expect("dependencies")[&alias]
            .version
            .to_string(),
        "1.2.0",
    );
    assert_eq!(
        sorted_snapshot_keys(&updated),
        vec!["baz@4.0.0(foo@1.2.0)".to_string(), "foo@1.2.0".to_string(), "qux@5.0.0".to_string()],
    );
}
#[test]
fn rejects_a_range_move_a_peer_suffix_names_the_version_it_moves_off() {
    let mut subject = parsed_lockfile(WITH_PEER_ON_ANOTHER_VERSION);
    subject
        .importers
        .get_mut(".")
        .expect("importer")
        .dependencies
        .as_mut()
        .expect("dependencies")
        .insert(
            "baz".parse().expect("alias"),
            serde_saphyr::from_str("{specifier: ^4.0.0, version: 4.0.0(foo@1.0.0)}")
                .expect("dependency"),
        );
    subject.snapshots.as_mut().expect("snapshots").insert(
        "baz@4.0.0(foo@1.0.0)".parse().expect("snapshot key"),
        serde_saphyr::from_str("dependencies:\n  foo: 1.0.0").expect("snapshot"),
    );
    let manifest = manifest_from(
        json!({ "dependencies": { "foo": "^1.1.0", "qux": "^5.0.0", "baz": "^4.0.0" } }),
    );

    assert!(
        try_fast_update_importers(&subject, &[(".".to_string(), &manifest)]).is_none(),
        "baz resolved the peer to the version the importer moves off, so its key would change",
    );
}
#[test]
fn rejects_a_range_when_the_alias_also_has_a_named_registry_key() {
    let mut subject = parsed_lockfile(WITH_TWO_LOCKED_VERSIONS);
    let packages = subject.packages.as_mut().expect("packages");
    let extra = packages[&"foo@1.2.0".parse::<PackageKey>().expect("package key")].clone();
    packages.insert("foo@work:1.4.0".parse().expect("package key"), extra);
    let snapshots = subject.snapshots.as_mut().expect("snapshots");
    let extra = snapshots[&"foo@1.2.0".parse::<PackageKey>().expect("snapshot key")].clone();
    snapshots.insert("foo@work:1.4.0".parse().expect("snapshot key"), extra);
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^1.1.0" } }));

    assert!(
        try_fast_update_importers(&subject, &[(".".to_string(), &manifest)]).is_none(),
        "the alias spans two registries, so a plain reference cannot say which is meant",
    );
}
#[test]
fn rejects_moving_a_range_onto_a_version_locked_only_as_a_peer_variant() {
    let manifest = manifest_from(
        json!({ "dependencies": { "bar": "^2.0.0", "qux": "^5.0.0", "foo": "^1.1.0" } }),
    );

    assert!(
        try_fast_update_importers(&with_a_lower_peerless_foo(), &[(".".to_string(), &manifest)])
            .is_none(),
        "the only locked 1.1.0 is a peer variant, which a moved edge cannot name unsuffixed",
    );
}
#[test]
fn rejects_a_range_change_on_an_edge_whose_peer_suffix_names_no_snapshot() {
    let manifest = manifest_from(
        json!({ "dependencies": { "bar": "^2.0.0", "qux": "^5.0.0", "foo": "^1.1.0" } }),
    );

    assert!(
        try_fast_update_importers(
            &with_a_direct_foo_at("1.1.0(bar@1.0.0)"),
            &[(".".to_string(), &manifest)],
        )
        .is_none(),
        "the recorded suffix names no snapshot, so keeping the record would keep the link dangling",
    );
}
