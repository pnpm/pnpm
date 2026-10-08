//! `pnpm run` and `pnpm exec` finding the Rust toolchain pnpm linked beside
//! a rustup toolchain file.

use crate::_utils::{pacquet_in, write_fake_bin};
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::fs;

const MARKER: &str = "linked-rust-toolchain";

fn workspace_with_linked_toolchain(cargo_enabled: bool) -> CommandTempCwd<()> {
    let env = CommandTempCwd::init();
    let workspace = &env.workspace;
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("cargo:\n  enabled: {cargo_enabled}\n"),
    )
    .unwrap();
    fs::write(workspace.join("rust-toolchain.toml"), "[toolchain]\nchannel = \"stable\"\n")
        .unwrap();
    let manifest = json!({ "name": "root", "private": true, "scripts": { "build": "cargo" } });
    fs::write(workspace.join("package.json"), manifest.to_string()).unwrap();
    write_fake_bin(&workspace.join(".pnpm/rust/bin"), "cargo", MARKER);
    env
}

#[test]
fn scripts_and_commands_run_the_linked_toolchain() {
    let CommandTempCwd { workspace, root, .. } = workspace_with_linked_toolchain(true);
    let member = workspace.join("crates/member");
    fs::create_dir_all(&member).unwrap();

    for (dir, args) in [(&workspace, ["run", "build"]), (&member, ["exec", "cargo"])] {
        let output = pacquet_in(dir)
            .with_args(args)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        eprintln!("{args:?} in {}:\n{stdout}", dir.display());
        assert!(output.status.success());
        assert!(stdout.contains(MARKER));
    }
    drop(root);
}

#[test]
fn the_toolchain_is_not_used_without_cargo_enabled() {
    let CommandTempCwd { workspace, root, .. } = workspace_with_linked_toolchain(false);

    let output = pacquet_in(&workspace)
        .with_args(["exec", "cargo", "--version"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("{stdout}");
    assert!(!stdout.contains(MARKER));
    drop(root);
}
