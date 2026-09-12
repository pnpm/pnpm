use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, IS_POSITIVE_PATCH, Path, configure_pnpr_auth,
    fs, get_all_files, is_symlink_or_junction, pacquet_at, point_npmrc_registry_at,
    read_workspace_lockfile, start_pnpr, text_block_fnl, workspace_importer_version,
};
use assert_cmd::assert::OutputAssertExt;

#[test]
fn install_via_pnpr_links_node_modules() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, store_dir, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": { "@foo/no-deps": "1.0.0" },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .with_arg("--pnpr-server")
        .with_arg(&pnpr_url)
        .assert()
        .success();

    let symlink_path = workspace.join("node_modules/@foo/no-deps");
    assert!(is_symlink_or_junction(&symlink_path).unwrap(), "direct dep should be symlinked");
    let virtual_path = workspace.join("node_modules/.pnpm/@foo+no-deps@1.0.0");
    assert!(virtual_path.exists(), "virtual store should hold the package");
    assert!(workspace.join("pnpm-lock.yaml").exists(), "pnpr should write the lockfile");
    // The client store was populated by the frozen install fetching tarballs
    // directly from the registry after pnpr returned the lockfile.
    assert!(store_dir.join("v11/index.db").exists(), "client store index should exist");

    drop((root, mock_instance));
}

#[test]
fn install_via_pnpr_replaces_a_conflicted_lockfile() {
    const CONFLICTED_LOCKFILE: &str = text_block_fnl! {
        "<<<<<<< HEAD"
        "lockfileVersion: '9.0'"
        "======="
        "lockfileVersion: '9.0'"
        ">>>>>>> branch"
    };

    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@foo/no-deps": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    fs::write(workspace.join("pnpm-lock.yaml"), CONFLICTED_LOCKFILE)
        .expect("write conflicted lockfile");

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    let lockfile = read_workspace_lockfile(&workspace);
    assert_eq!(workspace_importer_version(&lockfile, ".", "@foo/no-deps"), "1.0.0");
    assert!(is_symlink_or_junction(&workspace.join("node_modules/@foo/no-deps")).unwrap());

    fs::write(workspace.join("pnpm-lock.yaml"), CONFLICTED_LOCKFILE)
        .expect("rewrite conflicted lockfile");
    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--fix-lockfile", "--pnpr-server", &pnpr_url])
        .assert()
        .success();
    let repaired = read_workspace_lockfile(&workspace);
    assert_eq!(workspace_importer_version(&repaired, ".", "@foo/no-deps"), "1.0.0");

    drop((root, mock_instance));
}

#[test]
fn patched_dependencies_resolve_via_pnpr() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    fs::create_dir_all(workspace.join("patches")).expect("create patches dir");
    fs::write(workspace.join("patches/is-positive@1.0.0.patch"), IS_POSITIVE_PATCH)
        .expect("write patch file");
    crate::_utils::append_workspace_yaml_key(
        &workspace,
        "patchedDependencies",
        "{ 'is-positive@1.0.0': patches/is-positive@1.0.0.patch }",
    );

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    let installed = fs::read_to_string(workspace.join("node_modules/is-positive/index.js"))
        .expect("read installed package");
    eprintln!("INSTALLED SOURCE:\n{installed}\n");
    assert!(installed.contains("// patched"));
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    eprintln!("LOCKFILE:\n{lockfile}\n");
    assert!(lockfile.contains("patchedDependencies:"));
    assert!(lockfile.contains("patch_hash="));

    drop((root, mock_instance));
}

#[test]
fn package_extensions_resolve_via_pnpr() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "is-positive": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    crate::_utils::append_workspace_yaml_key(
        &workspace,
        "packageExtensions",
        "{ 'is-positive@1.0.0': { dependencies: { is-negative: 1.0.0 } } }",
    );

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    eprintln!("LOCKFILE:\n{lockfile}\n");
    assert!(lockfile.contains("packageExtensionsChecksum:"));
    assert!(lockfile.contains("is-negative: 1.0.0"));
    assert!(
        workspace.join("node_modules/.pnpm/is-positive@1.0.0/node_modules/is-negative").exists(),
    );

    drop((root, mock_instance));
}

