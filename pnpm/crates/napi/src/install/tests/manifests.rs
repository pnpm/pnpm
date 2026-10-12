use super::{
    EngineMode, HashMap, NodeApiProject, TestRegistry, install_options,
    reject_non_object_manifests, run_install_inner,
};

#[test]
fn non_object_project_manifests_are_rejected() {
    let ok = vec![NodeApiProject {
        root_dir: "/a".to_string(),
        manifest: serde_json::json!({ "name": "x" }),
        dependency_manifest: None,
    }];
    assert!(reject_non_object_manifests(&ok).is_ok());

    for bad in [
        serde_json::json!([1, 2, 3]),
        serde_json::json!("oops"),
        serde_json::json!(42),
        serde_json::json!(null),
    ] {
        let projects = vec![NodeApiProject {
            root_dir: "/a".to_string(),
            manifest: bad,
            dependency_manifest: None,
        }];
        assert!(reject_non_object_manifests(&projects).is_err());
    }
}

/// The `pnpm fetch` shape: every importer the lockfile records is imported
/// into the virtual store, and nothing is linked out of it — no importer
/// symlink for a direct dependency, and no top-level `.bin` entry. The
/// in-memory manifests are ignored entirely, so an empty one still fetches
/// the whole recorded graph.
#[test]
fn ignore_package_manifest_populates_the_virtual_store_without_linking() {
    let registry = TestRegistry::start();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let project_dir = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    std::fs::write(project_dir.join("package.json"), "{}\n").expect("write package.json");

    let project_dir_string = project_dir.to_string_lossy().into_owned();
    let mut options = install_options();
    options.dir = project_dir_string.clone();
    options.projects = vec![NodeApiProject {
        root_dir: project_dir_string,
        manifest: serde_json::json!({
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" }
        }),
        dependency_manifest: None,
    }];
    options.store_dir = Some(
        temp_dir
            .path()
            .join("store")
            .to_string_lossy()
            .into_owned(),
    );
    options.registries = Some(HashMap::from([("default".to_string(), registry.url().to_string())]));

    // Seed the lockfile with an ordinary install, then throw the linked
    // `node_modules` away so the fetch-shaped run starts from the lockfile
    // alone.
    options.lockfile_only = Some(true);
    run_install_inner(&options, None, EngineMode::Install(None)).expect("seed the lockfile");
    assert!(project_dir.join("pnpm-lock.yaml").exists(), "the seed run must write a lockfile");
    options.lockfile_only = None;

    // An empty manifest proves the run reads the lockfile, not the manifest.
    options.projects[0].manifest = serde_json::json!({});
    options.ignore_package_manifest = Some(true);
    run_install_inner(&options, None, EngineMode::Install(None)).expect("fetch-shaped install");

    let modules_dir = project_dir.join("node_modules");
    let virtual_store = modules_dir.join(".pnpm");
    let fetched: Vec<String> = std::fs::read_dir(&virtual_store)
        .expect("read the virtual store")
        .map(|entry| {
            entry
                .expect("virtual store entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.starts_with("@pnpm.e2e+hello-world-js-bin"))
        .collect();
    dbg!(&fetched);
    assert_eq!(fetched.len(), 1, "the recorded dependency must be imported into the virtual store");

    assert!(
        !modules_dir.join("@pnpm.e2e/hello-world-js-bin").exists(),
        "a fetch-shaped install links no importer symlinks",
    );
    assert!(!modules_dir.join(".bin").exists(), "a fetch-shaped install links no top-level bins");
}

/// A fetch-shaped install reads the lockfile by definition, so an ambient
/// `lockfile: false` must not disable it — that would leave the mode with
/// nothing to materialize from and fail with `ERR_PNPM_NO_LOCKFILE`.
#[test]
fn ignore_package_manifest_survives_an_ambient_lockfile_false() {
    let registry = TestRegistry::start();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let project_dir = temp_dir.path().join("project");
    std::fs::create_dir_all(&project_dir).expect("create project dir");
    std::fs::write(project_dir.join("package.json"), "{}\n").expect("write package.json");

    let project_dir_string = project_dir.to_string_lossy().into_owned();
    let mut options = install_options();
    options.dir = project_dir_string.clone();
    options.projects = vec![NodeApiProject {
        root_dir: project_dir_string,
        manifest: serde_json::json!({
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" }
        }),
        dependency_manifest: None,
    }];
    options.store_dir = Some(
        temp_dir
            .path()
            .join("store")
            .to_string_lossy()
            .into_owned(),
    );
    options.registries = Some(HashMap::from([("default".to_string(), registry.url().to_string())]));

    options.lockfile_only = Some(true);
    run_install_inner(&options, None, EngineMode::Install(None)).expect("seed the lockfile");
    options.lockfile_only = None;

    std::fs::write(project_dir.join("pnpm-workspace.yaml"), "lockfile: false\n")
        .expect("write workspace yaml");

    options.ignore_package_manifest = Some(true);
    run_install_inner(&options, None, EngineMode::Install(None))
        .expect("a fetch-shaped install must read the lockfile despite `lockfile: false`");

    let fetched: Vec<String> = std::fs::read_dir(project_dir.join("node_modules/.pnpm"))
        .expect("read the virtual store")
        .map(|entry| {
            entry
                .expect("virtual store entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.starts_with("@pnpm.e2e+hello-world-js-bin"))
        .collect();
    dbg!(&fetched);
    assert_eq!(fetched.len(), 1, "the recorded dependency must still be imported");
}

/// The fetch shape covers every importer the lockfile records, not just the
/// ones the caller named — pnpm's `initialImporterIds` under
/// `ignorePackageManifest`. Here the caller passes only the workspace root
/// while the dependency belongs to a member project.
#[test]
fn ignore_package_manifest_fetches_importers_the_caller_did_not_pass() {
    let registry = TestRegistry::start();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let root_dir = temp_dir.path().join("workspace");
    let member_dir = root_dir.join("packages/member");
    std::fs::create_dir_all(&member_dir).expect("create member dir");
    std::fs::write(root_dir.join("package.json"), "{}\n").expect("write root package.json");
    std::fs::write(member_dir.join("package.json"), "{}\n").expect("write member package.json");
    std::fs::write(root_dir.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace yaml");

    let root_dir_string = root_dir.to_string_lossy().into_owned();
    let member_dir_string = member_dir.to_string_lossy().into_owned();
    let mut options = install_options();
    options.dir = root_dir_string.clone();
    options.projects = vec![
        NodeApiProject {
            root_dir: root_dir_string,
            manifest: serde_json::json!({ "name": "root" }),
            dependency_manifest: None,
        },
        NodeApiProject {
            root_dir: member_dir_string,
            manifest: serde_json::json!({
                "name": "member",
                "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" }
            }),
            dependency_manifest: None,
        },
    ];
    options.store_dir = Some(
        temp_dir
            .path()
            .join("store")
            .to_string_lossy()
            .into_owned(),
    );
    options.registries = Some(HashMap::from([("default".to_string(), registry.url().to_string())]));

    options.lockfile_only = Some(true);
    run_install_inner(&options, None, EngineMode::Install(None)).expect("seed the lockfile");
    options.lockfile_only = None;

    // Drop the member importer from the call entirely: its dependency must
    // still be fetched, because the lockfile records it.
    options.projects.truncate(1);
    options.ignore_package_manifest = Some(true);
    run_install_inner(&options, None, EngineMode::Install(None)).expect("fetch-shaped install");

    let fetched: Vec<String> = std::fs::read_dir(root_dir.join("node_modules/.pnpm"))
        .expect("read the virtual store")
        .map(|entry| {
            entry
                .expect("virtual store entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.starts_with("@pnpm.e2e+hello-world-js-bin"))
        .collect();
    dbg!(&fetched);
    assert_eq!(fetched.len(), 1, "the unnamed importer's dependency must still be fetched");
    assert!(
        !member_dir.join("node_modules/@pnpm.e2e/hello-world-js-bin").exists(),
        "a fetch-shaped install links no importer symlinks",
    );
}

#[test]
fn repeat_install_uses_changed_in_memory_manifest() {
    let registry = TestRegistry::start();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let project_dir = temp_dir.path().join("project");
    std::fs::create_dir(&project_dir).expect("create project dir");
    std::fs::write(project_dir.join("package.json"), "{}\n").expect("write package.json");

    let project_dir_string = project_dir.to_string_lossy().into_owned();
    let mut options = install_options();
    options.dir = project_dir_string.clone();
    options.projects = vec![NodeApiProject {
        root_dir: project_dir_string,
        manifest: serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/foo": "100.0.0"
            }
        }),
        dependency_manifest: None,
    }];
    options.store_dir = Some(
        temp_dir
            .path()
            .join("store")
            .to_string_lossy()
            .into_owned(),
    );
    options.registries = Some(HashMap::from([("default".to_string(), registry.url().to_string())]));

    run_install_inner(&options, None, EngineMode::Install(None)).expect("first install");
    assert!(project_dir.join("node_modules/@pnpm.e2e/foo").exists());

    options.projects[0].manifest = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/bar": "100.0.0",
            "@pnpm.e2e/foo": "100.0.0"
        }
    });

    run_install_inner(&options, None, EngineMode::Install(None)).expect("second install");
    assert!(project_dir.join("node_modules/@pnpm.e2e/bar").exists());
    assert_eq!(
        std::fs::read_to_string(project_dir.join("package.json")).expect("read package.json"),
        "{}\n",
    );
}

/// The second run has no registry and no metadata cache: every path but the
/// repeat-install fast path (a resolve, the lockfile-verification fan-out, a
/// tarball fetch) would have to reach the dead registry and fail.
#[test]
fn repeat_install_with_unchanged_in_memory_manifest_needs_no_registry() {
    let registry = TestRegistry::start();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let project_dir = temp_dir.path().join("project");
    let cache_dir = temp_dir.path().join("cache");
    std::fs::create_dir(&project_dir).expect("create project dir");
    std::fs::write(project_dir.join("package.json"), "{}\n").expect("write package.json");

    let project_dir_string = project_dir.to_string_lossy().into_owned();
    let mut options = install_options();
    options.dir = project_dir_string.clone();
    options.projects = vec![NodeApiProject {
        root_dir: project_dir_string,
        manifest: serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/foo": "100.0.0"
            }
        }),
        dependency_manifest: None,
    }];
    options.store_dir = Some(
        temp_dir
            .path()
            .join("store")
            .to_string_lossy()
            .into_owned(),
    );
    options.cache_dir = Some(cache_dir.to_string_lossy().into_owned());
    options.registries = Some(HashMap::from([("default".to_string(), registry.url().to_string())]));

    run_install_inner(&options, None, EngineMode::Install(None)).expect("first install");
    assert!(project_dir.join("node_modules/@pnpm.e2e/foo").exists());

    std::fs::remove_dir_all(&cache_dir).expect("wipe the metadata cache");
    options.registries =
        Some(HashMap::from([("default".to_string(), "http://127.0.0.1:9/".to_string())]));

    run_install_inner(&options, None, EngineMode::Install(None))
        .expect("a repeat install with an unchanged in-memory manifest needs no registry");
    assert!(project_dir.join("node_modules/@pnpm.e2e/foo").exists());
}

