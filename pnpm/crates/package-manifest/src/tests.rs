use std::{collections::HashMap, fs::read_to_string};

use insta::assert_snapshot;
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use tempfile::{NamedTempFile, tempdir};

#[cfg(unix)]
use super::safe_read_package_json_from_dir;
use super::{
    BundleDependencies, PackageManifest, PackageManifestError, apply_runtime_on_fail_override,
    convert_dependencies_to_engines_runtime, convert_engines_runtime_to_dependencies,
    node_version_from_engines_runtime,
};
use crate::DependencyGroup;
use serde_json::json;
use std::io::Write;

#[cfg(unix)]
#[test]
fn save_leaves_the_original_intact_when_the_write_cannot_complete() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let original = r#"{"name":"intact","version":"1.0.0"}"#;
    std::fs::write(&path, original).unwrap();

    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();

    // Make the directory read-only so a sibling temp file cannot be created.
    // An atomic temp-file-then-rename write fails up front and leaves the
    // original untouched; a non-atomic in-place write would instead truncate
    // the existing package.json before failing.
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = manifest.save();
    // Restore permissions before the assertions so tempdir cleanup succeeds.
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(result.is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn save_preserves_the_existing_package_json_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    std::fs::write(&path, r#"{"name":"perm","version":"1.0.0"}"#).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    // The atomic temp-file-then-rename must keep the original mode, not leave
    // the NamedTempFile's default 0o600 behind.
    let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o640);
}

#[test]
fn test_init_package_json_content() {
    let manifest = PackageManifest::create_init_package_json("test");
    assert_snapshot!(serde_json::to_string_pretty(&manifest).unwrap());
}

#[test]
fn init_should_throw_if_exists() {
    let tmp = NamedTempFile::new().unwrap();
    write!(tmp.as_file(), "hello world").unwrap();
    PackageManifest::init(tmp.path()).expect_err("package.json already exist");
}

#[test]
fn init_should_create_package_json_if_not_exist() {
    let dir = tempdir().unwrap();
    let tmp = dir.path().join("package.json");
    PackageManifest::init(&tmp).unwrap();
    eprintln!("tmp={tmp:?} exists={} is_file={}", tmp.exists(), tmp.is_file());
    assert!(tmp.exists());
    assert!(tmp.is_file());
    assert_eq!(PackageManifest::from_path(tmp.clone()).unwrap().path, tmp);
}

#[test]
fn should_add_dependency() {
    let dir = tempdir().unwrap();
    let tmp = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(tmp.clone()).unwrap();
    manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();

    let dependencies: HashMap<_, _> = manifest.dependencies([DependencyGroup::Prod]).collect();
    dbg!(&dependencies);
    assert!(dependencies.contains_key("fastify"));
    assert_eq!(dependencies.get("fastify").unwrap(), &"1.0.0");
    manifest.save().unwrap();
    let saved = read_to_string(tmp).unwrap();
    eprintln!("SAVED:\n{saved}");
    assert!(saved.contains("fastify"));
}

#[test]
fn should_throw_on_missing_command() {
    let dir = tempdir().unwrap();
    let tmp = dir.path().join("package.json");
    let manifest = PackageManifest::create_if_needed(tmp).unwrap();
    manifest.script("dev", false).expect_err("dev command should not exist");
}

#[test]
fn should_execute_a_command() {
    let data = r#"
    {
        "scripts": {
            "test": "echo"
        }
    }
    "#;
    let tmp = NamedTempFile::new().unwrap();
    write!(tmp.as_file(), "{data}").unwrap();
    let manifest = PackageManifest::create_if_needed(tmp.path().to_path_buf()).unwrap();
    assert_eq!(manifest.script("test", false).unwrap(), Some("echo"));
    manifest.script("invalid", false).expect_err("invalid command should not exist");
    assert_eq!(manifest.script("invalid", true).unwrap(), None);
}

