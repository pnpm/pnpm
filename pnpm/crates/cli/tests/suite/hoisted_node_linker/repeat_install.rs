use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, MetadataExt, ModulesHost, fs,
    fs_remove_dir_all, is_real_dir, is_symlink_or_junction, pacquet_at, pacquet_in,
    read_modules_manifest, read_pkg_version, retouch_recorded_integrity, symlink, write_manifest,
    write_workspace_yaml,
};
use assert_cmd::assert::OutputAssertExt;

/// TS: `run pre/postinstall scripts in a project that uses
/// node-linker=hoisted. Should not fail on repeat install`
/// (`lifecycleScripts.ts:825`).
#[test]
fn lifecycle_scripts_do_not_fail_on_repeat_hoisted_install() {
    const SCRIPTS: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_manifest(&workspace, serde_json::json!({ SCRIPTS: "1.0.0" }));
    write_workspace_yaml(
        &workspace,
        &format!(
            "nodeLinker: hoisted\nsideEffectsCacheRead: true\nsideEffectsCacheWrite: true\nallowBuilds:\n  '{SCRIPTS}': true\n",
        ),
    );
    pacquet.with_arg("install").assert().success();

    write_manifest(
        &workspace,
        serde_json::json!({
            SCRIPTS: "1.0.0",
            "example": "npm:@pnpm.e2e/pre-and-postinstall-scripts-example@2.0.0",
        }),
    );
    pacquet_in(&workspace).with_arg("install").assert().success();

    for package_dir in
        [workspace.join("node_modules").join(SCRIPTS), workspace.join("node_modules/example")]
    {
        assert!(package_dir.join("generated-by-preinstall.js").exists());
        assert!(package_dir.join("generated-by-postinstall.js").exists());
    }

    drop((root, mock_instance));
}

/// A repeat `install --frozen-lockfile` over a complete hoisted tree
/// leaves the registry packages alone: each is recorded in
/// `.modules.yaml` `hoistedLocations` and still holds a `package.json`
/// of the recorded version, so the linker skips it, as pnpm's walker
/// does through `skipFetch`.
///
/// The `file:` dependency is what makes the repeat install reach the
/// linker at all (an unchanged registry-only tree short-circuits
/// earlier), and it is the one package that must still be re-copied,
/// since its source changes without its version changing. A re-import
/// stages and swaps the package directory, so an unchanged inode is the
/// evidence a package was left alone; `Packages: +1` is the one
/// re-copied directory.
#[test]
fn a_repeat_frozen_install_leaves_present_hoisted_packages_alone() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let local = workspace.join("local-pkg");
    fs::create_dir_all(&local).expect("create the local package dir");
    let write_local = |marker: &str| {
        fs::write(
            local.join("package.json"),
            serde_json::json!({ "name": "local-pkg", "version": "1.0.0" }).to_string(),
        )
        .expect("write the local package.json");
        fs::write(local.join("marker.txt"), marker).expect("write the marker");
    };
    write_local("first");
    write_manifest(
        &workspace,
        serde_json::json!({ "send": "0.17.2", "ms": "1.0.0", "local-pkg": "file:./local-pkg" }),
    );
    // `optimisticRepeatInstall` would answer the second install from
    // the manifest mtimes without running the linker; the linker is
    // what is under test here.
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\noptimisticRepeatInstall: false\n");
    pacquet.with_args(["install"]).assert().success();

    let registry_dirs =
        ["node_modules/ms", "node_modules/send", "node_modules/send/node_modules/ms"];
    let inode = |relative: &str| fs::metadata(workspace.join(relative)).unwrap().ino();
    let before: Vec<u64> = registry_dirs.iter().map(|relative| inode(relative)).collect();

    write_local("second");
    let output = pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .output()
        .expect("run pnpm install");
    assert!(output.status.success(), "repeat install failed: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("Packages: +1\n"), "stdout:\n{stdout}");

    assert_eq!(
        fs::read_to_string(workspace.join("node_modules/local-pkg/marker.txt")).unwrap(),
        "second",
        "the directory dependency is re-copied, so the linker did run",
    );
    let after: Vec<u64> = registry_dirs.iter().map(|relative| inode(relative)).collect();
    assert_eq!(before, after, "registry packages were left in place");

    drop((root, mock_instance));
}

