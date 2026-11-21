use std::{
    collections::BTreeMap,
    path::Path,
};

use pnpm_lockfile::PkgNameVerPeer;
use pnpm_reporter::SilentReporter;

use super::{
    VerifyError,
    VerifyLockfileResolutionsOptions,
    verify_lockfile_resolutions,
};
use crate::{
    LocalDepField,
    LocalResolutionPolicy,
    PristineDeclaredSpecs,
};

fn parse(yaml: &str) -> pnpm_lockfile::Lockfile {
    serde_saphyr::from_str(yaml).expect("parse lockfile")
}

fn pkg_key(raw: &str) -> PkgNameVerPeer {
    raw.parse().expect("parse PkgNameVerPeer")
}

#[tokio::test]
async fn bare_windows_drive_override_matches_file_prefixed_override_while_drive_relative_fails() {
    let lockfile = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  remote-pkg@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\n  helper@file:../helper:\n    resolution: {type: directory, directory: ../helper}\nsnapshots:\n  remote-pkg@1.0.0:\n    dependencies:\n      helper: file:../helper\n  helper@file:../helper: {}\n",
    );
    for spec in
        ["file:C:/work/helper", "C:/work/helper", r"C:\work\helper", "file:c:/WORK/app/../helper"]
    {
        let policy = LocalResolutionPolicy::from_scoped_inputs(
            [("remote-pkg>helper", spec)],
            [],
            false,
            Some(Path::new("C:/work/app")),
        );
        verify_lockfile_resolutions::<SilentReporter>(
            &lockfile,
            &[],
            &VerifyLockfileResolutionsOptions {
                local_policy: Some(policy),
                ..VerifyLockfileResolutionsOptions::default()
            },
        )
        .await
        .unwrap_or_else(|err| panic!("expected `{spec}` to authorize ../helper: {err:?}"));
    }

    let cross_volume_anchored = LocalResolutionPolicy::from_scoped_inputs_with_roots(
        [("remote-pkg>helper", "file:C:/work/helper")],
        [],
        false,
        (Some(Path::new("D:/workspace")), Some(Path::new("C:/work/app"))),
        |_, _| None,
    );
    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(cross_volume_anchored),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect("cross-volume overrides_root must retain anchored C:/work/helper target");

    let drive_relative = LocalResolutionPolicy::from_scoped_inputs(
        [("remote-pkg>helper", "C:helper")],
        [],
        false,
        Some(Path::new("C:/work/app")),
    );
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(drive_relative),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect_err("drive-relative C:helper must not authorize local edge");
    assert!(matches!(err, VerifyError::LocalSubdepNotAllowed { .. }));
}

#[tokio::test]
async fn unc_volume_roots_rebase_only_within_same_server_and_share() {
    let lockfile = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  remote-pkg@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\n  helper@file:../helper:\n    resolution: {type: directory, directory: ../helper}\nsnapshots:\n  remote-pkg@1.0.0:\n    dependencies:\n      helper: file:../helper\n  helper@file:../helper: {}\n",
    );
    let same_share = LocalResolutionPolicy::from_scoped_inputs(
        [("remote-pkg>helper", "file://SRV1/SHARE1/helper")],
        [],
        false,
        Some(Path::new("//srv1/share1/app")),
    );
    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(same_share),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect("same UNC server/share (case-insensitive) must rebase to ../helper");

    for cross_unc in ["file://srv1/share2/helper", "file://srv2/share1/helper"] {
        let policy = LocalResolutionPolicy::from_scoped_inputs(
            [("remote-pkg>helper", cross_unc)],
            [],
            false,
            Some(Path::new("//srv1/share1/app")),
        );
        let err = verify_lockfile_resolutions::<SilentReporter>(
            &lockfile,
            &[],
            &VerifyLockfileResolutionsOptions {
                local_policy: Some(policy),
                ..VerifyLockfileResolutionsOptions::default()
            },
        )
        .await
        .expect_err("different UNC share must not rebase to ../helper");
        assert!(matches!(err, VerifyError::LocalSubdepNotAllowed { .. }));
    }
}