/// A workspace whose `lib` project is injected into `app`, with an importer
/// manifest that drops the dependencies its `dependencyManifest` declares —
/// the shape Bit passes after transforming its importer manifests. One of
/// them injects the `util` project through `workspace:*`; another is also a
/// peer, which `app` provides at a version the dependency spec rejects.
fn injected_dependency_manifest_options(
    temp_dir: &std::path::Path,
    registry_url: &str,
) -> super::InstallOptions {
    let root_dir = temp_dir.join("workspace");
    let app_dir = root_dir.join("app");
    let lib_dir = root_dir.join("lib");
    let util_dir = root_dir.join("util");
    // Like the host's, the project directories hold no `package.json`: every
    // manifest is passed in memory.
    for dir in [&app_dir, &lib_dir, &util_dir] {
        std::fs::create_dir_all(dir).expect("create project dir");
    }
    let mut options = install_options();
    options.dir = root_dir.to_string_lossy().into_owned();
    options.projects = vec![
        NodeApiProject {
            root_dir: options.dir.clone(),
            manifest: serde_json::json!({ "name": "root" }),
            dependency_manifest: None,
        },
        NodeApiProject {
            root_dir: app_dir.to_string_lossy().into_owned(),
            manifest: serde_json::json!({
                "name": "app",
                "version": "1.0.0",
                "dependencies": { "@pnpm.e2e/foo": "100.1.0", "lib": "workspace:*" },
                "dependenciesMeta": { "lib": { "injected": true } },
            }),
            dependency_manifest: None,
        },
        NodeApiProject {
            root_dir: lib_dir.to_string_lossy().into_owned(),
            manifest: serde_json::json!({ "name": "lib", "version": "1.0.0" }),
            dependency_manifest: Some(serde_json::json!({
                "name": "lib",
                "version": "1.0.0",
                "dependencies": { "@pnpm.e2e/foo": "100.0.0", "util": "workspace:*" },
                "dependenciesMeta": { "util": { "injected": true } },
                "peerDependencies": { "@pnpm.e2e/foo": "^100.0.0" },
            })),
        },
        NodeApiProject {
            root_dir: util_dir.to_string_lossy().into_owned(),
            manifest: serde_json::json!({ "name": "util", "version": "1.0.0" }),
            dependency_manifest: None,
        },
    ];
    options.store_dir = Some(
        temp_dir
            .join("store")
            .to_string_lossy()
            .into_owned(),
    );
    options.cache_dir = Some(
        temp_dir
            .join("cache")
            .to_string_lossy()
            .into_owned(),
    );
    options.registries = Some(HashMap::from([("default".to_string(), registry_url.to_string())]));
    options
}

