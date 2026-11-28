//! `pack <spec>`: packing a positional package spec (a registry package,
//! a git repository, a tarball URL, a local directory, or a local tarball
//! file) instead of the project in the working directory. Registry tests
//! run against the mocked registry, URL tests against a `mockito` server,
//! and git tests against a local repository over `git+file://`.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils_registry::{
    bin::CommandTempCwd,
    fixtures::minimal_tarball,
    git_repo::GitRepoFixture,
};
use serde_json::{
    Value,
    json,
};
use std::{
    fs,
    path::Path,
};

#[test]
fn packs_a_registry_package_spec() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let output = pacquet
        .with_args(["pack", "@pnpm.e2e/foo@1.0.0", "--json"])
        .assert()
        .success();

    let tarball = workspace.join("pnpm.e2e-foo-1.0.0.tgz");
    assert!(tarball.exists(), "the downloaded tarball lands in the working directory");
    let result: Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.get_output().stdout)).unwrap();
    assert_eq!(result["name"], "@pnpm.e2e/foo");
    assert_eq!(result["version"], "1.0.0");
    assert_eq!(result["filename"], "pnpm.e2e-foo-1.0.0.tgz");
    assert!(
        result["integrity"]
            .as_str()
            .is_some_and(|i| i.starts_with("sha512-")),
    );
    assert!(result["size"].as_u64().unwrap() > 0);
    assert!(
        result["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["path"] == "package.json"),
    );
    assert_eq!(result["manifest"]["name"], "@pnpm.e2e/foo");

    drop(root);
}

#[test]
fn packs_a_registry_range_spec() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();

    pacquet
        .with_args(["pack", "@pnpm.e2e/foo@^1", "--json"])
        .assert()
        .success();

    assert!(
        workspace.join("pnpm.e2e-foo-1.3.0.tgz").exists(),
        "the range resolves to the highest matching fixture version",
    );

    drop(root);
}

#[test]
fn packs_a_tarball_url_without_a_tgz_suffix() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let tarball = minimal_tarball("vite", "0.0.0-pr-24234");
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/vite@24234")
        .with_status(200)
        .with_body(tarball.clone())
        .create();
    let url = format!("{}/vite@24234", server.url());

    pacquet
        .with_args(["pack", &url])
        .assert()
        .success();

    mock.assert();
    let written = workspace.join("vite-0.0.0-pr-24234.tgz");
    assert!(written.exists(), "the tarball is named from its own manifest");
    assert_eq!(fs::read(&written).unwrap(), tarball, "the bytes are written through");

    drop((root, server));
}

#[test]
fn registry_spec_fails_on_integrity_mismatch() {
    // The SHA-512 of the empty string: well-formed, but not the tarball's.
    let wrong_integrity = "sha512-z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg==";
    assert_registry_pack_rejects_dist(
        |tarball_url| json!({ "tarball": tarball_url, "integrity": wrong_integrity }),
    );
}

#[test]
fn registry_spec_fails_on_a_legacy_shasum_mismatch() {
    // The SHA-1 of the empty string: well-formed, but not the tarball's.
    let wrong_shasum = "da39a3ee5e6b4b0d3255bfef95601890afd80709";
    assert_registry_pack_rejects_dist(
        |tarball_url| json!({ "tarball": tarball_url, "shasum": wrong_shasum }),
    );
}