#[tokio::test]
async fn override_target_matching_never_confuses_workspace_project_subpaths_or_reverse_joins() {
    let reverse_lockfile = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\n  packages/app: {}\npackages:\n  remote-pkg@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\n  helper@file:../shared:\n    resolution: {type: directory, directory: ../shared}\nsnapshots:\n  remote-pkg@1.0.0:\n    dependencies:\n      helper: file:../shared\n  helper@file:../shared: {}\n",
    );
    let reverse_policy = LocalResolutionPolicy::from_scoped_inputs(
        [("remote-pkg>helper", "file:../../../shared")],
        [],
        false,
        None,
    )
    .with_live_workspace_dirs([".".to_string(), "packages/app".to_string()]);
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &reverse_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(reverse_policy),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect_err("../../../shared override must not authorize ../shared via packages/app");
    assert!(matches!(err, VerifyError::LocalSubdepNotAllowed { .. }));

    let cross_importer_lockfile = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\n  packages/other: {}\npackages:\n  remote-pkg@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\n  helper@file:packages/other/helper:\n    resolution: {type: directory, directory: packages/other/helper}\nsnapshots:\n  remote-pkg@1.0.0:\n    dependencies:\n      helper: file:packages/other/helper\n  helper@file:packages/other/helper: {}\n",
    );
    let cross_policy = LocalResolutionPolicy::from_scoped_inputs(
        [("remote-pkg>helper", "file:./helper")],
        [],
        false,
        None,
    )
    .with_live_workspace_dirs([".".to_string(), "packages/other".to_string()]);
    let err2 = verify_lockfile_resolutions::<SilentReporter>(
        &cross_importer_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(cross_policy),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect_err("./helper override must not authorize packages/other/helper");
    assert!(matches!(err2, VerifyError::LocalSubdepNotAllowed { .. }));
}

#[tokio::test]
async fn reachable_policy_producer_authorizes_same_importer_peer_while_unreachable_producer_does_not()
 {
    let reachable_lockfile = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      safe-parent:\n        specifier: 1.0.0\n        version: 1.0.0\n      peer-consumer:\n        specifier: 1.0.0\n        version: 1.0.0(helper@file:../private)\npackages:\n  safe-parent@1.0.0:\n    resolution: {integrity: sha512-safe}\n  peer-consumer@1.0.0:\n    resolution: {integrity: sha512-peer}\n    peerDependencies:\n      helper: '*'\n  helper@file:../private:\n    resolution: {type: directory, directory: ../private}\nsnapshots:\n  safe-parent@1.0.0:\n    dependencies:\n      helper: file:../private\n  peer-consumer@1.0.0(helper@file:../private):\n    dependencies:\n      helper: file:../private\n  helper@file:../private: {}\n",
    );
    let policy = LocalResolutionPolicy::from_scoped_inputs(
        [("safe-parent>helper", "file:../private")],
        [],
        false,
        None,
    );
    verify_lockfile_resolutions::<SilentReporter>(
        &reachable_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(policy.clone()),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect("reachable safe-parent must authorize helper peer in same importer");

    let unreachable_lockfile = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      peer-consumer:\n        specifier: 1.0.0\n        version: 1.0.0(helper@file:../private)\npackages:\n  safe-parent@1.0.0:\n    resolution: {integrity: sha512-safe}\n  peer-consumer@1.0.0:\n    resolution: {integrity: sha512-peer}\n    peerDependencies:\n      helper: '*'\n  helper@file:../private:\n    resolution: {type: directory, directory: ../private}\nsnapshots:\n  safe-parent@1.0.0:\n    dependencies:\n      helper: file:../private\n  peer-consumer@1.0.0(helper@file:../private):\n    dependencies:\n      helper: file:../private\n  helper@file:../private: {}\n",
    );
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &unreachable_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(policy),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect_err("unreachable safe-parent must not seed peer authorization");
    assert!(matches!(err, VerifyError::LocalSubdepNotAllowed { .. }));
}

