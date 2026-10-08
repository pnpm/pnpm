use super::{EngineMode, install_options_for, rebuild_options, run_install_inner};
use std::path::{Path, PathBuf};

const PACKAGE: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";

/// Replaces the package's build helper with one that leaves a `rebuilt`
/// marker, so whether a rebuild ran the scripts is observable. The file is
/// a hard link into the store, so it is unlinked before it is rewritten.
fn mark_rebuilds(pkg_dir: &Path) {
    let create_js = pkg_dir.join("create.js");
    std::fs::remove_file(&create_js).expect("unlink create.js");
    std::fs::write(&create_js, "require('fs').writeFileSync('rebuilt', '')\n")
        .expect("write create.js");
}

fn pkg_dir(project_dir: &Path) -> PathBuf {
    project_dir
        .join("node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0/node_modules")
        .join(PACKAGE)
}

#[test]
fn rebuild_restores_a_cached_build_with_skip_if_has_side_effects_cache() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut options = install_options_for(
        temp_dir.path(),
        "project",
        serde_json::json!({ "dependencies": { PACKAGE: "1.0.0" } }),
    );
    options.dangerously_allow_all_builds = Some(true);
    options.enable_global_virtual_store = Some(false);
    run_install_inner(&options, None, EngineMode::Install(None)).expect("install");
    let pkg_dir = pkg_dir(Path::new(&options.dir));

    mark_rebuilds(&pkg_dir);
    run_install_inner(&options, None, EngineMode::Rebuild(rebuild_options(None, true)))
        .expect("rebuild with skipIfHasSideEffectsCache");
    assert!(!pkg_dir.join("rebuilt").exists(), "the cached build must not run the scripts");
    assert!(pkg_dir.join("generated-by-postinstall.js").exists());
    let create_js = std::fs::read_to_string(pkg_dir.join("create.js")).expect("read create.js");
    assert!(!create_js.contains("rebuilt"), "the cached build must be restored: {create_js}");

    mark_rebuilds(&pkg_dir);
    run_install_inner(&options, None, EngineMode::Rebuild(rebuild_options(None, false)))
        .expect("rebuild");
    assert!(pkg_dir.join("rebuilt").exists(), "a plain rebuild runs the scripts");
}
