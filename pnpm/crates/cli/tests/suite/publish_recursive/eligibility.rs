use std::fs;

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use mockito::Matcher;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;

use super::{clear_ci, public_pkg, write_registry_npmrc, write_workspace};

#[test]
fn recursive_publish_skips_manifests_with_empty_name_or_empty_version() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_workspace(
        &workspace,
        &[
            ("pkg-valid", public_pkg("pkg-valid")),
            ("pkg-empty-version", json!({ "name": "pkg-empty-version", "version": "" })),
            ("pkg-empty-name", json!({ "name": "", "version": "1.0.0" })),
        ],
    );
    fs::write(
        workspace.join("package.json"),
        json!({ "name": "monorepo-root", "version": "" }).to_string(),
    )
    .expect("write root package.json with empty version");
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));

    let probe_valid = server
        .mock("GET", "/pkg-valid")
        .with_status(404)
        .expect(1)
        .create();
    let unexpected_probe = server
        .mock("GET", Matcher::Regex("^/(pkg-empty-version|monorepo-root)$".to_string()))
        .expect(0)
        .create();
    let put_valid = server
        .mock("PUT", "/pkg-valid")
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();

    clear_ci(pacquet)
        .with_args(["-r", "publish", "--no-git-checks"])
        .assert()
        .success();

    probe_valid.assert();
    unexpected_probe.assert();
    put_valid.assert();
    drop(root);
}
