//! Unit tests for the pure summary rendering and the alias-script writer.
//! The full `setup` flow installs the CLI globally and edits the user's
//! shell config, so it is exercised on a real host rather than here.

use super::{
    ConfigFileChangeType, ConfigReport, LEGACY_HOME_DIR_SHIM_NAMES, PathExtenderReport,
    create_alias_scripts, remove_legacy_homedir_shims, render_setup_output,
    write_github_actions_environment_files,
};
use pretty_assertions::assert_eq;
use std::path::PathBuf;

fn report(change_type: ConfigFileChangeType, old: &str, new: &str) -> PathExtenderReport {
    PathExtenderReport {
        config_file: Some(ConfigReport { path: PathBuf::from("/home/user/.bashrc"), change_type }),
        old_settings: old.to_string(),
        new_settings: new.to_string(),
    }
}

#[test]
fn no_changes_when_settings_are_unchanged() {
    let report = report(ConfigFileChangeType::Skipped, "same", "same");
    assert_eq!(
        render_setup_output(&report),
        "No changes to the environment were made. Everything is already up to date.",
    );
}

#[test]
fn created_config_reports_the_source_hint() {
    let report = report(ConfigFileChangeType::Created, "", "export PNPM_HOME=...");
    assert_eq!(
        render_setup_output(&report),
        "Created /home/user/.bashrc\n\nNext configuration changes were made:\nexport PNPM_HOME=...\n\nTo start using pnpm, run:\nsource /home/user/.bashrc\n",
    );
}

#[test]
fn windows_report_omits_the_source_hint() {
    let report = PathExtenderReport {
        config_file: None,
        old_settings: String::new(),
        new_settings: r"PNPM_HOME=C:\pnpm".to_string(),
    };
    assert_eq!(
        render_setup_output(&report),
        "Next configuration changes were made:\nPNPM_HOME=C:\\pnpm\n\nSetup complete. Open a new terminal to start using pnpm.",
    );
}

#[test]
fn alias_scripts_are_written_and_executable() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let bin_dir = dir.path().join("bin");
    create_alias_scripts(&bin_dir).expect("write alias scripts");

    let pn = bin_dir.join("pn");
    assert_eq!(std::fs::read_to_string(&pn).expect("read pn"), "#!/bin/sh\nexec pnpm \"$@\"\n");
    assert_eq!(
        std::fs::read_to_string(bin_dir.join("pnpx")).expect("read pnpx"),
        "#!/bin/sh\nexec pnpm dlx \"$@\"\n",
    );
    assert_eq!(
        std::fs::read_to_string(bin_dir.join("pnx")).expect("read pnx"),
        "#!/bin/sh\nexec pnpm dlx \"$@\"\n",
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&pn).expect("stat pn").permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }
}

#[test]
fn remove_legacy_homedir_shims_unlinks_all_v10_names() {
    // pnpm/pnpm#12496: setup must clean up the v10-layout shims at the top
    // of pnpm_home_dir, otherwise self-update keeps warning about a v10
    // layout forever.
    let dir = tempfile::tempdir().expect("create temp dir");
    for name in LEGACY_HOME_DIR_SHIM_NAMES {
        std::fs::write(dir.path().join(name), "stale shim\n").expect("write stale shim");
    }

    remove_legacy_homedir_shims(dir.path());

    for name in LEGACY_HOME_DIR_SHIM_NAMES {
        assert!(!dir.path().join(name).exists(), "{name} should have been removed");
    }
}

#[test]
fn remove_legacy_homedir_shims_tolerates_missing_files() {
    // On a fresh v11 install there is nothing to clean up; the helper must
    // not treat absent files as an error.
    let dir = tempfile::tempdir().expect("create temp dir");
    remove_legacy_homedir_shims(dir.path());
}

#[test]
fn github_actions_environment_files_receive_home_and_bin() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let pnpm_home_dir = dir.path().join("pnpm-home");
    let bin_dir = pnpm_home_dir.join("bin");
    let github_env = dir.path().join("github-env");
    let github_path = dir.path().join("github-path");

    write_github_actions_environment_files(&pnpm_home_dir, &bin_dir, &github_env, &github_path)
        .expect("write GitHub Actions environment files");

    assert_eq!(
        std::fs::read_to_string(github_env).expect("read github env"),
        format!("PNPM_HOME={}\n", pnpm_home_dir.display()),
    );
    assert_eq!(
        std::fs::read_to_string(github_path).expect("read github path"),
        format!("{}\n", bin_dir.display()),
    );
}