/// Serve `foo@1.0.0` from a mock registry with the `dist` that `make_dist`
/// builds from the tarball URL, and assert `pack foo@1.0.0` fails the
/// integrity check without writing the tarball.
fn assert_registry_pack_rejects_dist(make_dist: impl FnOnce(&str) -> Value) {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let auth_file = root.path().join("auth-npmrc");
    fs::write(&auth_file, "").unwrap();
    let mut server = mockito::Server::new();
    fs::write(workspace.join(".npmrc"), format!("registry={}/\n", server.url())).unwrap();
    let tarball = minimal_tarball("foo", "1.0.0");
    let tarball_url = format!("{}/foo/-/foo-1.0.0.tgz", server.url());
    let packument = json!({
        "name": "foo",
        "dist-tags": { "latest": "1.0.0" },
        "versions": {
            "1.0.0": { "name": "foo", "version": "1.0.0", "dist": make_dist(&tarball_url) }
        },
    })
    .to_string();
    server
        .mock("GET", "/foo")
        .with_status(200)
        .with_body(packument)
        .create();
    server
        .mock("GET", "/foo/-/foo-1.0.0.tgz")
        .with_status(200)
        .with_body(tarball)
        .create();

    let output = pacquet_at(&workspace)
        .with_arg("--npmrc-auth-file")
        .with_arg(&auth_file)
        .with_args(["pack", "foo@1.0.0"])
        .output()
        .expect("spawn pacquet pack");

    assert!(!output.status.success(), "an integrity mismatch must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("ERR_PNPM_TARBALL_INTEGRITY"),
        "stderr must name the integrity diagnostic; got:\n{stderr}",
    );
    assert!(!workspace.join("foo-1.0.0.tgz").exists(), "nothing written on mismatch");

    drop((root, server));
}

#[test]
fn packs_a_local_directory_spec_into_the_working_directory() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let sub = workspace.join("sub");
    fs::create_dir(&sub).unwrap();
    fs::write(
        sub.join("package.json"),
        json!({ "name": "sub-pkg", "version": "1.0.0" }).to_string(),
    )
    .unwrap();
    fs::write(sub.join("index.js"), "console.log()\n").unwrap();

    pacquet
        .with_args(["pack", "./sub"])
        .assert()
        .success();

    assert!(
        workspace.join("sub-pkg-1.0.0.tgz").exists(),
        "the tarball lands in the working directory, not the packed one",
    );
    assert!(!sub.join("sub-pkg-1.0.0.tgz").exists());

    drop(root);
}

#[test]
fn packs_a_local_tarball_file_spec() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let tarball = minimal_tarball("local-pkg", "2.3.4");
    fs::write(workspace.join("input.tgz"), &tarball).unwrap();

    pacquet
        .with_args(["pack", "./input.tgz", "--pack-destination", "out", "--json"])
        .assert()
        .success();

    let written = workspace.join("out/local-pkg-2.3.4.tgz");
    assert!(written.exists(), "the tarball is renamed from its manifest");
    assert_eq!(fs::read(&written).unwrap(), tarball);

    drop(root);
}

#[test]
fn packs_multiple_specs_as_a_json_array() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("a.tgz"), minimal_tarball("pkg-a", "1.0.0")).unwrap();
    fs::write(workspace.join("b.tgz"), minimal_tarball("pkg-b", "2.0.0")).unwrap();

    let output = pacquet
        .with_args(["pack", "./a.tgz", "./b.tgz", "--json"])
        .assert()
        .success();

    let results: Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.get_output().stdout)).unwrap();
    let results = results.as_array().expect("multiple specs report a JSON array");
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["name"], "pkg-a");
    assert_eq!(results[1]["name"], "pkg-b");
    assert!(workspace.join("pkg-a-1.0.0.tgz").exists());
    assert!(workspace.join("pkg-b-2.0.0.tgz").exists());

    drop(root);
}

#[test]
fn dry_run_reports_a_url_spec_without_writing() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/pkg.tgz")
        .with_status(200)
        .with_body(minimal_tarball("dry-pkg", "1.0.0"))
        .create();
    let url = format!("{}/pkg.tgz", server.url());

    let output = pacquet
        .with_args(["pack", &url, "--dry-run", "--json"])
        .assert()
        .success();

    let result: Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.get_output().stdout)).unwrap();
    assert_eq!(result["name"], "dry-pkg");
    assert!(result.get("integrity").is_none(), "a dry run reports no digest");
    assert!(!workspace.join("dry-pkg-1.0.0.tgz").exists(), "a dry run writes nothing");

    drop((root, server));
}

#[test]
fn packs_a_git_spec() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "git-pkg");
    repo.write_file("package.json", r#"{"name":"git-pkg","version":"1.0.0"}"#);
    repo.write_file("index.js", "module.exports = true\n");
    let commit = repo.commit("init");

    let output = pacquet
        .with_args(["pack", &repo.git_url_at(&commit), "--json"])
        .assert()
        .success();

    let result: Value =
        serde_json::from_str(&String::from_utf8_lossy(&output.get_output().stdout)).unwrap();
    assert_eq!(result["name"], "git-pkg");
    let entries = tarball_entry_names(&workspace.join("git-pkg-1.0.0.tgz"));
    assert!(entries.contains(&"package/index.js".to_string()), "{entries:?}");
    assert!(
        !entries
            .iter()
            .any(|entry| entry.contains(".git/")),
        "the checkout's git metadata is not packed: {entries:?}",
    );

    drop(root);
}

