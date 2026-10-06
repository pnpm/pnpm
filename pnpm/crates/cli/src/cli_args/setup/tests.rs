//! Unit tests for the pure summary rendering and the alias-script writer.
//! The full `setup` flow installs the CLI globally and edits the user's
//! shell config, so it is exercised on a real host rather than here.

use super::{
    ConfigFileChangeType, ConfigReport, LEGACY_HOME_DIR_SHIM_NAMES, PNPM_VERSION,
    PathExtenderReport, create_alias_scripts, remove_legacy_homedir_shims, render_setup_output,
    standalone_manifest, write_windows_alias_wrapper,
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
        "Configuration already up to date in /home/user/.bashrc\n\nNo changes to the environment were made. Everything is already up to date.",
    );
}

#[test]
fn no_changes_without_config_file() {
    let report = PathExtenderReport {
        config_file: None,
        old_settings: "same".to_string(),
        new_settings: "same".to_string(),
    };
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
        "Created /home/user/.bashrc\n\nThe following configuration changes were made:\nexport PNPM_HOME=...\n\nTo start using pnpm, run:\nsource /home/user/.bashrc\n",
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
        "The following configuration changes were made:\nPNPM_HOME=C:\\pnpm\n\nSetup complete. Open a new terminal to start using pnpm.",
    );
}

