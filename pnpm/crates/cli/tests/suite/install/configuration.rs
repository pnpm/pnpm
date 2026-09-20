use super::{
    AddMockedRegistry, Command, CommandExtra, CommandTempCwd, STORE_VERSION, fs,
    is_symlink_or_junction, pacquet_in, write_required_incompatible_engine_fixture,
};
#[cfg(unix)]
use super::{Pipe, enable_gvs_in_workspace_yaml};
use assert_cmd::{assert::OutputAssertExt, cargo::CommandCargoExt};

/// A build host that appends `--prod=false` to its install command is
/// asking for devDependencies, the way nopt reads an explicit boolean
/// value (pnpm/pnpm#14553).
#[test]
fn prod_takes_an_explicit_boolean_value() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/foo": "100.0.0" },
            "devDependencies": { "@pnpm.e2e/bar": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet
        .with_args(["install", "--prod=false"])
        .assert()
        .success();
    assert!(
        workspace.join("node_modules/@pnpm.e2e/bar/package.json").exists(),
        "--prod=false must install devDependencies",
    );

    pacquet_in(&workspace)
        .with_args(["install", "--prod=true"])
        .assert()
        .success();
    assert!(
        !workspace.join("node_modules/@pnpm.e2e/bar").exists(),
        "--prod=true must drop the dev dependency",
    );
    assert!(
        workspace.join("node_modules/@pnpm.e2e/foo/package.json").exists(),
        "the prod dependency must stay installed",
    );

    drop((root, mock_instance));
}

/// An explicitly allowed dev-only dependency must not execute its
/// lifecycle scripts during a production install (pnpm/pnpm#14864).
#[test]
fn prod_install_does_not_run_dev_dependency_postinstall() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // The script's cwd is the package's materialized virtual-store
    // directory, which a production install is free to leave out entirely.
    // Writing the marker under `INIT_CWD` puts it at a path that exists
    // either way, so the assertion below cannot pass just because the
    // package was never materialized.
    let dev_pkg = workspace.join("dev-tool");
    fs::create_dir(&dev_pkg).expect("create the dev dependency directory");
    fs::write(
        dev_pkg.join("package.json"),
        serde_json::json!({
            "name": "dev-tool",
            "version": "1.0.0",
            "scripts": {
                "postinstall":
                    r#"node -e "require('fs').writeFileSync(process.env.INIT_CWD + '/dev-postinstall-ran', '')""#,
            },
        })
        .to_string(),
    )
    .expect("write the dev dependency manifest");

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "project",
            "version": "1.0.0",
            "dependencies": {
                "@pnpm.e2e/install-script-example": "1.0.0",
            },
            "devDependencies": {
                "dev-tool": "file:./dev-tool",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    // Appended, not written: the harness already pinned `storeDir`,
    // `cacheDir`, and `enableGlobalVirtualStore` in this file.
    let workspace_yaml = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&workspace_yaml).expect("read pnpm-workspace.yaml");
    yaml.push_str(
        "allowBuilds:\n  \
         'dev-tool@file:dev-tool': true\n  \
         '@pnpm.e2e/install-script-example': true\n",
    );
    fs::write(&workspace_yaml, yaml).expect("write pnpm-workspace.yaml");

    pacquet
        .with_args(["install", "--lockfile-only", "--ignore-scripts"])
        .assert()
        .success();
    pacquet_in(&workspace)
        .with_args(["install", "--prod", "--frozen-lockfile"])
        .assert()
        .success();

    assert!(
        !workspace.join("dev-postinstall-ran").exists(),
        "a dev-only package must not run its postinstall during install --prod",
    );
    // Without this, the assertion above would also hold for an install
    // that ran no script at all.
    assert!(
        workspace
            .join(
                "node_modules/.pnpm/@pnpm.e2e+install-script-example@1.0.0\
                 /node_modules/@pnpm.e2e/install-script-example/generated-by-install.js",
            )
            .exists(),
        "the prod dependency must run its install script",
    );

    drop((root, mock_instance));
}

