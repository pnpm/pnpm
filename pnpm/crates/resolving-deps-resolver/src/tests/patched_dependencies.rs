use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use pnpm_lockfile::{DirectoryResolution, GitResolution, LockfileResolution};
use pnpm_package_manifest::DependencyGroup;
use pnpm_patching::{ExtendedPatchInfo, PatchGroup, PatchGroupRangeItem, PatchGroupRecord};
use pnpm_resolving_resolver_base::{PkgResolutionId, ResolveOptions};
use pretty_assertions::assert_eq;

use super::{StubResolver, fake_manifest, fake_result};
use crate::{
    resolve_dependency_tree::{
        ResolveDependencyTreeError, ResolveDependencyTreeOptions, resolve_dependency_tree,
    },
    resolve_peers::{ResolvePeersOptions, resolve_peers},
};
use pnpm_deps_path::DepPath;

fn exact_group(version: &str, key: &str, hash: &str) -> PatchGroup {
    let info =
        ExtendedPatchInfo { hash: hash.to_string(), patch_file_path: None, key: key.to_string() };
    let mut group = PatchGroup::default();
    group.exact.insert(version.to_string(), info);
    group
}

#[tokio::test]
async fn appends_patch_hash_to_pkg_id_and_records_applied_key() {
    let mut table = HashMap::default();
    table.insert(
        ("foo".to_string(), "^1.0.0".to_string()),
        fake_result("foo", "1.0.0", serde_json::json!({ "name": "foo", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

    let mut groups: PatchGroupRecord = PatchGroupRecord::new();
    groups.insert("foo".to_string(), exact_group("1.0.0", "foo@1.0.0", "abc123"));

    let mut tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: Some(Arc::new(groups)),
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .unwrap();

    assert_eq!(tree.direct.len(), 1);
    assert_eq!(tree.direct[0].id, "foo@1.0.0(patch_hash=abc123)");
    assert!(tree.packages.contains_key("foo@1.0.0(patch_hash=abc123)"));
    assert!(tree.applied_patches.contains("foo@1.0.0"));

    let result = resolve_peers(&mut tree, ResolvePeersOptions::default());
    assert_eq!(
        result.direct_dependencies_by_alias.get("foo"),
        Some(&DepPath::from("foo@1.0.0(patch_hash=abc123)".to_string())),
    );
}

#[tokio::test]
async fn patches_git_dependency_with_manifest_version() {
    let git_ref = "git+file:///repo#0123456789012345678901234567890123456789";
    let mut result =
        fake_result("foo", "1.0.0", serde_json::json!({ "name": "foo", "version": "1.0.0" }));
    result.id = PkgResolutionId::from(git_ref);
    result.name_ver = None;
    result.latest = None;
    result.resolution = LockfileResolution::Git(GitResolution {
        repo: "file:///repo".to_string(),
        commit: "0123456789012345678901234567890123456789".to_string(),
        integrity: None,
        path: None,
    });
    result.resolved_via = "git-repository".to_string();

    let mut table = HashMap::default();
    table.insert(("foo".to_string(), git_ref.to_string()), result);
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": git_ref }));
    let mut groups = PatchGroupRecord::new();
    groups.insert("foo".to_string(), exact_group("1.0.0", "foo@1.0.0", "abc123"));

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: Some(Arc::new(groups)),
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .unwrap();

    assert_eq!(tree.direct[0].id, format!("foo@{git_ref}(patch_hash=abc123)"));
    assert!(tree.applied_patches.contains("foo@1.0.0"));
}

#[tokio::test]
async fn leaves_local_directory_dependencies_unpatched() {
    let local_ref = "file:../foo";
    let mut result =
        fake_result("foo", "1.0.0", serde_json::json!({ "name": "foo", "version": "1.0.0" }));
    result.id = PkgResolutionId::from(local_ref);
    result.name_ver = None;
    result.latest = None;
    result.resolution =
        LockfileResolution::Directory(DirectoryResolution { directory: "../foo".to_string() });
    result.resolved_via = "local-filesystem".to_string();

    let mut table = HashMap::default();
    table.insert(("foo".to_string(), local_ref.to_string()), result);
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": local_ref }));
    let mut groups = PatchGroupRecord::new();
    groups.insert("foo".to_string(), exact_group("1.0.0", "foo@1.0.0", "abc123"));

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: Some(Arc::new(groups)),
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .unwrap();

    assert_eq!(tree.direct[0].id, "foo@file:../foo");
    assert!(tree.applied_patches.is_empty());
}

#[tokio::test]
async fn range_match_applies_patch_and_records_user_key() {
    let mut table = HashMap::default();
    table.insert(
        ("foo".to_string(), "^1.0.0".to_string()),
        fake_result("foo", "1.2.0", serde_json::json!({ "name": "foo", "version": "1.2.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

    let info = ExtendedPatchInfo {
        hash: "deadbeef".to_string(),
        patch_file_path: None,
        key: "foo@^1.0.0".to_string(),
    };
    let mut group = PatchGroup::default();
    group.range.push(PatchGroupRangeItem { version: "^1.0.0".to_string(), patch: info });
    let mut groups: PatchGroupRecord = PatchGroupRecord::new();
    groups.insert("foo".to_string(), group);

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: Some(Arc::new(groups)),
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .unwrap();

    assert_eq!(tree.direct[0].id, "foo@1.2.0(patch_hash=deadbeef)");
    assert!(tree.applied_patches.contains("foo@^1.0.0"));
}

#[tokio::test]
async fn unused_patch_leaves_ids_and_applied_set_alone() {
    let mut table = HashMap::default();
    table.insert(
        ("foo".to_string(), "^1.0.0".to_string()),
        fake_result("foo", "1.0.0", serde_json::json!({ "name": "foo", "version": "1.0.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

    let mut groups: PatchGroupRecord = PatchGroupRecord::new();
    groups.insert("bar".to_string(), exact_group("2.0.0", "bar@2.0.0", "abc"));

    let tree = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: Some(Arc::new(groups)),
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .unwrap();

    assert_eq!(tree.direct[0].id, "foo@1.0.0");
    assert!(tree.applied_patches.is_empty());
}

#[tokio::test]
async fn ambiguous_range_match_fails_with_patch_key_conflict() {
    let mut table = HashMap::default();
    table.insert(
        ("foo".to_string(), "^1.0.0".to_string()),
        fake_result("foo", "1.2.0", serde_json::json!({ "name": "foo", "version": "1.2.0" })),
    );
    let resolver = StubResolver { table, calls: Mutex::new(Vec::new()) };
    let (_tmp, manifest) = fake_manifest(serde_json::json!({ "foo": "^1.0.0" }));

    let mut group = PatchGroup::default();
    group.range.push(PatchGroupRangeItem {
        version: "^1.0.0".to_string(),
        patch: ExtendedPatchInfo {
            hash: "aaa".to_string(),
            patch_file_path: None,
            key: "foo@^1.0.0".to_string(),
        },
    });
    group.range.push(PatchGroupRangeItem {
        version: "~1.2.0".to_string(),
        patch: ExtendedPatchInfo {
            hash: "bbb".to_string(),
            patch_file_path: None,
            key: "foo@~1.2.0".to_string(),
        },
    });
    let mut groups: PatchGroupRecord = PatchGroupRecord::new();
    groups.insert("foo".to_string(), group);

    let err = resolve_dependency_tree(
        &resolver,
        &manifest,
        [DependencyGroup::Prod],
        ResolveDependencyTreeOptions {
            base_opts: ResolveOptions::default(),
            patched_dependencies: Some(Arc::new(groups)),
            manifest_hook: None,
            overrides_hook: None,
            pnpmfile_hook: None,
            read_package_log: None,
            auto_install_peers: false,
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ResolveDependencyTreeError::PatchKeyConflict(_)), "got: {err:?}");
}