#[test]
fn standalone_manifest_declares_package_files() {
    assert_eq!(
        standalone_manifest("pnpm.exe"),
        serde_json::json!({
            "name": "pnpm",
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
        // MSYS and Cygwin can hand the script a native Windows path, which has no
        // slash for `${self%/*}` to strip. There is no POSIX shell on Windows to
        // run this against, so pin the text, as the shim header's tests do. The
        // gate is what keeps a Unix path holding a backslash off this branch;
        // `alias_scripts_run_the_pnpm_beside_them` runs one.
        assert!(script.contains(r"  [A-Za-z]:\\*|\\\\*)"), "{name} = {script}");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(bin_dir.join("pn"))
            .expect("stat pn")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }
}

/// pnpm/pnpm#15494: an alias name can be a hardlink of the running pnpm
/// executable, which Linux refuses to open for writing. The test binary is
/// the running executable here, linked from beside it so the link stays on
/// one filesystem.
#[cfg(target_os = "linux")]
#[test]
fn alias_scripts_replace_a_hardlink_of_the_running_executable() {
    let exe = std::env::current_exe().expect("locate the test binary");
    let dir = tempfile::tempdir_in(exe.parent().expect("test binary has a parent"))
        .expect("create temp dir");
    let bin_dir = dir.path().join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create bin dir");
    for name in ["pn", "pnpx", "pnx"] {
        std::fs::hard_link(&exe, bin_dir.join(name)).expect("hardlink the test binary");
    }

    create_alias_scripts(&bin_dir).expect("write alias scripts");

    for name in ["pn", "pnpx", "pnx"] {
        let script = std::fs::read_to_string(bin_dir.join(name)).expect("read alias script");
        assert!(script.starts_with("#!/bin/sh\n"), "{name} = {script}");
    }
    assert_eq!(
        std::fs::read_dir(&bin_dir).expect("read bin dir").count(),
        3,
        "no temporary file is left behind",
    );
}

/// The aliases are `sh` scripts, so this runs where `sh` does. On Windows the
/// `.cmd` wrappers take over; see `windows_alias_scripts`.
#[cfg(unix)]
#[test]
fn alias_scripts_run_the_pnpm_beside_them() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("create temp dir");
    // Both oddities in the name are deliberate. The space, because the walk
    // resolves directories with `${self%/*}` and matches with `case`, neither of
    // which field-splits. The backslash, because it is an ordinary character
    // here and the walk must leave it alone: only a Windows path is rewritten.
    let bin_dir = dir.path().join(r"bin\dir with space");
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

/// The default path `command -p` searches can lack `readlink`, as inside a Nix
/// build sandbox. No test host is set up that way, so each alias's `command -p`
/// is rewritten to a `command` that searches a directory that does not exist.
#[cfg(unix)]
#[test]
fn alias_scripts_follow_a_symlink_when_the_default_path_lacks_readlink() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().expect("create temp dir");
    let bin_dir = dir.path().join("bin");
    create_alias_scripts(&bin_dir).expect("write alias scripts");
    let sibling = bin_dir.join("pnpm");
    std::fs::write(&sibling, "#!/bin/sh\necho \"sibling: $*\"\n").expect("write stub");
    std::fs::set_permissions(&sibling, std::fs::Permissions::from_mode(0o755))
        .expect("make stub executable");
    let link_dir = dir.path().join("links");
    std::fs::create_dir_all(&link_dir).expect("create link dir");

    for (name, subcommand) in [("pn", ""), ("pnpx", "dlx "), ("pnx", "dlx ")] {
        let alias = bin_dir.join(name);
        let script = std::fs::read_to_string(&alias).expect("read alias script");
        std::fs::write(&alias, script.replace("command -p ", "PATH=/nonexistent command "))
            .expect("rewrite alias script");
        std::os::unix::fs::symlink(&alias, link_dir.join(name)).expect("link the alias");

        let output = std::process::Command::new(link_dir.join(name))
            .args(["add", "foo"])
            .env("PATH", "/usr/bin:/bin")
            .output()
            .expect("run the alias script");

        let stdout = String::from_utf8(output.stdout).expect("alias stdout is UTF-8");
        assert_eq!(stdout, format!("sibling: {subcommand}add foo\n"), "{name} ran the wrong pnpm");
    }
}

/// The Windows counterpart of [`alias_scripts_run_the_pnpm_beside_them`]. The
/// stand-in sibling is named for the `pnpm.cmd` shim that `pnpm add -g` links
/// next to the aliases, which is what fixes the shape these wrappers have to
/// reach.
#[cfg(windows)]
mod windows_alias_scripts {
    use super::{Path, create_alias_scripts};

    /// `cmd.exe` needs `System32` for its own startup. It follows the decoy on the
    /// `PATH` handed to the child, and the decoy coming first is what this test
    /// turns on.
    const SYSTEM32: &str = r"C:\Windows\System32";
    /// What the stand-in shim exits with, so the wrappers are shown to hand the
    /// shim's status back rather than reporting their own success.
    const SHIM_EXIT_CODE: i32 = 3;

    /// A bin directory holding the aliases and a stand-in `pnpm.cmd`, the sibling
    /// shim the bin linker writes for a package named `pnpm`.
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
                .env("PATH", format!("{};{SYSTEM32}", decoy_dir.display()))
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

    /// The aliases in front of the `pnpm.cmd` the bin linker writes for the pnpm
    /// CLI, which ends its batch context before the CLI starts. The CLI here is a
    /// copy of `node.exe` running a script that prints its arguments.
    #[test]
    fn cmd_wrappers_pass_arguments_and_exit_code_through_a_batchless_shim() {
        use pnpm_cmd_shim::{CmdShimBatch, ScriptRuntime, generate_cmd_shim};
        use std::{fs, process::Command};

        let dir = tempfile::tempdir().expect("create temp dir");
        let bin_dir = dir.path().join("bin");
        create_alias_scripts(&bin_dir).expect("write alias scripts");
        let cli_dir = dir.path().join("global").join("pnpm");
        fs::create_dir_all(&cli_dir).expect("create the CLI dir");
        // The interpreter itself: a launcher found on `PATH` may dispatch on its
        // own file name, which the copy below changes.
        let exec_path = Command::new("node")
            .args(["-p", "process.execPath"])
            .output()
            .expect("run node");
        let node = String::from_utf8(exec_path.stdout).expect("node output is UTF-8");
        let node = node.trim();
        let cli = cli_dir.join("pnpm.exe");
        fs::copy(node, &cli).expect("copy node.exe");
        let script = cli_dir.join("cli.js");
        fs::write(
            &script,
            format!(
                "console.log(JSON.stringify(process.argv.slice(2)))\nprocess.exit({SHIM_EXIT_CODE})\n",
            ),
        )
        .expect("write the CLI script");
        let shim = bin_dir.join("pnpm.cmd");
        let runtime = ScriptRuntime { prog: None, args: format!(r#" "{}""#, script.display()) };
        fs::write(
            &shim,
            generate_cmd_shim(&cli, &shim, Some(&runtime), &[], CmdShimBatch::EndedBeforeTarget),
        )
        .expect("write pnpm.cmd");

        let args = ["--", "--flag", "a b", "", "50%", "x^y", "p&q", "r|s", "!bang!", "héllo"];
        let quoted = args
            .map(|arg| format!(r#""{arg}""#))
            .join(" ");
        for (name, subcommand) in [("pn", None), ("pnpx", Some("dlx")), ("pnx", Some("dlx"))] {
            let mut command = Command::new("cmd.exe");
            std::os::windows::process::CommandExt::raw_arg(
                &mut command,
                format!(
                    r#"/d /c ""{}" {quoted}""#,
                    bin_dir
                        .join(format!("{name}.cmd"))
                        .display(),
                ),
            );
            let output = command.output().expect("run the alias wrapper");
            eprintln!("{name}: {output:?}");
            let expected: Vec<&str> = subcommand
                .into_iter()
                .chain(args)
                .collect();
            let stdout = String::from_utf8(output.stdout).expect("wrapper stdout is UTF-8");
            let printed: Vec<String> =
                serde_json::from_str(stdout.trim_end()).expect("parse printed arguments");
            assert_eq!(printed, expected, "{name}.cmd changed the arguments");
            assert_eq!(
                output.status.code(),
                Some(SHIM_EXIT_CODE),
                "{name}.cmd dropped the CLI's exit status",
            );
        }
    }
}

/// PowerShell prefers `pn.ps1` over `pn.cmd`, so an unsigned leftover from an
/// earlier setup would keep failing under the default execution policy.
#[test]
fn windows_alias_wrapper_removes_a_stale_ps1() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let stale = dir.path().join("pn.ps1");
    std::fs::write(&stale, "stale wrapper\n").expect("write stale wrapper");

    write_windows_alias_wrapper(dir.path(), "pn", "").expect("write alias wrapper");

    assert!(dir.path().join("pn.cmd").is_file());
    assert!(!stale.exists());
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
