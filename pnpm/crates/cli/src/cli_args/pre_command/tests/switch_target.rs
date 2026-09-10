use super::{
    Config, LOCKED_9_1_1, LOCKED_9_3_0_WITH_FILE_DEP_PATH, LOCKED_9_3_0_WITH_PEER_SUFFIX,
    LOCKED_9_3_0_WITH_TARBALL_RESOLUTION, LOCKED_99_0_0, PmOnFail, SwitchSource, TempDir,
    pin_roots, switch_target, write_dev_engine_manifest, write_lockfile, write_manifest,
};

#[test]
fn switch_target_uses_global_env_when_lockfile_is_disabled() {
    let root = TempDir::new().expect("tmp dir");
    let global_pkg_dir = root.path().join("pnpm-home").join("global");
    write_dev_engine_manifest(root.path(), "99.0.0");

    let target = switch_target(
        &Config {
            lockfile: false,
            global_pkg_dir: Some(global_pkg_dir.clone()),
            ..Config::default()
        },
        &pin_roots(root.path()),
        false,
    )
    .expect("target")
    .expect("download pin should still produce a switch target");

    let SwitchSource::Resolve {
        env_root,
        frozen_lockfile: false,
        force_resync: false,
        locked_version: None,
    } = target.source
    else {
        panic!("expected resolve target into the global env, got {:?}", target.source);
    };
    assert_eq!(env_root, global_pkg_dir);
}

#[test]
fn switch_target_prefers_locked_dev_engine_version() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(
        root.path(),
        r#"{"devEngines":{"packageManager":{"name":"pnpm","version":"^11.0.0-rc.5","onFail":"download"}}}"#,
    );
    write_lockfile(
        root.path(),
        r"---
lockfileVersion: '9.0'

importers:

  .:
    configDependencies: {}
    packageManagerDependencies:
      '@pnpm/exe':
        specifier: 11.1.2
        version: 11.1.2
      pnpm:
        specifier: 11.1.2
        version: 11.1.2

packages:

  '@pnpm/exe@11.1.2':
    resolution: {integrity: sha512-di6YvqPO/2jvih6kCJ8r0ySzQNjQWrBXPEfqEHtrmwOamuNALnfASwhFBwEtMjWmaA8QG7TqAg2qEvAe+8cBkQ==}

  pnpm@11.1.2:
    resolution: {integrity: sha512-QVocwll0cx51RVwUaDcb50xapft2IbUNQFbSIkUWCfEUEvI/1gLmFp8eBgRmZB95hZfhvpYaEGiINqZ7FlaUmQ==}

snapshots:

  '@pnpm/exe@11.1.2': {}

  pnpm@11.1.2: {}
---
",
    );

    let target = switch_target(&Config::default(), &pin_roots(root.path()), false)
        .expect("target")
        .expect("switch");

    assert_eq!(target.spec, "^11.0.0-rc.5");
    let SwitchSource::LockedEnv { version, .. } = target.source else {
        panic!("expected locked env target");
    };
    assert_eq!(version, "11.1.2");
}

#[test]
fn switch_target_accepts_peer_suffixed_package_manager_lockfile() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), "9.3.0");
    write_lockfile(root.path(), LOCKED_9_3_0_WITH_PEER_SUFFIX);

    let target = switch_target(&Config::default(), &pin_roots(root.path()), false)
        .expect("target")
        .expect("switch");

    let SwitchSource::LockedEnv { version, .. } = target.source else {
        panic!("expected locked env target");
    };
    assert_eq!(version, "9.3.0");
}

#[test]
fn switch_target_accepts_v12_lockfile_without_legacy_wrapper_entry() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(root.path(), r#"{"packageManager":"pnpm@99.0.0"}"#);
    write_lockfile(root.path(), LOCKED_99_0_0);

    let target = switch_target(&Config::default(), &pin_roots(root.path()), false)
        .expect("target")
        .expect("switch");

    let SwitchSource::LockedEnv { version, .. } = target.source else {
        panic!("expected locked env target");
    };
    assert_eq!(version, "99.0.0");
}

#[test]
fn switch_target_discards_package_manager_lockfile_resolution_with_non_integrity_fields() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), "9.3.0");
    write_lockfile(root.path(), LOCKED_9_3_0_WITH_TARBALL_RESOLUTION);

    let target = switch_target(&Config::default(), &pin_roots(root.path()), false)
        .expect("target")
        .expect("switch");

    let SwitchSource::Resolve {
        env_root,
        frozen_lockfile: false,
        force_resync: true,
        locked_version,
    } = target.source
    else {
        panic!("expected a forced re-resolve, got {:?}", target.source);
    };
    assert_eq!(env_root, root.path());
    assert_eq!(locked_version.as_deref(), Some("9.3.0"));
}