#[tokio::test]
async fn policy_authorized_link_remains_bound_to_authorizing_parent_and_selected_importers_skip_absent()
 {
    let link_policy = LocalResolutionPolicy::from_scoped_inputs(
        [("safe-parent>secret", "link:../private")],
        [],
        false,
        None,
    );
    let safe_only_lockfile = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      safe-parent:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  safe-parent@1.0.0:\n    resolution: {integrity: sha512-safe}\nsnapshots:\n  safe-parent@1.0.0:\n    dependencies:\n      secret: link:../private\n",
    );
    verify_lockfile_resolutions::<SilentReporter>(
        &safe_only_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(link_policy.clone()),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect("safe-parent alone must pass under safe-parent>secret link override");

    let link_lockfile = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      safe-parent:\n        specifier: 1.0.0\n        version: 1.0.0\n      peer-consumer:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  safe-parent@1.0.0:\n    resolution: {integrity: sha512-safe}\n  peer-consumer@1.0.0:\n    resolution: {integrity: sha512-peer}\n    peerDependencies:\n      secret: '*'\nsnapshots:\n  safe-parent@1.0.0:\n    dependencies:\n      secret: link:../private\n  peer-consumer@1.0.0:\n    dependencies:\n      secret: link:../private\n",
    );
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &link_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(link_policy),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect_err("policy link for safe-parent must not authorize peer-consumer link");
    let VerifyError::LocalSubdepNotAllowed { breakdown, .. } = err else {
        panic!("expected LocalSubdepNotAllowed, got {err:?}");
    };
    assert!(breakdown.contains("peer-consumer@1.0.0"), "breakdown: {breakdown}");
    assert!(!breakdown.contains("safe-parent@1.0.0"), "breakdown: {breakdown}");

    let absent_project_lockfile = parse(
        "lockfileVersion: '9.0'\nimporters:\n  packages/app:\n    dependencies:\n      ok-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\n  packages/unused:\n    dependencies:\n      ext-lib:\n        specifier: file:../../outside-vendor\n        version: file:../outside-vendor\npackages:\n  ok-pkg@1.0.0:\n    resolution: {integrity: sha512-ok}\n  ext-lib@file:../outside-vendor:\n    resolution: {type: directory, directory: ../outside-vendor}\nsnapshots:\n  ok-pkg@1.0.0: {}\n  ext-lib@file:../outside-vendor: {}\n",
    );
    let selected_policy = LocalResolutionPolicy::default()
        .with_live_workspace_dirs(["packages/app".to_string()])
        .with_selected_importer_ids(["packages/app".to_string()]);
    verify_lockfile_resolutions::<SilentReporter>(
        &absent_project_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(selected_policy),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect("absent workspace project excluded from selected_importer_ids must not block install");
}

#[tokio::test]
async fn workspace_targets_restrict_to_workspace_projects_and_recompute_workspace_depth() {
    let ws_link_lockfile = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  remote-pkg@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\nsnapshots:\n  remote-pkg@1.0.0:\n    dependencies:\n      ws-pkg: link:packages/ws-pkg\n",
    );
    let live_ws_policy = LocalResolutionPolicy::default()
        .with_live_workspace_dirs([".".to_string(), "packages/ws-pkg".to_string()]);
    verify_lockfile_resolutions::<SilentReporter>(
        &ws_link_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(live_ws_policy.clone()),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect("link to discovered workspace project must succeed");

    let non_project_link = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  remote-pkg@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\nsnapshots:\n  remote-pkg@1.0.0:\n    dependencies:\n      cfg: link:private-config\n",
    );
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &non_project_link,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(live_ws_policy),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect_err("non-project directory inside workspace must not be linked by remote package");
    assert!(matches!(err, VerifyError::LocalSubdepNotAllowed { .. }));

    // Calling `with_lockfile_dir` before `with_workspace_dir` recomputes `workspace_depth_above_lockfile`.
    let nested_lockfile = parse(
        "lockfileVersion: '9.0'\nimporters:\n  ../shared: {}\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  remote-pkg@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\nsnapshots:\n  remote-pkg@1.0.0:\n    dependencies:\n      shared: link:../shared\n",
    );
    let depth_policy = LocalResolutionPolicy::default()
        .with_lockfile_dir(Path::new("/work/ws/sub"))
        .with_workspace_dir(Path::new("/work/ws"));
    verify_lockfile_resolutions::<SilentReporter>(
        &nested_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(depth_policy),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect("with_workspace_dir after with_lockfile_dir must recompute depth 1");
}

