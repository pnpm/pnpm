use super::{
    Decision,
    FOO_MANIFEST,
    RunDepsStatus,
    content_check_decision,
    setup_content_check_project,
    workspace_deps_status,
};
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace_state::{
    load_workspace_state,
    update_workspace_state,
};
use std::fs;

#[test]
fn auto_dedupe_baseline_survives_install_and_run_content_checks() {
    for check_before_run in [false, true] {
        let (dir, config) = setup_content_check_project();
        let mut config = config.clone();
        config.auto_dedupe = true;
        let config = config.leak();
        let mut state = load_workspace_state(dir.path()).unwrap().unwrap();
        state.settings.auto_dedupe = Some(true);
        update_workspace_state(dir.path(), &state).unwrap();
        fs::write(dir.path().join("package.json"), FOO_MANIFEST).unwrap();
        let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();
        let projects = [(dir.path().to_path_buf(), &manifest)];
        if check_before_run {
            assert_eq!(workspace_deps_status(&dir, config, &projects), RunDepsStatus::UpToDate);
        } else {
            assert_eq!(content_check_decision(&dir, config, true, &projects), Decision::UpToDate);
        }
        let refreshed = load_workspace_state(dir.path()).unwrap().unwrap();
        assert!(refreshed.last_validated_timestamp > state.last_validated_timestamp);
        assert_eq!(refreshed.settings.auto_dedupe, Some(true));
        assert_eq!(content_check_decision(&dir, config, true, &projects), Decision::UpToDate);
        assert_eq!(content_check_decision(&dir, config, true, &projects), Decision::UpToDate);
    }
}
