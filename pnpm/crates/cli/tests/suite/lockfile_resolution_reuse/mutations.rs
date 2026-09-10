use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, dead_registry_url, fs, pacquet_at,
    write_two_member_workspace,
};
use assert_cmd::assert::OutputAssertExt;

#[test]
fn adding_and_removing_an_ignored_optional_dependency_uses_the_safe_path() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    let manifest_path = workspace.join("package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-good-optional": "1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}trustLockfile: true\n"))
        .expect("enable trusted lockfile");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!("{workspace_yaml}ignoredOptionalDependencies:\n  - is-positive\n"),
    )
    .expect("add ignored optional dependency");
    let dead_registry = dead_registry_url();
    let live_npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead_npmrc = live_npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{dead_npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
    );

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let current = pnpm_lockfile::Lockfile::load_current_from_virtual_store_dir(
        &workspace.join("node_modules/.pnpm"),
    )
    .expect("load current lockfile")
    .expect("current lockfile");
    let parent_key = "@pnpm.e2e/pkg-with-good-optional@1.0.0".parse().expect("parent package key");
    let removed_key = "is-positive@1.0.0".parse().expect("removed package key");
    let removed_name = "is-positive".parse().expect("removed package name");
    for lockfile in [&wanted, &current] {
        assert_eq!(
            lockfile.ignored_optional_dependencies.as_deref(),
            Some(["is-positive".to_string()].as_slice()),
        );
        assert!(
            lockfile
                .snapshots
                .as_ref()
                .and_then(|snapshots| snapshots.get(&parent_key))
                .and_then(|snapshot| snapshot.optional_dependencies.as_ref())
                .is_none_or(|dependencies| !dependencies.contains_key(&removed_name)),
        );
        assert!(
            lockfile
                .snapshots
                .as_ref()
                .is_none_or(|snapshots| !snapshots.contains_key(&removed_key)),
        );
        assert!(
            lockfile.packages.as_ref().is_none_or(|packages| !packages.contains_key(&removed_key)),
        );
    }
    assert!(
        !workspace
            .join(
                "node_modules/.pnpm/@pnpm.e2e+pkg-with-good-optional@1.0.0/node_modules/is-positive",
            )
            .exists(),
    );

    fs::write(&npmrc_path, live_npmrc).expect("restore live registry");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        workspace_yaml.replace("ignoredOptionalDependencies:\n  - is-positive\n", ""),
    )
    .expect("remove ignored optional dependency");
    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        !String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
    );

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let parent_key = "@pnpm.e2e/pkg-with-good-optional@1.0.0".parse().expect("parent package key");
    let restored_key = "is-positive@1.0.0".parse().expect("restored package key");
    let restored_name = "is-positive".parse().expect("restored package name");
    assert!(
        wanted
            .snapshots
            .as_ref()
            .and_then(|snapshots| snapshots.get(&parent_key))
            .and_then(|snapshot| snapshot.optional_dependencies.as_ref())
            .is_some_and(|dependencies| dependencies.contains_key(&restored_name)),
    );
    assert!(
        wanted.snapshots.as_ref().is_some_and(|snapshots| snapshots.contains_key(&restored_key)),
    );
    assert!(
        workspace
            .join(
                "node_modules/.pnpm/@pnpm.e2e+pkg-with-good-optional@1.0.0/node_modules/is-positive",
            )
            .exists(),
    );

    drop((root, mock_instance));
}

#[test]
fn dropping_a_dependency_from_the_manifest_skips_resolution() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    let manifest_path = workspace.join("package.json");
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
                "is-positive": "1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}trustLockfile: true\n"))
        .expect("enable trusted lockfile");
    pacquet_at(&workspace).with_arg("install").assert().success();

    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0"
            }
        })
        .to_string(),
    )
    .expect("drop is-positive from package.json");
    let dead_registry = dead_registry_url();
    let live_npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead_npmrc = live_npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{dead_npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "dropping an importer edge needs no resolution",
    );
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    assert!(
        !wanted
            .packages
            .as_ref()
            .expect("packages")
            .keys()
            .any(|key| key.to_string().starts_with("is-positive@")),
        "the dropped package is pruned from the lockfile",
    );
    assert!(
        !workspace.join("node_modules").join("is-positive").exists(),
        "and unlinked from node_modules",
    );

    drop((root, mock_instance));
}

