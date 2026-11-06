//! A project reached through a UNC path (pnpm/pnpm#17747). The local
//! `C$` admin share stands in for a network share, so the test is skipped
//! where that share is not reachable.

#![cfg(windows)]

use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::{
    fs,
    path::{Component, Path, PathBuf, Prefix},
};

/// `C:\dir` as `\\localhost\C$\dir`, when the admin share serves it.
fn admin_share_form(path: &Path) -> Option<PathBuf> {
    let mut components = path.components();
    let Some(Component::Prefix(prefix)) = components.next() else { return None };
    let (Prefix::Disk(drive) | Prefix::VerbatimDisk(drive)) = prefix.kind() else { return None };
    let unc = Path::new(&format!(r"\\localhost\{}$\", drive as char)).join(components.as_path());
    unc.is_dir().then_some(unc)
}

#[test]
fn installs_from_a_unc_working_directory() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let Some(unc_workspace) = admin_share_form(&workspace) else {
        eprintln!("skipping: {} is not reachable through the C$ admin share", workspace.display());
        return;
    };
    // The hoisted linker keeps the test to copies: a junction cannot be
    // created on an SMB share, and a symlink needs a privilege CI may lack.
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let config = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml, format!("{config}nodeLinker: hoisted\n"))
        .expect("write pnpm-workspace.yaml");
    fs::write(workspace.join("package.json"), r#"{"dependencies":{"ms":"1.0.0"}}"#)
        .expect("write package.json");

    // Without these the default store sits under them, and the drive-based
    // default that panicked on a UNC working directory is never computed.
    // `storeDir` above still decides where the store goes.
    let output = pacquet
        .without_env("PNPM_HOME")
        .without_env("XDG_DATA_HOME")
        .with_current_dir(&unc_workspace)
        .with_arg("install")
        .output()
        .expect("run pnpm install");

    assert!(output.status.success(), "install from {unc_workspace:?} must succeed: {output:?}");
    assert!(workspace.join("node_modules/ms/package.json").is_file(), "ms is installed");
    drop(root);
}