#[tokio::test]
async fn enforces_local_git_containment_forged_importer_specifiers_catalogs_and_per_source_isolation()
 {
    // 1. Non-local snapshot -> `git+file:` child rejected; local-git -> local-git and overridden `git+file:` succeed.
    let remote_to_git = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  remote-pkg@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\n  git-sub@git+file:../repo#0123456789abcdef0123456789abcdef01234567:\n    resolution: {type: git, repo: 'file:../repo', commit: 0123456789abcdef0123456789abcdef01234567}\nsnapshots:\n  remote-pkg@1.0.0:\n    dependencies:\n      git-sub: git+file:../repo#0123456789abcdef0123456789abcdef01234567\n  git-sub@git+file:../repo#0123456789abcdef0123456789abcdef01234567: {}\n",
    );
    let err_git = verify_lockfile_resolutions::<SilentReporter>(
        &remote_to_git,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect_err("non-local snapshot referencing git+file: child must be rejected");
    assert!(matches!(err_git, VerifyError::LocalSubdepNotAllowed { .. }));

    let git_override = LocalResolutionPolicy::from_scoped_inputs(
        [("remote-pkg>git-sub", "git+file:../repo")],
        [],
        false,
        None,
    );
    verify_lockfile_resolutions::<SilentReporter>(
        &remote_to_git,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(git_override),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect("workspace override to git+file:../repo must authorize child");

    let local_git_chain = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      git-parent:\n        specifier: git+file:./parent-repo\n        version: git+file:parent-repo#0123456789abcdef0123456789abcdef01234567\npackages:\n  git-parent@git+file:parent-repo#0123456789abcdef0123456789abcdef01234567:\n    resolution: {type: git, repo: 'file:parent-repo', commit: 0123456789abcdef0123456789abcdef01234567}\n  git-child@git+file:../child-repo#fedcba9876543210fedcba9876543210fedcba98:\n    resolution: {type: git, repo: 'file:../child-repo', commit: fedcba9876543210fedcba9876543210fedcba98}\nsnapshots:\n  git-parent@git+file:parent-repo#0123456789abcdef0123456789abcdef01234567:\n    dependencies:\n      git-child: git+file:../child-repo#fedcba9876543210fedcba9876543210fedcba98\n  git-child@git+file:../child-repo#fedcba9876543210fedcba9876543210fedcba98: {}\n",
    );
    verify_lockfile_resolutions::<SilentReporter>(
        &local_git_chain,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect("importer git+file: parent may depend on git+file: child");

    let spoofed_https_git = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      git-spoof:\n        specifier: git+https://example.com/repo.git\n        version: https://example.com/repo.git#0123456789abcdef0123456789abcdef01234567\npackages:\n  git-spoof@https://example.com/repo.git#0123456789abcdef0123456789abcdef01234567:\n    resolution: {type: git, repo: 'file:///etc/repo', commit: 0123456789abcdef0123456789abcdef01234567}\nsnapshots:\n  git-spoof@https://example.com/repo.git#0123456789abcdef0123456789abcdef01234567: {}\n",
    );
    let err_spoof = verify_lockfile_resolutions::<SilentReporter>(
        &spoofed_https_git,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect_err("non-git+file key backed by local git repo must fail shape check");
    assert!(matches!(err_spoof, VerifyError::ResolutionShapeMismatch { .. }));

    // 2. Forged importer non-local specifier (`^1.0.0`) resolving to unauthorized local `file:` or `link:` fails.
    let forged_imp = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: ^1.0.0\n        version: file:../private\npackages:\n  remote-pkg@file:../private:\n    resolution: {type: directory, directory: ../private}\nsnapshots:\n  remote-pkg@file:../private: {}\n",
    );
    let err_imp = verify_lockfile_resolutions::<SilentReporter>(
        &forged_imp,
        &[],
        &VerifyLockfileResolutionsOptions::default(),
    )
    .await
    .expect_err("importer ^1.0.0 specifier resolved to unauthorized file:../private must fail");
    assert!(matches!(err_imp, VerifyError::LocalSubdepNotAllowed { .. }));

    // 3. Per-field `PristineDeclaredSpecs`, `catalog:` resolution, and per-`pkg_key` source isolation.
    let dual_source_lockfile = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      dup-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\n      evil-dup:\n        specifier: https://evil.example/dup-pkg.tgz\n        version: dup-pkg@https://evil.example/dup-pkg.tgz\npackages:\n  dup-pkg@1.0.0:\n    version: 1.0.0\n    resolution: {integrity: sha512-good}\n  dup-pkg@https://evil.example/dup-pkg.tgz:\n    version: 1.0.0\n    resolution: {tarball: https://evil.example/dup-pkg.tgz}\n  helper@file:local-helper:\n    resolution: {type: directory, directory: local-helper}\nsnapshots:\n  dup-pkg@1.0.0:\n    dependencies:\n      helper: file:local-helper\n  dup-pkg@https://evil.example/dup-pkg.tgz:\n    dependencies:\n      helper: file:local-helper\n  helper@file:local-helper: {}\n",
    );
    let scoped_hook = LocalResolutionPolicy::default()
        .with_probed_source_hook_rewrites([(
            pkg_key("dup-pkg@1.0.0"),
            "dup-pkg".to_string(),
            Some("1.0.0".to_string()),
            "helper".to_string(),
            "file:./local-helper".to_string(),
        )]);
    let registry_only_lockfile = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      dup-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  dup-pkg@1.0.0:\n    version: 1.0.0\n    resolution: {integrity: sha512-good}\n  helper@file:local-helper:\n    resolution: {type: directory, directory: local-helper}\nsnapshots:\n  dup-pkg@1.0.0:\n    dependencies:\n      helper: file:local-helper\n  helper@file:local-helper: {}\n",
    );
    verify_lockfile_resolutions::<SilentReporter>(
        &registry_only_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(scoped_hook.clone()),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect("hook rewrite on dup-pkg@1.0.0 must authorize its own helper edge");
    let err_dup = verify_lockfile_resolutions::<SilentReporter>(
        &dual_source_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(scoped_hook),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect_err(
        "hook rewrite on dup-pkg@1.0.0 must not authorize dup-pkg@https://evil.example/dup-pkg.tgz",
    );
    assert!(matches!(err_dup, VerifyError::LocalSubdepNotAllowed { .. }));
    let err_dup_text = format!("{err_dup:?}");
    assert!(err_dup_text.contains("evil.example"), "{err_dup_text}");

    let field_lockfile = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  remote-pkg@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\n  helper@file:local-helper:\n    resolution: {type: directory, directory: local-helper}\nsnapshots:\n  remote-pkg@1.0.0:\n    dependencies:\n      helper: file:local-helper\n  helper@file:local-helper: {}\n",
    );
    let mut wrong_field_specs: PristineDeclaredSpecs = std::collections::HashMap::new();
    wrong_field_specs.insert(
        (pkg_key("remote-pkg@1.0.0"), LocalDepField::OptionalDependencies, "helper".to_string()),
        "^1.0.0".to_string(),
    );
    wrong_field_specs.insert(
        (pkg_key("remote-pkg@1.0.0"), LocalDepField::Dependencies, "helper".to_string()),
        "^2.0.0".to_string(),
    );
    let wrong_field_policy = LocalResolutionPolicy::from_scoped_inputs(
        [("remote-pkg>helper@^1.0.0", "file:./local-helper")],
        [],
        false,
        None,
    )
    .with_pristine_declared_specs(wrong_field_specs);
    let err_field = verify_lockfile_resolutions::<SilentReporter>(
        &field_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(wrong_field_policy),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect_err("optionalDependencies ^1.0.0 must not authorize dependencies ^2.0.0");
    assert!(matches!(err_field, VerifyError::LocalSubdepNotAllowed { .. }));

    let mut catalog_specs: PristineDeclaredSpecs = std::collections::HashMap::new();
    catalog_specs.insert(
        (pkg_key("remote-pkg@1.0.0"), LocalDepField::Dependencies, "helper".to_string()),
        "catalog:".to_string(),
    );
    let mut default_cat = BTreeMap::new();
    default_cat.insert("helper".to_string(), "^1.0.0".to_string());
    let mut catalogs = BTreeMap::new();
    catalogs.insert("default".to_string(), default_cat);
    let catalog_policy = LocalResolutionPolicy::from_scoped_inputs(
        [("remote-pkg>helper@^1.0.0", "file:./local-helper")],
        [],
        false,
        None,
    )
    .with_catalogs(catalogs)
    .with_pristine_declared_specs(catalog_specs);
    verify_lockfile_resolutions::<SilentReporter>(
        &field_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            local_policy: Some(catalog_policy),
            ..VerifyLockfileResolutionsOptions::default()
        },
    )
    .await
    .expect("catalog: pristine spec resolving to ^1.0.0 must satisfy helper@^1.0.0 override");
}