/// A present package is not handed to the build phase either, so its
/// lifecycle scripts do not run again over the already-built directory
/// (pnpm marks an unfetched node `isBuilt`). `pnpm rebuild` still
/// reaches it.
#[test]
fn a_repeat_frozen_install_does_not_rebuild_present_hoisted_packages() {
    const SCRIPTS: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let local = workspace.join("local-pkg");
    fs::create_dir_all(&local).expect("create the local package dir");
    fs::write(
        local.join("package.json"),
        serde_json::json!({ "name": "local-pkg", "version": "1.0.0" }).to_string(),
    )
    .expect("write the local package.json");
    write_manifest(
        &workspace,
        serde_json::json!({ SCRIPTS: "1.0.0", "local-pkg": "file:./local-pkg" }),
    );
    write_workspace_yaml(
        &workspace,
        &format!(
            "nodeLinker: hoisted\noptimisticRepeatInstall: false\nallowBuilds:\n  '{SCRIPTS}': true\n",
        ),
    );
    pacquet.with_arg("install").assert().success();

    let generated =
        workspace.join("node_modules").join(SCRIPTS).join("generated-by-postinstall.js");
    assert!(generated.exists(), "the first install ran the postinstall");
    fs::remove_file(&generated).expect("remove the postinstall output");

    pacquet_at(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();
    assert!(!generated.exists(), "a repeat install does not rerun scripts of a present package");

    pacquet_at(&workspace).with_arg("rebuild").assert().success();
    assert!(generated.exists(), "an explicit rebuild still runs them");

    drop((root, mock_instance));
}

/// The fresh-lockfile path reads the record too: adding one dependency
/// imports that one package and leaves the rest of the tree alone.
#[test]
fn adding_a_dependency_leaves_present_hoisted_packages_alone() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, serde_json::json!({ "send": "0.17.2", "ms": "1.0.0" }));
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");
    pacquet.with_args(["install"]).assert().success();

    let registry_dirs =
        ["node_modules/ms", "node_modules/send", "node_modules/send/node_modules/ms"];
    let inode = |relative: &str| fs::metadata(workspace.join(relative)).unwrap().ino();
    let before: Vec<u64> = registry_dirs.iter().map(|relative| inode(relative)).collect();

    let output = pacquet_at(&workspace)
        .with_args(["add", "is-positive@1.0.0"])
        .output()
        .expect("run pnpm add");
    assert!(output.status.success(), "add failed: {output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(stdout.contains("Packages: +1\n"), "stdout:\n{stdout}");

    assert!(is_real_dir(&workspace, "node_modules/is-positive"), "the new package landed");
    let after: Vec<u64> = registry_dirs.iter().map(|relative| inode(relative)).collect();
    assert_eq!(before, after, "the packages already in place were left alone");

    drop((root, mock_instance));
}