#[test]
fn no_optional_excludes_transitive_optional_dependencies() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // `@pnpm.e2e/pkg-with-good-optional` is a prod dependency whose own
    // `optionalDependencies` pull in `is-positive`. `--no-optional` must
    // exclude that transitive optional, not just the root's own optionals.
    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-good-optional": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write to package.json");

    pacquet
        .with_args(["install", "--no-optional"])
        .assert()
        .success();

    let virtual_store = workspace.join("node_modules/.pnpm");
    assert!(
        virtual_store.join("@pnpm.e2e+pkg-with-good-optional@1.0.0").exists(),
        "the prod dependency must be installed",
    );
    assert!(
        !virtual_store.join("is-positive@1.0.0").exists(),
        "--no-optional must not materialize the transitive optional dependency",
    );

    // The exclusion is transient: the optional stays in the lockfile and is
    // not persisted to `.modules.yaml.skipped`, so a later install without
    // `--no-optional` restores it.
    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("is-positive@1.0.0"),
        "the excluded optional must remain in the lockfile:\n{lockfile}",
    );
    let modules_yaml = fs::read_to_string(workspace.join("node_modules/.modules.yaml"))
        .expect("read .modules.yaml");
    assert!(
        !modules_yaml.contains("is-positive"),
        "a `--no-optional` exclusion must not be recorded in .modules.yaml.skipped:\n{modules_yaml}",
    );

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["install"])
        .assert()
        .success();
    assert!(
        virtual_store.join("is-positive@1.0.0").exists(),
        "a normal install must restore the previously excluded optional dependency",
    );

    drop((root, mock_instance));
}

#[test]
fn fresh_isolated_install_rejects_required_incompatible_engine_in_strict_mode() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_required_incompatible_engine_fixture(&workspace, true);

    let assert = pacquet
        .with_arg("install")
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    eprintln!("STDERR:\n{stderr}\n");
    assert!(
        stderr.contains("Unsupported engine for incompatible-engine@file:incompatible-engine"),
        "stderr must identify the incompatible lockfile package ID; got:\n{stderr}",
    );
    assert!(
        stderr.contains(r#"wanted: {"node":">=999.0.0"}"#),
        "stderr must report the required Node.js version; got:\n{stderr}",
    );

    drop((root, mock_instance));
}

#[test]
fn fresh_isolated_install_allows_required_incompatible_engine_without_strict_mode() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_required_incompatible_engine_fixture(&workspace, false);

    pacquet
        .with_arg("install")
        .assert()
        .success();

    drop((root, mock_instance));
}

#[test]
fn store_dir_cli_option_overrides_config_and_resolves_from_dir() {
    let CommandTempCwd {
        mut pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry {
        store_dir: configured_store_dir,
        mock_instance,
        ..
    } = npmrc_info;

    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet
        .current_dir(root.path())
        .arg("--dir")
        .arg(&workspace)
        .args(["install", "--store-dir", "cli-store"])
        .assert()
        .success();

    let cli_store_dir = workspace.join("cli-store").join(STORE_VERSION);
    eprintln!("CLI store must be resolved from --dir and populated: {cli_store_dir:?}");
    assert!(cli_store_dir.join("index.db").is_file());

    eprintln!("Configured store must not be populated when the CLI overrides it");
    assert!(
        !configured_store_dir
            .join(STORE_VERSION)
            .join("index.db")
            .exists(),
    );

    drop((root, mock_instance));
}

#[cfg(unix)]
#[test]
fn store_dir_cli_option_updates_derived_global_virtual_store() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    enable_gvs_in_workspace_yaml(&workspace, "");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    pacquet
        .with_args(["install", "--store-dir=cli-store"])
        .assert()
        .success();

    let symlink_path = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent");
    let canonical = symlink_path.pipe(fs::canonicalize).expect("canonicalize symlink");
    let cli_store_dir = workspace.join("cli-store").join(STORE_VERSION);
    let canonical_store = cli_store_dir.pipe(fs::canonicalize).expect("canonicalize CLI store");
    let gvs_root = canonical_store.join("links");
    eprintln!("Derived global virtual store must follow the CLI store: {gvs_root:?}");
    assert!(
        canonical.starts_with(&gvs_root),
        "expected the package directory under {gvs_root:?}, got {canonical:?}",
    );

    drop((root, mock_instance));
}

#[test]
fn install_resolves_env_var_in_user_npmrc_registry() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;

    let mocked_registry_url = mock_instance.url();
    let original = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let patched = original.replace(&format!("registry={mocked_registry_url}\n"), "");
    eprintln!("npmrc_path={npmrc_path:?}\noriginal_npmrc={original:?}\npatched_npmrc={patched:?}");
    assert_ne!(original, patched, ".npmrc layout drifted; update this test");
    fs::write(&npmrc_path, &patched).expect("rewrite .npmrc");

    let user_npmrc_path = root.path().join("trusted-user.npmrc");
    fs::write(&user_npmrc_path, "registry=${PACQUET_TEST_REGISTRY}\n").expect("write user .npmrc");

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    eprintln!("Executing command with PACQUET_TEST_REGISTRY set...");
    pacquet
        .with_env("PACQUET_TEST_REGISTRY", mocked_registry_url)
        .with_arg("--npmrc-auth-file")
        .with_arg(user_npmrc_path)
        .with_arg("install")
        .assert()
        .success();

    eprintln!("Make sure the package was actually fetched from the resolved registry");
    let symlink_path = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent");
    let installed = is_symlink_or_junction(&symlink_path).unwrap();
    eprintln!("symlink_path={symlink_path:?} installed={installed}");
    assert!(installed, "expected installed symlink/junction at {symlink_path:?}");

    drop((root, mock_instance));
}

