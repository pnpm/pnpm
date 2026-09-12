use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pipe_trait::Pipe;
use pnpm_store_dir::STORE_VERSION;
use pnpm_testing_utils::bin::CommandTempCwd;
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
fn store_path_accepts_the_silent_shorthand() {
    // `--silent` / `-s` are universal shorthands for `--reporter=silent`
    // (pnpm expands them over argv before parsing); `pnpm store path
    // --silent` is how pnpm/setup queried the store dir historically, so
    // both spellings must keep working.
    for silent_arg in ["--silent", "-s"] {
        let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
        fs::write(workspace.join("pnpm-workspace.yaml"), "storeDir: foo/bar\n")
            .expect("write to pnpm-workspace.yaml");

        let output = pacquet
            .with_args(["store", "path", silent_arg])
            .output()
            .expect("run pacquet store path with the silent shorthand");
        assert!(output.status.success(), "store path {silent_arg} must succeed: {output:?}");

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
        let output = Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
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
    let default_output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
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
        let output = Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
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

#[test]
fn store_status_reports_an_untouched_store() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    pacquet.arg("add").arg("is-odd@3.0.1").assert().success();

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .args(["store", "status"])
        .output()
        .expect("run pacquet store status");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("stderr={stderr}");
    assert!(output.status.success(), "store status must succeed on a clean store");
    assert!(stderr.contains("Packages in the store are untouched"), "stderr={stderr}");
}

#[test]
fn store_status_reports_a_package_edited_after_it_was_linked_out() {
    let CommandTempCwd { mut pacquet, workspace, root: _root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    pacquet.arg("add").arg("is-odd@3.0.1").assert().success();

    let installed_index = workspace.join("node_modules/.pnpm/is-odd@3.0.1/node_modules/is-odd");
    fs::write(installed_index.join("index.js"), "module.exports = 'tampered'\n")
        .expect("edit the installed package");

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .args(["store", "status"])
        .output()
        .expect("run pacquet store status");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("stderr={stderr}");
    assert!(!output.status.success(), "store status must fail once a package is modified");
    assert!(stderr.contains("ERR_PNPM_MODIFIED_DEPENDENCY"), "stderr={stderr}");
    assert!(stderr.contains("is-odd@3.0.1"), "stderr={stderr}");
}

#[test]
fn store_add_fetches_a_package_without_touching_the_project() {
    let CommandTempCwd { pacquet, workspace, root: _root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet.with_args(["store", "add", "is-odd@3.0.1"]).assert().success();

    assert!(!workspace.join("node_modules").exists(), "store add must not install anything");
    assert!(!workspace.join("package.json").exists(), "store add must not write a manifest");
    assert!(!workspace.join("pnpm-lock.yaml").exists(), "store add must not write a lockfile");

    // The package is now in the store, which is the whole point: the row
    // the fetch wrote is what a later install reuses.
    let store_dir = pnpm_store_dir::StoreDir::from(npmrc_info.store_dir);
    let store_index = pnpm_store_dir::StoreIndex::open_readonly_in(&store_dir)
        .expect("open the store index store add just wrote");
    let keys = store_index.keys().expect("read the store index keys");
    assert!(
        keys.iter().any(|key| key.contains("is-odd@3.0.1")),
        "store add must record is-odd@3.0.1 in the store index, got {keys:?}",
    );
}

#[test]
fn store_add_fails_when_a_package_cannot_be_fetched() {
    let CommandTempCwd { pacquet, root: _root, .. } = CommandTempCwd::init().add_mocked_registry();

    let output = pacquet
        .with_args(["store", "add", "@pnpm/this-package-does-not-exist@1.0.0"])
        .output()
        .expect("run pacquet store add for a missing package");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("stderr={stderr}");
    assert!(!output.status.success(), "store add must fail when a package cannot be fetched");
    assert!(stderr.contains("ERR_PNPM_STORE_ADD_FAILURE"), "stderr={stderr}");
}

/// The resolver chain claims every protocol pnpm supports, but only an
/// archive can be put in the store. A local dependency is refused by name
/// rather than failing further down as a resolution-shape mismatch.
#[test]
fn store_add_refuses_a_specifier_with_no_archive_to_fetch() {
    let CommandTempCwd { pacquet, workspace, root: _root, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let local_package = workspace.join("local-pkg");
    fs::create_dir_all(&local_package).expect("create the local package");
    fs::write(local_package.join("package.json"), r#"{"name":"local-pkg","version":"1.0.0"}"#)
        .expect("write the local package manifest");

    let output = pacquet
        .with_args(["store", "add", "./local-pkg"])
        .output()
        .expect("run pacquet store add for a local dependency");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("stderr={stderr}");
    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_STORE_ADD_UNSUPPORTED_SPEC"), "stderr={stderr}");
}
