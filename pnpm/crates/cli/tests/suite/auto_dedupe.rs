use crate::_utils::{importer_version, read_lockfile};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    command_env::CommandTestExt,
};
use std::{fs, path::Path, process::Command};

pub(crate) const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const PARENT: &str = "@pnpm.e2e/pkg-with-1-dep";

pub(crate) fn pnpm_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm")
        .unwrap()
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
}

pub(crate) fn write_settings(workspace: &Path, settings: impl AsRef<str>) -> std::io::Result<()> {
    let path = workspace.join("pnpm-workspace.yaml");
    let mut current: serde_json::Map<String, serde_json::Value> =
        serde_saphyr::from_str(&fs::read_to_string(&path)?).unwrap();
    current.retain(|key, _| {
        matches!(key.as_str(), "storeDir" | "cacheDir" | "enableGlobalVirtualStore")
    });
    let new: serde_json::Map<String, serde_json::Value> =
        serde_saphyr::from_str(settings.as_ref()).unwrap();
    current.extend(new);
    fs::write(path, serde_saphyr::to_string(&current).unwrap())
}

pub(crate) fn write_project(workspace: &Path, project: &str, version: &str) {
    let dir = workspace.join(project);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.json"),
        serde_json::json!({
            "name": project,
            "dependencies": {DEP: version, PARENT: "100.0.0"},
        })
        .to_string(),
    )
    .unwrap();
}

fn prepare_duplicates(workspace: &Path) {
    write_settings(workspace, "packages:\n  - low\n  - high\n").unwrap();
    write_project(workspace, "low", "100.0.0");
    write_project(workspace, "high", "100.1.0");
    pnpm_at(workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    write_project(workspace, "low", "^100.0.0");
    let mut lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    let low = lockfile.importers.get_mut("low").unwrap();
    low.dependencies
        .as_mut()
        .unwrap()
        .get_mut(&DEP.parse().unwrap())
        .unwrap()
        .specifier = "^100.0.0".into();
    lockfile
        .save_to_path(&workspace.join("pnpm-lock.yaml"))
        .unwrap();
}

#[test]
fn enabling_auto_dedupe_consolidates_existing_versions_without_changing_lockfile_format() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    prepare_duplicates(&workspace);
    pnpm_at(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    assert_eq!(
        importer_version(&read_lockfile(&workspace.join("pnpm-lock.yaml")), "low", DEP),
        "100.0.0",
    );
    pnpm_at(&workspace)
        .with_args(["install", "--auto-dedupe", "--lockfile-only"])
        .assert()
        .success();
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_version(&lockfile, "low", DEP), "100.1.0");
    assert!(
        !lockfile.packages
            .as_ref()
            .unwrap()
            .contains_key(&format!("{DEP}@100.0.0").parse().unwrap()),
    );
    assert!(lockfile.extra.is_empty());
    drop((root, mock_instance));
}

#[test]
fn frozen_install_does_not_record_a_dedupe_baseline_and_repeat_install_skips_after_resolution() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    prepare_duplicates(&workspace);
    write_settings(&workspace, "packages:\n  - low\n  - high\nautoDedupe: true\n").unwrap();
    let path = workspace.join("pnpm-lock.yaml");
    let original = fs::read(&path).unwrap();
    pnpm_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_eq!(fs::read(&path).unwrap(), original);
    pnpm_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(
        importer_version(&read_lockfile(&workspace.join("pnpm-lock.yaml")), "low", DEP),
        "100.1.0",
    );
    let output = pnpm_at(&workspace)
        .with_args(["install", "--offline"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Already up to date"), "{stdout}");
    drop((root, mock_instance));
}

/// The `autoDedupe` the workspace state records, `None` when the key is
/// absent — what the next command's settings comparison reads as "no dedupe
/// baseline has been established".
fn recorded_auto_dedupe(workspace: &Path) -> Option<bool> {
    let path = workspace.join("node_modules/.pnpm-workspace-state-v1.json");
    let state: pnpm_workspace_state::WorkspaceState =
        serde_json::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    state.settings.auto_dedupe
}

/// Gives a project a `hello` script for the verify-deps gate to run. `node
/// -e` is the portable stand-in for the shell programs Windows has none of.
fn write_project_with_script(workspace: &Path, project: &str, version: &str) {
    let dir = workspace.join(project);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("package.json"),
        serde_json::json!({
            "name": project,
            "dependencies": {DEP: version, PARENT: "100.0.0"},
            "scripts": { "hello": r#"node -e "console.log('script-output')""# },
        })
        .to_string(),
    )
    .unwrap();
}

/// The workspace root is an importer of the shared lockfile, and the run
/// gate reads its manifest before it can check anything at all.
fn write_root_manifest(workspace: &Path) {
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "private": true }).to_string(),
    )
    .unwrap();
}

