use super::{
    WITH_HASHED_PEER_SUFFIX, WITH_PEER_ON_ANOTHER_VERSION, WITH_REMOVABLE_DEP,
    WITH_SHARED_OPTIONAL_CHILD, WITH_THREE_GROUPS, WITH_TWO_LOCKED_VERSIONS, manifest_from,
    parsed_lockfile, snapshot_optional, try_fast_update_importers,
    with_a_bare_snapshot_beside_the_peer_variant, with_a_second_locked_child,
};
use pnpm_config::ResolutionMode::LowestDirect as LOWEST_DIRECT;
use pnpm_lockfile::PkgName;
use serde_json::json;

#[test]
fn drops_a_dependency_the_manifest_no_longer_declares() {
    let manifest = manifest_from(json!({ "dependencies": { "bar": "^2.0.0" } }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_REMOVABLE_DEP),
        &[(".".to_string(), &manifest)],
    )
    .expect("dropping an importer edge needs no resolution");

    let importer = &updated.importers["."];
    let dependencies = importer.dependencies.as_ref().expect("dependencies");
    assert!(!dependencies.contains_key(&"foo".parse::<PkgName>().expect("alias")));
    assert!(dependencies.contains_key(&"bar".parse::<PkgName>().expect("alias")));
    assert_eq!(
        importer.specifiers.as_ref().expect("specifiers").keys().collect::<Vec<_>>(),
        vec!["bar"],
    );
    let mut packages: Vec<_> =
        updated.packages.as_ref().expect("packages").keys().map(ToString::to_string).collect();
    packages.sort();
    assert_eq!(
        packages,
        vec!["bar@2.0.0".to_string(), "child@3.0.0".to_string()],
        "the dropped package goes with its subtree, and shared entries stay",
    );
}
#[test]
fn rejects_dropping_a_dependency_when_a_surviving_suffix_is_hashed() {
    let manifest = manifest_from(json!({ "dependencies": { "baz": "^4.0.0" } }));

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_HASHED_PEER_SUFFIX),
            &[(".".to_string(), &manifest)],
        )
        .is_none(),
        "a shortened suffix cannot be checked for the dropped package",
    );
}
#[test]
fn moves_a_dependency_between_prod_and_dev_without_touching_snapshots() {
    let manifest = manifest_from(json!({
        "dependencies": { "foo": "^1.0.0" },
        "devDependencies": { "bar": "^2.0.0" },
    }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_REMOVABLE_DEP),
        &[(".".to_string(), &manifest)],
    )
    .expect("a group move needs no resolution");

    let importer = &updated.importers["."];
    let alias: PkgName = "bar".parse().expect("alias");
    assert!(importer.dependencies.as_ref().is_some_and(|deps| !deps.contains_key(&alias)));
    let moved = &importer.dev_dependencies.as_ref().expect("devDependencies")[&alias];
    assert_eq!((moved.specifier.as_str(), moved.version.to_string().as_str()), ("^2.0.0", "2.0.0"));
    assert_eq!(updated.snapshots, parsed_lockfile(WITH_REMOVABLE_DEP).snapshots);
}
#[test]
fn marks_the_subtree_optional_on_a_move_into_optional_dependencies() {
    let manifest = manifest_from(json!({
        "dependencies": { "foo": "^1.0.0" },
        "optionalDependencies": { "bar": "^2.0.0" },
    }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_REMOVABLE_DEP),
        &[(".".to_string(), &manifest)],
    )
    .expect("a group move needs no resolution");

    let alias: PkgName = "bar".parse().expect("alias");
    assert!(
        updated.importers["."]
            .optional_dependencies
            .as_ref()
            .is_some_and(|deps| deps.contains_key(&alias)),
    );
    assert!(snapshot_optional(&updated, "bar@2.0.0"));
    assert!(snapshot_optional(&updated, "child@3.0.0"));
    assert!(!snapshot_optional(&updated, "foo@1.1.0"));
}
#[test]
fn clears_the_subtree_flags_on_a_move_out_of_optional_dependencies() {
    let manifest = manifest_from(json!({
        "dependencies": { "bar": "^2.0.0" },
        "optionalDependencies": { "opt": "^5.0.0" },
    }));
    let mut subject = parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD);
    let importer = subject.importers.get_mut(".").expect("importer");
    let alias: PkgName = "bar".parse().expect("alias");
    let moved = importer.dependencies.as_mut().expect("dependencies").remove(&alias).expect("bar");
    importer.dependencies = None;
    importer
        .optional_dependencies
        .as_mut()
        .expect("optionalDependencies")
        .insert(alias.clone(), moved);
    let snapshots = subject.snapshots.as_mut().expect("snapshots");
    for key in ["bar@2.0.0", "child@3.0.0"] {
        snapshots.get_mut(&key.parse().expect("snapshot key")).expect("snapshot").optional = true;
    }

    let updated = try_fast_update_importers(&subject, &[(".".to_string(), &manifest)])
        .expect("a group move needs no resolution");

    assert!(
        updated.importers["."].dependencies.as_ref().is_some_and(|deps| deps.contains_key(&alias)),
    );
    assert!(!snapshot_optional(&updated, "bar@2.0.0"));
    assert!(!snapshot_optional(&updated, "child@3.0.0"), "bar reaches child non-optionally again");
    assert!(snapshot_optional(&updated, "opt@5.0.0"));
}
#[test]
fn stands_aside_when_every_dependency_is_in_its_recorded_group() {
    let manifest = manifest_from(json!({
        "dependencies": { "bar": "^2.0.0" },
        "optionalDependencies": { "opt": "^5.0.0" },
    }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD),
        &[(".".to_string(), &manifest)],
    );

    assert!(updated.is_none(), "nothing changed, so the handler stands aside");
}
#[test]
fn keeps_a_child_another_prod_dependency_reaches_non_optional() {
    let mut subject = parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD);
    let importer = subject.importers.get_mut(".").expect("importer");
    importer.dependencies.as_mut().expect("dependencies").insert(
        "keeper".parse().expect("alias"),
        serde_saphyr::from_str("{specifier: ^6.0.0, version: 6.0.0}").expect("dependency"),
    );
    let snapshots = subject.snapshots.as_mut().expect("snapshots");
    snapshots.insert(
        "keeper@6.0.0".parse().expect("snapshot key"),
        serde_saphyr::from_str("dependencies:\n  child: 3.0.0").expect("snapshot"),
    );
    let manifest = manifest_from(json!({
        "dependencies": { "keeper": "^6.0.0" },
        "optionalDependencies": { "bar": "^2.0.0", "opt": "^5.0.0" },
    }));

    let updated = try_fast_update_importers(&subject, &[(".".to_string(), &manifest)])
        .expect("a group move needs no resolution");

    assert!(snapshot_optional(&updated, "bar@2.0.0"));
    assert!(
        !snapshot_optional(&updated, "child@3.0.0"),
        "keeper still reaches child through prod edges",
    );
}
#[test]
fn flips_a_shared_child_optional_when_a_removal_severs_the_prod_path() {
    let manifest = manifest_from(json!({ "optionalDependencies": { "opt": "^5.0.0" } }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD),
        &[(".".to_string(), &manifest)],
    )
    .expect("dropping an importer edge needs no resolution");

    assert!(
        snapshot_optional(&updated, "child@3.0.0"),
        "only the optional path reaches child once bar is gone",
    );
}
#[test]
fn records_an_alias_declared_in_both_prod_and_optional_as_optional() {
    let manifest = manifest_from(json!({
        "dependencies": { "foo": "^1.0.0", "bar": "^2.0.0" },
        "optionalDependencies": { "bar": "^2.0.0" },
    }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_REMOVABLE_DEP),
        &[(".".to_string(), &manifest)],
    )
    .expect("a group move needs no resolution");

    let alias: PkgName = "bar".parse().expect("alias");
    assert!(
        updated.importers["."]
            .optional_dependencies
            .as_ref()
            .is_some_and(|deps| deps.contains_key(&alias)),
        "optional wins when the manifest declares both",
    );
}
#[test]
fn moves_several_dependencies_between_groups_in_one_pass() {
    let manifest = manifest_from(json!({
        "dependencies": { "qux": "^5.0.0" },
        "devDependencies": { "foo": "^1.0.0" },
        "optionalDependencies": { "bar": "^2.0.0" },
    }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_THREE_GROUPS),
        &[(".".to_string(), &manifest)],
    )
    .expect("group moves need no resolution");

    let importer = &updated.importers["."];
    let recorded_aliases = |group: &Option<pnpm_lockfile::ResolvedDependencyMap>| {
        group
            .as_ref()
            .map(|dependencies| {
                let mut aliases: Vec<_> = dependencies.keys().map(ToString::to_string).collect();
                aliases.sort();
                aliases
            })
            .unwrap_or_default()
    };
    assert_eq!(recorded_aliases(&importer.dependencies), vec!["qux".to_string()]);
    assert_eq!(recorded_aliases(&importer.dev_dependencies), vec!["foo".to_string()]);
    assert_eq!(recorded_aliases(&importer.optional_dependencies), vec!["bar".to_string()]);
    assert!(snapshot_optional(&updated, "bar@2.0.0"));
    assert!(snapshot_optional(&updated, "child@3.0.0"));
    assert!(!snapshot_optional(&updated, "foo@1.1.0"));
    assert!(!snapshot_optional(&updated, "qux@5.0.0"));
}
#[test]
fn moves_to_a_higher_locked_version_even_when_the_locked_one_still_satisfies() {
    let manifest = manifest_from(json!({ "dependencies": { "foo": ">=1.0.0" } }));
    let other = manifest_from(json!({ "dependencies": { "foo": "1.2.0" } }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_TWO_LOCKED_VERSIONS),
        &[(".".to_string(), &manifest), ("pkg-a".to_string(), &other)],
    )
    .expect("resolution would dedupe onto the higher locked version");

    let alias: PkgName = "foo".parse().expect("alias");
    assert_eq!(
        updated.importers["."].dependencies.as_ref().expect("dependencies")[&alias]
            .version
            .to_string(),
        "1.2.0",
    );
}
#[test]
fn rejects_a_higher_version_that_exists_only_under_a_named_registry() {
    let mut subject = parsed_lockfile(WITH_TWO_LOCKED_VERSIONS);
    let packages = subject.packages.as_mut().expect("packages");
    let higher = packages.remove(&"foo@1.2.0".parse().expect("package key")).expect("foo@1.2.0");
    packages.insert("foo@work:1.2.0".parse().expect("package key"), higher);
    let snapshots = subject.snapshots.as_mut().expect("snapshots");
    let higher = snapshots.remove(&"foo@1.2.0".parse().expect("snapshot key")).expect("foo@1.2.0");
    snapshots.insert("foo@work:1.2.0".parse().expect("snapshot key"), higher);
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^1.1.0" } }));

    assert!(
        try_fast_update_importers(&subject, &[(".".to_string(), &manifest)]).is_none(),
        "a registry-qualified key's semver only pins a version inside that registry",
    );
}
#[test]
fn drops_a_dependency_a_surviving_suffix_only_ends_with_the_name_of() {
    let mut subject = parsed_lockfile(WITH_PEER_ON_ANOTHER_VERSION);
    let snapshots = subject.snapshots.as_mut().expect("snapshots");
    snapshots.insert(
        "baz@4.0.0(@scope/foo@1.0.0)".parse().expect("snapshot key"),
        serde_saphyr::from_str("dependencies:\n  '@scope/foo': 1.0.0").expect("snapshot"),
    );
    snapshots.insert(
        "@scope/foo@1.0.0".parse().expect("snapshot key"),
        pnpm_lockfile::SnapshotEntry::default(),
    );
    snapshots
        .get_mut(&"qux@5.0.0".parse().expect("snapshot key"))
        .expect("qux")
        .dependencies
        .as_mut()
        .expect("dependencies")
        .insert(
            "baz".parse().expect("alias"),
            "4.0.0(@scope/foo@1.0.0)".parse().expect("reference"),
        );
    let manifest = manifest_from(json!({ "dependencies": { "qux": "^5.0.0" } }));

    assert!(
        try_fast_update_importers(&subject, &[(".".to_string(), &manifest)]).is_some(),
        "@scope/foo is not foo, however the two names end",
    );
}
#[test]
fn adds_a_dependency_at_the_highest_locked_version_satisfying_it() {
    let subject = with_a_second_locked_child();
    let manifest = manifest_from(
        json!({ "dependencies": { "bar": "^2.0.0", "opt": "^5.0.0", "child": "^3.0.0" } }),
    );
    let updated = try_fast_update_importers(&subject, &[(".".to_string(), &manifest)])
        .expect("the lockfile already holds a version satisfying the new dependency");

    let dependencies = updated.importers["."].dependencies.as_ref().expect("dependencies");
    let added = &dependencies[&"child".parse::<PkgName>().expect("alias")];
    assert_eq!(added.specifier, "^3.0.0");
    assert_eq!(added.version.to_string(), "3.1.0");
}
#[test]
fn adding_a_dependency_clears_the_optional_flag_of_what_it_reaches() {
    let mut subject = parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD);
    let importer = subject.importers.get_mut(".").expect("importer");
    importer.dependencies = None;
    let snapshots = subject.snapshots.as_mut().expect("snapshots");
    for key in ["bar@2.0.0", "child@3.0.0"] {
        snapshots.get_mut(&key.parse().expect("snapshot key")).expect("snapshot").optional = true;
    }
    let manifest = manifest_from(json!({
        "dependencies": { "child": "^3.0.0" },
        "optionalDependencies": { "opt": "^5.0.0" },
    }));

    let updated = try_fast_update_importers(&subject, &[(".".to_string(), &manifest)])
        .expect("the added dependency is already locked");

    assert!(!snapshot_optional(&updated, "child@3.0.0"), "a prod path now reaches child");
}
#[test]
fn adding_an_optional_dependency_leaves_the_flags_alone() {
    let mut subject = parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD);
    let importer = subject.importers.get_mut(".").expect("importer");
    importer.dependencies = None;
    let snapshots = subject.snapshots.as_mut().expect("snapshots");
    for key in ["bar@2.0.0", "child@3.0.0"] {
        snapshots.get_mut(&key.parse().expect("snapshot key")).expect("snapshot").optional = true;
    }
    let manifest =
        manifest_from(json!({ "optionalDependencies": { "opt": "^5.0.0", "child": "^3.0.0" } }));

    let updated = try_fast_update_importers(&subject, &[(".".to_string(), &manifest)])
        .expect("the added dependency is already locked");

    assert!(snapshot_optional(&updated, "child@3.0.0"));
}
#[test]
fn rejects_adding_a_dependency_no_locked_version_satisfies() {
    let manifest = manifest_from(
        json!({ "dependencies": { "bar": "^2.0.0", "opt": "^5.0.0", "child": "^4.0.0" } }),
    );

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD),
            &[(".".to_string(), &manifest)],
        )
        .is_none(),
        "only the resolver can fetch a version the lockfile does not hold",
    );
}
#[test]
fn rejects_adding_a_dependency_several_locked_versions_satisfy_when_resolution_picks_lowest() {
    let subject = with_a_second_locked_child();
    let manifest = manifest_from(
        json!({ "dependencies": { "bar": "^2.0.0", "opt": "^5.0.0", "child": "^3.0.0" } }),
    );
    let config = pnpm_config::Config { resolution_mode: LOWEST_DIRECT, ..Default::default() };

    assert!(
        crate::fast_update_compose::try_compose_fast_updates(
            &subject,
            &[(".".to_string(), &manifest)],
            &[],
            &config,
            None,
            false,
        )
        .is_none(),
        "which end of the range a direct dependency takes is not a property of the lockfile",
    );
}
#[test]
fn rejects_adding_a_dependency_whose_version_is_locked_both_ways() {
    let manifest = manifest_from(
        json!({ "dependencies": { "bar": "^2.0.0", "qux": "^5.0.0", "foo": "^1.0.0" } }),
    );

    assert!(
        try_fast_update_importers(
            &with_a_bare_snapshot_beside_the_peer_variant(),
            &[(".".to_string(), &manifest)],
        )
        .is_none(),
        "a bare snapshot beside the peer variant does not say which of the two a direct edge takes",
    );
}
