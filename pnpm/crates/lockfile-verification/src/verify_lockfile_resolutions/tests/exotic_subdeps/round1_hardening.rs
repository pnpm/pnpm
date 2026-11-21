use super::{
    SilentReporter,
    VerifyError,
    VerifyLockfileResolutionsOptions,
    parse,
    verify_lockfile_resolutions,
};
use crate::{
    ExoticSubdepsPolicy,
    ExoticSubdepsScope,
};

#[tokio::test]
async fn non_canonical_importer_keys_cannot_impersonate_root_to_grant_git_peer_exemption() {
    for forged_key in ["./", "ghost/.."] {
        let lockfile = parse(&format!(
            "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      reg-with-peer:
        specifier: 1.0.0
        version: 1.0.0(git-peer@git+https://example.com/peer.git#11111111)

  '{forged_key}':
    dependencies:
      git-peer:
        specifier: git+https://example.com/peer.git
        version: git+https://example.com/peer.git#11111111

packages:

  reg-with-peer@1.0.0:
    resolution: {{integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}}
    peerDependencies:
      git-peer: '*'

  git-peer@git+https://example.com/peer.git#11111111:
    resolution: {{type: git, repo: https://example.com/peer.git, commit: '11111111'}}
    version: 1.0.0

snapshots:

  reg-with-peer@1.0.0(git-peer@git+https://example.com/peer.git#11111111):
    dependencies:
      git-peer: git+https://example.com/peer.git#11111111

  git-peer@git+https://example.com/peer.git#11111111: {{}}
",
        ));

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
        .expect_err("forged non-canonical importer key must not grant peer exemption");
        assert!(
            matches!(err, VerifyError::ExoticSubdep { count: 1, .. }),
            "forged_key={forged_key}, got: {err:?}",
        );
    }
}

#[tokio::test]
async fn unverified_lockfile_publish_directory_and_workspace_link_targets_do_not_grant_exemptions()
{
    let forged_publish_dir = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    publishDirectory: packages/forged-pub
    dependencies:
      forged-pub:
        specifier: file:packages/forged-pub
        version: file:packages/forged-pub

packages:

  forged-pub@file:packages/forged-pub:
    resolution: {type: directory, directory: packages/forged-pub}

  evil@git+https://example.com/evil.git#22222222:
    resolution: {type: git, repo: https://example.com/evil.git, commit: '22222222'}
    version: 1.0.0

snapshots:

  forged-pub@file:packages/forged-pub:
    dependencies:
      evil: git+https://example.com/evil.git#22222222

  evil@git+https://example.com/evil.git#22222222: {}
",
    );

    let scoped = ExoticSubdepsPolicy::block(ExoticSubdepsScope::new(
        Some(["."]),
        std::iter::empty(),
        std::iter::empty(),
        std::iter::empty(),
    ));
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &forged_publish_dir,
        &[],
        &VerifyLockfileResolutionsOptions { exotic_subdeps: scoped.clone(), ..Default::default() },
    )
    .await
    .expect_err("locked publishDirectory alone must not exempt directory snapshot");
    assert!(matches!(err, VerifyError::ExoticSubdep { count: 1, .. }), "got: {err:?}");

    for forged_dir in ["packages/forged-ws", "../escape-ws", "/abs/escape"] {
        let forged_ws_link = parse(&format!(
            "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      forged-ws:
        specifier: workspace:*
        version: link:{forged_dir}

packages:

  forged-ws@file:{forged_dir}:
    resolution: {{type: directory, directory: '{forged_dir}'}}

  evil@git+https://example.com/evil.git#22222222:
    resolution: {{type: git, repo: https://example.com/evil.git, commit: '22222222'}}
    version: 1.0.0

snapshots:

  forged-ws@file:{forged_dir}:
    dependencies:
      evil: git+https://example.com/evil.git#22222222

  evil@git+https://example.com/evil.git#22222222: {{}}
",
        ));

        let link_err = verify_lockfile_resolutions::<SilentReporter>(
            &forged_ws_link,
            &[],
            &VerifyLockfileResolutionsOptions {
                exotic_subdeps: scoped.clone(),
                ..Default::default()
            },
        )
        .await
        .expect_err("locked workspace: target must not exempt unverified directory");
        assert!(
            matches!(
                link_err,
                VerifyError::ExoticSubdep { count: 1, .. }
                    | VerifyError::LocalSubdepNotAllowed { count: 1, .. }
            ),
            "forged_dir={forged_dir}, got: {link_err:?}",
        );
    }
}

#[tokio::test]
async fn runtime_urls_reject_encoded_traversal_and_version_mismatch() {
    let bad_urls = [
        "https://github.com/oven-sh/bun/releases/download/%2e%2e/%2e%2e/evil/repo/releases/download/bun-v1.1.0/bun-linux-x64.zip",
        "https://github.com/oven-sh/bun/releases/download/bun-v1.1.0%2f..%2fevil/bun-linux-x64.zip",
        "https://github.com/oven-sh/bun/releases/download/bun-v1.2.0/bun-linux-x64.zip",
        "https://user@github.com/oven-sh/bun/releases/download/bun-v1.1.0/bun-linux-x64.zip",
    ];
    for url in bad_urls {
        let lockfile = parse(&format!(
            "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      reg-parent:
        specifier: 1.0.0
        version: 1.0.0

packages:

  reg-parent@1.0.0:
    resolution: {{integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}}

  bun@runtime:1.1.0:
    resolution: {{type: binary, archive: zip, url: '{url}', integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==, bin: bun}}
    version: 1.1.0

snapshots:

  reg-parent@1.0.0:
    dependencies:
      bun: runtime:1.1.0

  bun@runtime:1.1.0: {{}}
",
        ));

        let err = verify_lockfile_resolutions::<SilentReporter>(
            &lockfile,
            &[],
            &VerifyLockfileResolutionsOptions {
                exotic_subdeps: ExoticSubdepsPolicy::block_unscoped(),
                ..Default::default()
            },
        )
        .await
        .expect_err("tampered runtime binary URL must be rejected");
        assert!(
            matches!(err, VerifyError::ExoticSubdep { count: 1, .. }),
            "url={url}, got: {err:?}",
        );
    }
}

#[tokio::test]
async fn forged_hashed_peer_suffix_and_mismatched_peer_variant_are_rejected() {
    let forged_hash = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      direct-git:
        specifier: git+https://example.com/repo.git
        version: git+https://example.com/repo.git#11111111
      reg-parent:
        specifier: 1.0.0
        version: 1.0.0(0123456789abcdef0123456789abcdef)

packages:

  reg-parent@1.0.0:
    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}
    peerDependencies:
      direct-git: '*'

  direct-git@git+https://example.com/repo.git#11111111:
    resolution: {type: git, repo: https://example.com/repo.git, commit: '11111111'}
    version: 1.0.0

snapshots:

  reg-parent@1.0.0(0123456789abcdef0123456789abcdef):
    dependencies:
      direct-git: git+https://example.com/repo.git#11111111

  direct-git@git+https://example.com/repo.git#11111111: {}
",
    );

    let err = verify_lockfile_resolutions::<SilentReporter>(
        &forged_hash,
        &[],
        &VerifyLockfileResolutionsOptions {
            exotic_subdeps: ExoticSubdepsPolicy::block_unscoped(),
            ..Default::default()
        },
    )
    .await
    .expect_err("unrecoverable hashed peer suffix must fail closed");
    assert!(matches!(err, VerifyError::ExoticSubdep { count: 1, .. }), "got: {err:?}");

    let mismatched_peer_variant = parse(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      other-peer:
        specifier: 1.0.0
        version: 1.0.0
      direct-git:
        specifier: git+https://example.com/repo.git
        version: git+https://example.com/repo.git#11111111(other-peer@1.0.0)
      reg-parent:
        specifier: 1.0.0
        version: 1.0.0(direct-git@git+https://example.com/repo.git#11111111(evil-peer@2.0.0))

packages:

  other-peer@1.0.0:
    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}

  evil-peer@2.0.0:
    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}

  reg-parent@1.0.0:
    resolution: {integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}
    peerDependencies:
      direct-git: '*'

  direct-git@git+https://example.com/repo.git#11111111:
    resolution: {type: git, repo: https://example.com/repo.git, commit: '11111111'}
    version: 1.0.0
    peerDependencies:
      other-peer: '*'

snapshots:

  other-peer@1.0.0: {}
  evil-peer@2.0.0: {}

  direct-git@git+https://example.com/repo.git#11111111(other-peer@1.0.0):
    dependencies:
      other-peer: 1.0.0

  direct-git@git+https://example.com/repo.git#11111111(evil-peer@2.0.0):
    dependencies:
      other-peer: evil-peer@2.0.0

  reg-parent@1.0.0(direct-git@git+https://example.com/repo.git#11111111(evil-peer@2.0.0)):
    dependencies:
      direct-git: git+https://example.com/repo.git#11111111(evil-peer@2.0.0)
",
    );

    let variant_err = verify_lockfile_resolutions::<SilentReporter>(
        &mismatched_peer_variant,
        &[],
        &VerifyLockfileResolutionsOptions {
            exotic_subdeps: ExoticSubdepsPolicy::block_unscoped(),
            ..Default::default()
        },
    )
    .await
    .expect_err("peer variant with different peer suffix than importer's must be rejected");
    assert!(
        matches!(variant_err, VerifyError::ExoticSubdep { count: 1, .. }),
        "got: {variant_err:?}",
    );
}

#[test]
fn scope_cache_digest_frames_entries_by_length() {
    let scope_a = ExoticSubdepsScope::new(
        Some(["a\0extra_dirs"]),
        std::iter::empty(),
        std::iter::empty(),
        std::iter::empty(),
    );
    let scope_b = ExoticSubdepsScope::new(
        Some(["a"]),
        ["extra_dirs"],
        std::iter::empty(),
        std::iter::empty(),
    );
    assert_ne!(scope_a.cache_digest(), scope_b.cache_digest());
}