/// The `$ <script>` echo a gated run writes to stderr; an install the gate
/// spawns adds its own output below it.
const SCRIPT_ECHO: &str = r#"$ node -e "console.log('script-output')""#;

/// Runs a project's script through the verify-deps gate with the given CI
/// verdict, which decides whether the install the gate spawns is frozen.
fn gate_run(project_dir: &Path, ci: &str) -> std::process::Output {
    pnpm_at(project_dir)
        .with_env("PNPM_CONFIG_CI", ci)
        .with_args(["run", "hello"])
        .output()
        .unwrap()
}

/// A no-op frozen install still refreshes the workspace state, and that
/// refresh must carry the recorded dedupe baseline forward: dropping it
/// makes the next command treat the tree as one that was never deduped
/// ([pnpm/pnpm#16374](https://github.com/pnpm/pnpm/issues/16374)).
#[test]
fn an_up_to_date_frozen_install_keeps_the_recorded_dedupe_baseline() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    write_settings(&workspace, "packages:\n  - low\n  - high\nautoDedupe: true\n").unwrap();
    write_project_with_script(&workspace, "low", "100.0.0");
    write_project(&workspace, "high", "100.1.0");
    pnpm_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(recorded_auto_dedupe(&workspace), Some(true));

    let output = pnpm_at(&workspace)
        .with_env("PNPM_CONFIG_CI", "true")
        .with_args(["install", "--frozen-lockfile"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");

    assert_eq!(
        recorded_auto_dedupe(&workspace),
        Some(true),
        "the up-to-date refresh must keep the baseline the resolving install recorded",
    );
    drop((root, mock_instance));
}

/// On CI the install the verify-deps gate spawns runs frozen, so it never
/// re-resolves and can never record the dedupe baseline a pending
/// `autoDedupe` setting asks for. The gate must not spawn an install before
/// every script for it ([pnpm/pnpm#16374](https://github.com/pnpm/pnpm/issues/16374)).
#[test]
fn a_pending_dedupe_baseline_does_not_spawn_an_install_on_ci() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    // Installed without the setting, so the recorded settings carry no
    // dedupe baseline.
    write_root_manifest(&workspace);
    write_settings(&workspace, "packages:\n  - low\n  - high\n").unwrap();
    write_project_with_script(&workspace, "low", "100.0.0");
    write_project(&workspace, "high", "100.1.0");
    pnpm_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    // Enabling it now leaves the baseline pending.
    write_settings(&workspace, "packages:\n  - low\n  - high\nautoDedupe: true\n").unwrap();
    assert_eq!(recorded_auto_dedupe(&workspace), None);

    let output = gate_run(&workspace.join("low"), "true");
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("script-output"), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr.trim(),
        SCRIPT_ECHO,
        "the gate must not spawn an install a frozen lockfile cannot complete",
    );

    // Outside CI the spawned install can resolve, so the gate still runs
    // one, and the baseline it records settles the runs after it.
    let output = gate_run(&workspace.join("low"), "false");
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Done in"),
        "a resolving install must still run for the pending baseline: {stderr}",
    );
    assert_eq!(recorded_auto_dedupe(&workspace), Some(true));
    let output = gate_run(&workspace.join("low"), "false");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        SCRIPT_ECHO,
        "the recorded baseline must settle the gate",
    );
    drop((root, mock_instance));
}

#[test]
fn auto_dedupe_preserves_incompatible_exact_versions_and_can_be_disabled() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    prepare_duplicates(&workspace);
    write_settings(&workspace, "packages:\n  - low\n  - high\nautoDedupe: true\n").unwrap();
    pnpm_at(&workspace)
        .with_args(["install", "--no-auto-dedupe", "--lockfile-only"])
        .assert()
        .success();
    assert_eq!(
        importer_version(&read_lockfile(&workspace.join("pnpm-lock.yaml")), "low", DEP),
        "100.0.0",
    );
    write_project(&workspace, "low", "100.0.0");
    pnpm_at(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_version(&lockfile, "low", DEP), "100.0.0");
    assert_eq!(importer_version(&lockfile, "high", DEP), "100.1.0");
    drop((root, mock_instance));
}