#[test]
fn install_ignores_env_var_in_project_npmrc_registry() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;

    let mocked_registry_url = mock_instance.url();
    let original = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let patched = original.replace(
        &format!("registry={mocked_registry_url}"),
        "registry=${PACQUET_TEST_REGISTRY}",
    );
    eprintln!("npmrc_path={npmrc_path:?}\noriginal_npmrc={original:?}\npatched_npmrc={patched:?}");
    assert_ne!(original, patched, ".npmrc layout drifted; update this test");
    fs::write(&npmrc_path, &patched).expect("rewrite .npmrc");

    let user_npmrc_path = root.path().join("trusted-user.npmrc");
    fs::write(&user_npmrc_path, format!("registry={mocked_registry_url}\n"))
        .expect("write user .npmrc");

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    eprintln!("Executing command with PACQUET_TEST_REGISTRY set...");
    pacquet
        .with_env("PACQUET_TEST_REGISTRY", "http://127.0.0.1:9/leaked/")
        .with_arg("--npmrc-auth-file")
        .with_arg(user_npmrc_path)
        .with_arg("install")
        .assert()
        .success();

    let symlink_path = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent");
    let installed = is_symlink_or_junction(&symlink_path).unwrap();
    assert!(installed, "expected installed symlink/junction at {symlink_path:?}");

    drop((root, mock_instance));
}

/// Fresh-install GVS regression: `pacquet install` (no flag, no
/// lockfile) on a clean project with `enableGlobalVirtualStore: true`
/// must materialize packages under the shared
/// `<store_dir>/v11/links/<scope>/<name>/<version>/<hash>` tree, not
/// the project-local `node_modules/.pnpm/` legacy layout. Pins the
/// fix for pnpm/pnpm#11814: before that fix the without-lockfile
/// path hardcoded `VirtualStoreLayout::legacy`, so the fresh-resolve
/// install silently fell through to project-local slots even with
/// GVS opted in.
///
/// Also asserts that the project gets registered under
/// `<store_dir>/v11/projects/`, mirroring the frozen-lockfile branch
/// — the prune sweep walks that directory to learn which projects
/// still reference the shared store.
#[cfg(unix)]
#[test]
fn fresh_install_honors_enable_global_virtual_store() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    enable_gvs_in_workspace_yaml(&workspace, "");

    eprintln!("Creating package.json...");
    let manifest_path = workspace.join("package.json");
    let package_json_content = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json_content.to_string()).expect("write to package.json");

    eprintln!("Running pacquet install (no flag, no lockfile, GVS opted in)...");
    pacquet
        .with_arg("install")
        .assert()
        .success();

    eprintln!("Direct-dep symlink must resolve under <store_dir>/v11/links/...");
    let symlink_path = workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent");
    assert!(is_symlink_or_junction(&symlink_path).unwrap());
    let canonical = symlink_path.pipe(fs::canonicalize).expect("canonicalize symlink");
    let canonical_store = store_dir.pipe(fs::canonicalize).expect("canonicalize store_dir");
    let gvs_root = canonical_store.join("v11").join("links");
    assert!(
        canonical.starts_with(&gvs_root),
        "expected the package directory to live under {gvs_root:?}, got {canonical:?}",
    );

    eprintln!("Project must be registered under <store_dir>/v11/projects/...");
    let projects_dir = canonical_store.join("v11").join("projects");
    let projects_entries =
        fs::read_dir(&projects_dir).expect("v11/projects must exist after a GVS install");
    let project_count = projects_entries.count();
    assert!(
        project_count >= 1,
        "expected at least one project-registry entry under {projects_dir:?}; got {project_count}",
    );

    drop((root, mock_instance));
}

