use std::sync::Arc;

use pnpm_resolving_resolver_base::ResolutionVerifier;
use tempfile::TempDir;

use super::{
    FailFor,
    ReplacedCandidate,
    ReplacedEntries,
    SilentReporter,
    URL_SUBDEP_LOCKFILE,
    VerifyError,
    VerifyLockfileResolutionsOptions,
    parse,
    verify_lockfile_resolutions,
};
use crate::{
    ExoticSubdepsPolicy,
    ExoticSubdepsScope,
    lockfile_verification_is_cached,
};

#[tokio::test]
async fn phantom_importer_cannot_provide_exotic_peer_when_known_importers_are_scoped() {
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      reg-with-peer:
        specifier: 1.0.0
        version: 1.0.0(ws@https://evil.example.com/payload.tgz)

  ghost-dir:
    dependencies:
      ws:
        specifier: https://evil.example.com/payload.tgz
        version: https://evil.example.com/payload.tgz

packages:

  reg-with-peer@1.0.0:
    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}
    peerDependencies:
      ws: '*'

  ws@https://evil.example.com/payload.tgz:
    resolution: {tarball: https://evil.example.com/payload.tgz, integrity: sha512-s4h96KtLDUQlsENhMn1ar8t2bEa+q/YAtj8pPPdIjPDGBDIVNsrD9aXNWqspUe6AzKCIG0C1HZZLqLV7qpOBGA==}
    version: 1.0.0

snapshots:

  reg-with-peer@1.0.0(ws@https://evil.example.com/payload.tgz):
    dependencies:
      ws: https://evil.example.com/payload.tgz

  ws@https://evil.example.com/payload.tgz: {}
",
    );

    let scoped = ExoticSubdepsPolicy::block(ExoticSubdepsScope::new(
        Some(["."]),
        std::iter::empty(),
        std::iter::empty(),
        std::iter::empty(),
    ));
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions { exotic_subdeps: scoped, ..Default::default() },
    )
    .await
    .expect_err("phantom importer must not grant peer-provider exemption");
    assert!(matches!(err, VerifyError::ExoticSubdep { count: 1, .. }), "got: {err:?}");
}

#[tokio::test]
async fn runtime_binary_requires_matching_alias_and_trusted_origin_or_mirror() {
    let forged_binary = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      reg-parent:
        specifier: 1.0.0
        version: 1.0.0

packages:

  reg-parent@1.0.0:
    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}

  node@runtime:22.0.0:
    resolution: {type: binary, archive: tarball, url: https://evil.example.com/v22.0.0/node-v22.0.0-linux-x64.tar.gz, integrity: sha512-s4h96KtLDUQlsENhMn1ar8t2bEa+q/YAtj8pPPdIjPDGBDIVNsrD9aXNWqspUe6AzKCIG0C1HZZLqLV7qpOBGA==, bin: bin/node}
    version: 22.0.0

snapshots:

  reg-parent@1.0.0:
    dependencies:
      node: runtime:22.0.0

  node@runtime:22.0.0: {}
",
    );

    let err = verify_lockfile_resolutions::<SilentReporter>(
        &forged_binary,
        &[],
        &VerifyLockfileResolutionsOptions {
            exotic_subdeps: ExoticSubdepsPolicy::block_unscoped(),
            ..Default::default()
        },
    )
    .await
    .expect_err("off-origin node@runtime binary URL must be rejected");
    assert!(matches!(err, VerifyError::ExoticSubdep { count: 1, .. }), "got: {err:?}");

    let mirrored = ExoticSubdepsPolicy::block(ExoticSubdepsScope::new(
        Some(["."]),
        std::iter::empty(),
        ["https://evil.example.com"],
        std::iter::empty(),
    ));
    verify_lockfile_resolutions::<SilentReporter>(
        &forged_binary,
        &[],
        &VerifyLockfileResolutionsOptions { exotic_subdeps: mirrored, ..Default::default() },
    )
    .await
    .expect("explicitly configured node mirror must allow matching node@runtime binary");
}