#[test]
fn partial_change_discovers_a_new_candidate_and_updates_existing_consumers() {
    for lockfile_only in [true, false] {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        write_settings(&workspace, "packages:\n  - low\n  - high\n").unwrap();
        write_project(&workspace, "low", "100.0.0");
        write_project(&workspace, "high", "100.0.0");
        pnpm_at(&workspace)
            .with_args(["install", "--lockfile-only"])
            .assert()
            .success();
        write_project(&workspace, "low", "^100.0.0");
        pnpm_at(&workspace)
            .with_args(["install", "--lockfile-only"])
            .assert()
            .success();
        assert_eq!(
            importer_version(&read_lockfile(&workspace.join("pnpm-lock.yaml")), "low", DEP),
            "100.0.0",
        );
        let mut command = pnpm_at(&workspace.join("high"));
        command.args(["add", &format!("{DEP}@100.1.0"), "--auto-dedupe"]);
        if lockfile_only {
            command.arg("--lockfile-only");
        }
        command.assert().success();
        if !lockfile_only {
            let parent =
                fs::canonicalize(workspace.join("high/node_modules").join(PARENT)).unwrap();
            let dependency = parent
                .parent()
                .unwrap()
                .join("dep-of-pkg-with-1-dep/package.json");
            let manifest: serde_json::Value =
                serde_json::from_slice(&fs::read(dependency).unwrap()).unwrap();
            assert_eq!(manifest["version"], "100.1.0");
        }
        let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
        assert_eq!(importer_version(&lockfile, "low", DEP), "100.1.0");
        assert_eq!(importer_version(&lockfile, "high", DEP), "100.1.0");
        assert!(
            !lockfile.packages
                .as_ref()
                .unwrap()
                .contains_key(&format!("{DEP}@100.0.0").parse().unwrap()),
        );
        drop((root, npmrc_info));
    }
}

#[test]
fn auto_dedupe_respects_overrides_and_explicit_updates() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    prepare_duplicates(&workspace);
    write_settings(
        &workspace,
        format!("packages:\n  - low\n  - high\nautoDedupe: true\noverrides:\n  '{DEP}': 100.0.0\n"),
    )
    .unwrap();
    pnpm_at(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_version(&lockfile, "low", DEP), "100.0.0");
    assert_eq!(importer_version(&lockfile, "high", DEP), "100.0.0");
    write_settings(&workspace, "packages:\n  - low\n  - high\nautoDedupe: true\n").unwrap();
    pnpm_at(&workspace)
        .with_args(["update", "-r", DEP, "--latest", "--lockfile-only"])
        .assert()
        .success();
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_version(&lockfile, "low", DEP), "101.0.0");
    assert_eq!(importer_version(&lockfile, "high", DEP), "101.0.0");
    drop((root, npmrc_info));
}

#[test]
fn auto_dedupe_from_environment_preserves_incompatible_auto_installed_peers() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_settings(&workspace, "packages:\n  - low\n  - high\n").unwrap();
    for (project, dependencies) in [
        ("low", serde_json::json!({"@pnpm.e2e/wants-peer-c-1": "1.0.0"})),
        ("high", serde_json::json!({"@pnpm.e2e/peer-c": "^2.0.0"})),
    ] {
        fs::create_dir_all(workspace.join(project)).unwrap();
        fs::write(
            workspace.join(project).join("package.json"),
            serde_json::json!({"name": project, "dependencies": dependencies}).to_string(),
        )
        .unwrap();
    }
    pnpm_at(&workspace)
        .with_args(["install", "--lockfile-only"])
        .with_env("PNPM_CONFIG_AUTO_DEDUPE", "true")
        .assert()
        .success();
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(
        importer_version(&lockfile, "low", "@pnpm.e2e/wants-peer-c-1"),
        "1.0.0(@pnpm.e2e/peer-c@1.0.1)",
    );
    assert_eq!(importer_version(&lockfile, "high", "@pnpm.e2e/peer-c"), "2.0.0");
    drop((root, npmrc_info));
}