/// Presence is decided on disk, not from `.modules.yaml` alone. A
/// package directory the user removed is put back, and one whose
/// `package.json` no longer carries the recorded version is imported
/// again (pnpm's `dirHasPackageJsonWithVersion`).
#[test]
fn a_repeat_frozen_install_restores_a_removed_or_altered_hoisted_package() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, serde_json::json!({ "send": "0.17.2", "ms": "1.0.0" }));
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\noptimisticRepeatInstall: false\n");
    pacquet.with_args(["install"]).assert().success();

    fs_remove_dir_all(&workspace.join("node_modules/send"));
    let ms_manifest = workspace.join("node_modules/ms/package.json");
    let altered = fs::read_to_string(&ms_manifest)
        .expect("read ms/package.json")
        .replace(r#""1.0.0""#, r#""0.0.0-stale""#);
    // The file is hard linked from the content-addressable store, so
    // writing through it would rewrite the store's own copy and the
    // re-import would read the altered version straight back. Unlink
    // first, the way an editor that writes a new file would.
    fs::remove_file(&ms_manifest).expect("unlink ms/package.json");
    fs::write(&ms_manifest, altered).expect("alter ms/package.json");
    assert_eq!(read_pkg_version(&workspace, "node_modules/ms"), "0.0.0-stale");

    pacquet_at(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    assert!(is_real_dir(&workspace, "node_modules/send"), "the removed package is back");
    assert_eq!(read_pkg_version(&workspace, "node_modules/send"), "0.17.2");
    assert_eq!(
        read_pkg_version(&workspace, "node_modules/ms"),
        "1.0.0",
        "the altered package was re-imported",
    );

    drop((root, mock_instance));
}

/// A dep path survives a change of tarball URL, integrity or revision,
/// so the recorded location and the manifest version on disk can both
/// still match while the package's contents are meant to change. The
/// repeat install compares the previous install's resolution and
/// imports the package again.
#[test]
fn a_repeat_frozen_install_reimports_a_hoisted_package_whose_resolution_changed() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // The `file:` dependency carries the repeat install into the linker;
    // an unchanged registry-only tree short-circuits before it.
    let local = workspace.join("local-pkg");
    fs::create_dir_all(&local).expect("create the local package dir");
    fs::write(
        local.join("package.json"),
        serde_json::json!({ "name": "local-pkg", "version": "1.0.0" }).to_string(),
    )
    .expect("write the local package.json");
    write_manifest(
        &workspace,
        serde_json::json!({
            "ms": "1.0.0",
            "is-positive": "1.0.0",
            "local-pkg": "file:./local-pkg",
        }),
    );
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\noptimisticRepeatInstall: false\n");
    pacquet.with_args(["install"]).assert().success();

    let inode = |relative: &str| fs::metadata(workspace.join(relative)).unwrap().ino();
    let is_positive_before = inode("node_modules/is-positive");
    let ms_before = inode("node_modules/ms");

    retouch_recorded_integrity(&workspace, "is-positive@1.0.0");

    pacquet_at(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    assert_ne!(
        is_positive_before,
        inode("node_modules/is-positive"),
        "the package whose resolution changed was imported again",
    );
    assert_eq!(ms_before, inode("node_modules/ms"), "its unchanged sibling was left in place");
    assert_eq!(read_pkg_version(&workspace, "node_modules/is-positive"), "1.0.0");

    drop((root, mock_instance));
}

/// The resolution comparison is not frozen-path-only: `pnpm add` builds
/// a fresh lockfile and is handed the current one too, so a package
/// whose recorded resolution no longer matches is imported again there
/// as well. Pins the claim, which a stale comment on
/// `HoistedLinkerInputs::current_lockfile` used to contradict.
#[test]
fn adding_a_dependency_reimports_a_hoisted_package_whose_resolution_changed() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, serde_json::json!({ "ms": "1.0.0", "is-positive": "1.0.0" }));
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\noptimisticRepeatInstall: false\n");
    pacquet.with_args(["install"]).assert().success();

    let inode = |relative: &str| fs::metadata(workspace.join(relative)).unwrap().ino();
    let is_positive_before = inode("node_modules/is-positive");
    let ms_before = inode("node_modules/ms");

    retouch_recorded_integrity(&workspace, "is-positive@1.0.0");

    pacquet_at(&workspace).with_args(["add", "@pnpm.e2e/foo@100.0.0"]).assert().success();

    assert_ne!(
        is_positive_before,
        inode("node_modules/is-positive"),
        "the package whose resolution changed was imported again on the fresh path",
    );
    assert_eq!(ms_before, inode("node_modules/ms"), "its unchanged sibling was left in place");

    drop((root, mock_instance));
}

