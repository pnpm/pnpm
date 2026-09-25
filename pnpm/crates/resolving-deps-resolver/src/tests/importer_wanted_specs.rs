use super::{DependencyGroup, PackageManifest};
use crate::resolve_dependency_tree::{ResolveDependencyTreeError, importer_direct_wanted_specs};
use pretty_assertions::assert_eq;

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helpers take owned literal fixtures by value to keep call sites clean"
)]
fn manifest_with(groups: serde_json::Value) -> (tempfile::TempDir, PackageManifest) {
    let mut json = serde_json::json!({ "name": "root", "version": "0.0.0" });
    json.as_object_mut()
        .unwrap()
        .extend(groups.as_object().unwrap().clone());
    manifest_of(json)
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helpers take owned literal fixtures by value to keep call sites clean"
)]
fn manifest_of(json: serde_json::Value) -> (tempfile::TempDir, PackageManifest) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let path = tmp.path().join("package.json");
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).expect("write package.json");
    let manifest = PackageManifest::from_path(path).expect("parse package.json");
    (tmp, manifest)
}

const ALL_GROUPS: [DependencyGroup; 3] =
    [DependencyGroup::Prod, DependencyGroup::Dev, DependencyGroup::Optional];

#[test]
fn regular_dep_wins_over_own_peer_with_auto_install_peers() {
    let (_tmp, manifest) = manifest_with(serde_json::json!({
        "devDependencies": { "foo": "workspace:*" },
        "peerDependencies": { "foo": "^1.0.0" },
    }));
    let wanted = importer_direct_wanted_specs(
        &manifest,
        ALL_GROUPS,
        true,
        &pnpm_catalogs_types::Catalogs::new(),
        None,
    )
    .unwrap();
    assert_eq!(wanted, vec![("foo".to_string(), "workspace:*".to_string(), false, false)]);
}

#[test]
fn peer_only_dep_is_wanted_with_auto_install_peers() {
    let (_tmp, manifest) = manifest_with(serde_json::json!({
        "peerDependencies": { "peer-only": "^2.0.0" },
    }));
    let wanted = importer_direct_wanted_specs(
        &manifest,
        ALL_GROUPS,
        true,
        &pnpm_catalogs_types::Catalogs::new(),
        None,
    )
    .unwrap();
    assert_eq!(wanted, vec![("peer-only".to_string(), "^2.0.0".to_string(), false, false)]);
}

#[test]
fn peer_only_dep_is_not_wanted_without_auto_install_peers() {
    let (_tmp, manifest) = manifest_with(serde_json::json!({
        "dependencies": { "regular": "^1.0.0" },
        "peerDependencies": { "peer-only": "^2.0.0" },
    }));
    let wanted = importer_direct_wanted_specs(
        &manifest,
        ALL_GROUPS,
        false,
        &pnpm_catalogs_types::Catalogs::new(),
        None,
    )
    .unwrap();
    assert_eq!(wanted, vec![("regular".to_string(), "^1.0.0".to_string(), false, false)]);
}

#[test]
fn later_regular_group_range_replaces_earlier_one() {
    let (_tmp, manifest) = manifest_with(serde_json::json!({
        "dependencies": { "foo": "^1.0.0" },
        "optionalDependencies": { "foo": "^2.0.0" },
    }));
    let wanted = importer_direct_wanted_specs(
        &manifest,
        ALL_GROUPS,
        false,
        &pnpm_catalogs_types::Catalogs::new(),
        None,
    )
    .unwrap();
    assert_eq!(wanted, vec![("foo".to_string(), "^2.0.0".to_string(), true, false)]);
}

/// Matches `filterDependenciesByType` in `@pnpm/pkg-manifest.utils`
/// (`{...dev, ...prod, ...optional}`): the regular range wins. The dev
/// range winning instead records an importer entry whose resolved
/// version can't satisfy the `dependencies` specifier, permanently
/// failing the prefer-frozen freshness check.
#[test]
fn regular_dep_range_wins_over_dev_range_of_same_alias() {
    let (_tmp, manifest) = manifest_with(serde_json::json!({
        "dependencies": { "foo": "1.0.0" },
        "devDependencies": { "foo": "2.0.0" },
    }));
    let wanted = importer_direct_wanted_specs(
        &manifest,
        ALL_GROUPS,
        false,
        &pnpm_catalogs_types::Catalogs::new(),
        None,
    )
    .unwrap();
    assert_eq!(wanted, vec![("foo".to_string(), "1.0.0".to_string(), false, false)]);
}

#[test]
fn rejects_invalid_peer_dependency_specification() {
    let (_tmp, manifest) = manifest_with(serde_json::json!({
        "name": "proj",
        "peerDependencies": { "@pnpm.e2e/foo": "@pnpm.e2e/foo@1.0.0" },
    }));
    let err = importer_direct_wanted_specs(
        &manifest,
        ALL_GROUPS,
        false,
        &pnpm_catalogs_types::Catalogs::new(),
        None,
    )
    .unwrap_err();
    let ResolveDependencyTreeError::InvalidPeerDependencySpecification {
        dep_name,
        project_id,
        specifier,
    } = err
    else {
        panic!("expected InvalidPeerDependencySpecification, got {err:?}");
    };
    assert_eq!(dep_name, "@pnpm.e2e/foo");
    assert_eq!(project_id, "proj");
    assert_eq!(specifier, "@pnpm.e2e/foo@1.0.0");
}

/// A workspace root usually has no `name`, so it is named by its directory.
#[test]
fn names_an_unnamed_project_by_its_directory() {
    let (tmp, manifest) = manifest_of(serde_json::json!({
        "peerDependencies": { "@pnpm.e2e/foo": "@pnpm.e2e/foo@1.0.0" },
    }));
    let err = importer_direct_wanted_specs(
        &manifest,
        ALL_GROUPS,
        false,
        &pnpm_catalogs_types::Catalogs::new(),
        None,
    )
    .unwrap_err();
    let ResolveDependencyTreeError::InvalidPeerDependencySpecification { project_id, .. } = err
    else {
        panic!("expected InvalidPeerDependencySpecification, got {err:?}");
    };
    assert_eq!(project_id, tmp.path().display().to_string());
}

#[test]
fn accepts_scheme_carrying_peer_specifiers() {
    let (_tmp, manifest) = manifest_with(serde_json::json!({
        "peerDependencies": { "foo": "workspace:^", "bar": "npm:baz@^5" },
    }));
    importer_direct_wanted_specs(
        &manifest,
        ALL_GROUPS,
        true,
        &pnpm_catalogs_types::Catalogs::new(),
        None,
    )
    .expect("scheme-carrying peer specifiers are accepted");
}
