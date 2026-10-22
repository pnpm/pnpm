//! `pnpm run` and `pnpm exec` finding the Rust toolchain pnpm linked beside
//! a rustup toolchain file, and `pnpm dlx` running a Rust release.

#[cfg(unix)]
use crate::_utils::write_executable;
use crate::_utils::{pacquet_in, write_fake_bin};
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, diagnostics::assert_diagnostic_contains};
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

/// `pnpm dlx --package=rust@<channel>` runs the tool from that release,
/// installed with the standard library of the target it builds for.
#[cfg(unix)]
#[test]
fn dlx_runs_a_tool_of_the_release_it_names() {
    let CommandTempCwd { workspace, root, .. } = workspace_with_linked_toolchain(true);
    let mut config = pnpm_config::Config::new();
    config.store_dir = pnpm_store_dir::StoreDir::from(root.path().join("store"));
    let nightly = pnpm_rust_toolchain::Channel::parse("nightly-2026-08-27").unwrap();
    let request = pnpm_rust_toolchain::ToolchainRequest::for_channel(nightly.clone())
        .with_targets(["wasm32-wasip1-threads"]);
    let toolchain = pnpm_rust_toolchain::installation_dir(&config, &nightly, &request).unwrap();
    fs::create_dir_all(toolchain.join("bin")).unwrap();
    write_executable(
        &toolchain.join("bin/cargo"),
        "#!/bin/sh\necho nightly-cargo \"$@\"\necho \"$RUSTUP_TOOLCHAIN\"\n",
    );

    let output = pacquet_in(&workspace)
        .with_args([
            "dlx",
            "--package=rust@nightly-2026-08-27",
            "cargo",
            "build",
            "--target=wasm32-wasip1-threads",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("{stdout}\n{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success());
    let mut lines = stdout.lines();
    assert_eq!(lines.next(), Some("nightly-cargo build --target=wasm32-wasip1-threads"));
    assert_eq!(
        lines
            .next()
            .map(|dir| fs::canonicalize(dir).unwrap()),
        Some(fs::canonicalize(&toolchain).unwrap()),
    );
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

/// `pnpm add rust@<channel>` pins the project's toolchain in the toolchain
/// file that governs it, keeping the rest of the file, or creates one in
/// the project.
#[test]
fn add_rust_pins_the_project_toolchain() {
    let CommandTempCwd { workspace, root, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "root", "private": true }).to_string(),
    )
    .unwrap();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - crates/*\n").unwrap();

    let output = pacquet_in(&workspace)
        .with_args(["add", "rust@1.95.0"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        fs::read_to_string(workspace.join("rust-toolchain.toml")).unwrap(),
        "[toolchain]\nchannel = \"1.95.0\"\n",
    );

    fs::write(
        workspace.join("rust-toolchain.toml"),
        "# pinned for CI\n[toolchain]\nchannel = \"1.95.0\"\ncomponents = [\"rust-src\"]\n",
    )
    .unwrap();
    let member = workspace.join("crates/member");
    fs::create_dir_all(&member).unwrap();
    fs::write(member.join("package.json"), json!({ "name": "member" }).to_string()).unwrap();
    let output = pacquet_in(&member)
        .with_args(["add", "rust@nightly-2026-01-01"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        fs::read_to_string(workspace.join("rust-toolchain.toml")).unwrap(),
        "# pinned for CI\n[toolchain]\nchannel = \"nightly-2026-01-01\"\ncomponents = [\"rust-src\"]\n",
    );
    assert!(!member.join("rust-toolchain.toml").exists());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(
            &fs::read_to_string(member.join("package.json")).unwrap()
        )
        .unwrap()
        .get("dependencies"),
        None,
    );
    drop(root);
}

/// With the Cargo integration enabled, the add also installs the toolchain
/// and links it beside the file. The toolchain is put in the store first,
/// so nothing is downloaded.
#[test]
fn add_rust_links_the_toolchain_where_cargo_is_enabled() {
    let CommandTempCwd { workspace, root, .. } = CommandTempCwd::init();
    let store = root.path().join("store");
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "root", "private": true }).to_string(),
    )
    .unwrap();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("storeDir: {}\ncargo:\n  enabled: true\n", store.display()),
    )
    .unwrap();
    let request_file = root.path().join("request");
    fs::write(&request_file, "1.95.0").unwrap();
    let request = pnpm_rust_toolchain::read_toolchain_file(&request_file).unwrap().unwrap();
    let mut config = pnpm_config::Config::new();
    config.store_dir = pnpm_store_dir::StoreDir::from(store);
    let pinned = pnpm_rust_toolchain::Channel::parse("1.95.0").unwrap();
    let toolchain = pnpm_rust_toolchain::installation_dir(&config, &pinned, &request).unwrap();
    write_fake_bin(&toolchain.join("bin"), "cargo", MARKER);

    let output = pacquet_in(&workspace)
        .with_args(["add", "rust@1.95.0"])
        .output()
        .unwrap();

    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(
        dunce::canonicalize(workspace.join(".pnpm/rust")).unwrap(),
        dunce::canonicalize(&toolchain).unwrap(),
    );
    drop(root);
}

#[test]
fn add_rust_through_a_filter_is_refused() {
    let CommandTempCwd { workspace, root, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "root", "private": true }).to_string(),
    )
    .unwrap();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - crates/*\n").unwrap();
    let member = workspace.join("crates/member");
    fs::create_dir_all(&member).unwrap();
    fs::write(member.join("package.json"), json!({ "name": "member" }).to_string()).unwrap();

    let output = pacquet_in(&workspace)
        .with_args(["--filter", "member", "add", "rust@1.95.0"])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert_diagnostic_contains(
        &String::from_utf8_lossy(&output.stderr),
        "cannot yet be pinned through a recursive or filtered selection",
    );
    assert!(!workspace.join("rust-toolchain.toml").exists());
    assert!(!member.join("rust-toolchain.toml").exists());
    drop(root);
}
