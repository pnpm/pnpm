use super::{
    Catalogs, Lockfile, LockfileSettingsCheck, PnpmfileChecksumCheck, StalenessReason, assert_eq,
    check_lockfile_settings, settings_check, text_block,
};

// ---------------------------------------------------------------------------
// `ignoredOptionalDependencies` — umbrella <https://github.com/pnpm/pacquet/issues/434> slice 7
// ---------------------------------------------------------------------------

#[test]
fn check_settings_passes_when_both_sides_empty() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    assert!(check_lockfile_settings(&lockfile, settings_check(&Catalogs::new())).is_ok());
    assert!(
        check_lockfile_settings(
            &lockfile,
            LockfileSettingsCheck {
                ignored_optional_dependencies: Some(&[]),
                ..settings_check(&Catalogs::new())
            }
        )
        .is_ok(),
    );
}

#[test]
fn check_settings_passes_when_sets_match_regardless_of_order() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "ignoredOptionalDependencies:"
        "  - foo"
        "  - bar"
    })
    .expect("parse lockfile with ignoredOptionalDependencies");
    let config_set = ["bar".to_string(), "foo".to_string()];
    assert!(
        check_lockfile_settings(
            &lockfile,
            LockfileSettingsCheck {
                ignored_optional_dependencies: Some(&config_set),
                ..settings_check(&Catalogs::new())
            }
        )
        .is_ok(),
    );
}

#[test]
fn check_settings_returns_drift_when_sets_differ() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "ignoredOptionalDependencies:"
        "  - foo"
    })
    .expect("parse lockfile with ignoredOptionalDependencies");
    let config_set = ["bar".to_string()];
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck {
            ignored_optional_dependencies: Some(&config_set),
            ..settings_check(&Catalogs::new())
        },
    )
    .expect_err("set drift must surface as IgnoredOptionalDependenciesChanged");
    assert_eq!(
        err,
        StalenessReason::IgnoredOptionalDependenciesChanged {
            lockfile: vec!["foo".to_string()],
            config: vec!["bar".to_string()],
        },
    );
}

#[test]
fn check_settings_returns_drift_when_lockfile_has_set_but_config_does_not() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "ignoredOptionalDependencies:"
        "  - foo"
    })
    .expect("parse lockfile with ignoredOptionalDependencies");
    let err = check_lockfile_settings(&lockfile, settings_check(&Catalogs::new()))
        .expect_err("removing a set in config while lockfile has it must surface drift");
    let StalenessReason::IgnoredOptionalDependenciesChanged { lockfile: l, config: c } = err else {
        panic!("expected IgnoredOptionalDependenciesChanged");
    };
    assert_eq!(l, vec!["foo".to_string()]);
    assert!(c.is_empty());
}

// ---------------------------------------------------------------------------
// `overrides` drift — the lockfile-side overrides check
// ---------------------------------------------------------------------------

#[test]
fn check_settings_passes_when_overrides_both_empty() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    assert!(check_lockfile_settings(&lockfile, settings_check(&Catalogs::new())).is_ok());

    let empty: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    assert!(
        check_lockfile_settings(
            &lockfile,
            LockfileSettingsCheck { overrides: Some(&empty), ..settings_check(&Catalogs::new()) }
        )
        .is_ok(),
    );
}

#[test]
fn check_settings_passes_when_overrides_match_regardless_of_order() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "overrides:"
        "  foo: 1.0.0"
        "  bar: 2.0.0"
    })
    .expect("parse lockfile with overrides");
    let mut config: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    config.insert("bar".to_string(), "2.0.0".to_string());
    config.insert("foo".to_string(), "1.0.0".to_string());
    assert!(
        check_lockfile_settings(
            &lockfile,
            LockfileSettingsCheck { overrides: Some(&config), ..settings_check(&Catalogs::new()) }
        )
        .is_ok(),
    );
}

#[test]
fn check_settings_returns_drift_on_overrides_value_change() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "overrides:"
        "  foo: 1.0.0"
    })
    .expect("parse lockfile with overrides");
    let mut config: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    config.insert("foo".to_string(), "2.0.0".to_string());
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck { overrides: Some(&config), ..settings_check(&Catalogs::new()) },
    )
    .expect_err("changed override value must surface drift");
    let StalenessReason::OverridesChanged { lockfile: l, config: c } = err else {
        panic!("expected OverridesChanged");
    };
    assert_eq!(l.get("foo").map(String::as_str), Some("1.0.0"));
    assert_eq!(c.get("foo").map(String::as_str), Some("2.0.0"));
}

