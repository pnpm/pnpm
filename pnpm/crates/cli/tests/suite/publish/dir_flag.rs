use std::{fs, process::Command};

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use mockito::Matcher;
use serde_json::json;

use super::{assert_success, pacquet, write_project};

#[test]
fn publish_with_dir_flag_resolves_relative_tarball_against_target_dir() {
    let outer = tempfile::tempdir().expect("outer dir");
    let subdir = outer.path().join("subdir");
    fs::create_dir_all(&subdir).expect("create subdir");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    write_project(
        &subdir,
        &registry,
        &json!({ "name": "test-dir-flag-tarball", "version": "1.0.0" }),
    );

    pacquet(&subdir)
        .with_args(["pack", "--out", "built.tgz"])
        .assert()
        .success();
    fs::remove_file(subdir.join("package.json")).expect("remove source manifest");

    let uploaded = server
        .mock("PUT", "/test-dir-flag-tarball")
        .match_body(Matcher::PartialJson(json!({
            "name": "test-dir-flag-tarball",
            "dist-tags": { "latest": "1.0.0" },
        })))
        .with_status(200)
        .with_body(r#"{"ok":true}"#)
        .expect(1)
        .create();

    let output = pacquet(outer.path())
        .with_args([
            "-C",
            subdir.to_str().expect("utf8 subdir"),
            "publish",
            "./built.tgz",
            "--no-git-checks",
        ])
        .output()
        .expect("spawn pnpm -C subdir publish ./built.tgz");
    assert_success(&output);
    uploaded.assert();
}

#[test]
fn publish_with_dir_flag_resolves_relative_folder_against_target_dir() {
    let outer = tempfile::tempdir().expect("outer dir");
    let subdir = outer.path().join("subdir");
    let nested = subdir.join("nested-pkg");
    fs::create_dir_all(&nested).expect("create nested dir");
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    fs::write(subdir.join(".npmrc"), format!("registry={registry}\n")).expect("write .npmrc");
    fs::write(
        nested.join("package.json"),
        json!({ "name": "test-dir-flag-folder", "version": "1.0.0" }).to_string(),
    )
    .expect("write nested package.json");

    let uploaded = server
        .mock("PUT", "/test-dir-flag-folder")
        .match_body(Matcher::PartialJson(json!({
            "name": "test-dir-flag-folder",
            "dist-tags": { "latest": "1.0.0" },
        })))
        .with_status(200)
        .with_body(r#"{"ok":true}"#)
        .expect(1)
        .create();

    let output = pacquet(outer.path())
        .with_args([
            "--dir",
            subdir.to_str().expect("utf8 subdir"),
            "publish",
            "./nested-pkg",
            "--no-git-checks",
        ])
        .output()
        .expect("spawn pnpm --dir subdir publish ./nested-pkg");
    assert_success(&output);
    uploaded.assert();
}

#[test]
fn publish_with_dir_flag_runs_git_checks_in_nested_target_repo() {
    let caller = tempfile::tempdir().expect("caller dir");
    let target_root = tempfile::tempdir().expect("target root");
    let nested = target_root.path().join("nested");
    fs::create_dir_all(&nested).expect("create nested dir");
    write_project(
        &nested,
        "http://127.0.0.1:1/",
        &json!({ "name": "test-nested-git-checks", "version": "1.0.0" }),
    );

    pnpm_testing_utils::git_repo::init_isolated_repo(caller.path());
    fs::write(caller.path().join("README.md"), "clean caller\n").expect("write caller readme");
    for args in [vec!["add", "."], vec!["commit", "-m", "init"]] {
        Command::new("git")
            .with_current_dir(caller.path())
            .with_args(args)
            .assert()
            .success();
    }

    pnpm_testing_utils::git_repo::init_isolated_repo(&nested);
    for args in [vec!["add", "."], vec!["commit", "-m", "init"]] {
        Command::new("git")
            .with_current_dir(&nested)
            .with_args(args)
            .assert()
            .success();
    }
    fs::write(nested.join("LICENSE"), "dirty uncommitted change\n").expect("dirty nested repo");

    let rejected = pacquet(caller.path())
        .with_args([
            "-C",
            target_root
                .path()
                .to_str()
                .expect("utf8 target root"),
            "publish",
            "nested",
            "--dry-run",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&rejected.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_GIT_UNCLEAN"), "stderr: {stderr}");
}

#[cfg(unix)]
#[test]
fn publish_with_dir_flag_preserves_symlink_parent_before_dotdot() {
    let outer = tempfile::tempdir().expect("outer dir");
    let subdir = outer.path().join("subdir");
    let real_release = outer.path().join("real-release");
    let real_build_sub = real_release.join("build");
    let real_folder_pkg = real_release.join("pkg");
    fs::create_dir_all(&subdir).expect("create subdir");
    fs::create_dir_all(&real_build_sub).expect("create real_build_sub");
    fs::create_dir_all(&real_folder_pkg).expect("create real_folder_pkg");
    std::os::unix::fs::symlink(&real_build_sub, subdir.join("link")).expect("create symlink");

    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());
    fs::write(subdir.join(".npmrc"), format!("registry={registry}\n"))
        .expect("write subdir .npmrc");

    write_project(
        &real_release,
        &registry,
        &json!({ "name": "test-symlink-tarball", "version": "1.0.0" }),
    );
    pacquet(&real_release)
        .with_args(["pack", "--out", "package.tgz"])
        .assert()
        .success();
    fs::remove_file(real_release.join("package.json")).expect("remove release manifest");
    fs::write(
        real_folder_pkg.join("package.json"),
        json!({ "name": "test-symlink-folder", "version": "1.0.0" }).to_string(),
    )
    .expect("write folder package.json");

    let tarball_uploaded = server
        .mock("PUT", "/test-symlink-tarball")
        .match_body(Matcher::PartialJson(json!({
            "name": "test-symlink-tarball",
            "dist-tags": { "latest": "1.0.0" },
        })))
        .with_status(200)
        .with_body(r#"{"ok":true}"#)
        .expect(1)
        .create();
    let folder_uploaded = server
        .mock("PUT", "/test-symlink-folder")
        .match_body(Matcher::PartialJson(json!({
            "name": "test-symlink-folder",
            "dist-tags": { "latest": "1.0.0" },
        })))
        .with_status(200)
        .with_body(r#"{"ok":true}"#)
        .expect(1)
        .create();

    let tarball_output = pacquet(outer.path())
        .with_args([
            "-C",
            subdir.to_str().expect("utf8 subdir"),
            "publish",
            "link/../package.tgz",
            "--no-git-checks",
        ])
        .output()
        .expect("spawn pnpm -C subdir publish link/../package.tgz");
    assert_success(&tarball_output);
    tarball_uploaded.assert();

    let folder_output = pacquet(outer.path())
        .with_args([
            "-C",
            subdir.to_str().expect("utf8 subdir"),
            "publish",
            "link/../pkg",
            "--no-git-checks",
        ])
        .output()
        .expect("spawn pnpm -C subdir publish link/../pkg");
    assert_success(&folder_output);
    folder_uploaded.assert();
}
