use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::CommandTempCwd;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn empty_auth_file(root: &Path) -> PathBuf {
    let auth_file = root.join("auth-npmrc");
    fs::write(&auth_file, "").expect("write empty auth .npmrc");
    auth_file
}

fn run_team(
    workspace: &Path,
    auth_file: &Path,
    registry: &str,
    args: &[&str],
) -> std::process::Output {
    let mut command = pacquet_at(workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(auth_file)
        .with_arg("--registry")
        .with_arg(registry)
        .with_arg("team");

    for arg in args {
        command = command.with_arg(arg);
    }

    command.output().expect("spawn pacquet team")
}

#[test]
fn fails_when_auth_is_missing() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());

    let _mock = server
        .mock("GET", "/-/team/myscope/myteam/user")
        .with_status(401)
        .with_body("Unauthorized")
        .create();

    let auth_file = empty_auth_file(root.path());

    let output = run_team(&workspace, &auth_file, &registry, &["ls", "@myscope:myteam"]);

    assert!(!output.status.success(), "team must fail when auth is missing");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Authentication required for registry access"),
        "stderr must contain auth error; got:\n{stderr}",
    );
    drop((root, server));
}

#[test]
fn team_create_succeeds_with_mock_registry() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    let registry = format!("{}/", server.url());

    let mock = server
        .mock("PUT", "/-/org/myscope/team")
        .match_header("authorization", "Bearer fake-token")
        .with_status(201)
        .create();

    let auth_file = root.path().join("auth-npmrc");
    let registry_no_scheme = server.url().replace("http://", "");
    fs::write(&auth_file, format!("//{registry_no_scheme}/:_authToken=fake-token\n"))
        .expect("write auth .npmrc");

    let output = run_team(&workspace, &auth_file, &registry, &["create", "@myscope:myteam"]);

    mock.assert();
    assert!(
        output.status.success(),
        "team create must succeed (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("+myscope:myteam"));
    drop((root, server));
}
