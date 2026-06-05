use std::{collections::HashMap, fs::read_to_string};

use insta::assert_snapshot;
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
use tempfile::{NamedTempFile, tempdir};

#[cfg(unix)]
use super::safe_read_package_json_from_dir;
use super::{
    BundleDependencies, PackageManifest, PackageManifestError,
    convert_engines_runtime_to_dependencies,
};
use crate::DependencyGroup;
use serde_json::json;
use std::io::Write;

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
    // `npm:foo` and `npm:@scope/foo` mean "latest" in pnpm.
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
    // Mirrors pnpm's `lastIndexOf('@')` so prerelease/build metadata
    // containing `@` would still be split at the *final* `@`.
    assert_eq!(
        PackageManifest::resolve_registry_dependency("foo-rc", "npm:@scope/foo@1.0.0-rc.1",),
        ("@scope/foo", "1.0.0-rc.1"),
    );
}

/// `devEngines.runtime` with `onFail: "download"` and an explicit
/// version is reified into `devDependencies` as `runtime:<version>`.
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

/// Skip when no `version` is set. Upstream warns and skips; pacquet
/// skips silently. The staleness check still surfaces the gap.
/// Mirrors upstream's
/// [`convertEnginesRuntimeToDependencies() skips runtime entries without a version`](https://github.com/pnpm/pnpm/blob/9cad8274fd/pkg-manifest/utils/test/convertEnginesRuntimeToDependencies.test.ts#L8-L21).
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

/// Skip when `onFail` is anything other than `"download"` (or absent).
/// Upstream gates the runtime reification on that flag, so an `error`
/// or `warn` setup must not silently morph into a `runtime:` dep.
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

/// An explicit user-declared entry in the target dependencies bucket
/// wins over the reified runtime, matching upstream's
/// [`manifest[dependenciesFieldName]?.[runtimeName]`](https://github.com/pnpm/pnpm/blob/9cad8274fd/pkg-manifest/utils/src/convertEnginesRuntimeToDependencies.ts#L17)
/// short-circuit.
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

/// `devEngines.runtime` accepts an array of entries (one per runtime
/// alias). Each `RUNTIME_NAMES` entry is matched by `name` and reified
/// independently — `node` and `bun` together is a valid declaration.
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

/// `engines.runtime` (rather than `devEngines.runtime`) targets
/// `dependencies` — upstream calls
/// `convertEnginesRuntimeToDependencies(manifest, 'engines', 'dependencies')`
/// alongside the `devEngines` pass.
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

/// `from_path` surfaces `NoImporterManifestFound` (the typed
/// equivalent of pnpm's `ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND`)
/// when the path does not exist, rather than letting the
/// underlying ENOENT escape as a generic IO error.
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

/// `add_dependency` rejects manifests where the target
/// dependency group exists but holds a non-object value (a quirk
/// upstream allows on disk but cannot insert into). Pin that the
/// error is the typed `InvalidAttribute` and not a panic.
#[test]
fn add_dependency_errors_when_field_is_not_an_object() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("package.json");
    // Pre-seed `dependencies` as a string instead of an object.
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

/// Without a `save_type`, `available_dependency_names` reports the union
/// of `dependencies`, `devDependencies`, and `optionalDependencies`
/// (peer excluded) in dev → prod → optional order, deduplicated — the
/// set `pnpm remove` validates against.
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

/// With a `save_type`, only that field's keys are reported.
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

/// Without a `save_type`, `remove_dependencies` drops the name from every
/// dependency field, including `peerDependencies` and `dependenciesMeta`.
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

/// With a `save_type`, only that field is touched — but `peerDependencies`
/// and `dependenciesMeta` are still cleared, matching pnpm's `removeDeps`.
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

/// `safe_read_package_json_from_dir` surfaces non-NotFound IO
/// errors via `PackageManifestError::Io`, rather than swallowing
/// them as `Ok(None)`. Mirrors upstream's contract: `null` is
/// returned only on ENOENT.
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

/// `convert_engines_runtime_to_dependencies` ignores `devEngines.runtime`
/// entries whose value is neither an array nor a single object.
/// Pin that the function returns without mutating the manifest in
/// that case, instead of panicking.
#[test]
fn convert_engines_ignores_non_array_non_object_runtime_entries() {
    let mut manifest = json!({
        "name": "x",
        "version": "1.0.0",
        // `runtime` is a bare string — neither an object nor an array.
        "devEngines": { "runtime": "not-supported" },
    });
    let before = manifest.clone();
    convert_engines_runtime_to_dependencies(&mut manifest, "devEngines", "devDependencies");
    assert_eq!(manifest, before, "manifest must be unchanged for unsupported `runtime` shape");
}
