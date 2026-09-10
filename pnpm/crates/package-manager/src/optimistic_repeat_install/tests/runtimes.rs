use super::{super::Decision, check, setup_fresh_install};

/// Settings drift (e.g. `node_linker` changed between installs)
/// invalidates the cached state.
#[test]
fn returns_skipped_when_node_linker_drifts() {
    // Previous install was Hoisted; today's call asks for Isolated.
    let (dir, config, manifest) =
        setup_fresh_install(pnpm_config::NodeLinker::Hoisted, "root", "1.0.0", "");

    let decision = check(
        dir.path(),
        config,
        pnpm_config::NodeLinker::Isolated,
        &[(dir.path().to_path_buf(), &manifest)],
    );
    assert!(matches!(decision, Decision::Skipped { reason } if reason.contains("settings")));
}