#[test]
fn importer_range_link_may_target_a_lockfile_importer_absent_from_the_run() {
    let with_importer = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      lib:\n        specifier: ^1.0.0\n        version: link:../shared\n  ../shared: {}\n",
    );
    let live_root = || LocalResolutionPolicy::default().with_live_workspace_dirs([".".to_string()]);
    let nested_lockfile = live_root().with_workspace_scope(false, Some(1));
    crate::verify_lockfile_local_resolutions(&with_importer, Some(nested_lockfile))
        .expect("a range linked to a lockfile importer inside the workspace must pass");
    let err = crate::verify_lockfile_local_resolutions(&with_importer, Some(live_root()))
        .expect_err("an importer outside the workspace must not vouch for a link");
    assert!(matches!(err, VerifyError::LocalSubdepNotAllowed { .. }));

    let without_importer = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      lib:\n        specifier: ^1.0.0\n        version: link:../shared\n",
    );
    let err = crate::verify_lockfile_local_resolutions(&without_importer, Some(live_root()))
        .expect_err("a range linked to a directory no importer records must fail");
    assert!(matches!(err, VerifyError::LocalSubdepNotAllowed { .. }));
}

#[test]
fn non_local_peer_matching_a_reachable_local_package_peer_passes() {
    let lockfile = |importer_deps: &str| {
        parse(&format!(
            "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n{importer_deps}      remote:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  local-lib@file:local-lib:\n    resolution: {{type: directory, directory: local-lib}}\n    peerDependencies:\n      helper: file:./helper\n  remote@1.0.0:\n    resolution: {{integrity: sha512-deadbeef}}\n    peerDependencies:\n      helper: file:./helper\n  helper@file:helper:\n    resolution: {{type: directory, directory: helper}}\nsnapshots:\n  helper@file:helper: {{}}\n  local-lib@file:local-lib:\n    dependencies:\n      helper: file:helper\n  remote@1.0.0:\n    dependencies:\n      helper: file:helper\n",
        ))
    };
    crate::verify_lockfile_local_resolutions(
        &lockfile(
            "      local-lib:\n        specifier: file:./local-lib\n        version: file:local-lib\n",
        ),
        None,
    )
    .expect("a local peer a reachable local package also declares must pass");

    let err = crate::verify_lockfile_local_resolutions(&lockfile(""), None)
        .expect_err("a local peer only an unreachable local package declares must fail");
    assert!(matches!(err, VerifyError::LocalSubdepNotAllowed { .. }));
}

