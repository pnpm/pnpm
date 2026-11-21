use super::validate_virtual_store_slot_containment;
use crate::VirtualStoreLayout;
use miette::Diagnostic;
use pnpm_lockfile::{
    PackageKey,
    SnapshotEntry,
};
use std::{
    collections::HashMap,
    path::PathBuf,
};

fn assert_invalid_dependency_name_code(err: &pnpm_lockfile_verification::VerifyError) {
    let code = err.code().map(|code| code.to_string());
    assert_eq!(code.as_deref(), Some("ERR_PNPM_INVALID_DEPENDENCY_NAME"));
}

#[test]
fn accepts_snapshots_whose_slots_stay_in_the_store() {
    let layout = VirtualStoreLayout::legacy(
        PathBuf::from("/project/node_modules/.pnpm"),
        pnpm_config::default_virtual_store_dir_max_length() as usize,
    );
    let mut snapshots = HashMap::new();
    snapshots.insert("@scope/foo@1.2.3".parse::<PackageKey>().unwrap(), SnapshotEntry::default());
    snapshots.insert("bar@4.5.6".parse::<PackageKey>().unwrap(), SnapshotEntry::default());
    validate_virtual_store_slot_containment(Some(&snapshots), &layout)
        .expect("contained slots must pass");
}

#[test]
fn rejects_a_global_virtual_store_version_escape() {
    // Under the global virtual store the slot path inserts the version
    // segment as a raw path component (unlike the legacy flat name, which
    // escapes `/`). A traversal-bearing version escapes the store root
    // even though the package name itself is valid, so the containment
    // check (not the name check) is what rejects it.
    let key: PackageKey = "evil@../../../escaped".parse().expect("parse escaping version key");

    let mut config = pnpm_config::Config::new();
    config.enable_global_virtual_store = true;
    config.global_virtual_store_dir = PathBuf::from("/store/links");

    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry::default());
    let layout = VirtualStoreLayout::new(&config, None, Some(&snapshots), None, None, None);
    assert!(
        !pnpm_fs::is_subdir(layout.package_store_dir(), &layout.slot_dir(&key)),
        "the crafted slot must actually escape the store for this test to be meaningful",
    );

    let err = validate_virtual_store_slot_containment(Some(&snapshots), &layout)
        .expect_err("a slot that escapes the store root must be rejected");
    assert_invalid_dependency_name_code(&err);
    assert!(err.to_string().contains("evil@"), "offender listed: {err}");
}

#[test]
fn rejects_a_global_virtual_store_two_level_version_escape() {
    let key: PackageKey = "evil@../../escaped".parse().expect("parse escaping version key");

    let mut config = pnpm_config::Config::new();
    config.enable_global_virtual_store = true;
    config.global_virtual_store_dir = PathBuf::from("/store/links");

    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry::default());
    let layout = VirtualStoreLayout::new(&config, None, Some(&snapshots), None, None, None);
    assert!(!layout.is_slot_contained(&key), "two-level traversal escapes the package directory");

    let err = validate_virtual_store_slot_containment(Some(&snapshots), &layout)
        .expect_err("a slot that escapes the package directory must be rejected");
    assert_invalid_dependency_name_code(&err);
    assert!(err.to_string().contains("evil@"), "offender listed: {err}");
}

#[test]
fn rejects_a_global_virtual_store_scoped_version_escape() {
    let key: PackageKey = "@scope/evil@../../escaped".parse().expect("parse escaping scoped key");

    let mut config = pnpm_config::Config::new();
    config.enable_global_virtual_store = true;
    config.global_virtual_store_dir = PathBuf::from("/store/links");

    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry::default());
    let layout = VirtualStoreLayout::new(&config, None, Some(&snapshots), None, None, None);
    assert!(!layout.is_slot_contained(&key), "scoped traversal escapes the package directory");

    let err = validate_virtual_store_slot_containment(Some(&snapshots), &layout)
        .expect_err("a scoped slot that escapes the package directory must be rejected");
    assert_invalid_dependency_name_code(&err);
    assert!(err.to_string().contains("@scope/evil@"), "offender listed: {err}");
}

