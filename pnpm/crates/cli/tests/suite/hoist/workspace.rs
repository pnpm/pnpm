use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, ManifestDeps, WorkspaceFixture, fs,
    generate_lockfile, is_symlink_or_junction, pacquet_in, repin_snapshot_dependency,
    write_workspace_yaml,
};
use assert_cmd::assert::OutputAssertExt;

/// A publicly hoisted *workspace* package's bin must land in
/// `<root>/node_modules/.bin/`. Neither of the other two bin passes
/// can produce it — `SymlinkDirectDependencies` runs before hoisting,
/// and the post-build top-level pass resolves bins out of
/// virtual-store slots, which a workspace project doesn't have — so
/// the link phase shims the hoist plan's public workspace aliases
/// directly.
#[test]
fn publicly_hoisted_workspace_package_bin_lands_in_root_bin_dir() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write root package.json");
    write_workspace_yaml(
        &workspace,
        "packages:\n  - 'packages/*'\npublicHoistPattern:\n  - '@local/*'\n",
    );

    let pkg_dir = workspace.join("packages/foo");
    fs::create_dir_all(&pkg_dir).expect("mkdir packages/foo");
    fs::write(
        pkg_dir.join("package.json"),
        serde_json::json!({
            "name": "@local/foo",
            "version": "1.0.0",
            "private": true,
            "bin": { "local-foo": "./cli.js" },
            "dependencies": { "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/foo/package.json");
    fs::write(pkg_dir.join("cli.js"), "#!/usr/bin/env node\nconsole.log('local-foo')\n")
        .expect("write packages/foo/cli.js");

    generate_lockfile(pnpm);
    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    let alias_link = workspace.join("node_modules/@local/foo");
    assert!(
        is_symlink_or_junction(&alias_link).unwrap(),
        "the workspace package must be publicly hoisted to {alias_link:?}",
    );
    let shim = workspace.join("node_modules/.bin/local-foo");
    assert!(shim.exists(), "the hoisted workspace package's bin must be shimmed at {shim:?}");

    drop((root, mock_instance));
}

/// Workspace install (pnpm/pacquet#431) lands per-importer
/// `node_modules` layouts; hoist must walk every importer's direct
/// deps, not just the root, so transitives unique to a workspace
/// project still reach the shared `<vs>/node_modules` private
/// hoist. Sets up a two-importer workspace where the workspace
/// package depends on `@pnpm.e2e/hello-world-js-bin-parent` (which
/// has `@pnpm.e2e/hello-world-js-bin` as a transitive). With the
/// default `hoistPattern: ["*"]` the transitive must end up
/// hoisted regardless of which importer dragged it in.
///
/// Pacquet-original — covers the multi-importer hoist case directly,
/// without relying on a single-project mutate-modules API pacquet
/// doesn't have.
#[test]
pub(super) fn workspace_hoist_walks_every_importer() {
    let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // Root package.json — no deps; the dependency lives only in the
    // workspace package, so the transitive can only reach the hoist
    // pass via the per-importer walk.
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write root package.json");

    // pnpm-workspace.yaml: enumerate `packages/*` (also keeps the
    // existing `storeDir`/`cacheDir` from `add_mocked_registry`).
    write_workspace_yaml(&workspace, "packages:\n  - 'packages/*'\n");

    let pkg_dir = workspace.join("packages/foo");
    fs::create_dir_all(&pkg_dir).expect("mkdir packages/foo");
    fs::write(
        pkg_dir.join("package.json"),
        serde_json::json!({
            "name": "@local/foo",
            "version": "1.0.0",
            "private": true,
            "dependencies": { "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/foo/package.json");

    generate_lockfile(pnpm);
    pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

    assert!(
        is_symlink_or_junction(&pkg_dir.join("node_modules/@pnpm.e2e/hello-world-js-bin-parent"))
            .unwrap(),
        "workspace package should have its direct dep linked under its own node_modules",
    );

    let private_hoist =
        workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/hello-world-js-bin");
    assert!(
        is_symlink_or_junction(&private_hoist).unwrap(),
        "transitive of workspace package must be privately hoisted at {private_hoist:?}",
    );

    drop((root, mock_instance));
}

/// `hoistWorkspacePackages` (default on): every named workspace
/// project is linked by name into the private hoisted modules dir,
/// pointing at the project directory itself — so anything resolving
/// from the hoisted tree can `require` workspace packages by name.
/// With `hoistWorkspacePackages: false` the name-links are absent
/// while ordinary transitive hoisting is untouched. Covers the TS
/// tail of `hoist.ts:813` too: deleting the root `node_modules` and
/// replaying `--frozen-lockfile` reproduces the same layout.
#[test]
pub(super) fn hoist_workspace_packages_links_projects_by_name() {
    for enabled in [true, false] {
        let CommandTempCwd { pacquet, pnpm, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        fs::write(
            workspace.join("package.json"),
            serde_json::json!({ "name": "root", "private": true }).to_string(),
        )
        .expect("write root package.json");

        let toggle =
            if enabled { String::new() } else { "hoistWorkspacePackages: false\n".to_string() };
        write_workspace_yaml(&workspace, &format!("packages:\n  - 'packages/*'\n{toggle}"));

        let pkg_dir = workspace.join("packages/foo");
        fs::create_dir_all(&pkg_dir).expect("mkdir packages/foo");
        fs::write(
            pkg_dir.join("package.json"),
            serde_json::json!({
                "name": "@local/foo",
                "version": "1.0.0",
                "private": true,
                "dependencies": { "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" },
            })
            .to_string(),
        )
        .expect("write packages/foo/package.json");

        generate_lockfile(pnpm);
        pacquet.with_args(["install", "--frozen-lockfile"]).assert().success();

        let assert_hoist_layout = || {
            let name_link = workspace.join("node_modules/.pnpm/node_modules/@local/foo");
            if enabled {
                assert!(
                    is_symlink_or_junction(&name_link).unwrap(),
                    "workspace project must be linked by name at {name_link:?}",
                );
                assert_eq!(
                    fs::canonicalize(&name_link).unwrap(),
                    fs::canonicalize(&pkg_dir).unwrap(),
                    "the name-link must point at the project directory",
                );
            } else {
                assert!(
                    !name_link.exists(),
                    "hoistWorkspacePackages: false must not create {name_link:?}",
                );
            }
            // Ordinary transitive hoisting is independent of the knob.
            let private_hoist =
                workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/hello-world-js-bin");
            assert!(
                is_symlink_or_junction(&private_hoist).unwrap(),
                "transitive hoisting must be unaffected (enabled={enabled})",
            );
        };
        assert_hoist_layout();

        fs::remove_dir_all(workspace.join("node_modules")).expect("remove root node_modules");
        pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
        assert_hoist_layout();

        drop((root, mock_instance));
    }
}

/// TS: `hoist packages which is in the dependencies tree of the
/// selected projects` (`hoist.ts:587`): with `hoistPattern: '*'` and a
/// lockfile that pins a different `@pnpm.e2e/foo` per project, a subset
/// install of the root plus project-2 must hoist project-2's version —
/// not the unselected project-1's, which sorts first among importers.
#[test]
fn workspace_hoist_packages_in_selected_projects_tree() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("hoistPattern:\n  - '*'\n");
    fixture.write_root_manifest("root", ManifestDeps::default());
    fixture.project(
        "project-1",
        "project-1",
        ManifestDeps { prod: &[("@pnpm.e2e/foo", "1.0.0")], ..Default::default() },
    );
    fixture.project(
        "project-2",
        "project-2",
        ManifestDeps { prod: &[("@pnpm.e2e/foo", "2.0.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);

    fixture.run(["--filter", "root", "--filter", "project-2", "install"]);

    let hoisted = fixture.workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/foo");
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(hoisted.join("package.json")).expect("read the hoisted manifest"),
    )
    .expect("parse the hoisted manifest");
    assert_eq!(manifest["version"], "2.0.0", "the selected project's version must win the hoist");
}

/// TS: `only hoist packages which is in the dependencies tree of the
/// selected projects with sub dependencies` (`hoist.ts:682`): the
/// hoisted transitives must come from the selected project's tree too.
/// The upstream test hand-writes a lockfile whose two parent versions
/// pin different subdependency versions; the port gets the same shape
/// by locking a third `dep-of-pkg-with-1-dep` version through a direct
/// dependency and repinning the unselected parent's edge to it.
#[test]
fn workspace_hoist_only_in_selected_projects_with_subdeps() {
    const PARENT: &str = "@pnpm.e2e/pkg-with-1-dep";
    const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("hoistPattern:\n  - '*'\n");
    fixture.write_root_manifest("root", ManifestDeps::default());
    fixture.project(
        "project-1",
        "project-1",
        ManifestDeps { prod: &[(PARENT, "100.0.0"), (DEP, "101.0.0")], ..Default::default() },
    );
    fixture.project(
        "project-2",
        "project-2",
        ManifestDeps { prod: &[(PARENT, "100.1.0")], ..Default::default() },
    );
    fixture.run(["install", "--lockfile-only"]);

    repin_snapshot_dependency(
        &fixture.workspace.join("pnpm-lock.yaml"),
        &format!("{PARENT}@100.0.0"),
        DEP,
        "101.0.0",
    );

    fixture.run(["--filter", "root", "--filter", "project-2", "install"]);

    for (name, version) in [(PARENT, "100.1.0"), (DEP, "100.1.0")] {
        let hoisted = fixture.workspace.join("node_modules/.pnpm/node_modules").join(name);
        let manifest: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(hoisted.join("package.json")).expect("read the hoisted manifest"),
        )
        .expect("parse the hoisted manifest");
        assert_eq!(
            manifest["version"], version,
            "{name} must be hoisted from the selected project's tree",
        );
    }
}

/// TS: `hoist-pattern: hoist all dependencies to the virtual store
/// node_modules` (`hoist.ts:341`), the frozen-reinstall tail: deleting
/// every importer's `node_modules` and replaying `--frozen-lockfile`
/// reproduces the exact hoist layout. The fresh-install half is
/// [`workspace_hoist_walks_every_importer`].
#[test]
fn workspace_hoist_all_to_virtual_store_node_modules() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_workspace_yaml(&workspace, "packages:\n  - package\n");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");
    fs::create_dir_all(workspace.join("package")).expect("create member dir");
    fs::write(
        workspace.join("package/package.json"),
        serde_json::json!({
            "name": "package",
            "dependencies": { "@pnpm.e2e/foobar": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write member package.json");

    pacquet.with_arg("install").assert().success();

    let assert_layout = || {
        assert!(workspace.join("node_modules/@pnpm.e2e/pkg-with-1-dep").exists());
        for name in ["dep-of-pkg-with-1-dep", "foobar", "foo", "bar"] {
            assert!(
                workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e").join(name).exists(),
                "expected {name} in the private hoist dir",
            );
        }
        for name in ["foobar", "foo", "bar"] {
            assert!(
                !workspace.join("node_modules/@pnpm.e2e").join(name).exists(),
                "{name} must not appear in root node_modules",
            );
        }
        assert!(workspace.join("package/node_modules/@pnpm.e2e/foobar").exists());
        for name in ["foo", "bar"] {
            assert!(
                !workspace.join("package/node_modules/@pnpm.e2e").join(name).exists(),
                "{name} must not appear in the member's node_modules",
            );
        }
    };
    assert_layout();

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove root node_modules");
    fs::remove_dir_all(workspace.join("package/node_modules")).expect("remove member node_modules");
    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert_layout();

    drop((root, mock_instance));
}

/// TS: `hoist when updating in one of the workspace projects`
/// (`hoist.ts:423`): editing one member's manifest and re-installing
/// rehoists that member's subtree without disturbing the rest.
#[test]
fn workspace_hoist_when_updating_one_project() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_workspace_yaml(&workspace, "packages:\n  - package\n");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");
    fs::create_dir_all(workspace.join("package")).expect("create member dir");
    let member_manifest = |deps: serde_json::Value| {
        serde_json::json!({ "name": "package", "dependencies": deps }).to_string()
    };
    fs::write(
        workspace.join("package/package.json"),
        member_manifest(serde_json::json!({ "@pnpm.e2e/foobar": "100.0.0" })),
    )
    .expect("write member package.json");
    pacquet.with_arg("install").assert().success();
    assert!(workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/foo").exists());

    fs::write(
        workspace.join("package/package.json"),
        member_manifest(serde_json::json!({ "@pnpm.e2e/foobarqar": "1.0.1" })),
    )
    .expect("update member package.json");
    pacquet_in(&workspace).with_arg("install").assert().success();
    assert!(workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/qar").exists());
    assert!(
        fs::symlink_metadata(workspace.join("node_modules/.pnpm/node_modules/@pnpm.e2e/foobar"))
            .is_err(),
        "the dropped dep's hoist link must be removed",
    );
    assert!(workspace.join("package/node_modules/@pnpm.e2e/foobarqar").exists());

    drop((root, mock_instance));
}

/// A workspace package that is *publicly* hoisted lands in the root
/// `node_modules`, but it is recorded in the hoist result's
/// `hoisted_workspace_aliases` rather than
/// `publicly_hoisted_aliases_with_bins`. The post-build top-level bin
/// link only sees the latter, so a bin declared by such a package
/// reaches the root `.bin` solely through the link phase's re-walk of
/// `node_modules`.
#[test]
fn publicly_hoisted_workspace_package_bins_reach_the_root_bin_dir() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true }).to_string(),
    )
    .expect("write root package.json");
    write_workspace_yaml(&workspace, "packages:\n  - 'packages/*'\npublicHoistPattern:\n  - '*'\n");

    // The root deliberately does not depend on this project, so its bin
    // can only arrive via hoisting.
    let pkg_dir = workspace.join("packages/foo");
    fs::create_dir_all(&pkg_dir).expect("mkdir packages/foo");
    fs::write(
        pkg_dir.join("package.json"),
        serde_json::json!({
            "name": "local-foo",
            "version": "1.0.0",
            "private": true,
            "bin": { "local-foo-cli": "index.js" },
            "dependencies": { "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/foo/package.json");
    fs::write(pkg_dir.join("index.js"), "#!/usr/bin/env node\n").expect("write index.js");

    let shim = workspace.join("node_modules/.bin/local-foo-cli");

    pacquet.with_arg("install").assert().success();
    assert!(shim.exists(), "fresh: the publicly hoisted workspace bin must be shimmed at {shim:?}");

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert!(
        shim.exists(),
        "frozen: the publicly hoisted workspace bin must be shimmed at {shim:?}",
    );

    drop((root, mock_instance));
}

/// A direct dependency's bin must win over a publicly hoisted
/// *workspace* package declaring the same bin name. The post-build
/// top-level link resolves precedence by [`BinOrigin`] tier, but it
/// never sees workspace-hoisted aliases, so the link phase's re-walk is
/// the only thing that shims them — and that scan treats every
/// candidate as direct. This pins which one ends up in the root `.bin`.
#[test]
fn direct_dep_bin_wins_over_a_publicly_hoisted_workspace_package() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // The direct dependency ships a `hello-world-js-bin` bin.
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "private": true,
            "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write root package.json");
    write_workspace_yaml(&workspace, "packages:\n  - 'packages/*'\npublicHoistPattern:\n  - '*'\n");

    // A workspace package claiming the same bin name.
    let pkg_dir = workspace.join("packages/collide");
    fs::create_dir_all(&pkg_dir).expect("mkdir packages/collide");
    fs::write(
        pkg_dir.join("package.json"),
        serde_json::json!({
            "name": "collide",
            "version": "1.0.0",
            "private": true,
            "bin": { "hello-world-js-bin": "index.js" },
            "dependencies": { "@pnpm.e2e/hello-world-js-bin-parent": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write packages/collide/package.json");
    fs::write(pkg_dir.join("index.js"), "#!/usr/bin/env node\n").expect("write index.js");

    let assert_direct_wins = |stage: &str| {
        let shim = fs::read_to_string(workspace.join("node_modules/.bin/hello-world-js-bin"))
            .expect("read shim");
        assert!(
            !shim.contains("packages/collide") && !shim.contains(r"packages\\collide"),
            "{stage}: the direct dependency must win over the hoisted workspace package:\n{shim}",
        );
    };

    pacquet.with_arg("install").assert().success();
    // Guard the guard: the hoisted workspace package must actually be
    // present, or the collision below is not being exercised at all.
    assert!(
        workspace.join("node_modules/collide").exists(),
        "the workspace package must be publicly hoisted for this to test anything",
    );
    assert_direct_wins("fresh");

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_in(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert!(workspace.join("node_modules/collide").exists(), "hoisted after frozen replay too");
    assert_direct_wins("frozen");

    drop((root, mock_instance));
}
