use super::{
    ImporterUpdateSeedPolicy, UpdateSeedPolicy, compute_package_extensions_checksum,
    full_resolution_required, importers_consuming_linked_peers,
    include_transitive_optional_dependencies, is_partial_workspace_selection, update_reuse_scopes,
    verify_merged_repair,
};
use pnpm_config::{Config, PackageExtension};
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::DependencyGroup;
use pnpm_reporter::SilentReporter;
use pretty_assertions::assert_eq;

fn config_with_extensions(entries: &[(&str, &[(&str, &str)])]) -> Box<Config> {
    let mut extensions = indexmap::IndexMap::new();
    for (selector, deps) in entries {
        let mut dependencies = std::collections::BTreeMap::new();
        for (name, range) in *deps {
            dependencies.insert((*name).to_string(), (*range).to_string());
        }
        extensions.insert(
            (*selector).to_string(),
            PackageExtension { dependencies: Some(dependencies), ..Default::default() },
        );
    }
    let mut config = Config::new();
    config.package_extensions = Some(extensions);
    Box::new(config)
}

#[test]
fn full_workspace_selection_keeps_resolution_prefetch_enabled() {
    let real = std::collections::HashSet::from_iter(["a".to_string(), "b".to_string()]);
    let all_selected = real.clone();
    let partial = std::collections::HashSet::from_iter(["a".to_string()]);

    assert!(!is_partial_workspace_selection(Some(&real), Some(&all_selected)));
    assert!(is_partial_workspace_selection(Some(&real), Some(&partial)));
    assert!(!is_partial_workspace_selection(None, None));
}

#[test]
fn partial_installs_keep_transitive_optional_dependencies() {
    let prod_only = [DependencyGroup::Prod];
    let with_optional = [DependencyGroup::Prod, DependencyGroup::Optional];

    assert!(include_transitive_optional_dependencies(false, &prod_only));
    assert!(!include_transitive_optional_dependencies(true, &prod_only));
    assert!(include_transitive_optional_dependencies(true, &with_optional));
}

#[tokio::test]
async fn filtered_repair_verifies_the_merged_lockfile() {
    let lockfile: Lockfile = serde_saphyr::from_str(
        "lockfileVersion: '9.0'\nimporters:\n  unselected:\n    dependencies:\n      '../../../escape':\n        specifier: 1.0.0\n        version: 1.0.0\n",
    )
    .expect("parse lockfile");

    let error = verify_merged_repair::<SilentReporter>(&lockfile, &[])
        .await
        .expect_err("the merged lockfile must pass structural verification");
    assert!(matches!(
        error,
        super::InstallWithFreshLockfileError::LockfileVerification(
            pnpm_lockfile_verification::VerifyError::InvalidDependencyAlias { .. }
        )
    ));
}

/// Ports `installing/.../packageExtensions.ts:103-153`
/// `packageExtensionsChecksum does not change regardless of keys
/// order` — two `Config::package_extensions` populated with the
/// same selectors and entries in a different declared order must
/// produce the same `sha256-…` lockfile checksum. Without the
/// sorted-keys hash, the order-sensitive `IndexMap` iteration
/// would flap the checksum and force a redundant full resolution
/// on every reorder.
#[test]
fn compute_checksum_is_order_invariant_across_outer_keys() {
    let config_a = config_with_extensions(&[
        ("is-odd", &[("is-number", "*")]),
        ("is-even", &[("is-number", "*")]),
    ]);
    let config_b = config_with_extensions(&[
        ("is-even", &[("is-number", "*")]),
        ("is-odd", &[("is-number", "*")]),
    ]);
    let checksum_a = compute_package_extensions_checksum(&config_a);
    let checksum_b = compute_package_extensions_checksum(&config_b);
    assert!(checksum_a.is_some(), "configured extensions must hash to Some");
    assert_eq!(checksum_a, checksum_b);
}

/// Empty / absent extensions round-trip to `None`, matching pnpm's
/// `hashObjectNullableWithPrefix(undefined) === undefined`
/// short-circuit. Without this, an absent `packageExtensions` and
/// a configured-but-empty one would write different lockfile
/// fields and the drift gate would fire on no-op installs.
#[test]
fn compute_checksum_is_none_when_extensions_absent() {
    let config = Config::new();
    assert_eq!(compute_package_extensions_checksum(&config), None);
}

