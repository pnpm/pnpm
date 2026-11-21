use super::{
    Arc,
    HookedManifests,
    Lockfile,
    PreparedFrozenInstall,
    PreparedPreResolution,
    matches_verified_lockfile,
};
use pnpm_lockfile::{
    LazyLockfile,
    MaybeLazyLockfile,
    WantedLockfileSelection,
};
use std::fs;
use tempfile::tempdir;

fn make_prepared(
    verified_wanted: Option<Arc<Lockfile>>,
    had_conflicts: bool,
) -> PreparedFrozenInstall {
    PreparedFrozenInstall {
        pnpmfile_hook: None,
        manifests: HookedManifests::default(),
        pre_resolution: PreparedPreResolution { hooked_wanted: None, hooked_current: None },
        verified_wanted,
        had_conflicts,
        verified_policy: Some(serde_json::Map::new()),
    }
}

fn plain_selection() -> WantedLockfileSelection {
    WantedLockfileSelection {
        file_name: "pnpm-lock.yaml".to_string(),
        merge_git_branch_lockfiles: false,
        branch_lockfile_candidates: Vec::new(),
    }
}

#[test]
fn matches_verified_lockfile_accepts_same_lazy_instance_and_rejects_modified_disk_lockfile() {
    let dir = tempdir().expect("create tempdir");
    let lock_v1 = "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies: {}\n";
    let lock_v2 = "lockfileVersion: '9.0'\nsettings:\n  autoInstallPeers: false\nimporters:\n  .:\n    dependencies: {}\n";
    fs::write(dir.path().join("pnpm-lock.yaml"), lock_v1).expect("write v1 lockfile");

    let validated_lazy = LazyLockfile::deferred(dir.path().to_path_buf(), plain_selection());
    let shared_v1 = MaybeLazyLockfile::Lazy(&validated_lazy)
        .shared()
        .expect("load validated lockfile")
        .expect("lockfile exists");
    let prepared = make_prepared(Some(shared_v1), false);

    assert!(
        matches_verified_lockfile(MaybeLazyLockfile::Lazy(&validated_lazy), &prepared),
        "carried LazyLockfile matches via Arc::ptr_eq even before or after disk changes",
    );

    fs::write(dir.path().join("pnpm-lock.yaml"), lock_v2).expect("write v2 lockfile");
    assert!(
        matches_verified_lockfile(MaybeLazyLockfile::Lazy(&validated_lazy), &prepared),
        "carried in-memory LazyLockfile remains pinned to the verified document",
    );

    let reopened_lazy = LazyLockfile::deferred(dir.path().to_path_buf(), plain_selection());
    assert!(
        !matches_verified_lockfile(MaybeLazyLockfile::Lazy(&reopened_lazy), &prepared),
        "reopened LazyLockfile seeing modified disk content invalidates prepared verification",
    );
    assert!(
        !matches_verified_lockfile(MaybeLazyLockfile::Loaded(None), &prepared),
        "missing lockfile invalidates prepared verification",
    );
    let conflict_mismatch = make_prepared(prepared.verified_wanted, true);
    assert!(
        !matches_verified_lockfile(MaybeLazyLockfile::Lazy(&validated_lazy), &conflict_mismatch),
        "had_conflicts mismatch invalidates prepared verification",
    );
}

#[test]
fn take_valid_override_rejects_weaker_prepared_policy() {
    use super::{
        SilentReporter,
        Verification,
    };
    use pnpm_lockfile_verification::{
        ExoticSubdepsPolicy,
        ExoticSubdepsScope,
        snapshot_verification_policy,
    };

    let unhooked_wanted = crate::install::run::wanted::WantedLockfile::default();

    let weaker_policy = snapshot_verification_policy(&[], &ExoticSubdepsPolicy::Allow);
    let stricter_verification = Verification {
        meta_cache: std::sync::Arc::default(),
        planned_canonical_fetches: std::sync::Arc::default(),
        resolution_verifiers: Vec::new(),
        exotic_subdeps: ExoticSubdepsPolicy::block(ExoticSubdepsScope::new(
            Some(["."]),
            std::iter::empty(),
            std::iter::empty(),
            std::iter::empty(),
        )),
        derived_lockfile: None,
        auto_dedupe_baseline: None,
    };

    let mut mismatched = crate::install::InstallVerificationOptions {
        override_check: Some(Box::pin(async { Ok(()) })),
        prepared_policy: Some(weaker_policy),
        extra_workspace_dirs: Vec::new(),
    };
    assert!(
        stricter_verification
            .take_valid_override::<SilentReporter>(&unhooked_wanted, &mut mismatched)
            .is_none(),
        "prepared verification under weaker policy must not bypass stricter run",
    );

    let matching_policy = snapshot_verification_policy(&[], &stricter_verification.exotic_subdeps);
    let mut matching = crate::install::InstallVerificationOptions {
        override_check: Some(Box::pin(async { Ok(()) })),
        prepared_policy: Some(matching_policy),
        extra_workspace_dirs: Vec::new(),
    };
    assert!(
        stricter_verification
            .take_valid_override::<SilentReporter>(&unhooked_wanted, &mut matching)
            .is_some(),
        "prepared verification under identical policy and scope must be reused",
    );
}