/// The lockfile records the injected `lib` with the dependencies of its
/// `dependencyManifest`, so a repeat install with unchanged inputs has to
/// compare it against that manifest, not the importer manifest. The repeat
/// run has no registry and no metadata cache, so anything but the fast path
/// fails it. A frozen run fails on a lockfile it judges outdated.
#[test]
fn repeat_install_compares_injected_project_with_its_dependency_manifest() {
    let registry = TestRegistry::start();
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let mut options = injected_dependency_manifest_options(temp_dir.path(), registry.url());
    let root_dir = std::path::PathBuf::from(&options.dir);

    run_install_inner(&options, None, EngineMode::Install(None)).expect("first install");
    // The peer suffix names the injected lib's slot, e.g.
    // `lib@file+lib(@pnpm.e2e+foo@100.1.0)`.
    let injected_lib_deps = std::fs::read_dir(root_dir.join("node_modules/.pnpm"))
        .expect("read the virtual store")
        .map(|entry| entry.expect("virtual store entry").path())
        .find(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("lib@file+lib"))
        })
        .expect("the injected lib's slot")
        .join("node_modules");
    let injected_foo = injected_lib_deps.join("@pnpm.e2e/foo");
    assert!(injected_foo.exists(), "the injected lib gets its dependency manifest's dependency");
    assert!(injected_lib_deps.join("util").exists(), "the injected lib gets the injected util");
    let lockfile = std::fs::read_to_string(root_dir.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile.contains("'@pnpm.e2e/foo': 100.1.0"),
        "the peer `app` provides wins over lib's own dependency:\n{lockfile}",
    );

    std::fs::remove_dir_all(temp_dir.path().join("cache")).expect("wipe the metadata cache");
    options.registries =
        Some(HashMap::from([("default".to_string(), "http://127.0.0.1:9/".to_string())]));
    options.fetch_retries = Some(0);

    run_install_inner(&options, None, EngineMode::Install(None))
        .expect("the repeat-install fast path accepts the lockfile");

    // The frozen run verifies the lockfile against the registry.
    options.registries = Some(HashMap::from([("default".to_string(), registry.url().to_string())]));
    options.frozen_lockfile = Some(true);
    run_install_inner(&options, None, EngineMode::Install(None))
        .expect("the frozen install accepts the lockfile");
    assert!(injected_foo.exists());
}
