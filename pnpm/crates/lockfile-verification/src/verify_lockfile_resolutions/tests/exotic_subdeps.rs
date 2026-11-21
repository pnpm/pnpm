mod hardening;
mod round1_hardening;
mod round2_hardening;

use std::sync::Arc;

use pnpm_diagnostics::miette::Diagnostic;
use pnpm_resolving_resolver_base::ResolutionVerifier;
use tempfile::TempDir;

use super::{
    FailFor,
    ReplacedCandidate,
    ReplacedEntries,
    SilentReporter,
    VerifyError,
    VerifyLockfileResolutionsOptions,
    parse,
    verify_lockfile_resolutions,
};
use crate::{
    ExoticSubdepsPolicy,
    lockfile_verification_is_cached,
    record_lockfile_verified_by_content,
};

const URL_SUBDEP_LOCKFILE: &str = "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      reg-parent:
        specifier: 1.0.0
        version: 1.0.0

packages:

  reg-parent@1.0.0:
    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}

  exotic@http://127.0.0.1:41999/exotic.tgz:
    resolution: {tarball: http://127.0.0.1:41999/exotic.tgz, integrity: sha512-s4h96KtLDUQlsENhMn1ar8t2bEa+q/YAtj8pPPdIjPDGBDIVNsrD9aXNWqspUe6AzKCIG0C1HZZLqLV7qpOBGA==}
    version: 0.0.0

snapshots:

  reg-parent@1.0.0:
    dependencies:
      exotic: http://127.0.0.1:41999/exotic.tgz

  exotic@http://127.0.0.1:41999/exotic.tgz: {}
";

#[tokio::test]
async fn rejects_url_tarball_subdep_when_block_exotic_subdeps_is_enabled() {
    let lockfile = parse(URL_SUBDEP_LOCKFILE);
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            exotic_subdeps: ExoticSubdepsPolicy::block_unscoped(),
            ..Default::default()
        },
    )
    .await
    .expect_err("URL tarball subdependency must be rejected");

    assert_eq!(
        err.code()
            .map(|code| code.to_string())
            .as_deref(),
        Some("ERR_PNPM_EXOTIC_SUBDEP"),
    );
    let VerifyError::ExoticSubdep { count, breakdown } = err else {
        panic!("expected VerifyError::ExoticSubdep, got: {err:?}");
    };
    assert_eq!(count, 1);
    assert!(
        breakdown.contains("exotic@0.0.0 (resolved via url) is not allowed in subdependencies when blockExoticSubdeps is enabled"),
        "unexpected breakdown: {breakdown}",
    );
}

#[tokio::test]
async fn allows_url_tarball_subdep_when_block_exotic_subdeps_is_disabled() {
    let lockfile = parse(URL_SUBDEP_LOCKFILE);
    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            exotic_subdeps: ExoticSubdepsPolicy::Allow,
            ..Default::default()
        },
    )
    .await
    .expect("URL tarball subdependency must pass when policy is Allow");
}

#[tokio::test]
async fn rejects_git_and_git_hosted_tarball_subdeps() {
    let lockfile = parse(
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

  git-pkg@git+https://example.com/repo.git#deadbeef:
    resolution: {type: git, repo: https://example.com/repo.git, commit: deadbeef}
    version: 1.2.3

  gh-pkg@https://codeload.github.com/acme/gh-pkg/tar.gz/0123456789abcdef0123456789abcdef01234567:
    resolution: {tarball: https://codeload.github.com/acme/gh-pkg/tar.gz/0123456789abcdef0123456789abcdef01234567}
    version: 2.0.0

snapshots:

  reg-parent@1.0.0:
    dependencies:
      git-pkg: git+https://example.com/repo.git#deadbeef
    optionalDependencies:
      gh-pkg: https://codeload.github.com/acme/gh-pkg/tar.gz/0123456789abcdef0123456789abcdef01234567

  git-pkg@git+https://example.com/repo.git#deadbeef: {}
  gh-pkg@https://codeload.github.com/acme/gh-pkg/tar.gz/0123456789abcdef0123456789abcdef01234567: {}
",
    );

    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            exotic_subdeps: ExoticSubdepsPolicy::block_unscoped(),
            ..Default::default()
        },
    )
    .await
    .expect_err("git subdependencies must be rejected");

    let VerifyError::ExoticSubdep { count, breakdown } = err else {
        panic!("expected VerifyError::ExoticSubdep, got: {err:?}");
    };
    assert_eq!(count, 2);
    assert!(
        breakdown.contains("gh-pkg@2.0.0 (resolved via git-repository)"),
        "breakdown:\n{breakdown}",
    );
    assert!(
        breakdown.contains("git-pkg@1.2.3 (resolved via git-repository)"),
        "breakdown:\n{breakdown}",
    );
}

