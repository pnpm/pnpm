use super::{
    BTreeMap, Catalogs, Lockfile, LockfileSettingsCheck, StalenessReason, assert_eq,
    check_lockfile_settings, settings_check, text_block,
};

// ---------------------------------------------------------------------------
// `catalogs` drift — the first lockfile-settings check
// ---------------------------------------------------------------------------

#[test]
fn check_settings_passes_when_catalog_snapshot_matches_config() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "catalogs:"
        "  default:"
        "    react:"
        "      specifier: ^18.2.0"
        "      version: 18.2.0"
    })
    .expect("parse lockfile with catalogs");
    let catalogs = Catalogs::from([(
        "default".to_string(),
        BTreeMap::from([("react".to_string(), "^18.2.0".to_string())]),
    )]);
    assert!(check_lockfile_settings(&lockfile, settings_check(&catalogs)).is_ok());
}

#[test]
fn check_settings_accepts_equivalent_git_catalog_specifiers() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "catalogs:"
        "  default:"
        "    is-positive:"
        "      specifier: git+https://github.com/kevva/is-positive.git#97edff6"
        "      version: git+https://github.com/kevva/is-positive.git#97edff6"
    })
    .expect("parse lockfile with catalogs");
    let catalogs = Catalogs::from([(
        "default".to_string(),
        BTreeMap::from([(
            "is-positive".to_string(),
            "github:kevva/is-positive#97edff6".to_string(),
        )]),
    )]);
    check_lockfile_settings(&lockfile, settings_check(&catalogs))
        .expect("equivalent git catalog specifiers must be current");
}

#[test]
fn check_settings_ignores_catalog_config_entries_absent_from_snapshot() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse lockfile without catalog snapshot");
    let catalogs = Catalogs::from([(
        "default".to_string(),
        BTreeMap::from([("react".to_string(), "^18.2.0".to_string())]),
    )]);
    assert!(check_lockfile_settings(&lockfile, settings_check(&catalogs)).is_ok());
}

#[test]
fn check_settings_returns_drift_when_catalog_snapshot_specifier_changes() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "catalogs:"
        "  default:"
        "    react:"
        "      specifier: ^18.2.0"
        "      version: 18.2.0"
    })
    .expect("parse lockfile with catalogs");
    let catalogs = Catalogs::from([(
        "default".to_string(),
        BTreeMap::from([("react".to_string(), "^19.0.0".to_string())]),
    )]);
    let err = check_lockfile_settings(&lockfile, settings_check(&catalogs))
        .expect_err("changed catalog entry must surface drift");
    let StalenessReason::CatalogsChanged { lockfile: snapshot, config } = err else {
        panic!("expected CatalogsChanged");
    };
    assert_eq!(
        snapshot
            .as_ref()
            .and_then(|catalogs| catalogs.get("default"))
            .and_then(|catalog| catalog.get("react"))
            .map(|entry| entry.specifier.as_str()),
        Some("^18.2.0"),
    );
    assert_eq!(
        config.get("default").and_then(|catalog| catalog.get("react")).map(String::as_str),
        Some("^19.0.0"),
    );
}

#[test]
fn check_settings_returns_drift_when_catalog_snapshot_entry_is_removed_from_config() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "catalogs:"
        "  default:"
        "    react:"
        "      specifier: ^18.2.0"
        "      version: 18.2.0"
    })
    .expect("parse lockfile with catalogs");
    let catalogs = Catalogs::from([("default".to_string(), BTreeMap::new())]);
    let err = check_lockfile_settings(&lockfile, settings_check(&catalogs))
        .expect_err("removed catalog entry must surface drift");
    assert!(matches!(err, StalenessReason::CatalogsChanged { .. }));
}

#[test]
fn check_settings_passes_when_inject_workspace_packages_both_false() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    assert!(check_lockfile_settings(&lockfile, settings_check(&Catalogs::new())).is_ok());
}

#[test]
fn check_settings_passes_when_inject_workspace_packages_both_true() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "settings:"
        "  autoInstallPeers: false"
        "  excludeLinksFromLockfile: false"
        "  injectWorkspacePackages: true"
    })
    .expect("parse lockfile with inject on");
    assert!(
        check_lockfile_settings(
            &lockfile,
            LockfileSettingsCheck {
                auto_install_peers: false,
                inject_workspace_packages: true,
                ..settings_check(&Catalogs::new())
            }
        )
        .is_ok(),
    );
}

#[test]
fn check_settings_returns_drift_when_config_enables_inject_workspace_packages() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck {
            inject_workspace_packages: true,
            ..settings_check(&Catalogs::new())
        },
    )
    .expect_err("enabling inject must surface drift");
    assert_eq!(
        err,
        StalenessReason::InjectWorkspacePackagesChanged { lockfile: false, config: true },
    );
}

#[test]
fn check_settings_returns_drift_when_config_disables_inject_workspace_packages() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "settings:"
        "  autoInstallPeers: false"
        "  excludeLinksFromLockfile: false"
        "  injectWorkspacePackages: true"
    })
    .expect("parse lockfile with inject on");
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck { auto_install_peers: false, ..settings_check(&Catalogs::new()) },
    )
    .expect_err("disabling inject must surface drift");
    assert_eq!(
        err,
        StalenessReason::InjectWorkspacePackagesChanged { lockfile: true, config: false },
    );
}
