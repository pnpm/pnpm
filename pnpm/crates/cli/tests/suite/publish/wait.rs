use std::fs;

use command_extra::CommandExtra;
use mockito::Matcher;
use serde_json::{Value, json};

use super::{assert_success, pacquet, publish, write_project};

#[test]
fn confirms_published_name_and_registry_before_lifecycle_scripts() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = mockito::Server::new();
    let mut default_registry = mockito::Server::new();
    let unused = default_registry
        .mock("GET", Matcher::Any)
        .expect(0)
        .create();
    write_project(
        dir.path(),
        &default_registry.url(),
        &json!({
            "name": "source-name", "version": "1.0.0", "publishConfig": {
                "name": "@scope/published-name", "registry": registry.url(),
            },
            "scripts": {"postpublish": r#"node -e "require('fs').writeFileSync('postpublish-ran', '')""#},
        }),
    );
    let upload = registry
        .mock("PUT", "/@scope%2fpublished-name")
        .with_status(201)
        .with_body("{}")
        .expect(1)
        .create();
    let metadata = registry.mock("GET", "/@scope%2Fpublished-name")
        .with_body(json!({"versions": {"1.0.0": {"dist": {"tarball": format!("{}/pkg.tgz", registry.url())}}}}).to_string())
        .expect(1).create();
    let script_marker = dir.path().join("postpublish-ran");
    let tarball = registry
        .mock("HEAD", "/pkg.tgz")
        .match_header("range", Matcher::Missing)
        .with_status(200)
        .with_header_from_request("content-length", move |_| {
            assert!(!script_marker.exists(), "postpublish must wait for tarball availability");
            "1".to_owned()
        })
        .expect(1)
        .create();
    assert_success(&publish(dir.path(), &["--publish-wait-timeout=3000"]));
    assert!(
        dir.path()
            .join("postpublish-ran")
            .exists(),
        "postpublish must run after confirmation",
    );
    upload.assert();
    metadata.assert();
    tarball.assert();
    unused.assert();
}

#[test]
fn timeout_is_an_error_after_one_upload_and_does_not_run_postpublish() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = mockito::Server::new();
    write_project(
        dir.path(),
        &registry.url(),
        &json!({"name": "pkg", "version": "1.0.0",
            "scripts": {"postpublish": r#"node -e "require('fs').writeFileSync('postpublish-ran', '')""#},
        }),
    );
    let upload = registry
        .mock("PUT", "/pkg")
        .with_status(201)
        .with_body("{}")
        .expect(1)
        .create();
    let metadata = registry
        .mock("GET", "/pkg")
        .with_body(r#"{"versions":{}}"#)
        .expect(1)
        .create();
    let output = publish(dir.path(), &["--publish-wait-timeout=100"]);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{output:?}");
    assert!(stderr.contains("ERR_PNPM_PUBLISH_AVAILABILITY_TIMEOUT"), "{stderr}");
    assert!(stderr.contains("upload was accepted"), "{stderr}");
    assert!(
        !dir.path()
            .join("postpublish-ran")
            .exists(),
        "postpublish must not run on timeout",
    );
    upload.assert();
    metadata.assert();
}

#[test]
fn zero_overrides_configuration_and_dry_run_does_not_check_availability() {
    for dry_run in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = mockito::Server::new();
        write_project(dir.path(), &registry.url(), &json!({"name":"pkg", "version":"1.0.0"}));
        fs::write(dir.path().join("pnpm-workspace.yaml"), "publishWaitTimeout: 3000\n").unwrap();
        let upload = registry
            .mock("PUT", "/pkg")
            .with_body("{}")
            .expect(usize::from(!dry_run))
            .create();
        let read = registry
            .mock("GET", Matcher::Any)
            .expect(0)
            .create();
        let args =
            if dry_run { ["--dry-run", "--json"] } else { ["--publish-wait-timeout=0", "--json"] };
        let output = publish(dir.path(), &args);
        assert_success(&output);
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["name"], "pkg");
        upload.assert();
        read.assert();
    }
}

