//! Unit tests for the pure summary rendering and the alias-script writer.
//! The full `setup` flow installs the CLI globally and edits the user's
//! shell config, so it is exercised on a real host rather than here.

use super::{
    ConfigFileChangeType, ConfigReport, LEGACY_HOME_DIR_SHIM_NAMES, PNPM_VERSION,
    PathExtenderReport, create_alias_scripts, remove_legacy_homedir_shims, render_setup_output,
    standalone_manifest,
};
use pretty_assertions::assert_eq;
use std::path::{Path, PathBuf};

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
fn standalone_manifest_declares_package_files() {
    assert_eq!(
        standalone_manifest("pnpm.exe"),
        serde_json::json!({
            "name": "@pnpm/exe",
            "version": PNPM_VERSION,
            "type": "module",
            "bin": { "pnpm": "pnpm.exe", "pn": "pnpm.exe" },
            "files": ["pnpm.exe", "dist/"],
        }),
    );
}

#[test]
fn alias_scripts_are_written_and_executable() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let bin_dir = dir.path().join("bin");
    create_alias_scripts(&bin_dir).expect("write alias scripts");

    // The pnpm each alias hands over to is the one beside it, not whatever `PATH`
    // names first; `alias_scripts_run_the_pnpm_beside_them` runs them.
    for (name, subcommand) in [("pn", ""), ("pnpx", " dlx"), ("pnx", " dlx")] {
        let script = std::fs::read_to_string(bin_dir.join(name)).expect("read alias script");
        assert!(script.starts_with("#!/bin/sh\n"), "{name} = {script}");
        assert!(
            script.ends_with(&format!("exec \"${{self%/*}}/pnpm\"{subcommand} \"$@\"\n")),
            "{name} = {script}",
        );
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(bin_dir.join("pn")).expect("stat pn").permissions().mode();
        assert_eq!(mode & 0o777, 0o755);
    }
}

/// The aliases are `sh` scripts, so this runs where `sh` does. On Windows the
/// `.cmd` and `.ps1` wrappers take over, and they have only `PATH` to go on.
#[cfg(unix)]
#[test]
fn alias_scripts_run_the_pnpm_beside_them() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("create temp dir");
    // The space is deliberate: the walk resolves directories with `${self%/*}`
    // and matches with `case`, neither of which field-splits.
    let bin_dir = dir.path().join("bin dir");
    create_alias_scripts(&bin_dir).expect("write alias scripts");

    let write_stub = |path: &Path, label: &str| {
        std::fs::write(path, format!("#!/bin/sh\necho \"{label}: $*\"\n")).expect("write stub");
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
            .expect("make stub executable");
    };
    write_stub(&bin_dir.join("pnpm"), "sibling");
    // Earlier on `PATH`, so it wins any lookup by name.
    let decoy_dir = dir.path().join("decoy");
    std::fs::create_dir_all(&decoy_dir).expect("create decoy dir");
    write_stub(&decoy_dir.join("pnpm"), "decoy");

    for (name, subcommand) in [("pn", ""), ("pnpx", "dlx "), ("pnx", "dlx ")] {
        let output = std::process::Command::new(bin_dir.join(name))
            .args(["add", "foo"])
            .env("PATH", format!("{}:/usr/bin:/bin", decoy_dir.display()))
            .output()
            .expect("run the alias script");

        let stdout = String::from_utf8(output.stdout).expect("alias stdout is UTF-8");
        assert_eq!(stdout, format!("sibling: {subcommand}add foo\n"), "{name} ran the wrong pnpm");
    }
}

/// The Windows counterparts of [`alias_scripts_run_the_pnpm_beside_them`]. The
/// stand-in siblings are named for the `pnpm.cmd` / `pnpm.ps1` shims that
/// `pnpm add -g` links next to the aliases, which is what fixes the shape these
/// wrappers have to reach.
#[cfg(windows)]
mod windows_alias_scripts {
    use super::{Path, create_alias_scripts};