#[tokio::test]
async fn rejects_exotic_subdep_under_direct_exotic_parent_and_versionless_package() {
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      direct-git:
        specifier: git+https://example.com/direct.git
        version: git+https://example.com/direct.git#11111111

packages:

  direct-git@git+https://example.com/direct.git#11111111:
    resolution: {type: git, repo: https://example.com/direct.git, commit: '11111111'}
    version: 1.0.0

  nested-exotic@https://example.com/nested.tgz:
    resolution: {tarball: https://example.com/nested.tgz}

snapshots:

  direct-git@git+https://example.com/direct.git#11111111:
    dependencies:
      nested-exotic: https://example.com/nested.tgz

  nested-exotic@https://example.com/nested.tgz: {}
",
    );

    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            exotic_subdeps: ExoticSubdepsPolicy::block_unscoped(),
            ..Default::default()
        },
    )
    .await
    .expect_err("exotic child of non-workspace exotic parent must be rejected");

    let VerifyError::ExoticSubdep { count, breakdown } = err else {
        panic!("expected VerifyError::ExoticSubdep, got: {err:?}");
    };
    assert_eq!(count, 1);
    assert!(
        breakdown.contains("nested-exotic@https://example.com/nested.tgz (resolved via url)"),
        "breakdown:\n{breakdown}",
    );
}

#[tokio::test]
async fn rejects_tampered_file_key_backed_by_remote_tarball() {
    let lockfile = parse(
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

  sneaky@file:local.tgz:
    resolution: {tarball: https://evil.example/payload.tgz}
    version: 1.0.0

snapshots:

  reg-parent@1.0.0:
    dependencies:
      sneaky: file:local.tgz

  sneaky@file:local.tgz: {}
",
    );

    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            exotic_subdeps: ExoticSubdepsPolicy::block_unscoped(),
            ..Default::default()
        },
    )
    .await
    .expect_err("file: key with remote tarball URL must be rejected");

    assert!(
        matches!(
            err,
            VerifyError::ExoticSubdep { count: 1, .. }
                | VerifyError::LocalSubdepNotAllowed { count: 1, .. }
        ),
        "got: {err:?}",
    );
}

