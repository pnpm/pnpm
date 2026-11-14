use std::fs;

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