#[test]
fn switch_target_discards_package_manager_lockfile_dependency_with_non_registry_dep_path() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), "9.3.0");
    write_lockfile(root.path(), LOCKED_9_3_0_WITH_FILE_DEP_PATH);

    let target = switch_target(&Config::default(), &pin_roots(root.path()), false)
        .expect("target")
        .expect("switch");

    let SwitchSource::Resolve {
        env_root,
        frozen_lockfile: false,
        force_resync: true,
        locked_version,
    } = target.source
    else {
        panic!("expected a forced re-resolve, got {:?}", target.source);
    };
    assert_eq!(env_root, root.path());
    assert_eq!(locked_version.as_deref(), Some("9.3.0"));
}

#[test]
fn switch_target_reresolves_when_locked_version_no_longer_satisfies_range() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), ">=9.1.2 <9.1.4");
    write_lockfile(root.path(), LOCKED_9_1_1);

    let target = switch_target(&Config::default(), &pin_roots(root.path()), false)
        .expect("target")
        .expect("switch");

    assert_eq!(target.spec, ">=9.1.2 <9.1.4");
    let SwitchSource::Resolve {
        env_root,
        frozen_lockfile: false,
        force_resync: false,
        locked_version: None,
    } = target.source
    else {
        panic!("expected resolve target");
    };
    assert_eq!(env_root, root.path());
}

#[test]
fn switch_target_uses_global_env_for_legacy_package_manager_field() {
    let root = TempDir::new().expect("tmp dir");
    let global_pkg_dir = root.path().join("pnpm-home").join("global");
    write_manifest(root.path(), r#"{"packageManager":"pnpm@9.3.0"}"#);

    let target = switch_target(
        &Config { global_pkg_dir: Some(global_pkg_dir.clone()), ..Config::default() },
        &pin_roots(root.path()),
        false,
    )
    .expect("target")
    .expect("switch");

    let SwitchSource::Resolve {
        env_root,
        frozen_lockfile: false,
        force_resync: false,
        locked_version: None,
    } = target.source
    else {
        panic!("expected resolve target");
    };
    assert_eq!(target.spec, "9.3.0");
    assert_eq!(env_root, global_pkg_dir);
}

#[test]
fn switch_target_respects_pm_on_fail_ignore() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(root.path(), r#"{"packageManager":"pnpm@9.3.0"}"#);

    let target = switch_target(
        &Config {
            pm_on_fail: Some(PmOnFail::Ignore),
            global_pkg_dir: Some(root.path().join("pnpm-home").join("global")),
            ..Config::default()
        },
        &pin_roots(root.path()),
        false,
    )
    .expect("target");

    assert!(target.is_none(), "unexpected switch target: {target:?}");
}

#[test]
fn switch_target_refuses_to_record_a_persisting_pin_under_frozen_lockfile() {
    let root = TempDir::new().expect("tmp dir");
    write_dev_engine_manifest(root.path(), ">=9.1.2 <9.1.4");
    write_lockfile(root.path(), LOCKED_9_1_1);

    let target = switch_target(&Config::default(), &pin_roots(root.path()), true)
        .expect("target")
        .expect("switch");

    let SwitchSource::Resolve {
        env_root,
        frozen_lockfile: true,
        force_resync: false,
        locked_version: None,
    } = target.source
    else {
        panic!("expected a frozen resolve target, got {:?}", target.source);
    };
    assert_eq!(env_root, root.path());
}

#[test]
fn switch_target_leaves_the_global_env_writable_under_frozen_lockfile() {
    let root = TempDir::new().expect("tmp dir");
    let global_pkg_dir = root.path().join("pnpm-home").join("global");
    write_manifest(root.path(), r#"{"packageManager":"pnpm@9.3.0"}"#);

    let target = switch_target(
        &Config { global_pkg_dir: Some(global_pkg_dir.clone()), ..Config::default() },
        &pin_roots(root.path()),
        true,
    )
    .expect("target")
    .expect("switch");

    let SwitchSource::Resolve {
        env_root,
        frozen_lockfile: false,
        force_resync: false,
        locked_version: None,
    } = target.source
    else {
        panic!("expected an unfrozen resolve target, got {:?}", target.source);
    };
    assert_eq!(env_root, global_pkg_dir);
}

#[test]
fn switch_target_does_not_switch_dev_engine_without_download() {
    let root = TempDir::new().expect("tmp dir");
    write_manifest(
        root.path(),
        r#"{"devEngines":{"packageManager":{"name":"pnpm","version":"9.3.0","onFail":"error"}}}"#,
    );

    let target = switch_target(&Config::default(), &pin_roots(root.path()), false).expect("target");

    assert!(target.is_none(), "unexpected switch target: {target:?}");
}