/// `Some({})` (an explicitly empty map) also collapses to `None`,
/// mirroring pnpm's `if (!object || isEmpty(object)) return undefined`.
/// Without the empty-map guard, an explicit `packageExtensions: {}`
/// in pnpm-workspace.yaml — or an env-var-driven override clearing
/// a parent layer — would hash to a checksum while pnpm omits the
/// field, causing spurious drift on cross-tool installs.
#[test]
fn compute_checksum_is_none_for_explicit_empty_map() {
    let mut config = Config::new();
    config.package_extensions = Some(indexmap::IndexMap::new());
    assert_eq!(compute_package_extensions_checksum(&config), None);
}

#[test]
fn importer_scoped_update_full_resolution_requires_every_importer_to_disable_reuse() {
    use pnpm_resolving_deps_resolver::UpdateReuseScope;

    let importer_ids = ["selected", "unselected"];
    let mixed =
        std::collections::BTreeMap::from([("selected".to_string(), UpdateReuseScope::None)]);
    assert!(!full_resolution_required(true, importer_ids, &UpdateReuseScope::All, &mixed,));

    let all_none = std::collections::BTreeMap::from([
        ("selected".to_string(), UpdateReuseScope::None),
        ("unselected".to_string(), UpdateReuseScope::None),
    ]);
    assert!(full_resolution_required(true, importer_ids, &UpdateReuseScope::All, &all_none,));
    assert!(full_resolution_required(
        false,
        importer_ids,
        &UpdateReuseScope::All,
        &std::collections::BTreeMap::new(),
    ));
}

#[test]
fn importer_scoped_update_custom_refresh_widens_every_importer() {
    use pnpm_resolving_deps_resolver::UpdateReuseScope;

    let scoped = std::collections::BTreeMap::from([(
        "selected".to_string(),
        UpdateReuseScope::Except(std::iter::once(("pkg".to_string(), None)).collect()),
    )]);
    assert!(full_resolution_required(
        true,
        ["selected", "unselected"],
        &UpdateReuseScope::None,
        &scoped,
    ));
}

#[test]
fn importer_scoped_update_absent_importer_keeps_all_reuse() {
    use pnpm_resolving_deps_resolver::UpdateReuseScope;

    let policy = UpdateSeedPolicy::ByImporter {
        policies: std::collections::BTreeMap::from([(
            "selected".to_string(),
            ImporterUpdateSeedPolicy::DropAll,
        )]),
        max_depth: pnpm_resolving_deps_resolver::UpdateDepth::UNLIMITED,
    };
    let (default_scope, scopes) = update_reuse_scopes(&policy);
    assert_eq!(default_scope, UpdateReuseScope::All);
    assert_eq!(scopes.get("selected"), Some(&UpdateReuseScope::None));
    assert!(!scopes.contains_key("unselected"));
}

fn workspace_manifests(
    projects: &[(&str, serde_json::Value)],
) -> std::collections::BTreeMap<String, pnpm_package_manifest::PackageManifest> {
    projects
        .iter()
        .map(|(importer_id, manifest)| {
            let path = std::path::PathBuf::from("/repo").join(importer_id).join("package.json");
            (
                (*importer_id).to_string(),
                pnpm_package_manifest::PackageManifest::from_value(path, manifest.clone()),
            )
        })
        .collect()
}

fn linked_peer_consumers(projects: &[(&str, serde_json::Value)]) -> Vec<String> {
    let owned = workspace_manifests(projects);
    let borrowed = owned.iter().map(|(id, manifest)| (id.clone(), manifest)).collect();
    let mut consumers: Vec<String> =
        importers_consuming_linked_peers(&borrowed, std::path::Path::new("/repo"))
            .into_iter()
            .collect();
    consumers.sort();
    consumers
}

/// The candidate set stays empty when nothing in the workspace declares
/// a peer — the case the report's cost is scoped for.
#[test]
fn a_workspace_without_peer_declarations_adds_no_candidates() {
    assert_eq!(
        linked_peer_consumers(&[
            (".", serde_json::json!({ "name": "root" })),
            (
                "packages/app",
                serde_json::json!({
                    "name": "app",
                    "dependencies": { "lib": "workspace:*", "is-positive": "1.0.0" },
                }),
            ),
            ("packages/lib", serde_json::json!({ "name": "lib" })),
        ]),
        Vec::<String>::new(),
    );
}

