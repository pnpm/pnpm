use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, ManifestDeps, WorkspaceFixture, fs,
    node_major, package_map_contents, pacquet_at, read_manifest, read_pkg_version,
    root_dependency_dir, run_node_with_package_map, write_manifest, write_manifest_value,
    write_workspace_yaml,
};
use assert_cmd::assert::OutputAssertExt;

#[test]
fn package_map_resolves_declared_hoisted_dependencies_at_runtime() {
    if node_major() < 27 {
        eprintln!("skipping package-map runtime smoke: Node.js major is below 27");
        return;
    }
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\nnodeExperimentalPackageMap: true\n");

    pacquet.with_args(["install"]).assert().success();

    let root_dependency_dir = root_dependency_dir(&workspace, "@pnpm.e2e/pkg-with-1-dep");
    let smoke = root_dependency_dir.join("package-map-smoke.cjs");
    fs::write(&smoke, "require('@pnpm.e2e/dep-of-pkg-with-1-dep')\n").expect("write smoke file");
    let output = run_node_with_package_map(&workspace, &smoke);
    assert!(
        output.status.success(),
        "declared package should resolve with package map\nstdout:\n{}\nstderr:\n{}\npackage map:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        package_map_contents(&workspace),
    );

    drop((root, mock_instance));
}

#[test]
fn standard_package_map_blocks_undeclared_hoisted_dependencies_at_runtime() {
    if node_major() < 27 {
        eprintln!("skipping package-map runtime smoke: Node.js major is below 27");
        return;
    }
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({
            "@pnpm.e2e/foo": "100.0.0",
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
        }),
    );
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\nnodeExperimentalPackageMap: true\n");

    pacquet.with_args(["install"]).assert().success();

    let root_dependency_dir = root_dependency_dir(&workspace, "@pnpm.e2e/pkg-with-1-dep");
    let smoke = root_dependency_dir.join("package-map-block-smoke.cjs");
    fs::write(&smoke, "require('@pnpm.e2e/foo/package.json')\n").expect("write smoke file");
    let output = run_node_with_package_map(&workspace, &smoke);
    assert!(
        !output.status.success(),
        "undeclared hoisted package should not resolve in standard package-map mode",
    );

    drop((root, mock_instance));
}

#[test]
fn loose_package_map_allows_undeclared_hoisted_dependencies_at_runtime() {
    if node_major() < 27 {
        eprintln!("skipping package-map runtime smoke: Node.js major is below 27");
        return;
    }
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(
        &workspace,
        serde_json::json!({
            "@pnpm.e2e/foo": "100.0.0",
            "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
        }),
    );
    write_workspace_yaml(
        &workspace,
        "nodeLinker: hoisted\nnodeExperimentalPackageMap: true\nnodePackageMapType: loose\n",
    );

    pacquet.with_args(["install"]).assert().success();

    let root_dependency_dir = root_dependency_dir(&workspace, "@pnpm.e2e/pkg-with-1-dep");
    let smoke = root_dependency_dir.join("package-map-loose-smoke.cjs");
    fs::write(&smoke, "require('@pnpm.e2e/foo/package.json')\n").expect("write smoke file");
    let output = run_node_with_package_map(&workspace, &smoke);
    assert!(
        output.status.success(),
        "undeclared hoisted package should resolve in loose package-map mode\nstdout:\n{}\nstderr:\n{}\npackage map:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
        package_map_contents(&workspace),
    );

    drop((root, mock_instance));
}

