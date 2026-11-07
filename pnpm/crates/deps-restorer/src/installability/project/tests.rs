use super::project_host_requirements;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn a_manifest_without_host_requirements_requires_nothing() {
    let manifest = json!({ "name": "app", "engines": { "vscode": "^1.80.0" }, "os": [] });
    assert!(project_host_requirements(&manifest, None).is_none());
}

#[test]
fn a_manifest_requires_its_engines_and_platforms() {
    let manifest = json!({
        "name": "app",
        "engines": { "node": ">=20", "vscode": "^1.80.0" },
        "os": "linux",
        "libc": ["glibc"],
    });
    let requirements =
        project_host_requirements(&manifest, None).expect("requirements").installability;
    assert_eq!(requirements.name, "app");
    let engines = requirements.engines.expect("engines");
    assert_eq!(engines.node.as_deref(), Some(">=20"));
    assert_eq!(engines.pnpm, None);
    assert_eq!(requirements.os, Some(vec!["linux".to_string()]));
    assert_eq!(requirements.cpu, None);
    assert_eq!(requirements.libc, Some(vec!["glibc".to_string()]));
}

#[test]
fn a_project_runtime_is_resolved_after_the_runtime_on_fail_override() {
    let manifest = json!({
        "engines": { "node": ">=24" },
        "devEngines": { "runtime": { "name": "node", "version": "24.0.0" } },
    });

    let requirements =
        project_host_requirements(&manifest, Some("download")).expect("requirements");

    assert_eq!(requirements.node_version.as_deref(), Some("24.0.0"));
}

#[test]
fn a_project_runtime_without_download_policy_does_not_replace_the_shared_node_version() {
    let manifest = json!({
        "engines": { "node": ">=24" },
        "devEngines": { "runtime": { "name": "node", "version": "24.0.0" } },
    });

    let requirements = project_host_requirements(&manifest, None).expect("requirements");

    assert_eq!(requirements.node_version, None);
}