#[test]
fn remove_command_drops_the_dependency_without_resolving() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
                "is-positive": "1.0.0"
            },
            "scripts": {
                "postinstall": r#"node -e "require('fs').writeFileSync('postinstall-ran','')""#
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}trustLockfile: true\n"))
        .expect("enable trusted lockfile");
    pacquet_at(&workspace).with_arg("install").assert().success();
    fs::remove_file(workspace.join("postinstall-ran"))
        .expect("the full install ran the project postinstall");

    let dead_registry = dead_registry_url();
    let live_npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead_npmrc = live_npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{dead_npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    let assert = pacquet_at(&workspace).with_args(["remove", "is-positive"]).assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "removing a dependency needs no resolution",
    );
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
    )
    .expect("parse package.json");
    assert!(manifest["dependencies"].get("is-positive").is_none(), "the manifest entry is gone");
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    assert!(
        !wanted
            .packages
            .as_ref()
            .expect("packages")
            .keys()
            .any(|key| key.to_string().starts_with("is-positive@")),
        "the removed package is pruned from the lockfile",
    );
    assert!(
        !workspace.join("node_modules").join("is-positive").exists(),
        "and unlinked from node_modules",
    );
    assert!(
        manifest["dependencies"].get("@pnpm.e2e/pkg-with-1-dep").is_some(),
        "the surviving dependency keeps its manifest entry",
    );
    assert!(
        wanted.importers["."].dependencies.as_ref().is_some_and(|dependencies| {
            dependencies.contains_key(&"@pnpm.e2e/pkg-with-1-dep".parse().expect("alias"))
        }),
        "and its importer entry",
    );
    assert!(
        workspace.join("node_modules").join("@pnpm.e2e").join("pkg-with-1-dep").exists(),
        "and its node_modules link",
    );
    assert!(
        !workspace.join("postinstall-ran").exists(),
        "a remove runs no project lifecycle script",
    );

    drop((root, mock_instance));
}

#[test]
fn moving_a_dependency_between_groups_skips_resolution() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
                "is-positive": "1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    pacquet_at(&workspace).with_arg("install").assert().success();

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "is-positive": "1.0.0"
            },
            "optionalDependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0"
            }
        })
        .to_string(),
    )
    .expect("move the dependency to optionalDependencies");
    let dead_registry = dead_registry_url();
    let live_npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead_npmrc = live_npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{dead_npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "moving a dependency between groups needs no resolution",
    );
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let importer = &wanted.importers["."];
    let moved_alias = "@pnpm.e2e/pkg-with-1-dep".parse().expect("alias");
    assert!(
        importer
            .optional_dependencies
            .as_ref()
            .is_some_and(|dependencies| dependencies.contains_key(&moved_alias)),
        "the importer records the moved dependency under optionalDependencies",
    );
    assert!(
        !importer
            .dependencies
            .as_ref()
            .is_some_and(|dependencies| dependencies.contains_key(&moved_alias)),
        "and no longer under dependencies",
    );
    let snapshots = wanted.snapshots.as_ref().expect("snapshots");
    for prefix in ["@pnpm.e2e/pkg-with-1-dep@", "@pnpm.e2e/dep-of-pkg-with-1-dep@"] {
        let (key, snapshot) = snapshots
            .iter()
            .find(|(key, _)| key.to_string().starts_with(prefix))
            .expect("moved subtree snapshot");
        assert!(snapshot.optional, "{key} is only reachable through an optional edge now");
    }
    let is_positive_snapshot = snapshots
        .iter()
        .find_map(|(key, snapshot)| key.to_string().starts_with("is-positive@").then_some(snapshot))
        .expect("is-positive snapshot");
    assert!(!is_positive_snapshot.optional, "the untouched prod dependency keeps its flags");
    assert!(
        workspace.join("node_modules").join("@pnpm.e2e").join("pkg-with-1-dep").exists(),
        "the moved dependency stays linked",
    );

    drop((root, mock_instance));
}

