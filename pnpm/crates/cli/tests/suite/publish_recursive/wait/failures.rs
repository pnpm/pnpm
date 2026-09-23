use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;

use super::{
    assert_published_packages, clear_ci, link_workspace_packages, public_pkg, write_registry_npmrc,
    write_workspace,
};

#[test]
fn batch_rejection_keeps_uploads_accepted_by_an_earlier_registry() {
    let CommandTempCwd { pacquet, root: _root, workspace, .. } = CommandTempCwd::init();
    let mut first_registry = mockito::Server::new();
    let mut second_registry = mockito::Server::new();
    write_workspace(
        &workspace,
        &[
            ("a", public_pkg("a")),
            ("b", json!({"name":"b","version":"1.0.0","dependencies":{"a":"1.0.0"}})),
            (
                "c",
                json!({"name":"c","version":"1.0.0","dependencies":{"b":"1.0.0"},
                "publishConfig":{"registry":second_registry.url()}}),
            ),
        ],
    );
    link_workspace_packages(&workspace);
    write_registry_npmrc(&workspace, &first_registry.url());
    let accepted = first_registry
        .mock("PUT", "/-/pnpm/v1/publish")
        .with_body("{}")
        .expect(1)
        .create();
    let available = mock_available_packages(&mut first_registry, &["a", "b"]);
    let rejected = second_registry
        .mock("PUT", "/-/pnpm/v1/publish")
        .with_status(403)
        .with_body(r#"{"error":"registry rejected upload"}"#)
        .expect(1)
        .create();
    let output = clear_ci(pacquet)
        .with_args([
            "publish",
            "-r",
            "--batch",
            "--force",
            "--no-git-checks",
            "--publish-wait-timeout=3000",
            "--report-summary",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("registry rejected upload"), "{stderr}");
    assert_published_packages(&workspace, &["a", "b"]);
    accepted.assert();
    rejected.assert();
    for mock in available {
        mock.assert();
    }
}

#[test]
fn publish_script_failure_keeps_sequential_uploads() {
    assert_lifecycle_failure_summary(false, "publish");
}

#[test]
fn postpublish_script_failure_keeps_sequential_uploads() {
    assert_lifecycle_failure_summary(false, "postpublish");
}

#[test]
fn publish_script_failure_keeps_all_batch_uploads() {
    assert_lifecycle_failure_summary(true, "publish");
}

#[test]
fn postpublish_script_failure_keeps_all_batch_uploads() {
    assert_lifecycle_failure_summary(true, "postpublish");
}

fn assert_lifecycle_failure_summary(batch: bool, hook: &str) {
    let CommandTempCwd { pacquet, root: _root, workspace, .. } = CommandTempCwd::init();
    let mut registry = mockito::Server::new();
    let mut failing = json!({"name":"b","version":"1.0.0","dependencies":{"a":"1.0.0"}});
    failing["scripts"] = json!({hook: r#"node -e "process.exit(7)""#});
    write_workspace(
        &workspace,
        &[
            ("a", public_pkg("a")),
            ("b", failing),
            ("c", json!({"name":"c","version":"1.0.0","dependencies":{"b":"1.0.0"}})),
        ],
    );
    link_workspace_packages(&workspace);
    write_registry_npmrc(&workspace, &registry.url());
    let accepted_names: &[&str] = if batch { &["a", "b", "c"] } else { &["a", "b"] };
    let endpoints: &[&str] = if batch { &["/-/pnpm/v1/publish"] } else { &["/a", "/b"] };
    let uploads = endpoints
        .iter()
        .map(|endpoint| {
            registry
                .mock("PUT", *endpoint)
                .with_body("{}")
                .expect(1)
                .create()
        })
        .collect::<Vec<_>>();
    let dependent = registry
        .mock("PUT", "/c")
        .expect(0)
        .create();
    let available = mock_available_packages(&mut registry, accepted_names);
    let mut command = clear_ci(pacquet)
        .with_args([
            "publish",
            "-r",
            "--force",
            "--no-git-checks",
            "--publish-wait-timeout=3000",
            "--report-summary",
        ]);
    if batch {
        command.arg("--batch");
    }
    let output = command.output().unwrap();
    assert!(!output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ERR_PNPM_EXECUTOR_LIFECYCLE_SCRIPT_FAILED"), "{stderr}");
    assert_published_packages(&workspace, accepted_names);
    dependent.assert();
    for mock in uploads.into_iter().chain(available) {
        mock.assert();
    }
}

fn mock_available_packages(
    registry: &mut mockito::ServerGuard,
    names: &[&str],
) -> Vec<mockito::Mock> {
    names
        .iter()
        .flat_map(|name| {
            let metadata = registry
                .mock("GET", format!("/{name}").as_str())
                .with_body(
                    json!({"versions":{"1.0.0":{
                        "dist":{"tarball":format!("{}/{name}.tgz",registry.url())}
                    }}})
                    .to_string(),
                )
                .expect(1)
                .create();
            let tarball = registry
                .mock("HEAD", format!("/{name}.tgz").as_str())
                .expect(1)
                .create();
            [metadata, tarball]
        })
        .collect()
}