#[test]
fn rejects_a_global_virtual_store_manifest_version_escape() {
    let key: PackageKey = "tar-dep@file:vendor/dep.tgz".parse().expect("parse tarball key");

    let mut config = pnpm_config::Config::new();
    config.enable_global_virtual_store = true;
    config.global_virtual_store_dir = PathBuf::from("/store/links");

    let mut snapshots = HashMap::new();
    snapshots.insert(key.clone(), SnapshotEntry::default());

    let mut packages = HashMap::new();
    packages.insert(
        key.without_peer(),
        pnpm_lockfile::PackageMetadata {
            specifiers: None,
            resolution: pnpm_lockfile::LockfileResolution::Tarball(
                pnpm_lockfile::TarballResolution {
                    tarball: "file:vendor/dep.tgz".to_string(),
                    integrity: None,
                    revision: None,
                    git_hosted: None,
                    path: None,
                },
            ),
            version: Some("../../escaped".to_string()),
            engines: None,
            cpu: None,
            os: None,
            libc: None,
            deprecated: None,
            has_bin: None,
            prepare: None,
            bundled_dependencies: None,
            peer_dependencies: None,
            peer_dependencies_meta: None,
        },
    );

    let layout =
        VirtualStoreLayout::new(&config, None, Some(&snapshots), Some(&packages), None, None);
    assert!(
        !layout.is_slot_contained(&key),
        "manifest version traversal escapes the package directory",
    );

    let err = validate_virtual_store_slot_containment(Some(&snapshots), &layout)
        .expect_err("a manifest version traversal slot must be rejected");
    assert_invalid_dependency_name_code(&err);
    assert!(err.to_string().contains("tar-dep@"), "offender listed: {err}");
}

#[test]
fn external_lockfile_dir_has_none_workspace_depth_while_dot_dot_build_has_depth_one() {
    let mut external_cfg = pnpm_config::Config::new();
    external_cfg.dir = PathBuf::from("/work/app");
    external_cfg.workspace_dir = Some(PathBuf::from("/work"));
    external_cfg.lockfile_dir = Some(PathBuf::from("/external-lockfiles"));
    let external_policy = super::local_resolution_policy_for_config(&external_cfg, true)
        .with_after_all_resolved_hook(true);
    assert_eq!(external_policy.scope.workspace_depth_above_lockfile, None);

    let lockfile: pnpm_lockfile::Lockfile = serde_saphyr::from_str(
        "lockfileVersion: '9.0'\npnpmfileChecksum: sha256-hook\nimporters:\n  ../work/app: {}\npackages:\n  remote@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\n  under-ext@file:./secret:\n    resolution: {type: directory, directory: ./secret}\nsnapshots:\n  remote@1.0.0:\n    dependencies:\n      under-ext: file:./secret\n  under-ext@file:./secret: {}\n",
    )
    .expect("parse external lockfile");
    let err = pnpm_lockfile_verification::verify_lockfile_local_resolutions(
        &lockfile,
        Some(external_policy),
    )
    .expect_err("external lockfile dir must not grant containment to its own directory");
    assert!(matches!(err, pnpm_lockfile_verification::VerifyError::LocalSubdepNotAllowed { .. }));

    let mut dot_build_cfg = pnpm_config::Config::new();
    dot_build_cfg.dir = PathBuf::from("/work/..build");
    dot_build_cfg.workspace_dir = Some(PathBuf::from("/work"));
    dot_build_cfg.lockfile_dir = Some(PathBuf::from("/work/..build"));
    let dot_build_policy = super::local_resolution_policy_for_config(&dot_build_cfg, true);
    assert_eq!(dot_build_policy.scope.workspace_depth_above_lockfile, Some(1));
}