#[test]
fn a_remove_with_an_unchanged_pnpmfile_skips_resolution() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"module.exports = { hooks: { readPackage: (pkg) => {
            if (pkg.name === '@pnpm.e2e/pkg-with-1-dep') {
                pkg.dependencies['@pnpm.e2e/dep-of-pkg-with-1-dep'] = '100.0.0';
            }
            return pkg;
        } } }",
    )
    .expect("write pnpmfile");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
                "is-positive": "1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let dead_registry = dead_registry_url();
    let live_npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead_npmrc = live_npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{dead_npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    let assert = pacquet_at(&workspace).with_args(["remove", "is-positive"]).assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "an unchanged pnpmfile does not force resolution",
    );
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let snapshots = wanted.snapshots.as_ref().expect("snapshots");
    let pinned = snapshots
        .iter()
        .find(|(key, _)| key.to_string().starts_with("@pnpm.e2e/pkg-with-1-dep@"))
        .expect("hooked package snapshot")
        .1;
    assert!(
        pinned.dependencies.as_ref().is_some_and(|dependencies| dependencies
            .get(&"@pnpm.e2e/dep-of-pkg-with-1-dep".parse().expect("alias"))
            .is_some_and(|reference| reference.to_string() == "100.0.0")),
        "the hook's pin survives the fast update",
    );

    drop((root, mock_instance));
}

#[test]
fn a_remove_keeps_the_specifiers_a_project_rewriting_pnpmfile_recorded() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"module.exports = { hooks: { readPackage: (pkg) => {
            if (pkg.dependencies && pkg.dependencies['is-positive']) {
                pkg.dependencies['is-positive'] = '1.0.0';
            }
            return pkg;
        } } }",
    )
    .expect("write pnpmfile");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "@pnpm.e2e/pkg-with-1-dep": "100.0.0",
                "is-positive": "^1.0.0"
            }
        })
        .to_string(),
    )
    .expect("write package.json");
    pacquet_at(&workspace).with_arg("install").assert().success();
    let recorded_specifier = |lockfile: &pnpm_lockfile::Lockfile| {
        lockfile.importers["."].dependencies.as_ref().expect("dependencies")
            [&"is-positive".parse().expect("alias")]
            .specifier
            .clone()
    };
    let initial = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile");
    let initial_specifier = recorded_specifier(&initial);

    let dead_registry = dead_registry_url();
    let live_npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead_npmrc = live_npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{dead_npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    let assert =
        pacquet_at(&workspace).with_args(["remove", "@pnpm.e2e/pkg-with-1-dep"]).assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "the removal needs no resolution",
    );
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    assert_eq!(
        recorded_specifier(&wanted),
        initial_specifier,
        "the fast update must not rewrite the specifier the hooked resolution recorded",
    );

    drop((root, mock_instance));
}

#[test]
fn a_frozen_install_tolerates_the_importer_of_a_removed_workspace_project() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_two_member_workspace(&workspace);
    pacquet_at(&workspace).with_arg("install").assert().success();

    fs::remove_dir_all(workspace.join("packages/b")).expect("remove the member");

    // pnpm's importer-set gate lives in the auto-frozen branch, which an
    // explicit `--frozen-lockfile` short-circuits past.
    pacquet_at(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    drop((root, mock_instance));
}

#[test]
fn removing_a_workspace_project_prunes_its_importer_without_resolving() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, npmrc_path, .. } = npmrc_info;
    write_two_member_workspace(&workspace);
    pacquet_at(&workspace).with_arg("install").assert().success();

    fs::remove_dir_all(workspace.join("packages/b")).expect("remove the member");
    let dead_registry = dead_registry_url();
    let live_npmrc = fs::read_to_string(&npmrc_path).expect("read .npmrc");
    let dead_npmrc = live_npmrc
        .lines()
        .filter(|line| !line.trim_start().starts_with("registry="))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&npmrc_path, format!("registry={dead_registry}\n{dead_npmrc}\n"))
        .expect("rewrite .npmrc with a dead registry");

    let assert = pacquet_at(&workspace).with_arg("install").assert().success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "dropping a workspace project needs no resolution",
    );
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let mut importers: Vec<_> = wanted.importers.keys().map(String::as_str).collect();
    importers.sort_unstable();
    assert_eq!(
        importers,
        vec![".", "packages/a"],
        "the departed project's importer is gone, the root and its sibling stay",
    );
    assert!(
        !wanted
            .packages
            .as_ref()
            .expect("packages")
            .keys()
            .any(|key| key.to_string().starts_with("@pnpm.e2e/bar@")),
        "and so is what only it depended on",
    );

    drop((root, mock_instance));
}