/// `virtualStoreOnly` populates the virtual store and creates no
/// importer links. `.pnp.cjs` is how a `PnP` project resolves, so it is a
/// project-level artifact of the same kind and must not be written
/// either — otherwise the project claims to resolve out of a store it
/// was never linked into.
///
/// Covers the fresh-resolution path: `pnpm fetch` pins
/// `frozenLockfile`, so only a plain install reaches this one.
#[test]
fn virtual_store_only_install_under_pnp_does_not_write_the_loader() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { store_dir, mock_instance, .. } = npmrc_info;

    // Append: the harness's own workspace manifest carries `storeDir` /
    // `cacheDir` / `registry`.
    let workspace_manifest = workspace.join("pnpm-workspace.yaml");
    let mut yaml = std::fs::read_to_string(&workspace_manifest).expect("read pnpm-workspace.yaml");
    yaml.push_str("nodeLinker: pnp\nvirtualStoreOnly: true\n");
    std::fs::write(&workspace_manifest, yaml).expect("write pnpm-workspace.yaml");

    std::fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/foo":"100.0.0"}}"#,
    )
    .expect("write package.json");

    pacquet
        .with_arg("install")
        .assert()
        .success();

    assert!(
        store_dir.join(STORE_VERSION).exists(),
        "the install must still populate the store, or the assertion below is vacuous",
    );
    assert!(
        !workspace.join(".pnp.cjs").exists(),
        "virtualStoreOnly must not write the PnP loader: it links no importers",
    );

    drop((root, mock_instance, store_dir));
}

const PROD_DIRECT: (&str, &str) = ("@pnpm.e2e/has-foo-100.1.0-dep-1", "1.0.0");
/// [`PROD_DIRECT`] pins this exact version as its own dependency, so production
/// reaches it too and a `--prod` run still has to download it.
const SHARED: (&str, &str) = ("@pnpm.e2e/foo", "100.1.0");
const DEV_DIRECT: (&str, &str) = ("@pnpm.e2e/bravo", "1.0.0");
const DEV_TRANSITIVE: (&str, &str) = ("@pnpm.e2e/bravo-dep", "1.1.0");

/// `pnpm cat-index` exits non-zero with `ERR_PNPM_INVALID_PACKAGE` for a
/// package that was never fetched, which is what separates a tarball this
/// install downloaded from one it only resolved.
fn store_holds(workspace: &std::path::Path, (name, version): (&str, &str)) -> bool {
    pacquet_in(workspace)
        .with_args(["cat-index", &format!("{name}@{version}")])
        .output()
        .expect("run pnpm cat-index")
        .status
        .success()
}

/// pnpm/pnpm#881. The resolve pass still walks every dependency group, so the
/// store, not `node_modules`, is where the filter's effect on the fetch shows.
/// `install::lockfile` covers the other half, that the lockfile keeps recording
/// every group (pnpm/pnpm#14912).
fn assert_prod_install_downloads_no_dev_only_package(extra_install_args: &[&str]) {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "project",
            "version": "1.0.0",
            "dependencies": { PROD_DIRECT.0: PROD_DIRECT.1 },
            "devDependencies": { DEV_DIRECT.0: DEV_DIRECT.1, SHARED.0: SHARED.1 },
        })
        .to_string(),
    )
    .expect("write package.json");

    let mut args = vec!["install", "--prod"];
    args.extend_from_slice(extra_install_args);
    pacquet
        .with_args(args)
        .assert()
        .success();

    // The absence assertions below name an exact version, so a fixture
    // registry that gained a newer `@pnpm.e2e/bravo-dep` would make them
    // pass without testing anything. The lockfile records every group, so
    // it is where the version this graph resolves to can be checked.
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile.contains(&format!("{}@{}", DEV_TRANSITIVE.0, DEV_TRANSITIVE.1)),
        "the dev-only transitive dependency resolves to some version other than {}@{}",
        DEV_TRANSITIVE.0,
        DEV_TRANSITIVE.1,
    );

    for present in [PROD_DIRECT, SHARED] {
        assert!(
            store_holds(&workspace, present),
            "install --prod {extra_install_args:?} must download {}@{}, which production reaches",
            present.0,
            present.1,
        );
    }
    for absent in [DEV_DIRECT, DEV_TRANSITIVE] {
        assert!(
            !store_holds(&workspace, absent),
            "install --prod {extra_install_args:?} downloaded {}@{}, which only a devDependency reaches",
            absent.0,
            absent.1,
        );
    }

    drop((root, mock_instance));
}

#[test]
fn prod_install_downloads_no_dev_only_package() {
    assert_prod_install_downloads_no_dev_only_package(&[]);
}

#[test]
fn prod_install_downloads_no_dev_only_package_with_the_hoisted_linker() {
    assert_prod_install_downloads_no_dev_only_package(&["--node-linker=hoisted"]);
}