#[test]
fn packs_a_git_spec_subdirectory() {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "monorepo");
    repo.write_file("package.json", r#"{"name":"monorepo-root","version":"0.0.0"}"#);
    repo.write_file("packages/sdk/package.json", r#"{"name":"sdk","version":"2.0.0"}"#);
    repo.write_file("packages/sdk/index.js", "module.exports = true\n");
    let commit = repo.commit("init");
    let spec = format!("{}&path:/packages/sdk", repo.git_url_at(&commit));

    pacquet
        .with_args(["pack", &spec])
        .assert()
        .success();

    assert!(workspace.join("sdk-2.0.0.tgz").exists(), "the sub-directory's package is packed");
    assert!(!workspace.join("monorepo-root-0.0.0.tgz").exists());

    drop(root);
}

#[cfg(unix)]
#[test]
fn a_git_spec_runs_its_prepare_script() {
    let (entries, root) = pack_git_repo_with_prepare_script(&[]);
    assert!(
        entries.contains(&"package/built.js".to_string()),
        "`prepare` builds the named package without an `allowBuilds` entry: {entries:?}",
    );
    drop(root);
}

#[cfg(unix)]
#[test]
fn ignore_scripts_skips_a_git_spec_prepare_script() {
    let (entries, root) = pack_git_repo_with_prepare_script(&["--ignore-scripts"]);
    assert!(!entries.contains(&"package/built.js".to_string()), "{entries:?}");
    drop(root);
}

/// Pack a git repo whose `prepare` script writes `built.js`, and return the
/// tarball's entry names.
#[cfg(unix)]
fn pack_git_repo_with_prepare_script(extra_args: &[&str]) -> (Vec<String>, tempfile::TempDir) {
    let CommandTempCwd { pacquet, root, workspace, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let repo = GitRepoFixture::init(root.path(), "built-pkg");
    repo.write_file(
        "package.json",
        &json!({
            "name": "built-pkg",
            "version": "1.0.0",
            "scripts": { "prepare": r#"node -e "require('fs').writeFileSync('built.js', '')""# },
        })
        .to_string(),
    );
    let commit = repo.commit("init");

    pacquet
        .with_args(["pack", &repo.git_url_at(&commit)])
        .with_args(extra_args)
        .assert()
        .success();

    (tarball_entry_names(&workspace.join("built-pkg-1.0.0.tgz")), root)
}

#[test]
fn rejects_a_missing_local_path() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_args(["pack", "./no-such-dir"])
        .output()
        .expect("spawn pacquet pack");

    assert!(!output.status.success(), "a missing local path must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no-such-dir"), "stderr must name the path; got:\n{stderr}");

    drop(root);
}

#[test]
fn rejects_recursive_with_a_spec() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_args(["-r", "pack", "./whatever"])
        .output()
        .expect("spawn pacquet pack");

    assert!(!output.status.success(), "--recursive with a spec must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--recursive"),
        "stderr must name the conflicting option; got:\n{stderr}",
    );

    drop(root);
}

#[test]
fn json_error_names_the_underlying_failure() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_args(["pack", "./no-such-dir", "--json"])
        .output()
        .expect("spawn pacquet pack");

    assert!(!output.status.success(), "a missing local path must fail");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let error: Value = serde_json::from_str(&stdout).expect("a JSON error on stdout");
    let message = error["error"]["message"].as_str().expect("error.message");
    assert!(
        message.contains("no file or directory"),
        "the message carries the underlying failure, not just the spec; got: {message}",
    );
    assert_eq!(error["error"]["code"], "ERR_PNPM_PACK_SPEC_NOT_FOUND");

    drop(root);
}