/// A package reached through a link is not present, even when the link
/// resolves to a `package.json` carrying the recorded version. The
/// hoisted linker writes real directories of regular files and
/// `import_indexed_dir` clears a link standing in that slot, so the
/// repeat install has to replace it rather than read a manifest through
/// it. Both halves of the path matter: `lstat` refuses to follow only
/// the last component, so probing the manifest still resolves a linked
/// package directory.
#[test]
fn a_repeat_frozen_install_replaces_a_hoisted_package_behind_a_link() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // The `file:` dependency is re-copied on every install, which is
    // what carries the repeat install into the linker; an unchanged
    // registry-only tree short-circuits before it.
    let local = workspace.join("local-pkg");
    fs::create_dir_all(&local).expect("create the local package dir");
    fs::write(
        local.join("package.json"),
        serde_json::json!({ "name": "local-pkg", "version": "1.0.0" }).to_string(),
    )
    .expect("write the local package.json");
    write_manifest(
        &workspace,
        serde_json::json!({ "ms": "1.0.0", "local-pkg": "file:./local-pkg" }),
    );
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\noptimisticRepeatInstall: false\n");
    pacquet.with_args(["install"]).assert().success();

    // Move the real directory aside and leave a link to it behind, so
    // the recorded location still answers with the recorded version.
    let hoisted = workspace.join("node_modules/ms");
    let elsewhere = workspace.join("ms-elsewhere");
    fs::rename(&hoisted, &elsewhere).expect("move ms aside");
    symlink(&elsewhere, &hoisted).expect("link ms back into node_modules");
    assert_eq!(read_pkg_version(&workspace, "node_modules/ms"), "1.0.0");

    pacquet_at(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    assert!(is_real_dir(&workspace, "node_modules/ms"), "the link was replaced by a real copy");
    assert_eq!(read_pkg_version(&workspace, "node_modules/ms"), "1.0.0");

    // Now leave the directory real and link only its manifest, again at
    // the recorded version.
    let manifest = hoisted.join("package.json");
    let manifest_elsewhere = workspace.join("ms-package.json");
    fs::copy(&manifest, &manifest_elsewhere).expect("copy the ms manifest aside");
    fs::remove_file(&manifest).expect("unlink the ms manifest");
    symlink(&manifest_elsewhere, &manifest).expect("link the ms manifest back");
    assert_eq!(read_pkg_version(&workspace, "node_modules/ms"), "1.0.0");

    pacquet_at(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    assert!(
        !is_symlink_or_junction(&manifest).expect("inspect the ms manifest"),
        "the linked manifest was replaced by a regular file",
    );
    assert_eq!(read_pkg_version(&workspace, "node_modules/ms"), "1.0.0");

    drop((root, mock_instance));
}

/// A scope directory turned into a link hides the same redirection one
/// level up: `lstat` follows every component but the last, so probing
/// the package directory resolves a linked `@scope` and reports the
/// link target's contents.
///
/// The link itself survives, because the import clears only the last
/// component of the path it writes. What the check buys is that the
/// package is written again rather than trusted, so the directory the
/// link points at ends up holding the package the lockfile asks for.
#[test]
fn a_repeat_frozen_install_replaces_a_hoisted_package_behind_a_linked_scope() {
    const SCOPED: &str = "@pnpm.e2e/foo";
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // The `file:` dependency carries the repeat install into the linker;
    // an unchanged registry-only tree short-circuits before it.
    let local = workspace.join("local-pkg");
    fs::create_dir_all(&local).expect("create the local package dir");
    fs::write(
        local.join("package.json"),
        serde_json::json!({ "name": "local-pkg", "version": "1.0.0" }).to_string(),
    )
    .expect("write the local package.json");
    write_manifest(
        &workspace,
        serde_json::json!({ SCOPED: "100.0.0", "local-pkg": "file:./local-pkg" }),
    );
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\noptimisticRepeatInstall: false\n");
    pacquet.with_args(["install"]).assert().success();

    // Move the whole scope directory aside and link it back, so every
    // package under it still answers with the recorded version.
    let scope = workspace.join("node_modules/@pnpm.e2e");
    let elsewhere = workspace.join("scope-elsewhere");
    fs::rename(&scope, &elsewhere).expect("move the scope directory aside");
    symlink(&elsewhere, &scope).expect("link the scope directory back");
    assert_eq!(read_pkg_version(&workspace, "node_modules/@pnpm.e2e/foo"), "100.0.0");
    // A re-import stages and swaps the package directory, so the inode
    // of what the link resolves to is the evidence it was written again.
    let linked_package = elsewhere.join("foo");
    let before = fs::metadata(&linked_package).expect("stat the linked package").ino();

    pacquet_at(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    assert_ne!(
        before,
        fs::metadata(&linked_package).expect("stat the linked package").ino(),
        "the package behind the linked scope was imported again",
    );
    assert_eq!(read_pkg_version(&workspace, "node_modules/@pnpm.e2e/foo"), "100.0.0");

    drop((root, mock_instance));
}

/// `allowBuilds` changing after the fact still reaches a present
/// package: a build the previous install ignored runs on the next plain
/// install once it is allowed, the same install the isolated linker
/// gives it.
#[test]
fn a_newly_allowed_build_runs_on_a_present_hoisted_package() {
    const SCRIPTS: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, serde_json::json!({ SCRIPTS: "1.0.0" }));
    write_workspace_yaml(
        &workspace,
        "nodeLinker: hoisted\nstrictDepBuilds: false\noptimisticRepeatInstall: false\n",
    );
    pacquet.with_arg("install").assert().success();

    let generated =
        workspace.join("node_modules").join(SCRIPTS).join("generated-by-postinstall.js");
    assert!(!generated.exists(), "the first install ignored the build");

    write_workspace_yaml(
        &workspace,
        &format!(
            "nodeLinker: hoisted\nstrictDepBuilds: false\noptimisticRepeatInstall: false\nallowBuilds:\n  '{SCRIPTS}': true\n",
        ),
    );
    pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(generated.exists(), "the newly allowed build ran on the package already in place");

    drop((root, mock_instance));
}

/// A present package that was already built is not recorded as a
/// pending build by a repeat `--ignore-scripts` install; only what this
/// install imported can be pending.
#[test]
fn a_repeat_ignore_scripts_install_does_not_defer_present_hoisted_builds() {
    const SCRIPTS: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let local = workspace.join("local-pkg");
    fs::create_dir_all(&local).expect("create the local package dir");
    fs::write(
        local.join("package.json"),
        serde_json::json!({ "name": "local-pkg", "version": "1.0.0" }).to_string(),
    )
    .expect("write the local package.json");
    write_manifest(
        &workspace,
        serde_json::json!({ SCRIPTS: "1.0.0", "local-pkg": "file:./local-pkg" }),
    );
    // `sideEffectsCache: false` takes the deferral shortcut that records
    // pending builds straight from the materialized list.
    write_workspace_yaml(
        &workspace,
        &format!(
            "nodeLinker: hoisted\noptimisticRepeatInstall: false\nsideEffectsCache: false\nallowBuilds:\n  '{SCRIPTS}': true\n",
        ),
    );
    pacquet.with_arg("install").assert().success();
    assert!(
        workspace.join("node_modules").join(SCRIPTS).join("generated-by-postinstall.js").exists(),
    );

    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile", "--ignore-scripts"])
        .assert()
        .success();

    let pending = read_modules_manifest::<ModulesHost>(&workspace.join("node_modules"))
        .expect("read .modules.yaml")
        .expect(".modules.yaml exists")
        .pending_builds;
    assert!(
        !pending.iter().any(|entry| entry.contains(SCRIPTS)),
        "the already-built package is not pending; got {pending:?}",
    );

    drop((root, mock_instance));
}

/// An explicit `allowBuilds: false` becoming `true` leaves no ignored
/// entry behind, so it is caught by comparing the recorded map instead;
/// the present package is built on the next full install.
#[test]
fn an_explicit_denial_turned_approval_builds_a_present_hoisted_package() {
    const SCRIPTS: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, serde_json::json!({ SCRIPTS: "1.0.0" }));
    write_workspace_yaml(
        &workspace,
        &format!("nodeLinker: hoisted\nallowBuilds:\n  '{SCRIPTS}': false\n"),
    );
    pacquet.with_arg("install").assert().success();
    let generated =
        workspace.join("node_modules").join(SCRIPTS).join("generated-by-postinstall.js");
    assert!(!generated.exists(), "the denied build did not run");

    write_workspace_yaml(
        &workspace,
        &format!("nodeLinker: hoisted\nallowBuilds:\n  '{SCRIPTS}': true\n"),
    );
    pacquet_at(&workspace).with_args(["add", "ms@1.0.0"]).assert().success();
    assert!(generated.exists(), "the newly approved build ran on the package already in place");

    drop((root, mock_instance));
}
