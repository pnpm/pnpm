use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_store_dir::STORE_VERSION;
use pacquet_testing_utils::bin::CommandTempCwd;
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// Canonicalize a path the same way the production CLI does. The CLI
/// runs `dunce::canonicalize` on `--dir` and threads that through to
/// `Config::current`, so on Windows the printed `storeDir` is the long
/// form (`C:\Users\runneradmin\...`) even when the surrounding test
/// runs in a `TEMP` directory whose env var resolves to the short DOS
/// form (`C:\Users\RUNNER~1\...`). Mirror that here so the expected
/// value matches what pacquet actually prints.
fn canonicalize(path: &Path) -> PathBuf {
    dunce::canonicalize(path).expect("canonicalize path")
}

#[test]
fn store_path_should_return_store_dir_from_pnpm_workspace_yaml() {
    // `storeDir` is a project-structural setting — in pnpm 11 (and now
    // pacquet) it's only honoured from `pnpm-workspace.yaml`, not `.npmrc`.
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();

    eprintln!("Creating pnpm-workspace.yaml...");
    fs::write(workspace.join("pnpm-workspace.yaml"), "storeDir: foo/bar\n")
        .expect("write to pnpm-workspace.yaml");

    eprintln!("Executing pacquet store path...");
    let output = pacquet.with_args(["store", "path"]).output().expect("run pacquet store path");
    dbg!(&output);

    eprintln!("Exit status code");
    assert!(output.status.success());

    eprintln!("Stdout");
    let normalize = |path: &str| path.replace('\\', "/");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end().pipe(normalize),
        canonicalize(&workspace)
            .join("foo/bar")
            .join(STORE_VERSION)
            .to_string_lossy()
            .pipe_as_ref(normalize),
    );

    drop(root);
}

#[test]
fn store_path_resolves_global_and_dotted_overrides_from_workspace_root() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    let package_dir = workspace.join("packages/app");
    fs::create_dir_all(&package_dir).expect("create nested workspace package");

    for (store_arg, expected_name) in [
        ("--store-dir=global-store", "global-store"),
        ("--config.store-dir=dotted-store", "dotted-store"),
    ] {
        let output = Command::cargo_bin("pacquet")
            .expect("find the pacquet binary")
            .with_current_dir(root.path())
            .arg("--dir")
            .arg(&package_dir)
            .arg(store_arg)
            .args(["store", "path"])
            .output()
            .expect("run pacquet store path with override");
        eprintln!("stderr={}", String::from_utf8_lossy(&output.stderr));
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim_end(),
            canonicalize(&workspace).join(expected_name).join(STORE_VERSION).to_string_lossy(),
        );
    }

    drop(root);
}

#[test]
fn store_path_expands_a_quoted_home_override() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();
    let home_dir = root.path().join("home");
    fs::create_dir_all(&home_dir).expect("create home directory");
    let output = pacquet
        .with_args(["store", "path", "--store-dir=~/pacquet-quoted-store"])
        .env("HOME", &home_dir)
        .env("USERPROFILE", &home_dir)
        .output()
        .expect("run pacquet store path with home-relative override");
    if !output.status.success() {
        eprintln!("stdout={}", String::from_utf8_lossy(&output.stdout));
        eprintln!("stderr={}", String::from_utf8_lossy(&output.stderr));
    }
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        home_dir.join("pacquet-quoted-store").join(STORE_VERSION).to_string_lossy(),
    );

    drop(root);
}

#[test]
fn empty_store_dir_override_restores_the_platform_default() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let default_output = Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(&workspace)
        .args(["store", "path"])
        .output()
        .expect("read the default store path");
    eprintln!("default status={}", default_output.status);
    eprintln!("default stdout={}", String::from_utf8_lossy(&default_output.stdout));
    eprintln!("default stderr={}", String::from_utf8_lossy(&default_output.stderr));
    assert!(default_output.status.success());
    let default_store = String::from_utf8_lossy(&default_output.stdout).trim_end().to_owned();

    fs::write(workspace.join("pnpm-workspace.yaml"), "storeDir: yaml-store\n")
        .expect("write configured store directory");
    for store_arg in ["--store-dir=", "--config.store-dir="] {
        let output = Command::cargo_bin("pacquet")
            .expect("find the pacquet binary")
            .with_current_dir(&workspace)
            .args(["store", "path", store_arg])
            .output()
            .expect("run store path with an empty override");
        eprintln!("stderr={}", String::from_utf8_lossy(&output.stderr));
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim_end(), default_store);
    }

    drop(root);
}
