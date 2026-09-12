use super::{
    Catalogs, Lockfile, LockfileSettingsCheck, assert_eq, check_lockfile_settings, settings_check,
    text_block,
};

/// pnpm compares `pnpmfileChecksum` after `settings.peersSuffixMaxLength`
/// and before `settings.injectWorkspacePackages`, and only the first
/// drifted field is reported.
#[test]
fn check_settings_reports_pnpmfile_checksum_between_its_pnpm_neighbors() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "pnpmfileChecksum: sha256-abc"
        "settings:"
        "  autoInstallPeers: true"
        "  excludeLinksFromLockfile: false"
        "  peersSuffixMaxLength: 10"
    })
    .expect("parse lockfile with a checksum and settings");
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck {
            peers_suffix_max_length: 100,
            inject_workspace_packages: true,
            ..settings_check(&Catalogs::new())
        },
    )
    .expect_err("all three fields drifted");
    assert_eq!(err.setting_name(), Some("settings.peersSuffixMaxLength"));

    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck {
            peers_suffix_max_length: 10,
            inject_workspace_packages: true,
            ..settings_check(&Catalogs::new())
        },
    )
    .expect_err("the checksum and injectWorkspacePackages drifted");
    assert_eq!(err.setting_name(), Some("pnpmfileChecksum"));
}