#[tokio::test]
async fn runtime_variations_with_exotic_variant_or_aliased_edge_is_rejected() {
    let forged_variations = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      reg-parent:
        specifier: 1.0.0
        version: 1.0.0

packages:

  reg-parent@1.0.0:
    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}

  node@runtime:22.0.0:
    resolution:
      type: variations
      variants:
        - targets:
            - os: linux
              cpu: x64
          resolution: {type: git, repo: https://github.com/example/exotic.git, commit: 0123456789abcdef0123456789abcdef01234567}
    version: 22.0.0

snapshots:

  reg-parent@1.0.0:
    dependencies:
      node: runtime:22.0.0

  node@runtime:22.0.0: {}
",
    );

    let err = verify_lockfile_resolutions::<SilentReporter>(
        &forged_variations,
        &[],
        &VerifyLockfileResolutionsOptions {
            exotic_subdeps: ExoticSubdepsPolicy::block_unscoped(),
            ..Default::default()
        },
    )
    .await
    .expect_err("git variant hidden inside node@runtime variations must be rejected");
    assert!(matches!(err, VerifyError::ExoticSubdep { count: 1, .. }), "got: {err:?}");

    let aliased_runtime = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      reg-parent:
        specifier: 1.0.0
        version: 1.0.0

packages:

  reg-parent@1.0.0:
    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}

  node@runtime:22.0.0:
    resolution: {type: binary, archive: tarball, url: https://nodejs.org/dist/v22.0.0/node-v22.0.0-linux-x64.tar.gz, integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==, bin: bin/node}
    version: 22.0.0

snapshots:

  reg-parent@1.0.0:
    dependencies:
      sneaky: node@runtime:22.0.0

  node@runtime:22.0.0: {}
",
    );

    let alias_err = verify_lockfile_resolutions::<SilentReporter>(
        &aliased_runtime,
        &[],
        &VerifyLockfileResolutionsOptions {
            exotic_subdeps: ExoticSubdepsPolicy::block_unscoped(),
            ..Default::default()
        },
    )
    .await
    .expect_err("aliased runtime binary edge must be rejected");
    assert!(matches!(alias_err, VerifyError::ExoticSubdep { count: 1, .. }), "got: {alias_err:?}");
}

#[tokio::test]
async fn extra_workspace_dirs_exempts_deployed_workspace_package_snapshot() {
    let deploy_lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      pkg-a:
        specifier: file:../packages/pkg-a
        version: file:../packages/pkg-a

packages:

  pkg-a@file:../packages/pkg-a:
    resolution: {type: directory, directory: ../packages/pkg-a}

  exotic@http://127.0.0.1:41999/exotic.tgz:
    resolution: {tarball: http://127.0.0.1:41999/exotic.tgz, integrity: sha512-s4h96KtLDUQlsENhMn1ar8t2bEa+q/YAtj8pPPdIjPDGBDIVNsrD9aXNWqspUe6AzKCIG0C1HZZLqLV7qpOBGA==}
    version: 0.0.0

snapshots:

  pkg-a@file:../packages/pkg-a:
    dependencies:
      exotic: http://127.0.0.1:41999/exotic.tgz

  exotic@http://127.0.0.1:41999/exotic.tgz: {}
",
    );

    let without_extra = ExoticSubdepsPolicy::block(ExoticSubdepsScope::new(
        Some(["."]),
        std::iter::empty(),
        std::iter::empty(),
        std::iter::empty(),
    ));
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &deploy_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions { exotic_subdeps: without_extra, ..Default::default() },
    )
    .await
    .expect_err("non-workspace file: directory must not exempt exotic subdeps");
    assert!(matches!(err, VerifyError::ExoticSubdep { count: 1, .. }), "got: {err:?}");

    let with_extra = ExoticSubdepsPolicy::block(ExoticSubdepsScope::new(
        Some(["."]),
        ["../packages/pkg-a"],
        std::iter::empty(),
        std::iter::empty(),
    ));
    verify_lockfile_resolutions::<SilentReporter>(
        &deploy_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions { exotic_subdeps: with_extra, ..Default::default() },
    )
    .await
    .expect("deployed workspace project directory must exempt its direct exotic deps");
}