#[test]
fn only_the_importers_linking_to_a_peer_declaring_project_become_candidates() {
    assert_eq!(
        linked_peer_consumers(&[
            (".", serde_json::json!({ "name": "root" })),
            (
                "packages/app",
                serde_json::json!({ "name": "app", "dependencies": { "lib": "workspace:*" } }),
            ),
            (
                "packages/relative",
                serde_json::json!({ "name": "relative", "dependencies": { "lib": "link:../lib" } }),
            ),
            (
                "packages/unrelated",
                serde_json::json!({ "name": "unrelated", "dependencies": { "app": "workspace:*" } }),
            ),
            (
                "packages/lib",
                serde_json::json!({ "name": "lib", "peerDependencies": { "react": "^18.0.0" } }),
            ),
        ]),
        vec!["packages/app".to_string(), "packages/relative".to_string()],
    );
}

/// `workspace:<name>@<range>` links to the project the specifier names,
/// not to the one the entry key happens to match.
#[test]
fn an_aliased_workspace_dependency_resolves_through_its_specifier() {
    assert_eq!(
        linked_peer_consumers(&[
            (".", serde_json::json!({ "name": "root" })),
            (
                "packages/app",
                serde_json::json!({
                    "name": "app",
                    "dependencies": { "decoy": "workspace:lib@*" },
                }),
            ),
            ("packages/decoy", serde_json::json!({ "name": "decoy" })),
            (
                "packages/lib",
                serde_json::json!({ "name": "lib", "peerDependencies": { "react": "^18.0.0" } }),
            ),
        ]),
        vec!["packages/app".to_string()],
    );
}

/// A workspace can hold several projects under one package name, with
/// the `workspace:` range picking between them, so any one of them
/// declaring a peer has to put the consumer in the candidate set.
#[test]
fn same_named_workspace_projects_count_when_any_version_declares_a_peer() {
    assert_eq!(
        linked_peer_consumers(&[
            (".", serde_json::json!({ "name": "root" })),
            (
                "packages/app",
                serde_json::json!({ "name": "app", "dependencies": { "lib": "workspace:*" } }),
            ),
            (
                "packages/lib-v1",
                serde_json::json!({
                    "name": "lib",
                    "version": "1.0.0",
                    "peerDependencies": { "react": "^18.0.0" },
                }),
            ),
            ("packages/lib-v2", serde_json::json!({ "name": "lib", "version": "2.0.0" })),
        ]),
        vec!["packages/app".to_string()],
    );
}

/// `link:` names a directory whatever it is called, so a tarball-looking
/// name must not exclude it the way the same name excludes a `file:`.
#[test]
fn a_link_to_a_tarball_named_directory_still_counts() {
    assert_eq!(
        linked_peer_consumers(&[
            (".", serde_json::json!({ "name": "root" })),
            (
                "packages/app",
                serde_json::json!({ "name": "app", "dependencies": { "lib": "link:../lib.tgz" } }),
            ),
            (
                "packages/lib.tgz",
                serde_json::json!({ "name": "lib", "peerDependencies": { "react": "^18.0.0" } }),
            ),
        ]),
        vec!["packages/app".to_string()],
    );
}

/// A `file:` tarball resolves to a package rather than to a directory,
/// so it never becomes the `link:` entry the report's walk inspects.
#[test]
fn a_file_tarball_dependency_adds_no_candidate() {
    assert_eq!(
        linked_peer_consumers(&[
            (".", serde_json::json!({ "name": "root" })),
            (
                "packages/app",
                serde_json::json!({
                    "name": "app",
                    "dependencies": {
                        "packed": "file:../../vendor/packed-1.0.0.tgz",
                        "archived": "file:../../vendor/archived.tar.gz",
                    },
                }),
            ),
        ]),
        Vec::<String>::new(),
    );
}

/// A `link:` target that is not a workspace project counts either way:
/// the report's walk reads its manifest when it resolves inside the
/// lockfile directory, and a symlink decides whether one that escapes
/// lexically still does.
#[test]
fn a_link_to_a_non_project_target_is_treated_as_peer_declaring() {
    let inside = linked_peer_consumers(&[
        (".", serde_json::json!({ "name": "root" })),
        (
            "packages/app",
            serde_json::json!({
                "name": "app",
                "dependencies": { "vendored": "link:../../vendor/thing" },
            }),
        ),
    ]);
    assert_eq!(inside, vec!["packages/app".to_string()]);

    let escaping = linked_peer_consumers(&[
        (".", serde_json::json!({ "name": "root" })),
        (
            "packages/app",
            serde_json::json!({
                "name": "app",
                "dependencies": { "vendored": "link:../../../outside" },
            }),
        ),
    ]);
    assert_eq!(escaping, vec!["packages/app".to_string()]);
}