#[test]
fn workspace_configuration_enables_waiting_and_json_stays_valid() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = mockito::Server::new();
    write_project(dir.path(), &registry.url(), &json!({"name":"pkg", "version":"1.0.0"}));
    fs::write(dir.path().join("pnpm-workspace.yaml"), "publishWaitTimeout: 3000\n").unwrap();
    let upload = registry
        .mock("PUT", "/pkg")
        .with_body("{}")
        .create();
    let metadata = registry.mock("GET", "/pkg")
        .with_body(json!({"versions": {"1.0.0": {"dist": {"tarball": format!("{}/pkg.tgz", registry.url())}}}}).to_string()).create();
    let tarball = registry.mock("HEAD", "/pkg.tgz").create();
    let output = publish(dir.path(), &["--json"]);
    assert_success(&output);
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["name"], "pkg");
    upload.assert();
    metadata.assert();
    tarball.assert();
}

#[test]
fn stage_ignores_configured_wait_defaults() {
    for source in ["workspace", "global", "environment"] {
        let dir = tempfile::tempdir().unwrap();
        let mut registry = mockito::Server::new();
        write_project(dir.path(), &registry.url(), &json!({"name":"pkg", "version":"1.0.0"}));
        let mut command = pacquet(dir.path())
            .with_args(["stage", "publish", "--no-git-checks", "--reporter=silent"]);
        match source {
            "workspace" => {
                fs::write(dir.path().join("pnpm-workspace.yaml"), "publishWaitTimeout: 3000\n")
                    .unwrap();
            }
            "global" => {
                let config_home = dir.path().join("config-home");
                let config_dir = config_home.join("pnpm");
                fs::create_dir_all(&config_dir).unwrap();
                fs::write(config_dir.join("config.yaml"), "publishWaitTimeout: 3000\n").unwrap();
                command.env("XDG_CONFIG_HOME", config_home);
            }
            "environment" => {
                command.env("PNPM_CONFIG_PUBLISH_WAIT_TIMEOUT", "3000");
            }
            _ => unreachable!(),
        }
        let upload = registry
            .mock("POST", "/-/stage/package/pkg")
            .with_status(201)
            .with_body(r#"{"stageId":"1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f"}"#)
            .expect(1)
            .create();
        let metadata = registry
            .mock("GET", Matcher::Any)
            .expect(0)
            .create();
        let tarball = registry
            .mock("HEAD", Matcher::Any)
            .expect(0)
            .create();
        let output = command.output().unwrap();
        assert_success(&output);
        assert_eq!(
            String::from_utf8_lossy(&output.stdout),
            "+ pkg@1.0.0 (staged with id 1de6f3db-2ed9-4d72-b3dd-8f0e2b474a2f)\n",
            "{source}",
        );
        upload.assert();
        metadata.assert();
        tarball.assert();
    }
}

#[test]
fn stage_rejects_wait_before_uploading_or_packing() {
    let dir = tempfile::tempdir().unwrap();
    let output = pacquet(dir.path())
        .with_args(["stage", "publish", "--no-git-checks", "--publish-wait-timeout=3000"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{output:?}");
    assert!(stderr.contains("ERR_PNPM_PUBLISH_WAIT_WITH_STAGE"), "{stderr}");
}

#[test]
fn wait_timeout_requires_a_nonnegative_integer() {
    let dir = tempfile::tempdir().unwrap();
    for value in ["-1", "true", "1.5", "abc"] {
        let output = publish(dir.path(), &[&format!("--publish-wait-timeout={value}")]);
        assert!(!output.status.success(), "must reject {value}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("invalid value"), "{stderr}");
    }
    let output = publish(dir.path(), &["--publish-wait-timeout"]);
    assert!(!output.status.success(), "flag requires a value: {output:?}");
}