#[test]
fn add_command_reuses_a_locked_version_without_resolving() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}trustLockfile: true\n"))
        .expect("enable trusted lockfile");
    pacquet_at(&workspace).with_arg("install").assert().success();

    // The transitive `^100.0.0` of `@pnpm.e2e/pkg-with-1-dep` locks
    // `100.1.0`, so promoting it to a direct dependency at that version
    // changes nothing but the importer edge.
    let assert = pacquet_at(&workspace)
        .with_args(["add", "@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0"])
        .assert()
        .success();
    assert!(
        String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "adding an already-locked version needs no resolution",
    );

    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
    )
    .expect("parse package.json");
    assert_eq!(manifest["dependencies"]["@pnpm.e2e/dep-of-pkg-with-1-dep"], "100.1.0");
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let added = &wanted.importers["."].dependencies.as_ref().expect("dependencies")
        [&"@pnpm.e2e/dep-of-pkg-with-1-dep".parse().expect("alias")];
    assert_eq!(added.specifier, "100.1.0");
    assert_eq!(added.version.to_string(), "100.1.0");
    assert!(
        workspace.join("node_modules").join("@pnpm.e2e").join("dep-of-pkg-with-1-dep").exists(),
        "the added dependency is linked into node_modules",
    );

    drop((root, mock_instance));
}

#[test]
fn add_command_resolves_a_version_the_lockfile_does_not_hold() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }
        })
        .to_string(),
    )
    .expect("write package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}trustLockfile: true\n"))
        .expect("enable trusted lockfile");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let assert = pacquet_at(&workspace)
        .with_args(["add", "@pnpm.e2e/dep-of-pkg-with-1-dep@101.0.0"])
        .assert()
        .success();
    assert!(
        !String::from_utf8_lossy(&assert.get_output().stdout)
            .contains("Lockfile is up to date, resolution step is skipped"),
        "a version nothing locks has to be fetched",
    );

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    assert_eq!(
        wanted.importers["."].dependencies.as_ref().expect("dependencies")
            [&"@pnpm.e2e/dep-of-pkg-with-1-dep".parse().expect("alias")]
            .version
            .to_string(),
        "101.0.0",
    );

    drop((root, mock_instance));
}

