//! The Rust tool shims `pnpm shim add rust` writes: the toolchain a
//! project's `rust-toolchain.toml` names runs, and outside such a project
//! the next program of the same name on `PATH` does.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::command_env::CommandTestExt;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;

/// The command, isolated from the developer's pnpm and from the rustup
/// that may have started the test run: rustup sets `RUSTUP_TOOLCHAIN` for
/// what it runs, which the shims leave to rustup.
fn isolated(mut command: Command, root: &TempDir, cwd: &Path) -> Command {
    command.env_remove("RUSTUP_TOOLCHAIN");
    command
        .without_ambient_pnpm_config()
        .with_current_dir(cwd)
        .with_env("PNPM_HOME", root.path().join("pnpm-home"))
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CONFIG_HOME", root.path().join("config"))
        .with_env("XDG_CACHE_HOME", root.path().join("cache-home"))
}

fn global_bin(root: &TempDir) -> PathBuf {
    root.path()
        .join("pnpm-home")
        .join("bin")
}

/// Add the Rust shims, with the store the dispatcher installs toolchains
/// into set in the global config.
fn add_rust_shims(root: &TempDir) {
    let config_dir = root.path().join("config/pnpm");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("config.yaml"), format!("storeDir: {}\n", store_dir(root).display()))
        .unwrap();
    let output = isolated(Command::cargo_bin("pnpm").unwrap(), root, root.path())
        .with_args(["shim", "add", "rust"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("cargo") && stdout.contains("rustc"), "{stdout}");
}

fn store_dir(root: &TempDir) -> PathBuf {
    root.path().join("store")
}

#[test]
fn shim_add_rust_writes_a_shim_for_every_tool() {
    let root = tempfile::tempdir().unwrap();
    add_rust_shims(&root);

    let exe = std::env::consts::EXE_SUFFIX;
    for bin in
        ["cargo", "cargo-clippy", "cargo-fmt", "clippy-driver", "rustc", "rustdoc", "rustfmt"]
    {
        assert!(
            global_bin(&root)
                .join(format!("{bin}{exe}"))
                .exists(),
            "{bin}",
        );
    }
    let config = fs::read_to_string(root.path().join("config/pnpm/config.yaml")).unwrap();
    assert!(config.contains("rust: auto"), "{config}");
}

#[cfg(unix)]
fn write_script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt as _;
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

#[cfg(unix)]
fn shim(root: &TempDir, cwd: &Path, name: &str, path: &[&Path]) -> Command {
    let path = std::env::join_paths(path).unwrap();
    isolated(Command::new(global_bin(root).join(name)), root, cwd).with_env("PATH", path)
}

/// The toolchain `rust-toolchain.toml` names, already in the store, runs
/// with its own `bin` first on `PATH`, so the `rustc` its `cargo` runs is
/// the same toolchain's.
#[cfg(unix)]
#[test]
fn a_bare_cargo_runs_the_toolchain_the_project_names() {
    let root = tempfile::tempdir().unwrap();
    add_rust_shims(&root);
    let project = root.path().join("project");
    let member = project.join("crates/member");
    fs::create_dir_all(&member).unwrap();
    let toolchain_file = project.join("rust-toolchain.toml");
    fs::write(&toolchain_file, "[toolchain]\nchannel = \"1.95.0\"\n").unwrap();

    let mut config = pnpm_config::Config::new();
    config.store_dir = pnpm_store_dir::StoreDir::from(store_dir(&root));
    let request = pnpm_rust_toolchain::read_toolchain_file(&toolchain_file).unwrap().unwrap();
    let pinned = pnpm_rust_toolchain::Channel::parse("1.95.0").unwrap();
    let toolchain = pnpm_rust_toolchain::installation_dir(&config, &pinned, &request).unwrap();
    write_script(
        &toolchain.join("bin/cargo"),
        r#"echo pinned-cargo "$@"; command -v rustc; echo "$RUSTUP_TOOLCHAIN""#,
    );
    write_script(&toolchain.join("bin/rustc"), "echo pinned-rustc");

    let output = shim(
        &root,
        &member,
        "cargo",
        &[&global_bin(&root), Path::new("/usr/bin"), Path::new("/bin")],
    )
    .with_args(["build", "--release"])
    .output()
    .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let mut lines = stdout.lines();
    assert_eq!(lines.next(), Some("pinned-cargo build --release"));
    assert_eq!(
        lines
            .next()
            .map(|rustc| fs::canonicalize(rustc).unwrap()),
        Some(fs::canonicalize(toolchain.join("bin/rustc")).unwrap()),
    );
    // A rustup proxy cargo runs, such as `$CARGO_HOME/bin/cargo-clippy`,
    // runs the same toolchain.
    assert_eq!(lines.next().map(Path::new), Some(toolchain.as_path()));
}