#[test]
fn check_settings_returns_drift_when_lockfile_has_overrides_but_config_does_not() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "overrides:"
        "  foo: 1.0.0"
    })
    .expect("parse lockfile with overrides");
    let err = check_lockfile_settings(&lockfile, settings_check(&Catalogs::new()))
        .expect_err("dropped override must surface drift");
    let StalenessReason::OverridesChanged { lockfile: l, config: c } = err else {
        panic!("expected OverridesChanged");
    };
    assert_eq!(l.get("foo").map(String::as_str), Some("1.0.0"));
    assert!(c.is_empty());
}

#[test]
fn check_settings_returns_drift_when_config_has_overrides_but_lockfile_does_not() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    let mut config: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    config.insert("foo".to_string(), "1.0.0".to_string());
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck { overrides: Some(&config), ..settings_check(&Catalogs::new()) },
    )
    .expect_err("added override must surface drift");
    let StalenessReason::OverridesChanged { lockfile: l, config: c } = err else {
        panic!("expected OverridesChanged");
    };
    assert!(l.is_empty());
    assert_eq!(c.get("foo").map(String::as_str), Some("1.0.0"));
}

// ---------------------------------------------------------------------------
// `patchedDependencies` drift — the lockfile-side patchedDependencies check
// ---------------------------------------------------------------------------

#[test]
fn check_settings_passes_when_patched_dependencies_match() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "patchedDependencies:"
        "  graceful-fs@4.2.11: abc123"
    })
    .expect("parse lockfile with patchedDependencies");
    let config = std::collections::BTreeMap::from([(
        "graceful-fs@4.2.11".to_string(),
        "abc123".to_string(),
    )]);
    assert!(
        check_lockfile_settings(
            &lockfile,
            LockfileSettingsCheck {
                patched_dependencies: Some(&config),
                ..settings_check(&Catalogs::new())
            }
        )
        .is_ok(),
    );
}

/// A changed patch-file hash (e.g. the user edited the patch) surfaces
/// as `PatchedDependenciesChanged` so the frozen install is rejected
/// rather than silently materializing against a stale `(patch_hash=...)`.
#[test]
fn check_settings_returns_drift_when_patch_hash_changes() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "patchedDependencies:"
        "  graceful-fs@4.2.11: oldhash"
    })
    .expect("parse lockfile with patchedDependencies");
    let config = std::collections::BTreeMap::from([(
        "graceful-fs@4.2.11".to_string(),
        "newhash".to_string(),
    )]);
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck {
            patched_dependencies: Some(&config),
            ..settings_check(&Catalogs::new())
        },
    )
    .expect_err("changed patch hash must surface drift");
    let StalenessReason::PatchedDependenciesChanged { lockfile: l, config: c } = err else {
        panic!("expected PatchedDependenciesChanged, got {err:?}");
    };
    assert_eq!(l.get("graceful-fs@4.2.11").map(String::as_str), Some("oldhash"));
    assert_eq!(c.get("graceful-fs@4.2.11").map(String::as_str), Some("newhash"));
}

#[test]
fn check_settings_returns_drift_when_patch_removed_from_config() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "patchedDependencies:"
        "  graceful-fs@4.2.11: abc123"
    })
    .expect("parse lockfile with patchedDependencies");
    let err = check_lockfile_settings(&lockfile, settings_check(&Catalogs::new()))
        .expect_err("dropped patch must surface drift");
    let StalenessReason::PatchedDependenciesChanged { lockfile: l, config: c } = err else {
        panic!("expected PatchedDependenciesChanged, got {err:?}");
    };
    assert_eq!(l.get("graceful-fs@4.2.11").map(String::as_str), Some("abc123"));
    assert!(c.is_empty());
}

#[test]
fn check_settings_returns_ok_when_no_package_extensions_checksum_on_either_side() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    assert!(check_lockfile_settings(&lockfile, settings_check(&Catalogs::new())).is_ok());
}