#[test]
fn get_dependencies_should_return_peers() {
    let data = r#"
    {
        "dependencies": {
            "fastify": "1.0.0"
        },
        "peerDependencies": {
            "fast-querystring": "1.0.0"
        }
    }
    "#;
    let tmp = NamedTempFile::new().unwrap();
    write!(tmp.as_file(), "{data}").unwrap();
    let manifest = PackageManifest::create_if_needed(tmp.path().to_path_buf()).unwrap();
    let dependencies = |groups| manifest.dependencies(groups).collect::<HashMap<_, _>>();
    let peer = dependencies([DependencyGroup::Peer]);
    dbg!(&peer);
    assert!(peer.contains_key("fast-querystring"));
    let prod = dependencies([DependencyGroup::Prod]);
    dbg!(&prod);
    assert!(prod.contains_key("fastify"));
}

#[test]
fn bundle_dependencies() {
    fn bundle_list<List>(list: List) -> BundleDependencies
    where
        List: IntoIterator,
        List::Item: Into<String>,
    {
        list.into_iter().map(Into::into).collect::<Vec<_>>().pipe(BundleDependencies::List)
    }

    macro_rules! case {
        ($input:expr => $output:expr) => {{
            let data = $input;
            eprintln!("CASE: {data}");
            let tmp = NamedTempFile::new().unwrap();
            write!(tmp.as_file(), "{}", data).unwrap();
            let manifest = PackageManifest::create_if_needed(tmp.path().to_path_buf()).unwrap();
            let bundle = manifest.bundle_dependencies().unwrap();
            assert_eq!(bundle, $output);
        }};
    }

    case!(r#"{ "bundleDependencies": ["foo", "bar"] }"# => Some(bundle_list(["foo", "bar"])));
    case!(r#"{ "bundledDependencies": ["foo", "bar"] }"# => Some(bundle_list(["foo", "bar"])));
    case!(r#"{ "bundleDependencies": false }"# => false.pipe(BundleDependencies::Boolean).pipe(Some));
    case!(r#"{ "bundledDependencies": false }"# => false.pipe(BundleDependencies::Boolean).pipe(Some));
    case!(r#"{ "bundleDependencies": true }"# => true.pipe(BundleDependencies::Boolean).pipe(Some));
    case!(r#"{ "bundledDependencies": true }"# => true.pipe(BundleDependencies::Boolean).pipe(Some));
    case!(r"{}" => None);
}

#[test]
fn resolve_registry_dependency_passes_through_plain_specs() {
    for (key, spec) in [
        ("foo", "^1.0.0"),
        ("foo", "1.2.3"),
        ("foo", "latest"),
        ("@scope/foo", "^1.0.0"),
        ("foo", "*"),
        ("foo", ">=1 <2"),
    ] {
        assert_eq!(
            PackageManifest::resolve_registry_dependency(key, spec),
            (key, spec),
            "plain spec ({key:?}, {spec:?}) should pass through unchanged",
        );
    }
}

#[test]
fn resolve_registry_dependency_strips_npm_alias_prefix() {
    assert_eq!(
        PackageManifest::resolve_registry_dependency("ansi-strip", "npm:strip-ansi@^6.0.1"),
        ("strip-ansi", "^6.0.1"),
    );
}

#[test]
fn resolve_registry_dependency_handles_scoped_target() {
    assert_eq!(
        PackageManifest::resolve_registry_dependency("react17", "npm:@types/react@^17.0.49"),
        ("@types/react", "^17.0.49"),
    );
}

#[test]
fn resolve_registry_dependency_handles_pinned_version() {
    assert_eq!(
        PackageManifest::resolve_registry_dependency("foo-cjs", "npm:foo@1.2.3"),
        ("foo", "1.2.3"),
    );
}

#[test]
fn resolve_registry_dependency_unversioned_npm_alias_defaults_to_latest() {
    assert_eq!(
        PackageManifest::resolve_registry_dependency("foo-cjs", "npm:foo"),
        ("foo", "latest"),
    );
    assert_eq!(
        PackageManifest::resolve_registry_dependency("react17", "npm:@types/react"),
        ("@types/react", "latest"),
    );
}

#[test]
fn resolve_registry_dependency_picks_last_at_for_alias() {
    assert_eq!(
        PackageManifest::resolve_registry_dependency("foo-rc", "npm:@scope/foo@1.0.0-rc.1",),
        ("@scope/foo", "1.0.0-rc.1"),
    );
}

/// This is the v11 install path: a manifest that declares its node
/// version through `devEngines.runtime` must produce the same flat-
/// record specifier set as the lockfile entry the resolver wrote.
#[test]
fn convert_engines_runtime_lifts_devengines_runtime_into_devdependencies() {
    let mut manifest = json!({
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
        Some("runtime:24.6.0"),
    );
}

#[test]
fn convert_engines_runtime_skips_entries_without_a_version() {
    let mut manifest = json!({
        "devEngines": {
            "runtime": {
                "name": "node",
                "onFail": "download",
            },
        },
    });
    convert_engines_runtime_to_dependencies(&mut manifest, "devEngines", "devDependencies");
    assert!(manifest.get("devDependencies").is_none(), "manifest: {manifest}");
}

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
fn convert_engines_runtime_trims_the_version() {
    for (version, expected) in [("", "runtime:"), ("  ", "runtime:"), (" 22 ", "runtime:22")] {
        let mut manifest = json!({
            "devEngines": {
                "runtime": {
                    "name": "node",
                    "version": version,
                    "onFail": "download",
                },
            },
        });
        convert_engines_runtime_to_dependencies(&mut manifest, "devEngines", "devDependencies");
        assert_eq!(
            manifest.get("devDependencies").and_then(|d| d.get("node")).and_then(|v| v.as_str()),
            Some(expected),
        );
    }
}

#[test]
fn runtime_on_fail_download_reifies_runtime_dependencies() {
    let mut manifest = json!({
        "devEngines": {
            "runtime": { "name": "node", "version": "22.20.0" },
        },
    });
    apply_runtime_on_fail_override(&mut manifest, "download");
    assert_eq!(
        manifest.get("devEngines").and_then(|v| v.get("runtime")).and_then(|v| v.get("onFail")),
        Some(&json!("download")),
    );
    assert_eq!(
        manifest.get("devDependencies").and_then(|v| v.get("node")),
        Some(&json!("runtime:22.20.0")),
    );
}

#[test]
fn runtime_on_fail_ignore_removes_only_synthesized_runtime_dependencies() {
    let mut manifest = json!({
        "devEngines": {
            "runtime": { "name": "node", "version": "22.20.0", "onFail": "download" },
        },
        "devDependencies": {
            "node": "runtime:22.20.0",
            "bun": "1.2.0",
        },
    });
    apply_runtime_on_fail_override(&mut manifest, "ignore");
    assert_eq!(
        manifest.get("devEngines").and_then(|v| v.get("runtime")).and_then(|v| v.get("onFail")),
        Some(&json!("ignore")),
    );
    assert!(manifest.get("devDependencies").and_then(|v| v.get("node")).is_none());
    assert_eq!(manifest.get("devDependencies").and_then(|v| v.get("bun")), Some(&json!("1.2.0")));
}

#[test]
fn node_version_uses_devengines_then_engines_and_returns_the_range_minimum() {
    assert_eq!(
        node_version_from_engines_runtime(&json!({
            "devEngines": {
                "runtime": { "name": "node", "version": "^22.0.0" },
            },
            "engines": {
                "runtime": { "name": "node", "version": "20.0.0" },
            },
        })),
        Some("22.0.0".to_string()),
    );
    assert_eq!(
        node_version_from_engines_runtime(&json!({
            "devEngines": {
                "runtime": { "name": "bun", "version": "1.2.0" },
            },
            "engines": {
                "runtime": [
                    { "name": "deno", "version": "2.0.0" },
                    { "name": "node", "version": "22.20.0" },
                ],
            },
        })),
        Some("22.20.0".to_string()),
    );
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
fn convert_engines_runtime_targets_dependencies_for_engines_field() {
    let mut manifest = json!({
        "engines": {
            "runtime": {
                "name": "node",
                "version": "22.0.0",
                "onFail": "download",
            },
        },
    });
    convert_engines_runtime_to_dependencies(&mut manifest, "engines", "dependencies");
    assert_eq!(
        manifest.get("dependencies").and_then(|d| d.get("node")).and_then(|v| v.as_str()),
        Some("runtime:22.0.0"),
    );
}

#[test]
fn convert_dependencies_runtime_writes_devengines_runtime() {
    let mut manifest = json!({
        "devDependencies": {
            "node": "runtime:22",
        },
    });
    convert_dependencies_to_engines_runtime(&mut manifest, "devDependencies", "devEngines")
        .unwrap();

    assert_eq!(
        manifest.get("devEngines"),
        Some(&json!({
            "runtime": {
                "name": "node",
                "version": "22",
                "onFail": "download",
            },
        })),
    );
    assert_eq!(manifest.get("devDependencies"), Some(&json!({})));
}

#[test]
fn convert_dependencies_runtime_trims_runtime_selector() {
    let mut manifest = json!({
        "devDependencies": {
            "node": "runtime:  ",
        },
    });
    convert_dependencies_to_engines_runtime(&mut manifest, "devDependencies", "devEngines")
        .unwrap();

    assert_eq!(
        manifest.get("devEngines"),
        Some(&json!({
            "runtime": {
                "name": "node",
                "version": "",
                "onFail": "download",
            },
        })),
    );
    assert_eq!(manifest.get("devDependencies"), Some(&json!({})));
}

#[test]
fn convert_dependencies_runtime_updates_existing_single_entry() {
    let mut manifest = json!({
        "devEngines": {
            "runtime": {
                "name": "node",
                "version": "16",
                "onFail": "warn",
            },
        },
        "devDependencies": {
            "node": "runtime:22",
        },
    });
    convert_dependencies_to_engines_runtime(&mut manifest, "devDependencies", "devEngines")
        .unwrap();

    assert_eq!(
        manifest.get("devEngines"),
        Some(&json!({
            "runtime": {
                "name": "node",
                "version": "22",
                "onFail": "download",
            },
        })),
    );
    assert_eq!(manifest.get("devDependencies"), Some(&json!({})));
}

#[test]
fn convert_dependencies_runtime_preserves_other_single_runtime_as_array() {
    let mut manifest = json!({
        "devEngines": {
            "runtime": {
                "name": "deno",
                "version": "1",
            },
        },
        "devDependencies": {
            "node": "runtime:22",
        },
    });
    convert_dependencies_to_engines_runtime(&mut manifest, "devDependencies", "devEngines")
        .unwrap();

    assert_eq!(
        manifest.get("devEngines"),
        Some(&json!({
            "runtime": [
                {
                    "name": "deno",
                    "version": "1",
                },
                {
                    "name": "node",
                    "version": "22",
                    "onFail": "download",
                },
            ],
        })),
    );
    assert_eq!(manifest.get("devDependencies"), Some(&json!({})));
}

#[test]
fn save_converts_runtime_dependencies_before_writing() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(path.clone()).unwrap();
    manifest.add_dependency("node", "runtime:22", DependencyGroup::Dev).unwrap();
    manifest.add_dependency("bun", "runtime:1.2.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let saved: serde_json::Value =
        serde_json::from_str(&read_to_string(path).unwrap()).expect("parse saved manifest");
    assert_eq!(
        saved.get("devEngines"),
        Some(&json!({
            "runtime": {
                "name": "node",
                "version": "22",
                "onFail": "download",
            },
        })),
    );
    assert_eq!(
        saved.get("engines"),
        Some(&json!({
            "runtime": {
                "name": "bun",
                "version": "1.2.0",
                "onFail": "download",
            },
        })),
    );
    assert_eq!(saved.get("devDependencies"), None);
    assert_eq!(saved.get("dependencies"), None);
}

#[test]
fn save_and_get_written_value_returns_saved_manifest() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(path.clone()).unwrap();
    manifest.add_dependency("node", "runtime:22", DependencyGroup::Dev).unwrap();

    let written = manifest.save_and_get_written_value().unwrap();
    let saved: serde_json::Value =
        serde_json::from_str(&read_to_string(path).unwrap()).expect("parse saved manifest");

    assert_eq!(written, saved);
    assert_eq!(
        saved.get("devEngines"),
        Some(&json!({
            "runtime": {
                "name": "node",
                "version": "22",
                "onFail": "download",
            },
        })),
    );
    // The reification-only `devDependencies` ends up empty after the fold
    // and is dropped from the written file, while the in-memory manifest
    // keeps the reified entry.
    assert_eq!(saved.get("devDependencies"), None);
    assert_eq!(manifest.value().get("devDependencies"), Some(&json!({ "node": "runtime:22" })));
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

#[test]
fn failed_save_preserves_existing_file_contents() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let raw = serde_json::to_string_pretty(&json!({
        "name": "fixture",
        "devEngines": "invalid",
        "devDependencies": {
            "node": "runtime:22",
        },
    }))
    .unwrap();
    std::fs::write(&path, &raw).unwrap();

    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    assert!(matches!(manifest.save(), Err(PackageManifestError::InvalidAttribute(_))));
    assert_eq!(read_to_string(path).unwrap(), raw);
}

#[test]
fn failed_save_preserves_existing_file_when_dependency_field_is_malformed() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let raw = serde_json::to_string_pretty(&json!({
        "name": "fixture",
        "devEngines": {
            "runtime": {
                "name": "node",
                "version": "22",
                "onFail": "download",
            },
        },
    }))
    .unwrap();
    std::fs::write(&path, &raw).unwrap();

    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.value_mut()["devDependencies"] = json!([]);
    let err =
        manifest.save().expect_err("malformed devDependencies must reject save before writing");
    match err {
        PackageManifestError::InvalidAttribute(msg) => {
            assert!(msg.contains("devDependencies"), "got: {msg:?}");
        }
        other => panic!("expected InvalidAttribute, got {other:?}"),
    }
    assert_eq!(read_to_string(path).unwrap(), raw);
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
fn from_path_errors_no_importer_when_missing() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("does-not-exist").join("package.json");
    let result = PackageManifest::from_path(missing);
    let Err(err) = result else { panic!("missing package.json should not parse") };
    assert!(
        matches!(err, PackageManifestError::NoImporterManifestFound(_)),
        "expected NoImporterManifestFound, got {err:?}",
    );
}

#[test]
fn add_dependency_errors_when_field_is_not_an_object() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let raw = json!({
        "name": "fixture",
        "version": "1.0.0",
        "dependencies": "not an object",
    });
    std::fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();

    let mut manifest = PackageManifest::from_path(path).unwrap();
    let err = manifest
        .add_dependency("foo", "1.0.0", DependencyGroup::Prod)
        .expect_err("non-object `dependencies` should reject insert");
    match err {
        PackageManifestError::InvalidAttribute(msg) => {
            assert!(msg.contains("dependencies"), "got: {msg:?}");
        }
        other => panic!("expected InvalidAttribute, got {other:?}"),
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "test helper called from multiple sites with owned literals; by-value keeps the call sites clean"
)]
fn manifest_from_json(value: serde_json::Value) -> (PackageManifest, tempfile::TempDir) {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    std::fs::write(&path, value.to_string()).unwrap();
    (PackageManifest::from_path(path).unwrap(), dir)
}

#[test]
fn available_dependency_names_unions_all_fields_without_save_type() {
    let (manifest, _dir) = manifest_from_json(json!({
        "dependencies": { "prod-dep": "1.0.0", "shared": "1.0.0" },
        "devDependencies": { "dev-dep": "1.0.0", "shared": "1.0.0" },
        "optionalDependencies": { "opt-dep": "1.0.0" },
        "peerDependencies": { "peer-dep": "1.0.0" },
    }));
    assert_eq!(
        manifest.available_dependency_names(None),
        vec![
            "dev-dep".to_string(),
            "shared".to_string(),
            "prod-dep".to_string(),
            "opt-dep".to_string(),
        ],
    );
}

#[test]
fn available_dependency_names_restricts_to_save_type() {
    let (manifest, _dir) = manifest_from_json(json!({
        "dependencies": { "prod-dep": "1.0.0" },
        "devDependencies": { "dev-dep": "1.0.0" },
    }));
    assert_eq!(
        manifest.available_dependency_names(Some(DependencyGroup::Dev)),
        vec!["dev-dep".to_string()],
    );
}

/// Mirrors pnpm's `removeDeps`.
#[test]
fn remove_dependencies_clears_all_fields_without_save_type() {
    let (mut manifest, _dir) = manifest_from_json(json!({
        "dependencies": { "foo": "1.0.0", "bar": "1.0.0" },
        "devDependencies": { "foo": "1.0.0" },
        "optionalDependencies": { "foo": "1.0.0" },
        "peerDependencies": { "foo": "1.0.0" },
        "dependenciesMeta": { "foo": { "injected": true } },
    }));
    manifest.remove_dependencies(&["foo".to_string()], None);

    let value = manifest.value();
    for field in ["dependencies", "devDependencies", "optionalDependencies", "peerDependencies"] {
        assert!(
            value.get(field).and_then(|f| f.get("foo")).is_none(),
            "`foo` must be gone from {field}: {value}",
        );
    }
    assert!(value.get("dependenciesMeta").and_then(|m| m.get("foo")).is_none());
    assert!(value.get("dependencies").and_then(|d| d.get("bar")).is_some(), "`bar` must remain");
}

#[test]
fn remove_dependencies_with_save_type_keeps_other_dependency_fields() {
    let (mut manifest, _dir) = manifest_from_json(json!({
        "dependencies": { "foo": "1.0.0" },
        "devDependencies": { "foo": "1.0.0" },
        "peerDependencies": { "foo": "1.0.0" },
    }));
    manifest.remove_dependencies(&["foo".to_string()], Some(DependencyGroup::Dev));

    let value = manifest.value();
    assert!(value.get("devDependencies").and_then(|d| d.get("foo")).is_none());
    assert!(
        value.get("dependencies").and_then(|d| d.get("foo")).is_some(),
        "prod entry must survive a dev-targeted remove: {value}",
    );
    assert!(
        value.get("peerDependencies").and_then(|d| d.get("foo")).is_none(),
        "peer entry is always cleared: {value}",
    );
}

#[cfg(unix)]
#[test]
fn safe_read_surfaces_non_not_found_io_errors() {
    let dir = tempdir().unwrap();
    // Plant `package.json` as a directory rather than a file.
    // `fs::read_to_string` returns `IsADirectory`, never `NotFound`.
    std::fs::create_dir(dir.path().join("package.json")).unwrap();

    let err = safe_read_package_json_from_dir(dir.path())
        .expect_err("read_to_string on a directory should fail");
    assert!(matches!(err, PackageManifestError::Io(_)), "expected Io error, got {err:?}");
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

#[test]
fn from_value_coerces_non_object_input_to_an_empty_object() {
    // A non-object manifest supplied across the FFI boundary must not later
    // panic when a dependency is inserted; `from_value` normalizes it to `{}`.
    for value in [json!([1, 2, 3]), json!("oops"), json!(42), json!(true), json!(null)] {
        let mut manifest = PackageManifest::from_value("/x/package.json".into(), value.clone());
        assert!(manifest.value().is_object(), "input {value} should normalize to an object");
        manifest
            .add_dependency("is-odd", "3.0.1", DependencyGroup::Prod)
            .expect("adding a dependency to a normalized manifest must not fail");
    }
}

/// A save round-trips the source file's final-newline state: a file that
/// ends with a newline keeps it, a file that doesn't stays without one.
#[test]
fn save_preserves_the_final_newline_state_of_the_source_file() {
    let dir = tempdir().unwrap();

    let with_newline = dir.path().join("with-newline.json");
    std::fs::write(&with_newline, "{\n  \"name\": \"foo\"\n}\n").unwrap();
    let mut manifest = PackageManifest::from_path(with_newline.clone()).unwrap();
    manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();
    assert!(read_to_string(with_newline).unwrap().ends_with('\n'));

    let without_newline = dir.path().join("without-newline.json");
    std::fs::write(&without_newline, "{\n  \"name\": \"foo\"\n}").unwrap();
    let mut manifest = PackageManifest::from_path(without_newline.clone()).unwrap();
    manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();
    assert!(!read_to_string(without_newline).unwrap().ends_with('\n'));
}

/// A manifest scaffolded for a project with no `package.json` ends with a
/// newline, both as created and after a save.
#[test]
fn new_manifests_end_with_a_final_newline() {
    let dir = tempdir().unwrap();
    let tmp = dir.path().join("package.json");
    let mut manifest = PackageManifest::create_if_needed(tmp.clone()).unwrap();
    assert!(read_to_string(&tmp).unwrap().ends_with('\n'));
    manifest.save().unwrap();
    assert!(read_to_string(&tmp).unwrap().ends_with('\n'));
}

/// A save re-serializes with the source file's own indentation unit: tabs
/// stay tabs, wider space units stay wide, and a single-line document
/// stays compact.
#[test]
fn save_preserves_the_source_indentation() {
    let dir = tempdir().unwrap();

    let cases = [
        ("tabs.json", "{\n\t\"name\": \"foo\"\n}\n", "\t\"name\""),
        ("wide.json", "{\n    \"name\": \"foo\"\n}\n", r#"    "name""#),
    ];
    for (file_name, source, expected_fragment) in cases {
        let path = dir.path().join(file_name);
        std::fs::write(&path, source).unwrap();
        let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
        manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();
        manifest.save().unwrap();
        let saved = read_to_string(&path).unwrap();
        eprintln!("{file_name} SAVED:\n{saved}");
        assert!(saved.contains(expected_fragment));
        assert!(saved.contains("fastify"));
    }

    let single_line = dir.path().join("single-line.json");
    std::fs::write(&single_line, r#"{"name":"foo"}"#).unwrap();
    let mut manifest = PackageManifest::from_path(single_line.clone()).unwrap();
    manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();
    assert_eq!(
        read_to_string(&single_line).unwrap(),
        r#"{"name":"foo","dependencies":{"fastify":"1.0.0"}}"#,
    );
}

/// The preserved indentation unit is capped at 10 characters on write,
/// like `JSON.stringify`'s `space` argument.
#[test]
fn save_caps_the_indentation_unit_at_ten_characters() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let twelve_spaces = " ".repeat(12);
    std::fs::write(&path, format!("{{\n{twelve_spaces}\"name\": \"foo\"\n}}\n")).unwrap();
    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.add_dependency("fastify", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let saved = read_to_string(&path).unwrap();
    eprintln!("SAVED:\n{saved}");
    assert!(saved.contains(&format!("\n{}\"name\"", " ".repeat(10))));
    assert!(!saved.contains(&format!("\n{twelve_spaces}\"name\"")));
}

/// A write sorts each dependency field by name and drops a dependency
/// field that ended up empty, like pnpm's on-write manifest normalization.
#[test]
fn save_sorts_dependency_fields_and_drops_empty_ones() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    std::fs::write(
        &path,
        "{\n  \"name\": \"foo\",\n  \"dependencies\": {\n    \"zebra\": \"1.0.0\"\n  },\n  \"devDependencies\": {}\n}\n",
    )
    .unwrap();
    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.add_dependency("aardvark", "2.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();

    let saved = read_to_string(&path).unwrap();
    eprintln!("SAVED:\n{saved}");
    let aardvark = saved.find("aardvark").unwrap();
    let zebra = saved.find("zebra").unwrap();
    assert!(aardvark < zebra);
    assert!(!saved.contains("devDependencies"));
}

/// A save that wouldn't change the manifest leaves the file byte-for-byte
/// untouched — even a file in non-canonical form (unsorted dependencies)
/// is only normalized when a real change triggers a write.
#[test]
fn noop_save_does_not_rewrite_the_file() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    let original = "{\n  \"name\": \"foo\",\n  \"dependencies\": {\n    \"zebra\": \"1.0.0\",\n    \"aardvark\": \"2.0.0\"\n  }\n}";
    std::fs::write(&path, original).unwrap();

    let mut manifest = PackageManifest::from_path(path.clone()).unwrap();
    manifest.save().unwrap();
    assert_eq!(read_to_string(&path).unwrap(), original);

    // Re-adding an already-declared dependency at its existing version is
    // also a no-op.
    manifest.add_dependency("zebra", "1.0.0", DependencyGroup::Prod).unwrap();
    manifest.save().unwrap();
    assert_eq!(read_to_string(&path).unwrap(), original);
}