#[test]
fn auto_dedupe_rejects_delegated_resolution_before_contacting_the_server() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_project(&workspace, "low", "100.0.0");
    let output = pnpm_at(&workspace.join("low"))
        .with_args([
            "install",
            "--auto-dedupe",
            "--pnpr-server",
            "http://127.0.0.1:1",
            "--lockfile-only",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_AUTO_DEDUPE_WITH_PNPR_SERVER"), "{stderr}");
    drop((root, npmrc_info));
}

#[test]
fn auto_dedupe_recreates_aliases_and_separate_major_versions_without_a_lockfile() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_settings(&workspace, "autoDedupe: true\n").unwrap();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": {
                "low": format!("npm:{DEP}@^100.0.0"),
                "high": format!("npm:{DEP}@100.1.0"),
                "next": format!("npm:{DEP}@101.0.0"),
                PARENT: "100.0.0",
            },
        })
        .to_string(),
    )
    .unwrap();
    let path = workspace.join("pnpm-lock.yaml");
    let mut original = None;
    for _ in 0..2 {
        pnpm_at(&workspace)
            .with_args(["install", "--lockfile-only"])
            .assert()
            .success();
        let lockfile = read_lockfile(&path);
        assert!(importer_version(&lockfile, ".", "low").ends_with("100.1.0"));
        assert!(importer_version(&lockfile, ".", "high").ends_with("100.1.0"));
        assert!(importer_version(&lockfile, ".", "next").ends_with("101.0.0"));
        assert!(
            !lockfile.packages
                .as_ref()
                .unwrap()
                .contains_key(&format!("{DEP}@100.0.0").parse().unwrap()),
        );
        let content = fs::read(&path).unwrap();
        if let Some(original) = &original {
            assert_eq!(&content, original);
        }
        original = Some(content);
        fs::remove_file(&path).unwrap();
    }
    drop((root, npmrc_info));
}

#[test]
fn auto_dedupe_discards_obsolete_versions_before_installing_their_peers() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_settings(&workspace, "packages:\n  - low\n  - high\n").unwrap();
    write_project(&workspace, "low", "100.0.0");
    write_project(&workspace, "high", "100.0.0");
    pnpm_at(&workspace)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    write_project(&workspace, "low", "^100.0.0");
    write_settings(&workspace, format!(
        "packages:\n  - low\n  - high\nautoDedupe: true\nautoInstallPeers: true\npackageExtensions:\n  '{DEP}@100.0.0':\n    peerDependencies:\n      nonexistent-obsolete-dedupe-peer: 1.0.0\n",
    )).unwrap();
    pnpm_at(&workspace.join("high"))
        .with_args(["add", &format!("{DEP}@100.1.0"), "--lockfile-only"])
        .assert()
        .success();
    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(importer_version(&lockfile, "low", DEP), "100.1.0");
    assert_eq!(importer_version(&lockfile, "high", DEP), "100.1.0");
    assert!(
        !fs::read_to_string(workspace.join("pnpm-lock.yaml"))
            .unwrap()
            .contains("nonexistent-obsolete-dedupe-peer"),
    );
    drop((root, npmrc_info));
}

#[test]
fn deduplication_converges_transitive_dependencies_on_the_direct_dependency_version() {
    let catalog = format!("catalog:\n  '{DEP}': 100.0.0\n");
    let specs = [("100.0.0", ""), ("catalog:", catalog.as_str())];
    let dedupe_commands: [&[&str]; 2] = [&["install", "--auto-dedupe"], &["dedupe"]];
    let cases = specs
        .into_iter()
        .flat_map(|spec| dedupe_commands.map(|command| (spec, command)));
    for ((spec, catalog), dedupe_command) in cases {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        write_settings(&workspace, format!("packages:\n  - pinned\n  - loose\n{catalog}")).unwrap();
        let write_manifest = |project: &str, dependencies: serde_json::Value| {
            fs::create_dir_all(workspace.join(project)).unwrap();
            fs::write(
                workspace.join(project).join("package.json"),
                serde_json::json!({"name": project, "dependencies": dependencies}).to_string(),
            )
            .unwrap();
        };
        let lockfile_has_higher_version = || {
            read_lockfile(&workspace.join("pnpm-lock.yaml")).packages
                .unwrap()
                .contains_key(&format!("{DEP}@100.1.0").parse().unwrap())
        };
        write_manifest("pinned", serde_json::json!({DEP: spec, PARENT: "100.0.0"}));
        write_manifest("loose", serde_json::json!({DEP: "100.1.0", PARENT: "100.1.0"}));
        pnpm_at(&workspace)
            .with_args(["install", "--lockfile-only"])
            .assert()
            .success();
        write_manifest("loose", serde_json::json!({PARENT: "100.1.0"}));
        pnpm_at(&workspace)
            .with_args(["install", "--lockfile-only"])
            .assert()
            .success();
        assert!(lockfile_has_higher_version(), "{spec}: the lockfile keeps the transitive pin");
        pnpm_at(&workspace)
            .with_args(dedupe_command)
            .with_arg("--lockfile-only")
            .assert()
            .success();
        assert!(
            !lockfile_has_higher_version(),
            "{spec}, {dedupe_command:?}: {}",
            fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap(),
        );
        drop((root, npmrc_info));
    }
}

