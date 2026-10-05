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
/// installs that cannot or must not resolve: on CI an install whose lockfile
/// is present runs frozen
/// ([pnpm/pnpm#16374](https://github.com/pnpm/pnpm/issues/16374)), and after a
/// frozen install the lockfile stays as it was accepted
/// ([pnpm/pnpm#16583](https://github.com/pnpm/pnpm/issues/16583)).
#[test]
fn a_pending_dedupe_baseline_is_not_drift_when_the_lockfile_stays_frozen() {
    const DRIFT: &str = "The value of the autoDedupe setting has changed";
    let cases = [
        // (ci, recorded frozen install, configured frozenLockfile, lockfile, drift)
        (true, false, None, true, false),
        (false, false, None, true, true),
        (true, false, None, false, true),
        (false, true, None, true, false),
        (false, true, Some(false), true, true),
        (false, true, None, false, true),
    ];
    for (ci, recorded_frozen, frozen_lockfile, lockfile, drift) in cases {
        let (dir, config) = setup_content_check_project();
        let mut config = config.clone();
        config.auto_dedupe = true;
        config.ci = ci;
        config.frozen_lockfile = frozen_lockfile;
        let config = config.leak();
        let mut state = load_workspace_state(dir.path()).unwrap().unwrap();
        state.settings.auto_dedupe = None;
        state.frozen_lockfile = recorded_frozen;
        update_workspace_state(dir.path(), &state).unwrap();
        if !lockfile {
            // An absent lockfile is never frozen: the install this gate
            // spawns re-resolves and records the baseline.
            fs::remove_file(dir.path().join(Lockfile::FILE_NAME)).unwrap();
        }
        let manifest = PackageManifest::from_path(dir.path().join("package.json")).unwrap();
        let projects = [(dir.path().to_path_buf(), &manifest)];
        let status = workspace_deps_status(&dir, config, &projects);
        let expected = if drift {
            RunDepsStatus::Outdated { issue: DRIFT.to_string(), install_args: Vec::new() }
        } else {
            RunDepsStatus::UpToDate
        };
        assert_eq!(
            status, expected,
            "ci={ci} recorded_frozen={recorded_frozen} frozen_lockfile={frozen_lockfile:?} lockfile={lockfile}",
        );
    }
}
