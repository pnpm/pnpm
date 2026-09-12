use super::{
    DependencyGroup, PackageManifest, assert_eq, convert_engines_runtime_to_dependencies, json,
    read_to_string, tempdir,
};

#[test]
fn convert_engines_runtime_only_reifies_onfail_download() {
    for on_fail in ["warn", "error", "ignore"] {
        let mut manifest = json!({
            "devEngines": {
                "runtime": {
                    "name": "node",
                    "version": "24.6.0",
                    "onFail": on_fail,
                },
            },
        });
        convert_engines_runtime_to_dependencies(&mut manifest, "devEngines", "devDependencies");
        assert!(
            manifest.get("devDependencies").is_none(),
            "onFail={on_fail} should not reify; manifest: {manifest}",
        );
    }
}

#[test]
fn convert_engines_runtime_preserves_explicit_user_dep() {
    let mut manifest = json!({
        "devDependencies": {
            "node": "23.0.0",
        },
        "devEngines": {
            "runtime": {
                "name": "node",
                "version": "24.6.0",
                "onFail": "download",
            },
        },
    });
    convert_engines_runtime_to_dependencies(&mut manifest, "devEngines", "devDependencies");
    assert_eq!(
        manifest.get("devDependencies").and_then(|d| d.get("node")).and_then(|v| v.as_str()),
        Some("23.0.0"),
    );
}

#[test]
fn convert_engines_runtime_handles_array_form_with_multiple_runtimes() {
    let mut manifest = json!({
        "devEngines": {
            "runtime": [
                { "name": "node", "version": "24.6.0", "onFail": "download" },
                { "name": "bun", "version": "1.1.40", "onFail": "download" },
            ],
        },
    });
    convert_engines_runtime_to_dependencies(&mut manifest, "devEngines", "devDependencies");
    let dev = manifest.get("devDependencies").expect("devDependencies inserted");
    assert_eq!(dev.get("node").and_then(|v| v.as_str()), Some("runtime:24.6.0"));
    assert_eq!(dev.get("bun").and_then(|v| v.as_str()), Some("runtime:1.1.40"));
}

#[test]
fn save_prunes_removed_reified_runtime_entry() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let raw = json!({
        "name": "fixture",
        "devEngines": {
            "runtime": {
                "name": "node",
                "version": "22",
                "onFail": "download",
            },
        },
    });
    std::fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();

    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.remove_dependencies(&["node".to_string()], Some(DependencyGroup::Dev));
    manifest.save().unwrap();

    let saved: serde_json::Value =
        serde_json::from_str(&read_to_string(path).unwrap()).expect("parse saved manifest");
    assert_eq!(saved.get("devEngines"), Some(&json!({})));
    assert_eq!(saved.get("devDependencies"), None);
}

#[test]
fn save_prunes_only_removed_reified_runtime_entry_from_array() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let raw = json!({
        "name": "fixture",
        "devEngines": {
            "runtime": [
                {
                    "name": "node",
                    "version": "22",
                    "onFail": "download",
                },
                {
                    "name": "deno",
                    "version": "2",
                    "onFail": "download",
                },
            ],
        },
    });
    std::fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();

    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.remove_dependencies(&["node".to_string()], Some(DependencyGroup::Dev));
    manifest.save().unwrap();

    let saved: serde_json::Value =
        serde_json::from_str(&read_to_string(path).unwrap()).expect("parse saved manifest");
    assert_eq!(
        saved.get("devEngines"),
        Some(&json!({
            "runtime": [
                {
                    "name": "deno",
                    "version": "2",
                    "onFail": "download",
                },
            ],
        })),
    );
    assert_eq!(saved.get("devDependencies"), None);
}

/// Reading a manifest with `devEngines.runtime` set must apply the
/// reification automatically — that's the hook upstream wires into
/// `convertManifestAfterRead`. Verifies the `from_path` end of the
/// pipeline, not just the standalone function.
#[test]
fn from_path_applies_convert_engines_runtime() {
    let dir = tempdir().unwrap();
    let manifest_path = dir.path().join("package.json");
    let raw = json!({
        "name": "fixture",
        "devEngines": {
            "runtime": {
                "name": "node",
                "version": "24.6.0",
                "onFail": "download",
            },
        },
    });
    std::fs::write(&manifest_path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();

    let manifest = PackageManifest::from_path(manifest_path).unwrap();
    let node_spec = manifest
        .value()
        .get("devDependencies")
        .and_then(|d| d.get("node"))
        .and_then(|v| v.as_str());
    assert_eq!(node_spec, Some("runtime:24.6.0"));
}

#[test]
fn convert_engines_ignores_non_array_non_object_runtime_entries() {
    let mut manifest = json!({
        "name": "x",
        "version": "1.0.0",
        "devEngines": { "runtime": "not-supported" },
    });
    let before = manifest.clone();
    convert_engines_runtime_to_dependencies(&mut manifest, "devEngines", "devDependencies");
    assert_eq!(manifest, before, "manifest must be unchanged for unsupported `runtime` shape");
}