#[test]
fn check_settings_returns_ok_when_package_extensions_checksum_matches() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "packageExtensionsChecksum: sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
    })
    .expect("parse lockfile");
    assert!(
        check_lockfile_settings(
            &lockfile,
            LockfileSettingsCheck {
                package_extensions_checksum: Some(
                    "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
                ),
                ..settings_check(&Catalogs::new())
            }
        )
        .is_ok(),
    );
}

/// A changed `packageExtensionsChecksum` between the lockfile and the
/// current config surfaces as drift.
#[test]
fn check_settings_returns_drift_on_package_extensions_checksum_value_change() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "packageExtensionsChecksum: sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
    })
    .expect("parse lockfile");
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck {
            package_extensions_checksum: Some(
                "sha256-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB=",
            ),
            ..settings_check(&Catalogs::new())
        },
    )
    .expect_err("changed checksum must surface drift");
    let StalenessReason::PackageExtensionsChecksumChanged { lockfile: l, config: c } = err else {
        panic!("expected PackageExtensionsChecksumChanged, got {err:?}");
    };
    assert_eq!(l.as_deref(), Some("sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="));
    assert_eq!(c.as_deref(), Some("sha256-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB="));
}

#[test]
fn check_settings_returns_drift_when_lockfile_has_checksum_but_config_does_not() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "packageExtensionsChecksum: sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
    })
    .expect("parse lockfile");
    let err = check_lockfile_settings(&lockfile, settings_check(&Catalogs::new()))
        .expect_err("dropped extensions must surface drift");
    let StalenessReason::PackageExtensionsChecksumChanged { lockfile: l, config: c } = err else {
        panic!("expected PackageExtensionsChecksumChanged, got {err:?}");
    };
    assert_eq!(l.as_deref(), Some("sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="));
    assert!(c.is_none());
}

#[test]
fn check_settings_returns_drift_when_config_has_checksum_but_lockfile_does_not() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck {
            package_extensions_checksum: Some(
                "sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            ),
            ..settings_check(&Catalogs::new())
        },
    )
    .expect_err("added extensions must surface drift");
    let StalenessReason::PackageExtensionsChecksumChanged { lockfile: l, config: c } = err else {
        panic!("expected PackageExtensionsChecksumChanged, got {err:?}");
    };
    assert!(l.is_none());
    assert_eq!(c.as_deref(), Some("sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="));
}

#[test]
fn check_settings_reports_overrides_before_ignored_optional() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "overrides:"
        "  foo: 1.0.0"
        "ignoredOptionalDependencies:"
        "  - bar"
    })
    .expect("parse lockfile");
    let mut config: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    config.insert("foo".to_string(), "2.0.0".to_string());
    let ignored: [String; 0] = [];
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck {
            overrides: Some(&config),
            ignored_optional_dependencies: Some(&ignored),
            ..settings_check(&Catalogs::new())
        },
    )
    .expect_err("both drifted; expect OverridesChanged surfaced");
    assert!(
        matches!(err, StalenessReason::OverridesChanged { .. }),
        "expected OverridesChanged first, got {err:?}",
    );
}

// ---------------------------------------------------------------------------
// `injectWorkspacePackages` drift — the lockfile-side Boolean-normalized
// comparison.
// ---------------------------------------------------------------------------

/// Both sides false → no drift. Pacquet's wire format omits the
/// `settings.injectWorkspacePackages` key when `false`, so a lockfile
/// missing the field entirely deserializes to `false` and compares
/// equal to a config that also has it off.
/// pnpm refuses a frozen install when `settings.autoInstallPeers`
/// drifts: the setting decides whether peers are folded into the
/// importer snapshot, so the recorded resolution isn't reproducible
/// under the other value.
#[test]
fn check_settings_returns_drift_when_auto_install_peers_differs() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "settings:"
        "  autoInstallPeers: true"
        "  excludeLinksFromLockfile: false"
    })
    .expect("parse lockfile with settings");
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck { auto_install_peers: false, ..settings_check(&Catalogs::new()) },
    )
    .expect_err("a flipped autoInstallPeers must surface drift");
    assert_eq!(err, StalenessReason::AutoInstallPeersChanged { lockfile: true, config: false });
    assert_eq!(err.setting_name(), Some("settings.autoInstallPeers"));
}