#[test]
fn packing_a_directory_outside_the_workspace_inherits_no_workspace_license() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - '.'\n").unwrap();
    fs::write(workspace.join("LICENSE"), "workspace license\n").unwrap();
    let external = root.path().join("external");
    fs::create_dir(&external).unwrap();
    fs::write(
        external.join("package.json"),
        json!({ "name": "external-pkg", "version": "1.0.0" }).to_string(),
    )
    .unwrap();
    fs::write(external.join("index.js"), "console.log()\n").unwrap();

    pacquet
        .with_args(["pack", "../external"])
        .assert()
        .success();

    let entries = tarball_entry_names(&workspace.join("external-pkg-1.0.0.tgz"));
    assert!(
        !entries
            .iter()
            .any(|entry| entry == "package/LICENSE"),
        "the calling workspace's LICENSE must not leak into another project's tarball: {entries:?}",
    );

    drop(root);
}

#[test]
fn a_failing_url_spec_redacts_credentials_from_the_error() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/pkg.tgz")
        .with_status(404)
        .create();
    let url = format!("{}/pkg.tgz", server.url()).replacen("http://", "http://user:secret@", 1);

    let output = pacquet_at(&workspace)
        .with_args(["pack", &url])
        .output()
        .expect("spawn pacquet pack");

    assert!(!output.status.success(), "a 404 download must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("secret"), "credentials are redacted; got:\n{stderr}");

    drop((root, server));
}

#[test]
fn a_git_spec_error_redacts_credentials() {
    let CommandTempCwd { pacquet, root, .. } = CommandTempCwd::init();

    let output = pacquet
        .with_args(["pack", "git+https://user:secret@example.test/repo.git"])
        .output()
        .expect("spawn pacquet pack");

    assert!(!output.status.success(), "an unreachable git repo must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("secret"), "credentials are redacted; got:\n{stderr}");

    drop(root);
}

#[test]
fn a_url_spec_error_omits_the_query_string() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let mut server = mockito::Server::new();
    server
        .mock("GET", "/pkg.tgz")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_body("not a tarball")
        .create();
    let url = format!("{}/pkg.tgz?token=secret", server.url());

    let output = pacquet_at(&workspace)
        .with_args(["pack", &url])
        .output()
        .expect("spawn pacquet pack");

    assert!(!output.status.success(), "an unreadable tarball must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("pkg.tgz"), "stderr must name the source; got:\n{stderr}");
    assert!(!stderr.contains("secret"), "the query string is dropped; got:\n{stderr}");

    drop((root, server));
}

#[test]
fn packing_a_directory_outside_the_workspace_ignores_workspace_package_versions() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - '.'\n  - 'bar'\n").unwrap();
    fs::create_dir(workspace.join("bar")).unwrap();
    fs::write(
        workspace.join("bar/package.json"),
        json!({ "name": "bar", "version": "2.0.0" }).to_string(),
    )
    .unwrap();
    let external = root.path().join("external");
    fs::create_dir(&external).unwrap();
    fs::write(
        external.join("package.json"),
        json!({
            "name": "external-pkg",
            "version": "1.0.0",
            "dependencies": { "bar": "workspace:*" },
        })
        .to_string(),
    )
    .unwrap();

    let output = pacquet
        .with_args(["pack", "../external", "--json"])
        .output()
        .expect("spawn pacquet pack");

    assert!(
        !output.status.success(),
        "the calling workspace's `bar@2.0.0` must not resolve the external `workspace:*`",
    );
    let error: Value = serde_json::from_slice(&output.stdout).expect("a JSON error on stdout");
    assert_eq!(error["error"]["code"], "ERR_PNPM_CANNOT_RESOLVE_WORKSPACE_PROTOCOL");
    assert!(!workspace.join("external-pkg-1.0.0.tgz").exists());

    drop(root);
}

fn tarball_entry_names(tarball: &Path) -> Vec<String> {
    let bytes = fs::read(tarball).expect("read tarball");
    let decoder = flate2::read::GzDecoder::new(bytes.as_slice());
    let mut archive = tar::Archive::new(decoder);
    archive
        .entries()
        .expect("iterate tarball entries")
        .map(|entry| {
            entry
                .expect("read tarball entry")
                .path()
                .expect("entry path")
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

fn pacquet_at(workspace: &Path) -> std::process::Command {
    std::process::Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
}