#[test]
fn catalogs_override_supplies_local_override_target_absent_from_config_catalogs() {
    let mut cfg = pnpm_config::Config::new();
    cfg.dir = PathBuf::from("/work/repo");
    cfg.workspace_dir = Some(PathBuf::from("/work/repo"));
    cfg.overrides =
        Some(indexmap::IndexMap::from([("remote>helper".to_string(), "catalog:".to_string())]));
    let catalogs_override = std::collections::BTreeMap::from([(
        "default".to_string(),
        std::collections::BTreeMap::from([("helper".to_string(), "file:../shared".to_string())]),
    )]);
    let lockfile: pnpm_lockfile::Lockfile = serde_saphyr::from_str(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      remote:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  remote@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\n  helper@file:../shared:\n    resolution: {type: directory, directory: ../shared}\nsnapshots:\n  remote@1.0.0:\n    dependencies:\n      helper: file:../shared\n  helper@file:../shared: {}\n",
    )
    .expect("parse lockfile");

    let repo_path = std::path::Path::new("/work/repo");
    let without_override =
        super::local_resolution_policy_for_workspace(&cfg, repo_path, repo_path, false);
    assert!(
        pnpm_lockfile_verification::verify_lockfile_local_resolutions(
            &lockfile,
            Some(without_override),
        )
        .is_err(),
        "without catalogs_override, unresolved catalog: spec must reject",
    );

    let with_override = super::local_resolution_policy_for_workspace_with_catalogs(
        &cfg,
        Some(&catalogs_override),
        (None, repo_path, repo_path),
        false,
    );
    pnpm_lockfile_verification::verify_lockfile_local_resolutions(&lockfile, Some(with_override))
        .expect("catalogs_override entry must authorize remote>helper -> file:../shared");
}

struct ThrowingReadPackage;

#[async_trait::async_trait]
impl pnpm_hooks::PnpmfileHooks for ThrowingReadPackage {
    async fn read_package(
        &self,
        _: serde_json::Value,
        _: pnpm_hooks::HookContext,
    ) -> Result<pnpm_hooks::ReadPackageResult, pnpm_hooks::HookError> {
        Err(pnpm_hooks::HookError::Execution {
            pnpmfile: ".pnpmfile.cjs".to_string(),
            message: "hook exploded".to_string(),
        })
    }

    async fn after_all_resolved(
        &self,
        _: serde_json::Value,
        _: pnpm_hooks::HookContext,
    ) -> Result<serde_json::Value, pnpm_hooks::HookError> {
        Ok(serde_json::Value::Null)
    }

    async fn pre_resolution(
        &self,
        ctx: pnpm_hooks::PreResolutionHookContext,
        _: pnpm_hooks::PreResolutionHookLogger,
    ) -> Result<pnpm_hooks::PreResolutionHookContext, pnpm_hooks::HookError> {
        Ok(ctx)
    }

    async fn filter_log(&self, _: serde_json::Value, _: pnpm_hooks::HookContext) -> bool {
        true
    }

    async fn calculate_pnpmfile_checksum(&self) -> Option<String> {
        None
    }

    fn source_path(&self) -> Option<&std::path::Path> {
        None
    }
}

#[tokio::test]
async fn read_package_error_on_a_real_manifest_reaches_the_caller() {
    let parent = pnpm_lockfile_verification::LocalResolutionProbeParent {
        pkg_key: "remote@1.0.0".parse().expect("parse key"),
        resolution: None,
        name: "remote".to_string(),
        version: Some("1.0.0".to_string()),
        dep_aliases: vec!["helper".to_string()],
        optional_aliases: Vec::new(),
        peer_aliases: Vec::new(),
    };
    let mut rewrites = Vec::new();
    let pristine = serde_json::json!({ "name": "remote", "dependencies": { "helper": "^1.0.0" } });
    super::collect_probed_parent_rewrites(
        &ThrowingReadPackage,
        &parent,
        Some(&pristine),
        &mut rewrites,
    )
    .await
    .expect_err("the hook's error on the real manifest must surface");
    super::collect_probed_parent_rewrites(&ThrowingReadPackage, &parent, None, &mut rewrites)
        .await
        .expect("an error on a placeholder manifest only withholds rewrites");
    assert!(rewrites.is_empty());
}
