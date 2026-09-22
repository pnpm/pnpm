use super::{
    Host,
    NodeLinker,
    SilentReporter,
    api,
    fixture,
    install_module,
    json,
};

/// `pack` bundles dependencies listed in `bundleDependencies`.
/// Covers the `fs-packlist` `bundleDependencies` recursion and the
/// `node_linker: hoisted` allow-path.
#[test]
fn bundles_dependencies_listed_in_bundle_dependencies() {
    let (dir, mut opts) = fixture(&json!({
        "name": "pkg-with-bundle-deps",
        "version": "0.0.0",
        "bundleDependencies": ["bundled-dep"],
    }));
    opts.manifest.node_linker = NodeLinker::Hoisted;
    install_module(dir.path(), "bundled-dep", "1.0.0", &[("index.js", "module.exports = 42")]);
    install_module(dir.path(), "not-bundled", "1.0.0", &[]);

    let result = api::<SilentReporter, Host>(&opts).unwrap();

    assert!(result.contents.contains(&"node_modules/bundled-dep/package.json".to_string()));
    assert!(result.contents.contains(&"node_modules/bundled-dep/index.js".to_string()));
    assert!(
        !result.contents
            .iter()
            .any(|path| path.contains("not-bundled")),
    );
}

/// `pack` bundles every dependency when `bundleDependencies` is true.
/// Covers `bundle_dep_names`' `bundleDependencies: true` branch, which
/// materializes the names from `dependencies`.
#[test]
fn bundles_every_dependency_when_bundle_dependencies_is_true() {
    let (dir, mut opts) = fixture(&json!({
        "name": "pkg-with-bundle-deps-true",
        "version": "0.0.0",
        "dependencies": { "bundled-dep": "1.0.0" },
        "bundleDependencies": true,
    }));
    opts.manifest.node_linker = NodeLinker::Hoisted;
    install_module(dir.path(), "bundled-dep", "1.0.0", &[("index.js", "module.exports = 42")]);
    install_module(dir.path(), "not-a-dep", "1.0.0", &[]);

    let result = api::<SilentReporter, Host>(&opts).unwrap();

    assert!(result.contents.contains(&"node_modules/bundled-dep/index.js".to_string()));
    assert!(result.contents.contains(&"node_modules/bundled-dep/package.json".to_string()));
    assert!(
        !result.contents
            .iter()
            .any(|path| path.contains("not-a-dep")),
    );
}

/// `pack` bundles transitive dependencies of bundled dependencies
/// (hoisted).
/// A bundled dep's own (hoisted) `dependencies` ship too: `top` is
/// bundled and depends on `nested`, which is hoisted to the root
/// `node_modules`, so `nested` must be bundled under its real path. The
/// transitive walk this exercises lives in `fs-packlist`.
#[test]
fn bundles_transitive_dependencies_of_bundled_dependencies() {
    let (dir, mut opts) = fixture(&json!({
        "name": "pkg-with-transitive-bundle-deps",
        "version": "0.0.0",
        "bundledDependencies": ["top"],
    }));
    opts.manifest.node_linker = NodeLinker::Hoisted;
    // `top` (directly bundled) depends on `nested`, hoisted to the root.
    let top = dir
        .path()
        .join("node_modules")
        .join("top");
    std::fs::create_dir_all(&top).unwrap();
    std::fs::write(
        top.join("package.json"),
        r#"{"name":"top","version":"1.0.0","dependencies":{"nested":"1.0.0"}}"#,
    )
    .unwrap();
    std::fs::write(top.join("index.js"), "top").unwrap();
    install_module(dir.path(), "nested", "1.0.0", &[("index.js", "nested")]);

    let result = api::<SilentReporter, Host>(&opts).unwrap();

    assert!(result.contents.contains(&"node_modules/top/index.js".to_string()));
    assert!(
        result.contents.contains(&"node_modules/nested/index.js".to_string()),
        "hoisted transitive dep `nested` must be bundled: {:?}",
        result.contents,
    );
}

/// `pack` reads from the correct `node_modules` when publishing from a
/// custom directory.
/// Covers the `publishConfig.directory` redirect: the manifest is read
/// from `dist/`, but `workspace:` deps still resolve against the project
/// root's `node_modules`.
#[test]
fn reads_node_modules_from_original_dir_when_publishing_from_custom_directory() {
    let (dir, opts) = fixture(&json!({
        "name": "custom-publish-dir",
        "version": "0.0.0",
        "publishConfig": { "directory": "dist" },
        "dependencies": { "local": "workspace:*" },
    }));
    let dist = dir.path().join("dist");
    std::fs::create_dir_all(&dist).unwrap();
    std::fs::copy(dir.path().join("package.json"), dist.join("package.json")).unwrap();
    install_module(dir.path(), "local", "1.0.0", &[]);

    let result = api::<SilentReporter, Host>(&opts).unwrap();

    // The `workspace:*` spec resolves to the installed version, read from
    // the project root's node_modules (not `dist/node_modules`).
    assert_eq!(result.published_manifest["dependencies"]["local"], json!("1.0.0"));
}