/// A lockfile with no `settings` block records no value to disagree
/// with.
#[test]
fn check_settings_ignores_auto_install_peers_without_a_settings_block() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    assert!(
        check_lockfile_settings(
            &lockfile,
            LockfileSettingsCheck { auto_install_peers: false, ..settings_check(&Catalogs::new()) },
        )
        .is_ok(),
    );
}

/// `dedupePeers` is written only while it is on, so an absent key reads
/// as `false` and turning the setting on is drift.
#[test]
fn check_settings_returns_drift_when_dedupe_peers_is_enabled() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "settings:"
        "  autoInstallPeers: true"
        "  excludeLinksFromLockfile: false"
    })
    .expect("parse lockfile with settings");
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck { dedupe_peers: true, ..settings_check(&Catalogs::new()) },
    )
    .expect_err("enabling dedupePeers must surface drift");
    assert_eq!(err, StalenessReason::DedupePeersChanged { lockfile: false, config: true });
    assert_eq!(err.setting_name(), Some("settings.dedupePeers"));
}

#[test]
fn check_settings_returns_drift_when_exclude_links_from_lockfile_differs() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "settings:"
        "  autoInstallPeers: true"
        "  excludeLinksFromLockfile: false"
    })
    .expect("parse lockfile with settings");
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck {
            exclude_links_from_lockfile: true,
            ..settings_check(&Catalogs::new())
        },
    )
    .expect_err("a flipped excludeLinksFromLockfile must surface drift");
    assert_eq!(
        err,
        StalenessReason::ExcludeLinksFromLockfileChanged { lockfile: false, config: true },
    );
    assert_eq!(err.setting_name(), Some("settings.excludeLinksFromLockfile"));
}

/// Only the first drifted field is reported, and pnpm's
/// `getOutdatedLockfileSetting` reports them in a fixed order — with
/// both `overrides` and `settings.autoInstallPeers` drifted, both
/// stacks must name `overrides`.
#[test]
fn check_settings_reports_the_field_pnpm_reports_first() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "settings:"
        "  autoInstallPeers: true"
        "  excludeLinksFromLockfile: false"
        "overrides:"
        "  is-number: 6.0.0"
    })
    .expect("parse lockfile with overrides and settings");
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck { auto_install_peers: false, ..settings_check(&Catalogs::new()) },
    )
    .expect_err("both fields drifted");
    assert_eq!(err.setting_name(), Some("overrides"));
}

// ---------------------------------------------------------------------------
// `peersSuffixMaxLength` drift — the lockfile-side peersSuffixMaxLength check
// ---------------------------------------------------------------------------

#[test]
fn check_settings_passes_when_peers_suffix_max_length_unset_and_config_is_default() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    assert!(check_lockfile_settings(&lockfile, settings_check(&Catalogs::new())).is_ok());
}

/// Lockfile carries no `settings.peersSuffixMaxLength` (writer used
/// the default — the field is stripped at that point), but the current
/// config asks for a non-default value. That's drift: the recorded
/// dep paths assume 1000; re-resolving under a different cap would
/// produce a different graph.
#[test]
fn check_settings_returns_drift_when_lockfile_implicit_default_differs_from_config() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck { peers_suffix_max_length: 10, ..settings_check(&Catalogs::new()) },
    )
    .expect_err("config != default must surface drift when lockfile is unset");
    assert_eq!(
        err,
        StalenessReason::PeersSuffixMaxLengthChanged {
            lockfile: crate::DEFAULT_PEERS_SUFFIX_MAX_LENGTH,
            config: 10,
        },
    );
}

#[test]
fn check_settings_passes_when_explicit_peers_suffix_max_length_matches() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "settings:"
        "  autoInstallPeers: false"
        "  excludeLinksFromLockfile: false"
        "  peersSuffixMaxLength: 10"
    })
    .expect("parse lockfile with settings");
    assert!(
        check_lockfile_settings(
            &lockfile,
            LockfileSettingsCheck {
                auto_install_peers: false,
                peers_suffix_max_length: 10,
                ..settings_check(&Catalogs::new())
            }
        )
        .is_ok(),
    );
}

