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
    let store = env.root.path().join("store");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("storeDir: {}\ncargo:\n  enabled: {cargo_enabled}\n", store.display()),
    )
    .unwrap();
    fs::write(workspace.join("rust-toolchain.toml"), "[toolchain]\nchannel = \"stable\"\n")
        .unwrap();
    let manifest = json!({ "name": "root", "private": true, "scripts": { "build": "cargo" } });
    fs::write(workspace.join("package.json"), manifest.to_string()).unwrap();
    let mut config = pnpm_config::Config::new();
    config.store_dir = pnpm_store_dir::StoreDir::from(store);
    let request = pnpm_rust_toolchain::read_toolchain_file(&workspace.join("rust-toolchain.toml"))
        .unwrap()
        .unwrap();
    let pinned = pnpm_rust_toolchain::Channel::parse("1.95.0").unwrap();
    let toolchain = pnpm_rust_toolchain::installation_dir(&config, &pinned, &request).unwrap();
    write_fake_bin(&toolchain.join("bin"), "cargo", MARKER);
    fs::create_dir_all(workspace.join(".pnpm")).unwrap();
    pnpm_fs::force_symlink_dir(&toolchain, &workspace.join(".pnpm/rust")).unwrap();
    env
}

#[test]
fn a_toolchain_the_checkout_committed_is_not_put_on_the_path() {
    let CommandTempCwd { workspace, root, .. } = workspace_with_linked_toolchain(true);
    fs::remove_file(workspace.join(".pnpm/rust"))
        .or_else(|_| fs::remove_dir(workspace.join(".pnpm/rust")))
        .unwrap();
    write_fake_bin(&workspace.join(".pnpm/rust/bin"), "cargo", MARKER);

    let output = pacquet_in(&workspace)
        .with_args(["exec", "cargo", "--version"])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("{stdout}");
    assert!(!stdout.contains(MARKER));
    drop(root);
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
