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
    options.store_dir = Some(temp_dir.path().join("store").to_string_lossy().into_owned());
    options.registries = Some(HashMap::from([("default".to_string(), registry.url())]));

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
        .map(|entry| entry.expect("virtual store entry").file_name().to_string_lossy().into_owned())
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
    options.store_dir = Some(temp_dir.path().join("store").to_string_lossy().into_owned());
    options.registries = Some(HashMap::from([("default".to_string(), registry.url())]));

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
        .map(|entry| entry.expect("virtual store entry").file_name().to_string_lossy().into_owned())
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
    options.store_dir = Some(temp_dir.path().join("store").to_string_lossy().into_owned());
    options.registries = Some(HashMap::from([("default".to_string(), registry.url())]));

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
        .map(|entry| entry.expect("virtual store entry").file_name().to_string_lossy().into_owned())
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
    options.store_dir = Some(temp_dir.path().join("store").to_string_lossy().into_owned());
    options.registries = Some(HashMap::from([("default".to_string(), registry.url())]));

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
