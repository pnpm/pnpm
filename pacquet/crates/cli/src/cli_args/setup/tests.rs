//! Unit tests for the pure summary rendering and the alias-script writer.
//! The full `setup` flow installs the CLI globally and edits the user's
//! shell config, so it is exercised on a real host rather than here.

use super::{
    ConfigFileChangeType, ConfigReport, PathExtenderReport, create_alias_scripts,
    render_setup_output,
};
use pretty_assertions::assert_eq;
use std::path::PathBuf;

fn report(
    change_type: ConfigFileChangeType,
    old: impl Into<String>,
    new: impl Into<String>,
) -> PathExtenderReport {
    PathExtenderReport {
        config_file: Some(ConfigReport { path: PathBuf::from("/home/user/.bashrc"), change_type }),
        old_settings: old.into(),
        new_settings: new.into(),
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
