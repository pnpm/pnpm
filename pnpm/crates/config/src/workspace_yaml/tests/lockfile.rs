use super::{Config, Path, ResolutionMode, WorkspaceSettings, assert_eq};

/// `resolutionMode` accepts the three upstream string values; an
/// absent key leaves the [`ResolutionMode::Highest`] default in place.
#[test]
fn resolution_mode_yaml_values_round_trip() {
    for (yaml, expected) in [
        ("resolutionMode: highest\n", ResolutionMode::Highest),
        ("resolutionMode: time-based\n", ResolutionMode::TimeBased),
        ("resolutionMode: lowest-direct\n", ResolutionMode::LowestDirect),
    ] {
        let settings: WorkspaceSettings = serde_saphyr::from_str(yaml).unwrap();
        assert_eq!(settings.resolution_mode, Some(expected));
        let mut config = Config::new();
        settings.apply_to(&mut config, Path::new("/irrelevant"));
        assert_eq!(config.resolution_mode, expected);
    }

    let settings: WorkspaceSettings = serde_saphyr::from_str("").unwrap();
    assert!(settings.resolution_mode.is_none());
    let mut config = Config::new();
    settings.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(
        config.resolution_mode,
        ResolutionMode::Highest,
        "default stays highest when the key is absent",
    );
}

/// `frozenStore` parses from `pnpm-workspace.yaml` as a camelCase
/// boolean and `apply_to` pushes it onto the `Config`. Defaults to
/// `false` when the key is absent, matching pnpm's `frozen-store`
/// default. Drives the read-only-store open path (`immutable=1`) and
/// the disabled `index.db` writer.
#[test]
fn parses_frozen_store_from_yaml_and_applies() {
    let absent: WorkspaceSettings = serde_saphyr::from_str("hoist: true").unwrap();
    assert_eq!(absent.frozen_store, None);
    let mut config = Config::new();
    assert!(!config.frozen_store, "frozen_store must default to false");
    absent.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(!config.frozen_store, "absent frozenStore must leave the default in place");

    let enabled: WorkspaceSettings = serde_saphyr::from_str("frozenStore: true").unwrap();
    assert_eq!(enabled.frozen_store, Some(true));
    let mut config = Config::new();
    enabled.apply_to(&mut config, Path::new("/irrelevant"));
    assert!(config.frozen_store, "frozenStore: true must apply onto the config");
}

/// `frozenLockfile` parses from `pnpm-workspace.yaml` as a camelCase
/// boolean and `apply_to` pushes it onto the `Config` as an explicit
/// `Some`, which the CLI layers `--frozen-lockfile` /
/// `--no-frozen-lockfile` over. It is excluded from the global
/// `config.yaml`, matching pnpm's `excludedPnpmKeys`.
#[test]
fn parses_frozen_lockfile_from_yaml_and_applies() {
    let absent: WorkspaceSettings = serde_saphyr::from_str("hoist: true").unwrap();
    assert_eq!(absent.frozen_lockfile, None);
    let mut config = Config::new();
    assert_eq!(config.frozen_lockfile, None, "frozen_lockfile must default to unset");
    absent.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.frozen_lockfile, None, "absent frozenLockfile must leave the default");

    let enabled: WorkspaceSettings = serde_saphyr::from_str("frozenLockfile: true").unwrap();
    assert_eq!(enabled.frozen_lockfile, Some(true));
    let mut config = Config::new();
    enabled.apply_to(&mut config, Path::new("/irrelevant"));
    assert_eq!(config.frozen_lockfile, Some(true));

    let mut global: WorkspaceSettings = serde_saphyr::from_str("frozenLockfile: true").unwrap();
    global.clear_workspace_only_fields();
    assert_eq!(global.frozen_lockfile, None);
}
