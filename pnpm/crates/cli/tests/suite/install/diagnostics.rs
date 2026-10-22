use crate::_utils::{flatten_report, ndjson_records};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::fs;

#[test]
fn ndjson_reports_fatal_resolution_errors_with_codes_and_prefix() {
    for (specifier, code) in [
        ("99.99.99", "ERR_PNPM_NO_MATCHING_VERSION"),
        ("patch:got@npm%3A11.8.2#~/.yarn/patches/got.patch", "ERR_PNPM_UNSUPPORTED_PROTOCOL"),
    ] {
        let fixture = CommandTempCwd::init().add_mocked_registry();
        fs::write(
            fixture.workspace.join("package.json"),
            serde_json::json!({
                "dependencies": { "@pnpm.e2e/hello-world-js-bin-parent": specifier }
            })
            .to_string(),
        )
        .expect("write package.json");
        let output = fixture.pacquet
            .with_args(["install", "--lockfile-only", "--reporter=ndjson"])
            .assert()
            .failure();
        let stderr = String::from_utf8_lossy(&output.get_output().stderr);
        eprintln!("stderr={stderr}");
        let records: Vec<serde_json::Value> = stderr
            .lines()
            .map(|line| serde_json::from_str(line).expect("every stderr line must be JSON"))
            .collect();
        let errors: Vec<_> = records
            .iter()
            .filter(|record| record["level"] == "error")
            .collect();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0]["name"], "pnpm");
        assert_eq!(errors[0]["code"], code);
        let prefix = dunce::canonicalize(&fixture.workspace).expect("canonicalize project path");
        assert_eq!(errors[0]["prefix"], prefix.display().to_string());
        assert!(
            errors[0]["message"]
                .as_str()
                .is_some_and(|message| !message.is_empty()),
        );
    }
}

#[test]
fn resolution_errors_name_transitive_dependencies_and_their_parents() {
    for reporter in ["--reporter=append-only", "--reporter=ndjson"] {
        let fixture = CommandTempCwd::init().add_mocked_registry();
        fs::write(
            fixture.workspace.join("package.json"),
            serde_json::json!({
                "dependencies": { "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" }
            })
            .to_string(),
        )
        .expect("write package.json");
        fs::write(
            fixture.workspace.join(".pnpmfile.cjs"),
            r"
module.exports = { hooks: { readPackage(pkg) {
    if (pkg.name === '@pnpm.e2e/hello-world-js-bin') {
        pkg.dependencies = { 'missing-child': 'patch:got@npm%3A11.8.2#~/.yarn/patches/got.patch' }
    }
    return pkg
} } }
",
        )
        .expect("write pnpmfile");
        let output = fixture.pacquet
            .with_args(["install", "--lockfile-only", reporter])
            .assert()
            .failure();
        let stderr = String::from_utf8_lossy(&output.get_output().stderr);
        eprintln!("stderr={stderr}");
        assert!(stderr.contains("ERR_PNPM_UNSUPPORTED_PROTOCOL"));
        assert!(
            flatten_report(&stderr)
                .contains("missing-child@patch:got@npm%3A11.8.2#~/.yarn/patches/got.patch"),
        );
        if reporter == "--reporter=ndjson" {
            let records = ndjson_records(output.get_output());
            let error = records
                .iter()
                .find(|record| record["level"] == "error")
                .expect("fatal error");
            let parents = error["pkgsStack"].as_array().expect("parent chain");
            assert_eq!(parents.len(), 2);
            assert_eq!(parents[0]["name"], "@pnpm.e2e/hello-world-js-bin-parent");
            assert_eq!(parents[1]["name"], "@pnpm.e2e/hello-world-js-bin");
            assert_eq!(parents[0]["version"], "1.0.0");
        } else {
            let flattened = flatten_report(&stderr);
            assert!(flattened.contains("Thiserrorhappenedwhileinstallingthedependenciesof@pnpm.e2e/hello-world-js-bin-parent@1.0.0"));
            assert!(flattened.contains("Thiserrorhappenedwhileinstallingthedependenciesof@pnpm.e2e/hello-world-js-bin@1.0.0"));
        }
    }
}

#[test]
fn fatal_resolution_context_redacts_tarball_credentials() {
    for reporter in ["--reporter=append-only", "--reporter=ndjson"] {
        let fixture = CommandTempCwd::init().add_mocked_registry();
        fs::write(fixture.workspace.join("package.json"), serde_json::json!({
            "dependencies": { "secret-tarball": "https://alice:credential-marker@127.0.0.1:9/package.tgz" }
        }).to_string()).expect("write package.json");
        let output = fixture.pacquet
            .with_args(["install", "--lockfile-only", "--config.fetch-retries=0", reporter])
            .assert()
            .failure();
        let stderr = String::from_utf8_lossy(&output.get_output().stderr);
        eprintln!("stderr={stderr}");
        let message = if reporter == "--reporter=ndjson" {
            ndjson_records(output.get_output())
                .into_iter()
                .find(|record| record["level"] == "error")
                .expect("fatal error")["message"]
                .as_str()
                .expect("error message")
                .to_string()
        } else {
            stderr.to_string()
        };
        assert!(!message.contains("credential-marker"), "credentials leaked: {message}");
        assert!(!message.contains("alice:"), "username leaked: {message}");
        assert!(
            flatten_report(&message)
                .contains("Failedtoresolvesecret-tarball@https://127.0.0.1:9/package.tgz"),
        );
    }
}

#[test]
fn ndjson_reports_early_directory_errors() {
    let fixture = CommandTempCwd::init();
    let missing = fixture.workspace.join("missing-directory");
    let output = fixture.pacquet
        .with_args(["install", "--reporter=ndjson", "--dir"])
        .with_arg(&missing)
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    eprintln!("stderr={stderr}");
    let records: Vec<serde_json::Value> = stderr
        .lines()
        .map(|line| serde_json::from_str(line).expect("every stderr line must be JSON"))
        .collect();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0]["name"], "pnpm");
    assert_eq!(records[0]["level"], "error");
    assert!(
        records[0]["message"]
            .as_str()
            .expect("error message")
            .contains("missing-directory"),
    );
}
