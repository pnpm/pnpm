use std::{fs, path::Path};

use command_extra::CommandExtra;
use mockito::Matcher;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::{Value, json};

use super::{clear_ci, public_pkg, write_registry_npmrc, write_workspace};

mod failures;

#[test]
fn timeout_stops_dependents_and_reports_the_accepted_upload() {
    let CommandTempCwd { pacquet, root: _root, workspace, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    write_workspace(
        &workspace,
        &[
            ("a", public_pkg("a")),
            ("b", json!({"name":"b","version":"1.0.0","dependencies":{"a":"workspace:*"}})),
        ],
    );
    write_registry_npmrc(&workspace, &registry.url());
    let upload = registry
        .mock("PUT", "/a")
        .with_body("{}")
        .expect(1)
        .create();
    let dependent = registry
        .mock("PUT", "/b")
        .expect(0)
        .create();
    let metadata = registry
        .mock("GET", "/a")
        .with_body(r#"{"versions":{}}"#)
        .expect(1)
        .create();
    let output = clear_ci(pacquet)
        .with_args([
            "publish",
            "-r",
            "--force",
            "--no-git-checks",
            "--publish-wait-timeout=100",
            "--report-summary",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_PUBLISH_AVAILABILITY_TIMEOUT"), "{stderr}");
    assert_published_packages(&workspace, &["a"]);
    upload.assert();
    dependent.assert();
    metadata.assert();
}

#[test]
fn already_published_version_still_needs_an_available_tarball() {
    let CommandTempCwd { pacquet, root: _root, workspace, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    write_workspace(&workspace, &[("a", public_pkg("a"))]);
    write_registry_npmrc(&workspace, &registry.url());
    let upload = registry
        .mock("PUT", Matcher::Any)
        .expect(0)
        .create();
    let metadata = registry.mock("GET", "/a").with_body(json!({"name":"a", "dist-tags":{"latest":"1.0.0"}, "versions":{
        "1.0.0":{"name":"a","version":"1.0.0","dist":{"tarball":format!("{}/a.tgz",registry.url())}}
    }}).to_string()).expect(2).create();
    let tarball = registry
        .mock("HEAD", "/a.tgz")
        .with_status(404)
        .expect(1)
        .create();
    let output = clear_ci(pacquet)
        .with_args([
            "publish",
            "-r",
            "--no-git-checks",
            "--publish-wait-timeout=100",
            "--report-summary",
        ])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{output:?}");
    assert!(stderr.contains("ERR_PNPM_PUBLISH_AVAILABILITY_TIMEOUT"), "{stderr}");
    assert_published_packages(&workspace, &[]);
    upload.assert();
    metadata.assert();
    tarball.assert();
}

#[test]
fn batch_timeout_records_every_accepted_package() {
    let CommandTempCwd { pacquet, root: _root, workspace, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    write_workspace(&workspace, &[("a", public_pkg("a")), ("b", public_pkg("b"))]);
    write_registry_npmrc(&workspace, &registry.url());
    let upload = registry
        .mock("PUT", "/-/pnpm/v1/publish")
        .with_body("{}")
        .expect(1)
        .create();
    let metadata_a = registry
        .mock("GET", "/a")
        .with_body(r#"{"versions":{}}"#)
        .expect(1)
        .create();
    let metadata_b = registry
        .mock("GET", "/b")
        .with_body(r#"{"versions":{}}"#)
        .expect(1)
        .create();
    let output = clear_ci(pacquet)
        .with_args([
            "publish",
            "-r",
            "--batch",
            "--force",
            "--no-git-checks",
            "--publish-wait-timeout=100",
            "--report-summary",
        ])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{output:?}");
    assert!(stderr.contains("ERR_PNPM_PUBLISH_AVAILABILITY_TIMEOUT"), "{stderr}");
    assert_published_packages(&workspace, &["a", "b"]);
    upload.assert();
    metadata_a.assert();
    metadata_b.assert();
}

#[test]
fn confirms_a_dependency_before_publishing_its_dependent() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let CommandTempCwd { pacquet, root: _root, workspace, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    write_workspace(
        &workspace,
        &[
            ("a", public_pkg("a")),
            ("b", json!({"name":"b","version":"1.0.0","dependencies":{"a":"1.0.0"}})),
        ],
    );
    link_workspace_packages(&workspace);
    write_registry_npmrc(&workspace, &registry.url());
    let confirmed = Arc::new(AtomicBool::new(false));
    let dependency_confirmed = Arc::clone(&confirmed);
    let upload_a = registry
        .mock("PUT", "/a")
        .with_body("{}")
        .expect(1)
        .create();
    let upload_b = registry
        .mock("PUT", "/b")
        .with_body_from_request(move |_| {
            assert!(
                dependency_confirmed.load(Ordering::SeqCst),
                "dependency must be confirmed before the next upload",
            );
            b"{}".to_vec()
        })
        .expect(1)
        .create();
    let metadata_a = registry
        .mock("GET", "/a")
        .with_body(
            json!({"versions": {"1.0.0": {
                "dist": {"tarball": format!("{}/a.tgz", registry.url())}
            }}})
            .to_string(),
        )
        .expect(1)
        .create();
    let metadata_b = registry
        .mock("GET", "/b")
        .with_body(
            json!({"versions": {"1.0.0": {
                "dist": {"tarball": format!("{}/b.tgz", registry.url())}
            }}})
            .to_string(),
        )
        .expect(1)
        .create();
    let tarball_a = registry
        .mock("HEAD", "/a.tgz")
        .with_header_from_request("content-length", move |_| {
            confirmed.store(true, Ordering::SeqCst);
            "1".to_owned()
        })
        .expect(1)
        .create();
    let tarball_b = registry
        .mock("HEAD", "/b.tgz")
        .expect(1)
        .create();
    let output = clear_ci(pacquet)
        .with_args([
            "publish",
            "-r",
            "--force",
            "--no-git-checks",
            "--publish-wait-timeout=3000",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result.as_array().unwrap().len(), 2);
    upload_a.assert();
    upload_b.assert();
    metadata_a.assert();
    metadata_b.assert();
    tarball_a.assert();
    tarball_b.assert();
}

fn assert_published_packages(workspace: &Path, expected: &[&str]) {
    let report: Value =
        serde_json::from_slice(&fs::read(workspace.join("pnpm-publish-summary.json")).unwrap())
            .unwrap();
    let mut names = report["publishedPackages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|package| package["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(names, expected);
}

fn link_workspace_packages(workspace: &Path) {
    let yaml = workspace.join("pnpm-workspace.yaml");
    fs::write(
        &yaml,
        format!("{}linkWorkspacePackages: true\n", fs::read_to_string(&yaml).unwrap()),
    )
    .unwrap();
}
