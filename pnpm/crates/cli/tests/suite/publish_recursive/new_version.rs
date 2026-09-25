//! `publish --recursive --new-version`: every selected package is set to the
//! given version before publishing, mirroring
//! `pnpm version <version> -r --no-git-tag-version` followed by
//! `pnpm publish -r`.

use super::{clear_ci, private_pkg, public_pkg, write_registry_npmrc, write_workspace};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use mockito::Matcher;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::Value;
use std::fs;

#[test]
fn recursive_new_version_bumps_and_publishes_every_selected_package() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_workspace(
        &workspace,
        &[("project-1", public_pkg("project-1")), ("project-2", public_pkg("project-2"))],
    );
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));

    // The already-published probe runs after the bump, so it asks about the
    // new version; a 404 answers "not published" for either.
    let probe_1 = server
        .mock("GET", "/project-1")
        .with_status(404)
        .create();
    let probe_2 = server
        .mock("GET", "/project-2")
        .with_status(404)
        .create();
    let put_1 = server
        .mock("PUT", "/project-1")
        .match_body(Matcher::PartialJsonString(r#"{"dist-tags":{"latest":"3.0.0"}}"#.to_owned()))
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();
    let put_2 = server
        .mock("PUT", "/project-2")
        .match_body(Matcher::PartialJsonString(r#"{"dist-tags":{"latest":"3.0.0"}}"#.to_owned()))
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();

    clear_ci(pacquet)
        .with_arg("-r")
        .with_arg("publish")
        .with_arg("--new-version")
        .with_arg("3.0.0")
        .with_arg("--no-git-checks")
        .assert()
        .success();

    probe_1.assert();
    probe_2.assert();
    put_1.assert();
    put_2.assert();
    for name in ["project-1", "project-2"] {
        let manifest: Value = serde_json::from_str(
            &fs::read_to_string(workspace.join(name).join("package.json")).expect("read"),
        )
        .expect("parse package.json");
        assert_eq!(manifest["version"], "3.0.0", "{name} must stay bumped on disk");
    }

    drop(root);
}

/// A private package gets the new version on disk — as `pnpm version -r`
/// would bump it — but is not published.
#[test]
fn recursive_new_version_bumps_but_skips_private_packages() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    write_workspace(
        &workspace,
        &[("project-1", public_pkg("project-1")), ("project-2", private_pkg("project-2"))],
    );
    write_registry_npmrc(&workspace, &format!("{}/", server.url()));

    let probe = server
        .mock("GET", "/project-1")
        .with_status(404)
        .create();
    let put_1 = server
        .mock("PUT", "/project-1")
        .with_status(200)
        .with_body("{}")
        .expect(1)
        .create();
    let put_2 = server
        .mock("PUT", "/project-2")
        .expect(0)
        .create();

    clear_ci(pacquet)
        .with_arg("-r")
        .with_arg("publish")
        .with_arg("--new-version")
        .with_arg("3.0.0")
        .with_arg("--no-git-checks")
        .assert()
        .success();

    probe.assert();
    put_1.assert();
    put_2.assert();
    for name in ["project-1", "project-2"] {
        let manifest: Value = serde_json::from_str(
            &fs::read_to_string(workspace.join(name).join("package.json")).expect("read"),
        )
        .expect("parse package.json");
        assert_eq!(manifest["version"], "3.0.0", "{name} must stay bumped on disk");
    }

    drop(root);
}