#[tokio::test]
async fn allows_direct_importer_workspace_snapshot_and_peer_provided_exotic_deps() {
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      direct-tarball:
        specifier: https://example.com/direct.tgz
        version: https://example.com/direct.tgz
      ws-injected:
        specifier: workspace:*
        version: file:packages/ws-injected
      ext-ws:
        specifier: workspace:../ext-ws
        version: file:../ext-ws
      react-dom:
        specifier: 18.2.0
        version: 18.2.0(direct-tarball@https://example.com/direct.tgz)

  packages/ws-injected:
    dependencies:
      ws-git:
        specifier: git+https://example.com/ws-git.git
        version: git+https://example.com/ws-git.git#22222222

packages:

  direct-tarball@https://example.com/direct.tgz:
    resolution: {tarball: https://example.com/direct.tgz, integrity: sha512-s4h96KtLDUQlsENhMn1ar8t2bEa+q/YAtj8pPPdIjPDGBDIVNsrD9aXNWqspUe6AzKCIG0C1HZZLqLV7qpOBGA==}
    version: 1.0.0

  ws-injected@file:packages/ws-injected:
    resolution: {type: directory, directory: packages/ws-injected}

  ext-ws@file:../ext-ws:
    resolution: {type: directory, directory: ../ext-ws}

  ws-git@git+https://example.com/ws-git.git#22222222:
    resolution: {type: git, repo: https://example.com/ws-git.git, commit: '22222222'}
    version: 1.0.0

  ext-tarball@https://example.com/ext.tgz:
    resolution: {tarball: https://example.com/ext.tgz}
    version: 1.0.0

  react-dom@18.2.0:
    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}
    peerDependencies:
      direct-tarball: '*'

  local-tgz@file:vendor/local.tgz:
    resolution: {tarball: file:vendor/local.tgz, integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}
    version: 1.0.0

  node@runtime:22.0.0:
    resolution: {type: binary, archive: tarball, bin: bin/node, integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==, url: https://nodejs.org/dist/v22.0.0/node-v22.0.0-linux-x64.tar.gz}
    version: 22.0.0

snapshots:

  direct-tarball@https://example.com/direct.tgz: {}

  ws-injected@file:packages/ws-injected:
    dependencies:
      local-tgz: file:vendor/local.tgz
      ws-git: git+https://example.com/ws-git.git#22222222

  ext-ws@file:../ext-ws:
    dependencies:
      ext-tarball: https://example.com/ext.tgz

  ws-git@git+https://example.com/ws-git.git#22222222: {}
  ext-tarball@https://example.com/ext.tgz: {}

  react-dom@18.2.0(direct-tarball@https://example.com/direct.tgz):
    dependencies:
      direct-tarball: https://example.com/direct.tgz
      node: runtime:22.0.0

  local-tgz@file:vendor/local.tgz: {}
  node@runtime:22.0.0: {}
",
    );

    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            exotic_subdeps: ExoticSubdepsPolicy::block(crate::ExoticSubdepsScope::new(
                Some([".", "packages/ws-injected"]),
                ["../ext-ws"],
                std::iter::empty(),
                std::iter::empty(),
            )),
            ..Default::default()
        },
    )
    .await
    .expect("direct importer, workspace snapshot, peer-provided exotic, local-file, and runtime subdeps must pass");
}

#[tokio::test]
async fn rejects_forged_peer_suffix_when_no_importer_or_workspace_provides_exotic_pkg() {
    let lockfile = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      reg-parent:
        specifier: 1.0.0
        version: 1.0.0(evil@https://evil.example/payload.tgz)

packages:

  reg-parent@1.0.0:
    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}
    peerDependencies:
      evil: '*'

  evil@https://evil.example/payload.tgz:
    resolution: {tarball: https://evil.example/payload.tgz}
    version: 1.0.0

snapshots:

  reg-parent@1.0.0(evil@https://evil.example/payload.tgz):
    dependencies:
      evil: https://evil.example/payload.tgz

  evil@https://evil.example/payload.tgz: {}
",
    );

    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            exotic_subdeps: ExoticSubdepsPolicy::block_unscoped(),
            ..Default::default()
        },
    )
    .await
    .expect_err("forged peer binding not provided by importer/workspace must be rejected");

    assert!(matches!(err, VerifyError::ExoticSubdep { count: 1, .. }), "got: {err:?}");
}

#[tokio::test]
async fn enabling_block_exotic_subdeps_invalidates_verification_cache() {
    let dir = TempDir::new().expect("tempdir");
    let lockfile_path = dir.path().join("pnpm-lock.yaml");
    std::fs::write(&lockfile_path, URL_SUBDEP_LOCKFILE).expect("write lockfile");
    let lockfile = parse(URL_SUBDEP_LOCKFILE);
    let cache_dir = dir.path().join("cache");
    let verifier = FailFor::new("UNUSED", "n/a", vec![]) as Arc<dyn ResolutionVerifier>;

    record_lockfile_verified_by_content(
        Some(&cache_dir),
        &lockfile,
        std::slice::from_ref(&verifier),
        &ExoticSubdepsPolicy::Allow,
    );
    assert!(
        lockfile_verification_is_cached(
            &cache_dir,
            &lockfile_path,
            &lockfile,
            std::slice::from_ref(&verifier),
            &ExoticSubdepsPolicy::Allow,
        ),
        "cache hit expected while policy stays Allow",
    );
    assert!(
        !lockfile_verification_is_cached(
            &cache_dir,
            &lockfile_path,
            &lockfile,
            std::slice::from_ref(&verifier),
            &ExoticSubdepsPolicy::block_unscoped(),
        ),
        "cache must miss when policy tightens from Allow to Block",
    );

    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        std::slice::from_ref(&verifier),
        &VerifyLockfileResolutionsOptions {
            lockfile_path: Some(&lockfile_path),
            cache_dir: Some(&cache_dir),
            exotic_subdeps: ExoticSubdepsPolicy::block_unscoped(),
            ..Default::default()
        },
    )
    .await
    .expect_err("must re-verify and reject after policy tightens");
    assert!(matches!(err, VerifyError::ExoticSubdep { count: 1, .. }), "got: {err:?}");
}

#[tokio::test]
async fn replaced_parent_skips_exotic_subdep_and_does_not_cache_lockfile() {
    fn is_reg_parent(candidate: ReplacedCandidate<'_>) -> bool {
        candidate.name.to_string() == "reg-parent" && candidate.version == "1.0.0"
    }

    let dir = TempDir::new().expect("tempdir");
    let lockfile_path = dir.path().join("pnpm-lock.yaml");
    std::fs::write(&lockfile_path, URL_SUBDEP_LOCKFILE).expect("write lockfile");
    let lockfile = parse(URL_SUBDEP_LOCKFILE);
    let cache_dir = dir.path().join("cache");

    verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            lockfile_path: Some(&lockfile_path),
            cache_dir: Some(&cache_dir),
            replaced: Some(ReplacedEntries(&is_reg_parent)),
            exotic_subdeps: ExoticSubdepsPolicy::block_unscoped(),
            ..Default::default()
        },
    )
    .await
    .expect("replaced parent edge should be skipped during targeted update");

    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            lockfile_path: Some(&lockfile_path),
            cache_dir: Some(&cache_dir),
            exotic_subdeps: ExoticSubdepsPolicy::block_unscoped(),
            ..Default::default()
        },
    )
    .await
    .expect_err("unreplaced run must still reject because skipped run was not cached");
    assert!(matches!(err, VerifyError::ExoticSubdep { count: 1, .. }), "got: {err:?}");
}