    /// `cmd.exe` needs `System32` for its own startup, and `powershell.exe` lives
    /// a few levels deeper. Both are on the `PATH` handed to the child, since that
    /// is what a command name may be resolved against; the decoy stays first
    /// either way, which is what these tests turn on.
    const SYSTEM32: &str = r"C:\Windows\System32";
    const POWERSHELL_DIR: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0";
    /// What the stand-in shims exit with, so the wrappers are shown to hand the
    /// shim's status back rather than reporting their own success.
    const SHIM_EXIT_CODE: i32 = 3;

    /// A bin directory holding the aliases and a stand-in `pnpm.cmd`, which is the
    /// only sibling shim guaranteed to be there: the bin linker omits `pnpm.ps1`
    /// for a package named `pnpm`, so neither wrapper may rely on it. No
    /// `pnpm.ps1` is planted, so a wrapper that reached for one would fail here.
    fn bin_dir_with_cmd_sibling() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("create temp dir");
        let bin_dir = dir.path().join("bin");
        create_alias_scripts(&bin_dir).expect("write alias scripts");
        write_stub(&bin_dir.join("pnpm.cmd"), "sibling");
        // Earlier on `PATH`, so it wins any lookup by name.
        let decoy_dir = dir.path().join("decoy");
        std::fs::create_dir_all(&decoy_dir).expect("create decoy dir");
        write_stub(&decoy_dir.join("pnpm.cmd"), "decoy");
        dir
    }

    fn write_stub(path: &Path, label: &str) {
        let body = format!("@echo off\r\necho {label}: %*\r\nexit /b {SHIM_EXIT_CODE}\r\n");
        std::fs::write(path, body).expect("write stub");
    }

    #[test]
    fn cmd_wrappers_call_the_shim_beside_them() {
        let dir = bin_dir_with_cmd_sibling();
        let bin_dir = dir.path().join("bin");
        let decoy_dir = dir.path().join("decoy");

        for (name, subcommand) in [("pn", ""), ("pnpx", "dlx "), ("pnx", "dlx ")] {
            let output = std::process::Command::new("cmd")
                .arg("/c")
                .arg(bin_dir.join(format!("{name}.cmd")))
                .args(["add", "foo"])
                .env("PATH", format!("{};{SYSTEM32};{POWERSHELL_DIR}", decoy_dir.display()))
                .output()
                .expect("run the alias wrapper");

            let stdout = String::from_utf8(output.stdout).expect("wrapper stdout is UTF-8");
            assert_eq!(
                stdout.trim_end(),
                format!("sibling: {subcommand}add foo"),
                "{name}.cmd ran the wrong pnpm",
            );
            assert_eq!(
                output.status.code(),
                Some(SHIM_EXIT_CODE),
                "{name}.cmd dropped the shim's exit status",
            );
        }
    }

    #[test]
    fn ps1_wrappers_call_the_shim_beside_them() {
        let dir = bin_dir_with_cmd_sibling();
        let bin_dir = dir.path().join("bin");
        let decoy_dir = dir.path().join("decoy");

        for (name, subcommand) in [("pn", ""), ("pnpx", "dlx "), ("pnx", "dlx ")] {
            // `-ExecutionPolicy Bypass` because a runner's default policy blocks
            // running a script from disk, which is not what this is testing.
            let output = std::process::Command::new("powershell")
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(bin_dir.join(format!("{name}.ps1")))
                .args(["add", "foo"])
                .env("PATH", format!("{};{SYSTEM32};{POWERSHELL_DIR}", decoy_dir.display()))
                .output()
                .expect("run the alias wrapper");

            let stdout = String::from_utf8(output.stdout).expect("wrapper stdout is UTF-8");
            assert_eq!(
                stdout.trim_end(),
                format!("sibling: {subcommand}add foo"),
                "{name}.ps1 ran the wrong pnpm",
            );
            assert_eq!(
                output.status.code(),
                Some(SHIM_EXIT_CODE),
                "{name}.ps1 dropped the shim's exit status",
            );
        }
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