/// A pnpr-resolved lockfile is rewritten wholesale from the server's
/// answer, so `time:` has to survive the round trip the same way a
/// locally resolved install preserves it.
#[test]
fn install_via_pnpr_preserves_the_lockfiles_time_section() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let manifest_path = workspace.join("package.json");
    let write_manifest = |dependencies: serde_json::Value| {
        fs::write(&manifest_path, serde_json::json!({ "dependencies": dependencies }).to_string())
            .expect("write package.json");
    };
    let install_via_pnpr = || {
        pacquet_at(&workspace)
            .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
            .with_arg("install")
            .with_arg("--pnpr-server")
            .with_arg(&pnpr_url)
            .assert()
            .success();
    };

    write_manifest(serde_json::json!({ "@foo/has-dep-from-same-scope": "1.0.0" }));
    install_via_pnpr();

    // Appended as text: saving would prune the transitive entry before
    // the install under test ever sees it.
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let mut recorded = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    recorded.push_str(
        "\ntime:\n  '@foo/has-dep-from-same-scope@1.0.0': '2024-01-01T00:00:00.000Z'\n  '@foo/no-deps@1.0.0': '2024-01-02T00:00:00.000Z'\n",
    );
    fs::write(&lockfile_path, recorded).expect("record publish dates in pnpm-lock.yaml");

    // A new dependency is what sends the second install back through the
    // server rather than short-circuiting on the lockfile it just wrote.
    write_manifest(
        serde_json::json!({ "@foo/has-dep-from-same-scope": "1.0.0", "is-positive": "1.0.0" }),
    );
    install_via_pnpr();

    let time = read_workspace_lockfile(&workspace).time.expect("`time:` survives a pnpr install");
    assert_eq!(
        time.into_iter().collect::<Vec<_>>(),
        [(
            "@foo/has-dep-from-same-scope@1.0.0".to_string(),
            "2024-01-01T00:00:00.000Z".to_string(),
        )],
    );

    drop((root, mock_instance));
}

#[test]
fn frozen_install_via_pnpr_verifies_the_local_lockfile_without_resolving_or_redownloading() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, cache_dir, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": { "@foo/no-deps": "1.0.0" },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .with_arg("--pnpr-server")
        .with_arg(&pnpr_url)
        .assert()
        .success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    // The first install recorded its lockfile as verified; wipe that
    // cache so this test still exercises the server-delegated
    // verification path instead of the cache short-circuit.
    fs::remove_dir_all(&cache_dir).expect("wipe the client cache dir");

    let mut verifier = mockito::Server::new();
    let verify_mock = verifier
        .mock("POST", "/-/pnpr/v0/verify-lockfile")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body("{\"type\":\"done\"}\n")
        .expect(1)
        .create();

    // The first install warmed the store, so the frozen restore must not
    // fetch a single tarball: point the registry at a server that rejects
    // every request.
    let mut silent_registry = mockito::Server::new();
    let no_downloads = silent_registry.mock("GET", mockito::Matcher::Any).expect(0).create();
    point_npmrc_registry_at(&npmrc_path, &silent_registry.url());

    pacquet_at(&workspace)
        .with_arg("install")
        .with_arg("--frozen-lockfile")
        .with_arg("--pnpr-server")
        .with_arg(verifier.url())
        .assert()
        .success();

    verify_mock.assert();
    no_downloads.assert();
    let symlink_path = workspace.join("node_modules/@foo/no-deps");
    assert!(is_symlink_or_junction(&symlink_path).unwrap(), "direct dep should be symlinked");

    drop((root, mock_instance));
}

/// The up-to-date verdict is decided purely locally
/// ([pnpm/pnpm#13904](https://github.com/pnpm/pnpm/issues/13904)).
#[test]
fn repeat_install_via_pnpr_short_circuits_without_contacting_the_server() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": { "@foo/no-deps": "1.0.0" },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .with_arg("--pnpr-server")
        .with_arg(&pnpr_url)
        .assert()
        .success();

    let mut silent_pnpr = mockito::Server::new();
    let no_pnpr_requests = silent_pnpr.mock("POST", mockito::Matcher::Any).expect(0).create();

    let assert = pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .with_arg("--pnpr-server")
        .with_arg(silent_pnpr.url())
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    assert!(
        stdout.contains("Already up to date"),
        "expected the up-to-date fast path's output; got:\n{stdout}",
    );
    no_pnpr_requests.assert();

    drop((root, mock_instance));
}

