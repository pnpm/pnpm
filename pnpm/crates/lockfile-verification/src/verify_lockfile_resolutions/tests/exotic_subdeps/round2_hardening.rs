use std::fmt::Write as _;

use pnpm_lockfile::PeerBindings;

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

fn hashed_suffix(segments: &[&str]) -> String {
    PeerBindings::Explicit(
        segments
            .iter()
            .map(|seg| (*seg).to_string())
            .collect(),
    )
    .to_suffix(0)
    .expect("hashed suffix")
}

#[tokio::test]
async fn backslash_directory_resolution_does_not_receive_workspace_exemption() {
    let lockfile = parse(
        r"lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      evil-dir:
        specifier: 'file:packages\evil'
        version: 'file:packages\evil'

packages:

  'evil-dir@file:packages\evil':
    resolution: {type: directory, directory: 'packages\evil'}

  evil@git+https://example.com/evil.git#22222222:
    resolution: {type: git, repo: https://example.com/evil.git, commit: '22222222'}
    version: 1.0.0

snapshots:

  'evil-dir@file:packages\evil':
    dependencies:
      evil: git+https://example.com/evil.git#22222222

  evil@git+https://example.com/evil.git#22222222: {}
",
    );

    let scoped = ExoticSubdepsPolicy::block(ExoticSubdepsScope::new(
        Some(["."]),
        ["packages/evil"],
        std::iter::empty(),
        std::iter::empty(),
    ));
    let err = verify_lockfile_resolutions::<SilentReporter>(
        &lockfile,
        &[],
        &VerifyLockfileResolutionsOptions { exotic_subdeps: scoped, ..Default::default() },
    )
    .await
    .expect_err("backslash directory resolution must not match packages/evil exemption");
    assert!(matches!(err, VerifyError::ExoticSubdep { count: 1, .. }), "got: {err:?}");
}

#[tokio::test]
async fn hashed_peer_recovery_succeeds_with_over_1024_unrelated_leaf_children() {
    let tarball_key = "tarball-peer@https://example.com/tarball-peer.tgz";
    let trans_key = "trans-peer@1.0.0";
    let hash_suffix = hashed_suffix(&[tarball_key, trans_key]);

    for leaf_count in [2_usize, 1_050_usize] {
        let mut parent_deps = String::from(
            "      tarball-peer: https://example.com/tarball-peer.tgz\n      carrier: 1.0.0\n",
        );
        let mut packages_yaml = String::new();
        let mut snapshots_yaml = String::new();
        for idx in 0..leaf_count {
            let _ = writeln!(parent_deps, "      leaf-{idx}: 1.0.0");
            let _ = writeln!(
                packages_yaml,
                "  leaf-{idx}@1.0.0:\n    resolution: {{integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}}",
            );
            let _ = writeln!(snapshots_yaml, "  leaf-{idx}@1.0.0: {{}}");
        }
        let lockfile = parse(&format!(
            "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      tarball-peer:
        specifier: https://example.com/tarball-peer.tgz
        version: https://example.com/tarball-peer.tgz
      reg-parent:
        specifier: 1.0.0
        version: 1.0.0{hash_suffix}

packages:

  reg-parent@1.0.0:
    resolution: {{integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}}
    peerDependencies:
      tarball-peer: '*'

  carrier@1.0.0:
    resolution: {{integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}}

  trans-peer@1.0.0:
    resolution: {{integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}}

  {tarball_key}:
    resolution: {{tarball: https://example.com/tarball-peer.tgz}}
    version: 1.0.0
{packages_yaml}
snapshots:

  reg-parent@1.0.0{hash_suffix}:
    dependencies:
{parent_deps}    transitivePeerDependencies:
      - trans-peer

  carrier@1.0.0:
    dependencies:
      trans-peer: 1.0.0

  trans-peer@1.0.0: {{}}
  {tarball_key}: {{}}
{snapshots_yaml}",
        ));

        verify_lockfile_resolutions::<SilentReporter>(
            &lockfile,
            &[],
            &VerifyLockfileResolutionsOptions {
                exotic_subdeps: ExoticSubdepsPolicy::block_unscoped(),
                ..Default::default()
            },
        )
        .await
        .unwrap_or_else(|err| panic!("leaf_count={leaf_count} failed: {err:?}"));
    }
}

#[tokio::test]
async fn hashed_peer_recovery_supports_workspace_link_and_mixed_cyclic_peers() {
    let tarball_no_peers = "tarball-peer@https://example.com/tarball-peer.tgz";
    let link_hash = hashed_suffix(&["ws-peer@packages+ws-peer", tarball_no_peers]);
    let link_lockfile = parse(&format!(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      ws-peer:
        specifier: workspace:*
        version: link:packages/ws-peer
      tarball-peer:
        specifier: https://example.com/tarball-peer.tgz
        version: https://example.com/tarball-peer.tgz
      reg-parent:
        specifier: 1.0.0
        version: 1.0.0{link_hash}

  packages/ws-peer: {{}}

packages:

  reg-parent@1.0.0:
    resolution: {{integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}}
    peerDependencies:
      ws-peer: '*'
      tarball-peer: '*'

  {tarball_no_peers}:
    resolution: {{tarball: https://example.com/tarball-peer.tgz}}
    version: 1.0.0

snapshots:

  reg-parent@1.0.0{link_hash}:
    dependencies:
      ws-peer: link:packages/ws-peer
      tarball-peer: https://example.com/tarball-peer.tgz

  {tarball_no_peers}: {{}}
",
    ));
    let scoped = ExoticSubdepsPolicy::block(ExoticSubdepsScope::new(
        Some([".", "packages/ws-peer"]),
        std::iter::empty(),
        std::iter::empty(),
        std::iter::empty(),
    ));
    verify_lockfile_resolutions::<SilentReporter>(
        &link_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions { exotic_subdeps: scoped, ..Default::default() },
    )
    .await
    .expect("workspace link peer + allowed tarball peer with hashed suffix must pass");

    let tarball_with_peer = "tarball-peer@https://example.com/tarball-peer.tgz(sub-peer@1.0.0)";
    let mixed_hash = hashed_suffix(&["cyclic-a@1.0.0", tarball_with_peer]);
    let mixed_lockfile = parse(&format!(
        "lockfileVersion: '9.0'

importers:

  .:
    dependencies:
      sub-peer:
        specifier: 1.0.0
        version: 1.0.0
      cyclic-a:
        specifier: 1.0.0
        version: 1.0.0(cyclic-b@1.0.0)
      cyclic-b:
        specifier: 1.0.0
        version: 1.0.0(cyclic-a@1.0.0)
      tarball-peer:
        specifier: https://example.com/tarball-peer.tgz
        version: https://example.com/tarball-peer.tgz(sub-peer@1.0.0)
      reg-parent:
        specifier: 1.0.0
        version: 1.0.0{mixed_hash}

packages:

  sub-peer@1.0.0:
    resolution: {{integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}}

  cyclic-a@1.0.0:
    resolution: {{integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}}
    peerDependencies:
      cyclic-b: '*'

  cyclic-b@1.0.0:
    resolution: {{integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}}
    peerDependencies:
      cyclic-a: '*'

  tarball-peer@https://example.com/tarball-peer.tgz:
    resolution: {{tarball: https://example.com/tarball-peer.tgz}}
    version: 1.0.0
    peerDependencies:
      sub-peer: '*'

  reg-parent@1.0.0:
    resolution: {{integrity: sha512-TIE61hcgbI/SlJh/0c1sT1SZbBlpg7WiZcs65WPJhoIZQPhH1SCpcGA7LgrVXT15lwN3HV4GQM/MJ9aKEn3Qfg==}}
    peerDependencies:
      cyclic-a: '*'
      tarball-peer: '*'

snapshots:

  sub-peer@1.0.0: {{}}
  cyclic-a@1.0.0(cyclic-b@1.0.0):
    dependencies:
      cyclic-b: 1.0.0(cyclic-a@1.0.0)
  cyclic-b@1.0.0(cyclic-a@1.0.0):
    dependencies:
      cyclic-a: 1.0.0(cyclic-b@1.0.0)
  {tarball_with_peer}:
    dependencies:
      sub-peer: 1.0.0
  reg-parent@1.0.0{mixed_hash}:
    dependencies:
      cyclic-a: 1.0.0(cyclic-b@1.0.0)
      tarball-peer: https://example.com/tarball-peer.tgz(sub-peer@1.0.0)
",
    ));
    verify_lockfile_resolutions::<SilentReporter>(
        &mixed_lockfile,
        &[],
        &VerifyLockfileResolutionsOptions {
            exotic_subdeps: ExoticSubdepsPolicy::block_unscoped(),
            ..Default::default()
        },
    )
    .await
    .expect("mixed cyclic registry peer + full-depPath tarball peer must pass");
}