#[test]
fn downgrading_a_dependency_moves_other_projects_to_the_named_version() {
    let cases = ["peerDependencies", "dependencies"]
        .into_iter()
        .flat_map(|group| [true, false].map(|filtered| (group, filtered)))
        .flat_map(|(group, filtered)| [None, Some(0)].map(|age| (group, filtered, age)));
    for (group, filtered, minimum_release_age) in cases {
        eprintln!("{group}, filtered: {filtered}, minimumReleaseAge: {minimum_release_age:?}");
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let age_setting = minimum_release_age.map_or_else(String::new, |age| {
            format!("minimumReleaseAge: {age}\n")
        });
        write_settings(
            &workspace,
            format!("packages:\n  - low\n  - high\nautoDedupe: true\n{age_setting}"),
        )
        .unwrap();
        for (project, manifest) in [
            ("low", serde_json::json!({"name": "low", "dependencies": {DEP: "100.1.0"}})),
            ("high", serde_json::json!({"name": "high", group: {DEP: "^100.0.0"}})),
        ] {
            fs::create_dir_all(workspace.join(project)).unwrap();
            fs::write(workspace.join(project).join("package.json"), manifest.to_string()).unwrap();
        }
        // A full install fills the store, which lets the resolver reuse a
        // locked version it has the manifest of without picking again.
        pnpm_at(&workspace)
            .with_arg("install")
            .assert()
            .success();
        assert_eq!(
            importer_version(&read_lockfile(&workspace.join("pnpm-lock.yaml")), "high", DEP),
            "100.1.0",
        );
        let add = [&format!("{DEP}@100.0.0"), "--lockfile-only"];
        if filtered {
            pnpm_at(&workspace)
                .with_args(["--filter=low", "add"])
                .with_args(add)
        } else {
            pnpm_at(&workspace.join("low")).with_arg("add").with_args(add)
        }
        .assert()
        .success();
        let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
        assert_eq!(importer_version(&lockfile, "low", DEP), "100.0.0");
        assert_eq!(importer_version(&lockfile, "high", DEP), "100.0.0");
        assert!(
            !lockfile.packages
                .as_ref()
                .unwrap()
                .contains_key(&format!("{DEP}@100.1.0").parse().unwrap()),
        );
        drop((root, npmrc_info));
    }
}

#[test]
fn filtered_downgrade_reaches_other_projects_on_the_next_install() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    write_settings(&workspace, "packages:\n  - low\n  - high\nautoDedupe: true\n").unwrap();
    for (project, version) in [("low", "100.1.0"), ("high", "^100.0.0")] {
        fs::create_dir_all(workspace.join(project)).unwrap();
        fs::write(
            workspace.join(project).join("package.json"),
            serde_json::json!({"name": project, "dependencies": {DEP: version}}).to_string(),
        )
        .unwrap();
    }
    pnpm_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let installed = |project: &str| {
        let manifest = workspace
            .join(project)
            .join("node_modules")
            .join(DEP)
            .join("package.json");
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
        manifest["version"].clone()
    };
    assert_eq!(installed("high"), "100.1.0");
    pnpm_at(&workspace)
        .with_args(["--filter=low", "add", &format!("{DEP}@100.0.0")])
        .assert()
        .success();
    assert_eq!(installed("low"), "100.0.0");
    assert_eq!(installed("high"), "100.1.0");
    assert_eq!(
        importer_version(&read_lockfile(&workspace.join("pnpm-lock.yaml")), "high", DEP),
        "100.0.0",
    );
    pnpm_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(installed("high"), "100.0.0");
    drop((root, npmrc_info));
}