/// An explicit `settings.peersSuffixMaxLength` in the lockfile that
/// differs from the current config surfaces as drift.
#[test]
fn check_settings_returns_drift_when_explicit_peers_suffix_max_length_differs() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "settings:"
        "  autoInstallPeers: false"
        "  excludeLinksFromLockfile: false"
        "  peersSuffixMaxLength: 10"
    })
    .expect("parse lockfile with settings");
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck {
            auto_install_peers: false,
            peers_suffix_max_length: 100,
            ..settings_check(&Catalogs::new())
        },
    )
    .expect_err("changed peersSuffixMaxLength must surface drift");
    assert_eq!(err, StalenessReason::PeersSuffixMaxLengthChanged { lockfile: 10, config: 100 });
}

// ---------------------------------------------------------------------------
// `pnpmfileChecksum` drift — an added, edited, or removed pnpmfile
// ---------------------------------------------------------------------------

/// A lockfile written without a pnpmfile, checked by an install that
/// still has none.
#[test]
fn check_settings_passes_when_neither_side_has_a_pnpmfile() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    assert!(check_lockfile_settings(&lockfile, settings_check(&Catalogs::new())).is_ok());
}

#[test]
fn check_settings_passes_when_pnpmfile_checksum_matches() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "pnpmfileChecksum: sha256-abc"
    })
    .expect("parse lockfile with a pnpmfile checksum");
    assert!(
        check_lockfile_settings(
            &lockfile,
            LockfileSettingsCheck {
                pnpmfile_checksum: PnpmfileChecksumCheck::Current(Some("sha256-abc")),
                ..settings_check(&Catalogs::new())
            },
        )
        .is_ok(),
    );
}

/// An edited pnpmfile hashes differently, so the lockfile no longer
/// describes what this install's `readPackage` hooks would produce.
#[test]
fn check_settings_returns_drift_when_pnpmfile_checksum_differs() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "pnpmfileChecksum: sha256-abc"
    })
    .expect("parse lockfile with a pnpmfile checksum");
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck {
            pnpmfile_checksum: PnpmfileChecksumCheck::Current(Some("sha256-def")),
            ..settings_check(&Catalogs::new())
        },
    )
    .expect_err("an edited pnpmfile must surface drift");
    assert_eq!(
        err,
        StalenessReason::PnpmfileChecksumChanged {
            lockfile: Some("sha256-abc".to_string()),
            config: Some("sha256-def".to_string()),
        },
    );
    assert_eq!(err.setting_name(), Some("pnpmfileChecksum"));
}

/// The reproduction from pnpm/pnpm#13385: a hooks-exporting pnpmfile
/// added after the lockfile was written.
#[test]
fn check_settings_returns_drift_when_a_pnpmfile_appeared() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
    })
    .expect("parse minimal lockfile");
    let err = check_lockfile_settings(
        &lockfile,
        LockfileSettingsCheck {
            pnpmfile_checksum: PnpmfileChecksumCheck::Current(Some("sha256-abc")),
            ..settings_check(&Catalogs::new())
        },
    )
    .expect_err("a new pnpmfile must surface drift");
    assert_eq!(
        err,
        StalenessReason::PnpmfileChecksumChanged {
            lockfile: None,
            config: Some("sha256-abc".to_string()),
        },
    );
}

#[test]
fn check_settings_returns_drift_when_the_pnpmfile_was_removed() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "pnpmfileChecksum: sha256-abc"
    })
    .expect("parse lockfile with a pnpmfile checksum");
    let err = check_lockfile_settings(&lockfile, settings_check(&Catalogs::new()))
        .expect_err("a removed pnpmfile must surface drift");
    assert_eq!(
        err,
        StalenessReason::PnpmfileChecksumChanged {
            lockfile: Some("sha256-abc".to_string()),
            config: None,
        },
    );
}

/// A caller that can't produce the checksum leaves the field alone
/// rather than reading its absence as "no pnpmfile", which would fail
/// every lockfile that legitimately records one.
#[test]
fn check_settings_skips_the_pnpmfile_checksum_on_request() {
    let lockfile: Lockfile = serde_saphyr::from_str(text_block! {
        "lockfileVersion: '9.0'"
        "pnpmfileChecksum: sha256-abc"
    })
    .expect("parse lockfile with a pnpmfile checksum");
    assert!(
        check_lockfile_settings(
            &lockfile,
            LockfileSettingsCheck {
                pnpmfile_checksum: PnpmfileChecksumCheck::Skip,
                ..settings_check(&Catalogs::new())
            },
        )
        .is_ok(),
    );
}
