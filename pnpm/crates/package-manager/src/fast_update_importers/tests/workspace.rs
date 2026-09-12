use super::{
    TWO_IMPORTERS, WITH_A_NEW_PROJECT, WITH_ONLY_A_PEER_VARIANT, WITH_SHARED_OPTIONAL_CHILD,
    WITH_TWO_IMPORTERS, WITH_TWO_LOCKED_VERSIONS, a_new_project_lockfile_projects, manifest,
    manifest_from, parsed_lockfile, projects_of_a_new_project_lockfile, snapshot_optional,
    try_fast_update_importers, try_prune_stale_importers, with_a_direct_foo_at,
};
use pnpm_config::ResolutionMode::LowestDirect as LOWEST_DIRECT;
use pnpm_lockfile::PkgName;
use pnpm_package_manifest::PackageManifest;
use serde_json::json;
use std::path::PathBuf;

#[test]
fn moves_a_widened_range_to_the_higher_version_another_importer_locks() {
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^1.1.0" } }));
    let other = manifest_from(json!({ "dependencies": { "foo": "1.2.0" } }));

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_TWO_LOCKED_VERSIONS),
        &[(".".to_string(), &manifest), ("pkg-a".to_string(), &other)],
    )
    .expect("the version is already in the lockfile, so nothing needs resolving");

    let alias: PkgName = "foo".parse().expect("alias");
    let recorded = &updated.importers["."].dependencies.as_ref().expect("dependencies")[&alias];
    assert_eq!(
        (recorded.specifier.as_str(), recorded.version.to_string().as_str()),
        ("^1.1.0", "1.2.0"),
    );
    let mut packages: Vec<_> =
        updated.packages.as_ref().expect("packages").keys().map(ToString::to_string).collect();
    packages.sort();
    assert_eq!(
        packages,
        vec!["foo@1.2.0".to_string()],
        "the version it left is unreachable and goes",
    );
}
#[test]
fn writes_a_new_project_importer_from_the_highest_locked_versions() {
    let added = manifest_from(json!({ "devDependencies": { "child": "^3.0.0" } }));
    let [existing, locks_the_higher_child] = a_new_project_lockfile_projects(&added);

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_A_NEW_PROJECT),
        &projects_of_a_new_project_lockfile(&existing, &locks_the_higher_child, &added),
    )
    .expect("every version the new project needs is already locked");

    let alias: PkgName = "child".parse().expect("alias");
    let recorded =
        &updated.importers["pkg-b"].dev_dependencies.as_ref().expect("devDependencies")[&alias];
    assert_eq!(
        (recorded.specifier.as_str(), recorded.version.to_string().as_str()),
        ("^3.0.0", "3.1.0"),
        "resolution dedupes onto the highest locked version the range admits",
    );
    assert!(
        snapshot_optional(&updated, "child@3.0.0"),
        "the version the new project did not take keeps the flag its only path gives it",
    );
    assert!(snapshot_optional(&updated, "opt@5.0.0"), "nothing else changed about the old path");
}
#[test]
fn clears_an_optional_flag_a_new_projects_plain_dependency_reaches() {
    let added = manifest_from(json!({ "dependencies": { "child": "3.0.0" } }));
    let [existing, locks_the_higher_child] = a_new_project_lockfile_projects(&added);

    let updated = try_fast_update_importers(
        &parsed_lockfile(WITH_A_NEW_PROJECT),
        &projects_of_a_new_project_lockfile(&existing, &locks_the_higher_child, &added),
    )
    .expect("the version the new project pins is already locked");

    assert!(
        !snapshot_optional(&updated, "child@3.0.0"),
        "the new project reaches it outside optionalDependencies",
    );
}
#[test]
fn rejects_a_new_project_whose_dependency_is_not_locked() {
    let added = manifest_from(json!({ "dependencies": { "extra": "^1.0.0" } }));
    let [existing, locks_the_higher_child] = a_new_project_lockfile_projects(&added);

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_A_NEW_PROJECT),
            &projects_of_a_new_project_lockfile(&existing, &locks_the_higher_child, &added),
        )
        .is_none(),
        "only the resolver can fetch a version the lockfile does not hold",
    );
}
#[test]
fn rejects_a_new_project_that_declares_a_workspace_protocol_dependency() {
    let added = manifest_from(json!({ "dependencies": { "child": "workspace:^" } }));
    let [existing, locks_the_higher_child] = a_new_project_lockfile_projects(&added);

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_A_NEW_PROJECT),
            &projects_of_a_new_project_lockfile(&existing, &locks_the_higher_child, &added),
        )
        .is_none(),
        "a workspace dependency resolves to a directory, not to a locked version",
    );
}
#[test]
fn rejects_a_new_project_that_declares_a_link_protocol_dependency() {
    let added = manifest_from(json!({ "dependencies": { "child": "link:../child" } }));
    let [existing, locks_the_higher_child] = a_new_project_lockfile_projects(&added);

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_A_NEW_PROJECT),
            &projects_of_a_new_project_lockfile(&existing, &locks_the_higher_child, &added),
        )
        .is_none(),
        "a link resolves to a directory, not to a locked version",
    );
}
#[test]
fn rejects_a_new_project_that_depends_on_a_workspace_sibling() {
    let added = manifest_from(json!({ "dependencies": { "child": "^3.0.0" } }));
    let [existing, locks_the_higher_child] = a_new_project_lockfile_projects(&added);
    let sibling = PackageManifest::from_value(
        PathBuf::from("/workspace/child/package.json"),
        json!({ "name": "child", "version": "3.0.0" }),
    );

    assert!(
        crate::fast_update_compose::try_compose_fast_updates(
            &parsed_lockfile(WITH_A_NEW_PROJECT),
            &projects_of_a_new_project_lockfile(&existing, &locks_the_higher_child, &added),
            &[(PathBuf::from("/workspace/child"), &sibling)],
            &pnpm_config::Config::default(),
            None,
            false,
        )
        .is_none(),
        "a plain range on a workspace project may resolve to a link, which only the resolver decides",
    );
}
#[test]
fn rejects_a_new_project_when_resolution_would_pick_the_lowest_of_several_locked_versions() {
    let added = manifest_from(json!({ "dependencies": { "child": "^3.0.0" } }));
    let [existing, locks_the_higher_child] = a_new_project_lockfile_projects(&added);
    let config = pnpm_config::Config { resolution_mode: LOWEST_DIRECT, ..Default::default() };

    assert!(
        crate::fast_update_compose::try_compose_fast_updates(
            &parsed_lockfile(WITH_A_NEW_PROJECT),
            &projects_of_a_new_project_lockfile(&existing, &locks_the_higher_child, &added),
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
fn drops_the_importer_of_a_workspace_project_that_is_gone() {
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^1.0.0" } }));

    let updated = try_prune_stale_importers(
        &parsed_lockfile(WITH_TWO_IMPORTERS),
        &[("packages/a".to_string(), &manifest)],
    )
    .expect("dropping a project's importer needs no resolution");

    assert_eq!(updated.importers.keys().collect::<Vec<_>>(), vec!["packages/a"]);
    let mut packages: Vec<_> =
        updated.packages.as_ref().expect("packages").keys().map(ToString::to_string).collect();
    packages.sort();
    assert_eq!(packages, vec!["foo@1.1.0".to_string()], "what only it needed goes with it");
}
#[test]
fn keeps_the_importer_when_the_run_does_not_see_every_project() {
    let manifest = manifest_from(json!({ "dependencies": { "foo": "^1.0.0" } }));

    assert!(
        crate::fast_update_compose::try_compose_fast_updates(
            &parsed_lockfile(WITH_TWO_IMPORTERS),
            &[("packages/a".to_string(), &manifest)],
            &[],
            &pnpm_config::Config::default(),
            None,
            false,
        )
        .is_none(),
        "a filtered run cannot tell a removed project from an unselected one",
    );
}
#[test]
fn rejects_dropping_an_importer_a_survivor_links_to() {
    let mut subject = parsed_lockfile(WITH_TWO_IMPORTERS);
    subject
        .importers
        .get_mut("packages/a")
        .expect("importer")
        .dependencies
        .as_mut()
        .expect("dependencies")
        .insert(
            "b".parse().expect("alias"),
            serde_saphyr::from_str("{specifier: workspace:1.0.0, version: link:../b}")
                .expect("dependency"),
        );
    let manifest =
        manifest_from(json!({ "dependencies": { "foo": "^1.0.0", "b": "workspace:1.0.0" } }));

    assert!(
        try_prune_stale_importers(&subject, &[("packages/a".to_string(), &manifest)]).is_none(),
        "a project that is gone while something links to it is a broken workspace",
    );
}
#[test]
fn rejects_adding_a_dependency_naming_a_workspace_project() {
    let manifest = manifest_from(
        json!({ "dependencies": { "bar": "^2.0.0", "opt": "^5.0.0", "child": "^3.0.0" } }),
    );
    let sibling = manifest_from(json!({ "name": "child" }));

    assert!(
        crate::fast_update_compose::try_compose_fast_updates(
            &parsed_lockfile(WITH_SHARED_OPTIONAL_CHILD),
            &[(".".to_string(), &manifest)],
            &[(PathBuf::from("/child/package.json"), &sibling)],
            &pnpm_config::Config::default(),
            None,
            false,
        )
        .is_none(),
        "only the resolver decides whether a workspace project is linked",
    );
}
#[test]
fn a_resolve_needing_importer_vetoes_absorbable_siblings_in_either_order() {
    let lockfile = parsed_lockfile(TWO_IMPORTERS);
    let absorbable = manifest(">=1 <2");
    let needs_resolve = manifest("^2");
    assert!(
        try_fast_update_importers(
            &lockfile,
            &[("a".to_string(), &absorbable), ("b".to_string(), &needs_resolve)],
        )
        .is_none(),
        "needs-resolve after absorbable must veto",
    );
    assert!(
        try_fast_update_importers(
            &lockfile,
            &[("a".to_string(), &needs_resolve), ("b".to_string(), &absorbable)],
        )
        .is_none(),
        "needs-resolve before absorbable must veto",
    );

    // Control: with both importers absorbable the compose applies.
    let clean = manifest("^1.0.0");
    let updated = try_fast_update_importers(
        &lockfile,
        &[("a".to_string(), &absorbable), ("b".to_string(), &clean)],
    )
    .expect("absorbable + clean should compose");
    assert_eq!(
        updated.importers["a"].dependencies.as_ref().expect("dependencies")
            [&"foo".parse().expect("package name")]
            .specifier,
        ">=1 <2",
    );
}
#[test]
fn rejects_a_new_project_whose_dependency_is_locked_only_as_a_peer_variant() {
    let existing = manifest_from(json!({ "dependencies": { "bar": "^2.0.0", "qux": "^5.0.0" } }));
    let added = manifest_from(json!({ "dependencies": { "foo": "^1.0.0" } }));

    assert!(
        try_fast_update_importers(
            &parsed_lockfile(WITH_ONLY_A_PEER_VARIANT),
            &[(".".to_string(), &existing), ("packages/a".to_string(), &added)],
        )
        .is_none(),
        "a whole new importer is written from the same locked versions as a single edge",
    );
}
#[test]
fn updates_a_range_that_stays_on_the_peer_variant_the_importer_records() {
    let manifest = manifest_from(
        json!({ "dependencies": { "bar": "^2.0.0", "qux": "^5.0.0", "foo": "^1.1.0" } }),
    );

    let updated = try_fast_update_importers(
        &with_a_direct_foo_at("1.1.0(bar@2.0.0)"),
        &[(".".to_string(), &manifest)],
    )
    .expect("the edge stays on the version it already names, suffix and all");

    let foo = &updated.importers["."].dependencies.as_ref().expect("dependencies")
        [&"foo".parse::<PkgName>().expect("alias")];
    assert_eq!(foo.specifier, "^1.1.0");
    assert_eq!(foo.version.to_string(), "1.1.0(bar@2.0.0)");
}