#[test]
fn local_containment_rejects_a_semver_key_backed_by_a_directory() {
    let lockfile = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      remote:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  remote@1.0.0:\n    resolution: {type: directory, directory: ../outside}\nsnapshots:\n  remote@1.0.0: {}\n",
    );
    let err = crate::verify_lockfile_local_resolutions(&lockfile, None)
        .expect_err("a semver key backed by a directory must fail the local gate");
    assert!(matches!(err, VerifyError::ResolutionShapeMismatch { .. }), "{err:?}");
}

#[test]
fn override_target_does_not_match_a_path_that_steps_back_through_a_segment() {
    let lockfile = |target: &str| {
        parse(&format!(
            "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      remote-pkg:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  remote-pkg@1.0.0:\n    resolution: {{integrity: sha512-deadbeef}}\nsnapshots:\n  remote-pkg@1.0.0:\n    dependencies:\n      helper: link:{target}\n",
        ))
    };
    let policy = || {
        LocalResolutionPolicy::from_scoped_inputs(
            [("remote-pkg>helper", "link:./shared")],
            [],
            false,
            None,
        )
    };
    crate::verify_lockfile_local_resolutions(&lockfile("shared"), Some(policy()))
        .expect("the overridden target must pass");
    let err =
        crate::verify_lockfile_local_resolutions(&lockfile("linked/../shared"), Some(policy()))
            .expect_err("a target that steps back through `linked` must not match");
    assert!(matches!(err, VerifyError::LocalSubdepNotAllowed { .. }), "{err:?}");
}

