use super::{
    DependencyGroup, HashMap, NamedTempFile, PackageManifest, PackageManifestError, Write,
    apply_runtime_on_fail_override, assert_eq, convert_dependencies_to_engines_runtime,
    convert_engines_runtime_to_dependencies, json, manifest_from_json,
    node_version_from_engines_runtime, read_to_string, tempdir,
};

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
fn runtime_on_fail_ignore_preserves_explicit_runtime_dependencies() {
    let mut manifest = json!({
        "devEngines": {
            "runtime": { "name": "bun", "version": "1.2.0", "onFail": "download" },
        },
        "devDependencies": {
            "node": "runtime:22.20.0",
        },
    });
    apply_runtime_on_fail_override(&mut manifest, "ignore");
    assert_eq!(
        manifest.get("devDependencies").and_then(|value| value.get("node")),
        Some(&json!("runtime:22.20.0")),
    );
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
    assert_eq!(
        node_version_from_engines_runtime(&json!({
            "engines": {
                "runtime": { "name": "node", "version": " 24.6.0 " },
            },
        })),
        Some("24.6.0".to_string()),
    );
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