/// `@pnpm.e2e/abc-parent-with-missing-peers` depends on `@pnpm.e2e/abc`,
/// whose peers `peer-a`, `peer-b` and `peer-c` the root provides, so the
/// lockfile holds `abc` only as a peer-suffixed snapshot. Promoting it to
/// a direct dependency has to go through the resolver: the bare
/// `abc@1.0.0` the `packages:` block names has no snapshot to link.
#[test]
fn promoting_a_peer_suffixed_transitive_dependency_resolves_its_importer_edge() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let manifest_path = workspace.join("package.json");
    let mut dependencies = serde_json::json!({
        "@pnpm.e2e/abc-parent-with-missing-peers": "1.0.0",
        "@pnpm.e2e/peer-a": "1.0.0",
        "@pnpm.e2e/peer-b": "1.0.0",
        "@pnpm.e2e/peer-c": "1.0.0",
    });
    fs::write(&manifest_path, serde_json::json!({ "dependencies": dependencies }).to_string())
        .expect("write package.json");
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(&workspace_yaml_path, format!("{workspace_yaml}trustLockfile: true\n"))
        .expect("enable trusted lockfile");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let abc: pnpm_lockfile::PkgName = "@pnpm.e2e/abc".parse().expect("package name");
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile");
    let snapshots = wanted.snapshots.as_ref().expect("snapshots");
    let locked_abc: Vec<_> = snapshots.keys().filter(|key| key.name == abc).collect();
    assert!(!locked_abc.is_empty(), "the fixture reaches abc through its parent");
    assert!(
        locked_abc.iter().all(|key| !key.suffix.peer().is_empty()),
        "the fixture holds abc only as a peer variant: {locked_abc:?}",
    );

    dependencies["@pnpm.e2e/abc"] = "1.0.0".into();
    fs::write(&manifest_path, serde_json::json!({ "dependencies": dependencies }).to_string())
        .expect("promote abc to a direct dependency");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let edge = &wanted.importers["."].dependencies.as_ref().expect("dependencies")[&abc];
    let linked: pnpm_lockfile::PackageKey =
        format!("@pnpm.e2e/abc@{}", edge.version).parse().expect("snapshot key");
    assert!(
        !linked.suffix.peer().is_empty(),
        "the importer edge carries the peers abc resolved: {}",
        edge.version,
    );
    assert!(
        wanted.snapshots.as_ref().expect("snapshots").contains_key(&linked),
        "the importer edge names a snapshot the lockfile holds: {}",
        edge.version,
    );
    assert!(
        workspace.join("node_modules").join("@pnpm.e2e").join("abc").join("package.json").exists(),
        "the promoted dependency links to a package the virtual store holds",
    );

    drop((root, mock_instance));
}

/// The new-importer shape of the same defect: a workspace member added
/// after the lockfile was written declares `@pnpm.e2e/abc`, and its whole
/// importer entry is written from the versions the lockfile already holds
/// rather than one edge at a time.
#[test]
fn a_new_workspace_member_links_a_dependency_locked_only_as_a_peer_variant() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    fs::write(
        &workspace_yaml_path,
        format!("{workspace_yaml}trustLockfile: true\npackages:\n  - packages/*\n"),
    )
    .expect("declare the workspace members");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "version": "1.0.0" }).to_string(),
    )
    .expect("write the root package.json");
    let dependencies = serde_json::json!({
        "@pnpm.e2e/abc-parent-with-missing-peers": "1.0.0",
        "@pnpm.e2e/peer-a": "1.0.0",
        "@pnpm.e2e/peer-b": "1.0.0",
        "@pnpm.e2e/peer-c": "1.0.0",
    });
    let existing = workspace.join("packages").join("existing");
    fs::create_dir_all(&existing).expect("create the member directory");
    fs::write(
        existing.join("package.json"),
        serde_json::json!({ "name": "existing", "version": "1.0.0", "dependencies": dependencies })
            .to_string(),
    )
    .expect("write the member package.json");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let mut added_dependencies = dependencies;
    added_dependencies["@pnpm.e2e/abc"] = "1.0.0".into();
    let added = workspace.join("packages").join("web-ui");
    fs::create_dir_all(&added).expect("create the new member directory");
    fs::write(
        added.join("package.json"),
        serde_json::json!({
            "name": "web-ui",
            "version": "1.0.0",
            "dependencies": added_dependencies,
        })
        .to_string(),
    )
    .expect("write the new member package.json");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let abc: pnpm_lockfile::PkgName = "@pnpm.e2e/abc".parse().expect("package name");
    let wanted = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load updated wanted lockfile")
        .expect("updated wanted lockfile");
    let edge =
        &wanted.importers["packages/web-ui"].dependencies.as_ref().expect("dependencies")[&abc];
    let linked: pnpm_lockfile::PackageKey =
        format!("@pnpm.e2e/abc@{}", edge.version).parse().expect("snapshot key");
    assert!(
        wanted.snapshots.as_ref().expect("snapshots").contains_key(&linked),
        "the new member's edge names a snapshot the lockfile holds: {}",
        edge.version,
    );
    assert!(
        added.join("node_modules").join("@pnpm.e2e").join("abc").join("package.json").exists(),
        "the new member links to a package the virtual store holds",
    );

    drop((root, mock_instance));
}