#[tokio::test]
async fn non_registry_exotic_child_cannot_spoof_is_replaced_via_semver_version() {
    fn is_exotic_zero(candidate: ReplacedCandidate<'_>) -> bool {
        candidate.name.to_string() == "exotic" && candidate.version == "0.0.0"
    }

    let lockfile = parse(URL_SUBDEP_LOCKFILE);
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            replaced: Some(ReplacedEntries(&is_exotic_zero)),
            exotic_subdeps: ExoticSubdepsPolicy::block_unscoped(),
            ..Default::default()
        },
    )
    .await
    .expect_err("exotic URL child with version 0.0.0 must not match registry is_replaced");
    assert!(matches!(err, VerifyError::ExoticSubdep { count: 1, .. }), "got: {err:?}");
}

#[tokio::test]
async fn workspace_discovery_rechecks_exotic_subdeps_and_reuses_registry_verification() {
    let dir = TempDir::new().expect("tempdir");
    let lockfile_path = dir.path().join("pnpm-lock.yaml");
    let raw = "lockfileVersion: '9.0'\nimporters:\n  .: {}\n  packages/a:\n    dependencies:\n      safe: {specifier: 1.0.0, version: 1.0.0}\npackages:\n  safe@1.0.0:\n    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}\nsnapshots:\n  safe@1.0.0: {}\n";
    std::fs::write(&lockfile_path, raw).expect("write lockfile");
    let lockfile = parse(raw);
    let cache_dir = dir.path().join("cache");
    let verifier = FailFor::new("UNUSED", "n/a", vec![]) as Arc<dyn ResolutionVerifier>;

    let wide = ExoticSubdepsPolicy::block(ExoticSubdepsScope::new(
        Some([".", "packages/a"]),
        std::iter::empty(),
        std::iter::empty(),
        std::iter::empty(),
    ));
    let narrow = ExoticSubdepsPolicy::block(ExoticSubdepsScope::new(
        Some(["."]),
        std::iter::empty(),
        std::iter::empty(),
        std::iter::empty(),
    ));

    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        std::slice::from_ref(&verifier),
        &VerifyLockfileResolutionsOptions {
            cache_dir: Some(&cache_dir),
            exotic_subdeps: narrow.clone(),
            ..Default::default()
        },
    )
    .await
    .expect("verify before discovering workspace manifests");
    assert!(lockfile_verification_is_cached(
        &cache_dir,
        &lockfile_path,
        &lockfile,
        std::slice::from_ref(&verifier),
        &narrow,
    ));
    assert!(!lockfile_verification_is_cached(
        &cache_dir,
        &lockfile_path,
        &lockfile,
        std::slice::from_ref(&verifier),
        &wide,
    ));

    let offline = FailFor::new("OFFLINE", "registry unavailable", vec!["safe"]);
    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[offline as Arc<dyn ResolutionVerifier>],
        &VerifyLockfileResolutionsOptions {
            cache_dir: Some(&cache_dir),
            lockfile_path: Some(&lockfile_path),
            lockfile_stat: Some(crate::stat_from_metadata(
                &std::fs::metadata(&lockfile_path).unwrap(),
            )),
            exotic_subdeps: wide.clone(),
            ..Default::default()
        },
    )
    .await
    .expect("rechecking workspace scope should reuse registry verification");
    assert!(lockfile_verification_is_cached(
        &cache_dir,
        &lockfile_path,
        &lockfile,
        std::slice::from_ref(&verifier),
        &wide,
    ));
}