/// The hoisted linker builds its package map from the real
/// `node_modules` layout rather than the virtual store, so it writes
/// the file from its own linker instead of the shared gate. Both
/// answer `nodeExperimentalPackageMap`, and nothing reads the map
/// without it.
#[test]
fn hoisted_install_writes_no_package_map_unless_the_setting_is_on() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, serde_json::json!({ "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }));
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\n");

    pacquet.with_args(["install"]).assert().success();

    assert!(
        !workspace.join("node_modules/.package-map.json").exists(),
        "a hoisted install must not write a map nothing will read",
    );

    drop((root, mock_instance));
}

/// TS: `run pre/postinstall scripts in a workspace that uses
/// node-linker=hoisted` (`lifecycleScripts.ts:718`). Two projects pin
/// `@pnpm.e2e/pre-and-postinstall-scripts-example@1` and two pin `@2`;
/// the hoisted layout keeps one version at the workspace root and
/// nests the other under its consumers, and the build step must run
/// the scripts at every materialized copy. This case retains frozen
/// reinstall coverage; fresh hoisted installs are covered below.
#[test]
fn run_pre_and_postinstall_scripts_in_a_workspace_with_hoisted_linker() {
    const SCRIPTS: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml(&format!(
        "nodeLinker: hoisted\nallowBuilds:\n  '{SCRIPTS}': true\n",
    ));
    let mut projects = Vec::new();
    for (dir, spec) in
        [("project-1", "1"), ("project-2", "1"), ("project-3", "2"), ("project-4", "2")]
    {
        projects.push(fixture.project(
            dir,
            dir,
            ManifestDeps { prod: &[(SCRIPTS, spec)], ..Default::default() },
        ));
    }
    fixture.run(["install", "--lockfile-only"]);

    fixture.run(["install", "--frozen-lockfile"]);

    assert_eq!(
        read_pkg_version(
            &fixture.workspace,
            "node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example"
        ),
        "1.0.0",
        "the majority-tie version must win the workspace-root slot, matching upstream",
    );
    for generated in ["generated-by-preinstall.js", "generated-by-postinstall.js"] {
        assert!(
            fixture.workspace.join("node_modules").join(SCRIPTS).join(generated).exists(),
            "the hoisted root copy must be built ({generated})",
        );
        // Only the versions that lost the root slot are nested, and
        // every nested copy must be built.
        for project in &projects[2..] {
            assert!(
                project.join("node_modules").join(SCRIPTS).join(generated).exists(),
                "every nested copy must be built ({generated})",
            );
        }
    }
    // Asserting the nested version too: a nested copy of the *root's*
    // version would satisfy the build checks above while still being the
    // wrong layout.
    for project in &projects[2..] {
        assert_eq!(
            read_pkg_version(project, &format!("node_modules/{SCRIPTS}")),
            "2.0.0",
            "the nested copy must be the version that lost the root slot",
        );
    }
    // The projects whose version won the root slot reach it by walking
    // up, so they must not carry a second copy of their own.
    for project in &projects[..2] {
        assert!(
            !project.join("node_modules").join(SCRIPTS).exists(),
            "a project on the hoisted version must not nest its own copy",
        );
    }
}

/// TS: `run pre/postinstall scripts. bin files should be linked in a
/// hoisted node_modules` (`hoistedNodeLinker/install.ts:187`).
#[test]
fn run_pre_and_postinstall_scripts_and_link_bins() {
    const SCRIPTS: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_manifest(&workspace, serde_json::json!({ SCRIPTS: "1.0.0" }));
    write_workspace_yaml(
        &workspace,
        &format!("nodeLinker: hoisted\nallowBuilds:\n  '{SCRIPTS}': true\n"),
    );

    pacquet.with_arg("install").assert().success();

    let package_dir = workspace.join("node_modules").join(SCRIPTS);
    assert!(!package_dir.join("generated-by-prepare.js").exists());
    assert!(package_dir.join("generated-by-preinstall.js").exists());
    assert!(package_dir.join("generated-by-postinstall.js").exists());

    drop((root, mock_instance));
}

/// TS: `running install scripts in a workspace that has no root project`
/// (`hoistedNodeLinker/install.ts:210`).
#[test]
fn running_install_scripts_in_workspace_without_root_project() {
    const SCRIPTS: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml(&format!(
        "nodeLinker: hoisted\nallowBuilds:\n  '{SCRIPTS}': true\n",
    ));
    fixture.project(
        "project-1",
        "project-1",
        ManifestDeps { prod: &[(SCRIPTS, "1.0.0")], ..Default::default() },
    );

    fixture.run(["install"]);

    assert!(
        fixture
            .workspace
            .join("node_modules")
            .join(SCRIPTS)
            .join("generated-by-preinstall.js")
            .exists(),
    );
}

/// TS: `linking bins of local projects when node-linker is set to
/// hoisted` (`hoistedNodeLinker/install.ts:262`).
#[test]
fn linking_bins_of_local_projects() {
    let fixture = WorkspaceFixture::new();
    fixture.append_workspace_yaml("nodeLinker: hoisted\n");
    let consumer = fixture.project(
        "project-1",
        "project-1",
        ManifestDeps { prod: &[("project-2", "workspace:*")], ..Default::default() },
    );
    let provider = fixture.project("project-2", "project-2", ManifestDeps::default());
    let mut provider_manifest = read_manifest(&provider);
    provider_manifest["bin"] = serde_json::json!({ "project-2": "index.js" });
    write_manifest_value(&provider, &provider_manifest);
    fs::write(provider.join("index.js"), "#!/usr/bin/env node\nconsole.log('hello')\n")
        .expect("write project bin");

    fixture.run(["install"]);

    assert!(consumer.join("node_modules/.bin/project-2").exists());
}

/// The hoisted linker turns `preferSymlinkedExecutables` on by
/// default, so on Unix `.bin` entries are symlinks to the bin file
/// instead of shell shims — pnpm's `nodeLinker: hoisted` behavior. An
/// explicit `preferSymlinkedExecutables: false` restores the shims.
#[test]
#[cfg_attr(target_os = "windows", ignore = "preferSymlinkedExecutables is inert on Windows")]
fn hoisted_linker_symlinks_bins_by_default() {
    for (yaml, expect_symlink) in [
        ("nodeLinker: hoisted\n", true),
        ("nodeLinker: hoisted\npreferSymlinkedExecutables: false\n", false),
    ] {
        let fixture = WorkspaceFixture::new();
        fixture.append_workspace_yaml(yaml);
        let consumer = fixture.project(
            "project-1",
            "project-1",
            ManifestDeps { prod: &[("project-2", "workspace:*")], ..Default::default() },
        );
        let provider = fixture.project("project-2", "project-2", ManifestDeps::default());
        let mut provider_manifest = read_manifest(&provider);
        provider_manifest["bin"] = serde_json::json!({ "project-2": "index.js" });
        write_manifest_value(&provider, &provider_manifest);
        fs::write(provider.join("index.js"), "#!/usr/bin/env node\nconsole.log('hello')\n")
            .expect("write project bin");

        fixture.run(["install"]);

        let bin = consumer.join("node_modules/.bin/project-2");
        let is_symlink =
            fs::symlink_metadata(&bin).expect("bin must exist").file_type().is_symlink();
        assert_eq!(is_symlink, expect_symlink, "yaml: {yaml}");
    }
}

/// A package the previous install ignored is judged by the build policy
/// again even though it is present. With `strictDepBuilds` turned on
/// afterwards, the next full install still fails on it.
#[test]
fn a_present_ignored_build_still_fails_a_strict_install() {
    const SCRIPTS: &str = "@pnpm.e2e/pre-and-postinstall-scripts-example";
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace, serde_json::json!({ SCRIPTS: "1.0.0" }));
    write_workspace_yaml(&workspace, "nodeLinker: hoisted\nstrictDepBuilds: false\n");
    pacquet.with_arg("install").assert().success();

    write_workspace_yaml(&workspace, "nodeLinker: hoisted\nstrictDepBuilds: true\n");
    let output =
        pacquet_at(&workspace).with_args(["add", "ms@1.0.0"]).output().expect("run pnpm add");
    assert!(!output.status.success(), "the unapproved build is still unapproved: {output:?}");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.contains("ERR_PNPM_IGNORED_BUILDS"), "stderr:\n{stderr}");

    drop((root, mock_instance));
}
