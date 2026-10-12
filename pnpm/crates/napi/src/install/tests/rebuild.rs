use super::{EngineMode, install_options_for, rebuild_options, run_install_inner};
use pnpm_modules_yaml::{Host, NodeLinker, read_modules_manifest, write_modules_manifest};
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

/// Records a `.modules.yaml` the way an embedder that links `node_modules`
/// itself does: without the layout settings pnpm compares against.
fn drop_recorded_layout_settings(modules_dir: &Path) {
    let mut modules = read_modules_manifest::<Host>(modules_dir)
        .expect("read .modules.yaml")
        .expect(".modules.yaml exists");
    modules.hoist_pattern = None;
    modules.node_linker = None;
    write_modules_manifest::<Host>(modules_dir, modules).expect("write .modules.yaml");
}

#[test]
fn rebuild_keeps_a_tree_whose_recorded_settings_drifted() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut options = install_options_for(
        temp_dir.path(),
        "project",
        serde_json::json!({ "dependencies": { PACKAGE: "1.0.0" } }),
    );
    options.dangerously_allow_all_builds = Some(true);
    options.enable_global_virtual_store = Some(false);
    run_install_inner(&options, None, EngineMode::Install(None)).expect("install");
    let project_dir = Path::new(&options.dir);
    let modules_dir = project_dir.join("node_modules");
    let pkg_dir = pkg_dir(project_dir);

    drop_recorded_layout_settings(&modules_dir);
    let kept = modules_dir.join(".pnpm/kept-by-rebuild");
    std::fs::write(&kept, "").expect("write marker");
    mark_rebuilds(&pkg_dir);
    run_install_inner(&options, None, EngineMode::Rebuild(rebuild_options(None, false)))
        .expect("rebuild");

    assert!(kept.exists(), "a rebuild must not recreate node_modules");
    assert!(pkg_dir.join("rebuilt").exists(), "a rebuild runs the scripts");
    let modules = read_modules_manifest::<Host>(&modules_dir)
        .expect("read .modules.yaml")
        .expect(".modules.yaml exists");
    assert_eq!(modules.node_linker, Some(NodeLinker::Isolated));
}

/// Leaves the records of a hoisted install behind, in `.modules.yaml` and in
/// the workspace state the repeat-install check reads.
fn record_hoisted_install(modules_dir: &Path) {
    let mut modules = read_modules_manifest::<Host>(modules_dir)
        .expect("read .modules.yaml")
        .expect(".modules.yaml exists");
    modules.node_linker = Some(NodeLinker::Hoisted);
    write_modules_manifest::<Host>(modules_dir, modules).expect("write .modules.yaml");

    let state_path = modules_dir.join(".pnpm-workspace-state-v1.json");
    let mut state: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&state_path).expect("read workspace state"))
            .expect("parse workspace state");
    state["settings"]["nodeLinker"] = serde_json::Value::from("hoisted");
    std::fs::write(&state_path, state.to_string()).expect("write workspace state");
}

#[test]
fn rebuild_keeps_the_recorded_layout_for_the_next_install_to_check() {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut options = install_options_for(
        temp_dir.path(),
        "project",
        serde_json::json!({ "dependencies": { PACKAGE: "1.0.0" } }),
    );
    options.dangerously_allow_all_builds = Some(true);
    options.enable_global_virtual_store = Some(false);
    run_install_inner(&options, None, EngineMode::Install(None)).expect("install");
    let project_dir = Path::new(&options.dir);
    let modules_dir = project_dir.join("node_modules");
    let kept = modules_dir.join(".pnpm/kept-by-rebuild");
    std::fs::write(&kept, "").expect("write marker");

    record_hoisted_install(&modules_dir);
    run_install_inner(&options, None, EngineMode::Rebuild(rebuild_options(None, false)))
        .expect("rebuild");
    assert!(kept.exists(), "a rebuild must not recreate node_modules");
    let modules = read_modules_manifest::<Host>(&modules_dir)
        .expect("read .modules.yaml")
        .expect(".modules.yaml exists");
    assert_eq!(
        modules.node_linker,
        Some(NodeLinker::Hoisted),
        "a rebuild records the layout it found",
    );

    run_install_inner(&options, None, EngineMode::Install(None)).expect("install");
    assert!(!kept.exists(), "the next install relinks a tree whose linker changed");
    let modules = read_modules_manifest::<Host>(&modules_dir)
        .expect("read .modules.yaml")
        .expect(".modules.yaml exists");
    assert_eq!(modules.node_linker, Some(NodeLinker::Isolated));
}