/// Outside a project that names a toolchain, the shim steps aside for the
/// next `cargo` on `PATH`, such as rustup's.
#[cfg(unix)]
#[test]
fn without_a_toolchain_file_the_next_cargo_on_path_runs() {
    let root = tempfile::tempdir().unwrap();
    add_rust_shims(&root);
    let elsewhere = root.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let rustup = root.path().join("rustup-bin");
    write_script(&rustup.join("cargo"), r#"echo rustup-cargo "$@""#);

    let output =
        shim(&root, &elsewhere, "cargo", &[&global_bin(&root), &rustup, Path::new("/bin")])
            .with_args(["--version"])
            .output()
            .unwrap();

    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "rustup-cargo --version");

    let output = shim(&root, &elsewhere, "cargo", &[&global_bin(&root)]).output().unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_SHIM_NO_TARGET"), "{stderr}");
}

/// `cargo +nightly` and `RUSTUP_TOOLCHAIN` choose a toolchain the way only
/// rustup can, so they reach the next `cargo` on `PATH` even in a project
/// whose toolchain file pnpm manages.
#[cfg(unix)]
#[test]
fn rustup_toolchain_overrides_reach_rustup() {
    let root = tempfile::tempdir().unwrap();
    add_rust_shims(&root);
    let project = root.path().join("project");
    fs::create_dir_all(&project).unwrap();
    // A release that is not in the store: the overrides must not reach the
    // toolchain file at all.
    fs::write(project.join("rust-toolchain.toml"), "1.95.0").unwrap();
    let rustup = root.path().join("rustup-bin");
    write_script(&rustup.join("cargo"), r#"echo rustup-cargo "$@""#);
    let global_bin = global_bin(&root);
    let path = [global_bin.as_path(), rustup.as_path(), Path::new("/bin")];

    let output = shim(&root, &project, "cargo", &path)
        .with_args(["+nightly", "build"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "rustup-cargo +nightly build");

    let output = shim(&root, &project, "cargo", &path)
        .with_env("RUSTUP_TOOLCHAIN", "nightly")
        .with_args(["build"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "rustup-cargo build");

    // A shim switched off gives way the same way.
    let output = shim(&root, &project, "cargo", &path)
        .with_env("PNPM_SHIM_BYPASS", "1")
        .with_args(["build"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "rustup-cargo build");
}

/// Put a fake toolchain for `channel`, as a bare `channel` request asks for
/// it, where the shims install toolchains, so nothing is downloaded.
#[cfg(unix)]
fn seed_toolchain(root: &TempDir, channel: &str, cargo_says: &str) -> PathBuf {
    let request_file = root.path().join("request");
    fs::write(&request_file, channel).unwrap();
    let request = pnpm_rust_toolchain::read_toolchain_file(&request_file).unwrap().unwrap();
    let mut config = pnpm_config::Config::new();
    config.store_dir = pnpm_store_dir::StoreDir::from(store_dir(root));
    let pinned = pnpm_rust_toolchain::Channel::parse(channel).unwrap();
    let toolchain = pnpm_rust_toolchain::installation_dir(&config, &pinned, &request).unwrap();
    write_script(&toolchain.join("bin/cargo"), &format!(r#"echo {cargo_says} "$@""#));
    toolchain
}

/// `pnpm add -g rust@<channel>` installs the toolchain the shims run
/// outside a project that names one, `pnpm ls -g` lists it, and
/// `pnpm remove -g rust` takes it and the shims away.
#[cfg(unix)]
#[test]
fn rust_installs_globally() {
    let root = tempfile::tempdir().unwrap();
    let config_dir = root.path().join("config/pnpm");
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(
        config_dir.join("config.yaml"),
        format!("storeDir: {}\n", store_dir(&root).display()),
    )
    .unwrap();
    let toolchain = seed_toolchain(&root, "1.95.0", "global-cargo");
    let elsewhere = root.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    let pnpm = |args: &[&str]| {
        isolated(Command::cargo_bin("pnpm").unwrap(), &root, &elsewhere)
            .with_env(
                "PATH",
                std::env::join_paths([global_bin(&root), PathBuf::from("/bin")]).unwrap(),
            )
            .with_args(args)
            .output()
            .unwrap()
    };

    let output = pnpm(&["add", "-g", "rust@1.95.0"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));

    let output = shim(&root, &elsewhere, "cargo", &[&global_bin(&root)])
        .with_args(["build"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "global-cargo build");

    let listed = pnpm(&["ls", "-g", "--json"]);
    let listed: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    let rust = &listed[0]["dependencies"]["rust"];
    assert_eq!(rust["version"], "1.95.0");
    assert_eq!(Path::new(rust["path"].as_str().unwrap()), toolchain);

    let output = pnpm(&["remove", "-g", "rust"]);
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let exe = std::env::consts::EXE_SUFFIX;
    assert!(
        !global_bin(&root)
            .join(format!("cargo{exe}"))
            .exists(),
    );
    let output = pnpm(&["remove", "-g", "rust"]);
    assert!(!output.status.success());
}
