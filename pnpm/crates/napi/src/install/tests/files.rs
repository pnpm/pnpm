use super::{STORE_VERSION, build_overlay, install_options, resolve_config};

/// `pnpmHomeDir` resolves the default store under that home, and an
/// explicit `storeDir` still wins.
#[test]
fn pnpm_home_dir_places_the_default_store_under_that_home() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let home_dir = temp_dir.path().join("pnpm-home");
    let project_dir = temp_dir.path().join("project");
    std::fs::create_dir_all(&home_dir).expect("create home dir");
    std::fs::create_dir_all(&project_dir).expect("create project dir");

    let mut options = install_options();
    options.pnpm_home_dir = Some(home_dir.to_string_lossy().into_owned());
    let overlay = build_overlay(&options, false).expect("overlay");
    let config = resolve_config(&project_dir, &overlay).expect("config");
    assert_eq!(config.store_dir.root(), home_dir.join("store").join(STORE_VERSION));

    let store_dir = temp_dir.path().join("explicit-store");
    options.store_dir = Some(store_dir.to_string_lossy().into_owned());
    let overlay = build_overlay(&options, false).expect("overlay");
    let config = resolve_config(&project_dir, &overlay).expect("config");
    assert_eq!(config.store_dir.root(), store_dir.join(STORE_VERSION));
}
