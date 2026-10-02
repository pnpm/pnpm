use super::{
    Decision, FOO_MANIFEST, RunDepsStatus, content_check_decision, setup_content_check_project,
    workspace_deps_status,
};
use pnpm_lockfile::Lockfile;
use pnpm_package_manifest::PackageManifest;
use pnpm_workspace_state::{load_workspace_state, update_workspace_state};
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

/// A pending dedupe baseline must not keep the pre-run gate spawning
/// installs an environment cannot complete: on CI an install whose lockfile is
/// present runs frozen, so it never re-resolves and can never record the
/// baseline ([pnpm/pnpm#16374](https://github.com/pnpm/pnpm/issues/16374)).
#[test]
fn a_pending_dedupe_baseline_is_not_drift_when_the_install_would_run_frozen() {
    const DRIFT: &str = "The value of the autoDedupe setting has changed";
    for (ci, lockfile) in [(true, true), (false, true), (true, false)] {
        let (dir, config) = setup_content_check_project();
        let mut config = config.clone();
        config.auto_dedupe = true;
        config.ci = ci;
        let config = config.leak();
        let mut state = load_workspace_state(dir.path()).unwrap().unwrap();
        state.settings.auto_dedupe = None;
        update_workspace_state(dir.path(), &state).unwrap();
        if !lockfile {
            // An absent lockfile is never frozen: the install this gate
            // spawns re-resolves and records the baseline.
            fs::remove_file(dir.path().join(Lockfile::FILE_NAME)).unwrap();
        }
        let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();
        let projects = [(dir.path().to_path_buf(), &manifest)];
        let status = workspace_deps_status(&dir, config, &projects);
        let expected = if ci && lockfile {
            RunDepsStatus::UpToDate
        } else {
            RunDepsStatus::Outdated { issue: DRIFT.to_string(), install_args: Vec::new() }
        };
        assert_eq!(status, expected);
    }
}
