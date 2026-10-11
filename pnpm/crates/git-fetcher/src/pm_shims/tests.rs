use super::{write_pm_shims, write_running_pnpm_shim};
use crate::preferred_pm::{PreferredPm, WantedPm};
use std::{fs, path::Path};
use tempfile::tempdir;

fn shim_body(dir: &Path, name: &str) -> String {
    let file_name = if cfg!(windows) { format!("{name}.cmd") } else { name.to_string() };
    fs::read_to_string(dir.join(file_name)).expect("read the generated shim")
}

#[test]
fn a_pinned_package_manager_is_forwarded_with_its_version() {
    let dir = tempdir().unwrap();
    let wanted =
        WantedPm { pm: PreferredPm::Yarn, version_spec: Some("1".to_string()), pinned: true };
    write_pm_shims(dir.path(), &wanted, Path::new("/opt/pnpm")).expect("write the shims");

    let body = shim_body(dir.path(), "yarn");
    assert!(body.contains("dlx"), "{body}");
    assert!(body.contains("yarn@1"), "{body}");
    assert!(body.contains("/opt/pnpm"), "{body}");
}

/// Without a pin the package manager's own name is the whole spec, so
/// `pnpm with` falls through to that channel's current line.
#[test]
fn an_unpinned_package_manager_is_forwarded_by_name() {
    let dir = tempdir().unwrap();
    let wanted = WantedPm { pm: PreferredPm::Bun, version_spec: None, pinned: false };
    write_pm_shims(dir.path(), &wanted, Path::new("/opt/pnpm")).expect("write the shims");

    let body = shim_body(dir.path(), "bun");
    let spec = if cfg!(windows) { r#"--package "bun""# } else { "--package 'bun'" };
    assert!(body.contains(spec), "an unpinned spec carries no version: {body}");
}

/// A build that shells out to `yarnpkg` has to find the same Yarn.
#[test]
fn yarn_is_reachable_under_both_of_its_names() {
    let dir = tempdir().unwrap();
    let wanted = WantedPm { pm: PreferredPm::Yarn, version_spec: None, pinned: false };
    let written = write_pm_shims(dir.path(), &wanted, Path::new("/opt/pnpm")).expect("write");
    assert_eq!(written.len(), 2);
    for name in ["yarn", "yarnpkg"] {
        let body = shim_body(dir.path(), name);
        assert!(body.contains("--package"), "{name}: {body}");
        assert!(body.contains(name), "{name}: {body}");
    }
}

/// Bun ships one executable and answers to `bunx` as `bun x`, so that
/// shim carries the subcommand rather than a bin name Bun does not
/// publish.
#[test]
fn bun_is_reachable_through_bunx_too() {
    let dir = tempdir().unwrap();
    let version_spec = Some("1.3.0".to_string());
    let wanted = WantedPm { pm: PreferredPm::Bun, version_spec, pinned: true };
    let written = write_pm_shims(dir.path(), &wanted, Path::new("/opt/pnpm")).expect("write");

    assert_eq!(written.len(), 2);
    let body = shim_body(dir.path(), "bunx");
    let runs = if cfg!(windows) { r#""bun@1.3.0" "bun" "x""# } else { "'bun@1.3.0' 'bun' 'x'" };
    assert!(body.contains(runs), "{body}");
}

/// `npx` is a command npm publishes beside itself, not another name for
/// it, so the shim has to run npm's `npx` rather than npm.
#[test]
fn npm_is_reachable_through_npx_too() {
    let dir = tempdir().unwrap();
    let version_spec = Some("11".to_string());
    let wanted = WantedPm { pm: PreferredPm::Npm, version_spec, pinned: true };
    let written = write_pm_shims(dir.path(), &wanted, Path::new("/opt/pnpm")).expect("write");

    assert_eq!(written.len(), 2);
    let body = shim_body(dir.path(), "npx");
    let runs = if cfg!(windows) { r#""npm@11" "npx""# } else { "'npm@11' 'npx'" };
    assert!(body.contains(runs), "{body}");
}

#[cfg(unix)]
#[test]
fn the_shims_are_executable() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let wanted = WantedPm { pm: PreferredPm::Npm, version_spec: None, pinned: false };
    write_pm_shims(dir.path(), &wanted, Path::new("/opt/pnpm")).expect("write the shims");

    let mode = fs::metadata(dir.path().join("npm"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o111, 0o111, "mode was {mode:o}");
}

/// A path holding a shell metacharacter must not change what the shim
/// runs.
#[cfg(unix)]
#[test]
fn a_hostile_pnpm_path_is_quoted() {
    let dir = tempdir().unwrap();
    let wanted = WantedPm { pm: PreferredPm::Npm, version_spec: None, pinned: false };
    write_pm_shims(dir.path(), &wanted, Path::new("/opt/p'; touch /tmp/pwned; '")).expect("write");

    // Every quote in the path closes and reopens the literal, so the
    // whole path stays one shell word and nothing in it is ever parsed
    // as a command.
    assert_eq!(
        shim_body(dir.path(), "npm"),
        "#!/bin/sh\nexec '/opt/p'\\''; touch /tmp/pwned; '\\''' dlx --package 'npm' 'npm' \"$@\"\n",
    );
}

/// A quote cannot be escaped inside a quoted `cmd.exe` argument, so a
/// specifier that is not a semver range never reaches the command line at
/// all — on either platform, since only the check is shared.
#[test]
fn a_hostile_version_spec_is_dropped() {
    let dir = tempdir().unwrap();
    let hostile = r#"1.0.0" & calc & ""#.to_string();
    let wanted = WantedPm { pm: PreferredPm::Yarn, version_spec: Some(hostile), pinned: true };
    write_pm_shims(dir.path(), &wanted, Path::new("/opt/pnpm")).expect("write the shims");

    let body = shim_body(dir.path(), "yarn");
    let spec = if cfg!(windows) { r#"--package "yarn""# } else { "--package 'yarn'" };
    assert!(body.contains(spec), "{body}");
    assert!(!body.contains("calc"), "{body}");
}

/// The shim is regenerated every time, so an entry planted at its path
/// cannot survive to be executed — and a symlink there cannot redirect
/// the write.
#[test]
fn a_planted_entry_is_replaced() {
    let dir = tempdir().unwrap();
    let planted = dir
        .path()
        .join(if cfg!(windows) { "npm.cmd" } else { "npm" });
    let elsewhere = dir.path().join("elsewhere");
    fs::write(&elsewhere, "original\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&elsewhere, &planted).unwrap();
    #[cfg(windows)]
    fs::write(&planted, "@echo planted\r\n").unwrap();

    let wanted = WantedPm { pm: PreferredPm::Npm, version_spec: None, pinned: false };
    write_pm_shims(dir.path(), &wanted, Path::new("/opt/pnpm")).expect("write the shims");

    assert!(shim_body(dir.path(), "npm").contains("dlx"));
    assert_eq!(fs::read_to_string(&elsewhere).unwrap(), "original\n");
}

/// The running pnpm is called directly, so nothing is provisioned for it.
#[test]
fn the_running_pnpm_is_called_without_dlx() {
    let dir = tempdir().unwrap();
    write_running_pnpm_shim(dir.path(), Path::new("/opt/pnpm")).expect("write the shim");

    let body = shim_body(dir.path(), "pnpm");
    assert!(body.contains("/opt/pnpm"), "{body}");
    assert!(!body.contains("dlx"), "{body}");
}

fn provisioned(bins: &[(&str, &str)], runs_on_node: bool) -> crate::ProvisionedTool {
    crate::ProvisionedTool {
        name: "npm".to_string(),
        version: "10.9.2".to_string(),
        bins: bins
            .iter()
            .map(|(name, path)| ((*name).to_string(), std::path::PathBuf::from(path)))
            .collect(),
        runs_on_node,
    }
}

/// A recorded package manager runs from its own directory, on the recorded
/// Node.js, with no `pnpm dlx` resolving it again.
#[test]
fn a_locked_package_manager_runs_its_recorded_files() {
    let dir = tempdir().unwrap();
    let tool = provisioned(
        &[("npm", "/tools/npm/bin/npm-cli.js"), ("npx", "/tools/npm/bin/npx-cli.js")],
        true,
    );
    super::write_locked_pm_shims(
        dir.path(),
        PreferredPm::Npm,
        &tool,
        Some(Path::new("/tools/node/bin/node")),
    )
    .expect("write the shims");

    let npm = shim_body(dir.path(), "npm");
    assert!(npm.contains("/tools/node/bin/node"), "{npm}");
    assert!(npm.contains("/tools/npm/bin/npm-cli.js"), "{npm}");
    assert!(!npm.contains("dlx"), "{npm}");
    let npx = shim_body(dir.path(), "npx");
    assert!(npx.contains("/tools/npm/bin/npx-cli.js"), "{npx}");
}

#[test]
fn a_locked_package_manager_without_a_recorded_node_runs_on_the_hosts() {
    let dir = tempdir().unwrap();
    let tool = provisioned(
        &[("npm", "/tools/npm/bin/npm-cli.js"), ("npx", "/tools/npm/bin/npx-cli.js")],
        true,
    );
    super::write_locked_pm_shims(dir.path(), PreferredPm::Npm, &tool, None)
        .expect("write the shims");

    let npm = shim_body(dir.path(), "npm");
    assert!(npm.contains("node "), "{npm}");
}

/// `bunx` is `bun x` of the recorded executable.
#[test]
fn a_locked_executable_runs_directly() {
    let dir = tempdir().unwrap();
    let tool = crate::ProvisionedTool {
        name: "bun".to_string(),
        ..provisioned(&[("bun", "/tools/bun/bun")], false)
    };
    super::write_locked_pm_shims(
        dir.path(),
        PreferredPm::Bun,
        &tool,
        Some(Path::new("/tools/node")),
    )
    .expect("write the shims");

    let bunx = shim_body(dir.path(), "bunx");
    assert!(bunx.contains("/tools/bun/bun"), "{bunx}");
    assert!(!bunx.contains("/tools/node"), "{bunx}");
    let subcommand = if cfg!(windows) { r#""x""# } else { "'x'" };
    assert!(bunx.contains(subcommand), "{bunx}");
}

#[test]
fn a_locked_tool_without_the_command_fails() {
    let dir = tempdir().unwrap();
    let tool = provisioned(&[("npx", "/tools/npm/bin/npx-cli.js")], true);
    let error = super::write_locked_pm_shims(dir.path(), PreferredPm::Npm, &tool, None)
        .expect_err("npm is missing");
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}