#[test]
fn unresolved_non_local_local_peer_declaration_passes() {
    let lockfile = parse(
        "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      remote:\n        specifier: 1.0.0\n        version: 1.0.0\npackages:\n  remote@1.0.0:\n    resolution: {integrity: sha512-deadbeef}\n    peerDependencies:\n      helper: file:../helper\nsnapshots:\n  remote@1.0.0: {}\n",
    );
    crate::verify_lockfile_local_resolutions(&lockfile, None)
        .expect("a local peer no snapshot resolves must pass");
}

#[tokio::test]
async fn localhost_git_file_url_matches_its_repository_target() {
    let lockfile = |repo: &str| {
        parse(&format!(
            "lockfileVersion: '9.0'\nimporters:\n  .:\n    dependencies:\n      repo:\n        specifier: git+file://localhost/tmp/repo\n        version: git+file://localhost/tmp/repo#0123456789abcdef0123456789abcdef01234567\npackages:\n  repo@git+file://localhost/tmp/repo#0123456789abcdef0123456789abcdef01234567:\n    resolution: {{type: git, repo: '{repo}', commit: 0123456789abcdef0123456789abcdef01234567}}\nsnapshots:\n  repo@git+file://localhost/tmp/repo#0123456789abcdef0123456789abcdef01234567: {{}}\n",
        ))
    };
    let verify = |lockfile: pnpm_lockfile::Lockfile| async move {
        verify_lockfile_resolutions::<SilentReporter>(
            &lockfile,
            &[],
            &VerifyLockfileResolutionsOptions {
                local_policy: Some(
                    LocalResolutionPolicy::default().with_lockfile_dir(Path::new("/work/app")),
                ),
                ..VerifyLockfileResolutionsOptions::default()
            },
        )
        .await
    };
    verify(lockfile("git+file://localhost/tmp/repo")).await
        .expect("a file://localhost/ key must match its file://localhost/ repository");
    let err = verify(lockfile("git+file://localhost/tmp/other")).await
        .expect_err("a file://localhost/ key must not match a different repository");
    assert!(matches!(err, VerifyError::ResolutionShapeMismatch { .. }), "{err:?}");
}
