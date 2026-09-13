use super::{
    NetworkConfigInput, STORE_VERSION, build_overlay, install_options, network_config,
    resolve_config,
};

#[test]
fn top_level_fetch_warning_options_override_network_config() {
    let mut options = install_options();
    options.fetch_warn_timeout_ms = Some(1_234);
    options.fetch_min_speed_ki_bps = Some(12);
    options.network_config = Some(NetworkConfigInput {
        fetch_warn_timeout_ms: Some(5_678),
        fetch_min_speed_ki_bps: Some(56),
        ..network_config()
    });

    let overlay = build_overlay(&options, false).expect("overlay");
    assert_eq!(overlay.fetch_warn_timeout_ms, Some(1_234));
    assert_eq!(overlay.fetch_min_speed_ki_bps, Some(12));
}

#[test]
fn resolved_config_applies_trust_lockfile() {
    let dir = tempfile::tempdir().expect("tempdir");

    for (trust_lockfile, expected) in [(Some(false), false), (Some(true), true), (None, false)] {
        let mut options = install_options();
        options.trust_lockfile = trust_lockfile;
        let overlay = build_overlay(&options, false).expect("overlay");
        assert_eq!(resolve_config(dir.path(), &overlay).expect("config").trust_lockfile, expected);
    }
}

#[test]
fn resolved_config_applies_allow_unused_patches() {
    let dir = tempfile::tempdir().expect("tempdir");

    for (allow_unused_patches, expected) in
        [(Some(false), false), (Some(true), true), (None, false)]
    {
        let mut options = install_options();
        options.allow_unused_patches = allow_unused_patches;
        let overlay = build_overlay(&options, false).expect("overlay");
        assert_eq!(
            resolve_config(dir.path(), &overlay).expect("config").allow_unused_patches,
            expected,
        );
    }
}

/// `ignorePackageManifest` carries pnpm's `pnpm fetch` shape into the
/// overlay: post-import linking off, and the modules dir forced back on so
/// an ambient `enableModulesDir: false` cannot leave the virtual store with
/// nowhere to go.
#[test]
fn ignore_package_manifest_pins_the_fetch_shaped_config() {
    let mut options = install_options();
    options.ignore_package_manifest = Some(true);
    options.enable_modules_dir = Some(false);

    let overlay = build_overlay(&options, true).expect("overlay");

    assert_eq!(overlay.virtual_store_only, Some(true));
    assert_eq!(overlay.enable_modules_dir, Some(true));
}

/// A `storeDir` a config source set explicitly outranks `pnpmHomeDir`,
/// which only supplies the *default* store location.
#[test]
fn a_configured_store_dir_outranks_pnpm_home_dir() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home_dir = temp_dir.path().join("pnpm-home");
    let configured_store = temp_dir.path().join("configured-store");
    let project_dir = temp_dir.path().join("project");
    std::fs::create_dir_all(&home_dir).expect("create home dir");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    std::fs::write(
        project_dir.join("pnpm-workspace.yaml"),
        format!("storeDir: {}\n", configured_store.display()),
    )
    .expect("write workspace yaml");

    let mut options = install_options();
    options.pnpm_home_dir = Some(home_dir.to_string_lossy().into_owned());
    let overlay = build_overlay(&options, false).expect("overlay");
    let config = resolve_config(&project_dir, &overlay).expect("config");

    assert_eq!(config.store_dir.root(), configured_store.join(STORE_VERSION));
}