/// Zero exchanges, not one: the verification round trip is covered by
/// the record the previous install left in the local verification cache
/// ([pnpm/pnpm#13904](https://github.com/pnpm/pnpm/issues/13904)).
#[test]
fn install_via_pnpr_skips_the_server_when_the_lockfile_satisfies_the_manifest() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": { "@foo/no-deps": "1.0.0" },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .with_arg("--pnpr-server")
        .with_arg(&pnpr_url)
        .assert()
        .success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    let mut silent_pnpr = mockito::Server::new();
    let no_pnpr_requests = silent_pnpr.mock("POST", mockito::Matcher::Any).expect(0).create();
    // The warm store must serve every tarball; reject any registry fetch.
    let mut silent_registry = mockito::Server::new();
    let no_downloads = silent_registry.mock("GET", mockito::Matcher::Any).expect(0).create();
    point_npmrc_registry_at(&npmrc_path, &silent_registry.url());

    pacquet_at(&workspace)
        .with_arg("install")
        .with_arg("--pnpr-server")
        .with_arg(silent_pnpr.url())
        .assert()
        .success();

    no_pnpr_requests.assert();
    no_downloads.assert();
    let symlink_path = workspace.join("node_modules/@foo/no-deps");
    assert!(is_symlink_or_junction(&symlink_path).unwrap(), "direct dep should be symlinked");

    drop((root, mock_instance));
}

/// The satisfaction check skips only the *resolve* exchange on its own
/// ([pnpm/pnpm#13904](https://github.com/pnpm/pnpm/issues/13904)).
#[test]
fn satisfied_install_via_pnpr_delegates_verification_when_the_cache_is_cold() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, cache_dir, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": { "@foo/no-deps": "1.0.0" },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .with_arg("--pnpr-server")
        .with_arg(&pnpr_url)
        .assert()
        .success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    fs::remove_dir_all(&cache_dir).expect("wipe the client cache dir");

    let mut verifier = mockito::Server::new();
    let verify_mock = verifier
        .mock("POST", "/-/pnpr/v0/verify-lockfile")
        .with_status(200)
        .with_header("content-type", "application/x-ndjson")
        .with_body("{\"type\":\"done\"}\n")
        .expect(1)
        .create();
    let no_resolve = verifier.mock("POST", "/-/pnpr/v0/resolve").expect(0).create();

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .with_arg("--pnpr-server")
        .with_arg(verifier.url())
        .assert()
        .success();

    verify_mock.assert();
    no_resolve.assert();
    let symlink_path = workspace.join("node_modules/@foo/no-deps");
    assert!(is_symlink_or_junction(&symlink_path).unwrap(), "direct dep should be symlinked");

    drop((root, mock_instance));
}

#[test]
fn install_via_pnpr_lockfile_only_writes_lockfile_without_linking() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, store_dir, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": { "@foo/no-deps": "1.0.0" },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("install")
        .with_arg("--pnpr-server")
        .with_arg(&pnpr_url)
        .with_arg("--lockfile-only")
        .assert()
        .success();

    assert!(workspace.join("pnpm-lock.yaml").exists(), "pnpr should write the lockfile");
    assert!(!workspace.join("node_modules").exists(), "lockfile-only must not link node_modules");
    assert!(
        !store_dir.join("v11/index.db").exists(),
        "lockfile-only must not populate the client store",
    );

    drop((root, mock_instance));
}

/// `pnpm import` has to control the version each dependency resolves to,
/// which the pnpr protocol cannot yet express, so it resolves locally and
/// says so rather than silently ignoring the server.
#[test]
fn import_ignores_the_pnpr_server_and_resolves_locally() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, store_dir, mock_instance, .. } = npmrc_info;

    let (pnpr_url, token) = start_pnpr(&mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": { "@foo/no-deps": "1.0.0" },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");
    fs::write(
        workspace.join("package-lock.json"),
        serde_json::json!({
            "lockfileVersion": 1,
            "dependencies": {
                "@foo/no-deps": { "version": "1.0.0" },
            },
        })
        .to_string(),
    )
    .expect("write package-lock.json");

    let output = pacquet
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_arg("import")
        .with_arg("--pnpr-server")
        .with_arg(&pnpr_url)
        .assert()
        .success()
        .get_output()
        .clone();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!("the pnpr server at {pnpr_url} is not used")),
        "import must say the pnpr server was skipped:\n{stdout}",
    );
    assert!(workspace.join("pnpm-lock.yaml").exists(), "import must write the lockfile");
    assert!(!workspace.join("node_modules").exists(), "import must not link node_modules");
    // The store writer task always creates an empty `v11/index.db`, so the
    // absence of fetched package content is what says nothing was downloaded.
    let cas_blobs: Vec<String> = get_all_files(&store_dir)
        .into_iter()
        .filter(|path| {
            Path::new(path).components().any(|component| component.as_os_str() == "files")
        })
        .collect();
    assert!(cas_blobs.is_empty(), "import must not fetch package content: {cas_blobs:?}");

    drop((root, mock_instance));
}
