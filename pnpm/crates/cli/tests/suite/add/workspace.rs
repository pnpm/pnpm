use super::{
    Command, CommandExtra, CommandTempCwd, DependencyGroup, HERMETIC_STORE_YAML, Lockfile,
    PackageManifest, Pipe, PkgName, TempDir, add_in, assert_eq, linking_settings, prod_spec,
    saved_spec, workspace_with_lib, write_json, write_workspace_with_local_fixtures,
};
use assert_cmd::{assert::OutputAssertExt, cargo::CommandCargoExt};

#[test]
fn add_to_multi_pattern_workspace_root_requires_workspace_root_flag() {
    let root = TempDir::new().unwrap();
    std::fs::write(root.path().join("package.json"), r#"{"name":"root","version":"1.0.0"}"#)
        .unwrap();
    std::fs::write(
        root.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n  - tools/*\n",
    )
    .unwrap();

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(root.path())
        .with_args(["add", "foo"])
        .output()
        .expect("run pnpm add");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "adding to the workspace root must fail");
    assert!(stderr.contains("ERR_PNPM_ADDING_TO_ROOT"), "unexpected stderr: {stderr}");

    let local = root.path().join("local");
    std::fs::create_dir(&local).unwrap();
    std::fs::write(local.join("package.json"), r#"{"name":"local","version":"1.0.0"}"#).unwrap();
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(root.path())
        .with_args(["add", "--ignore-workspace-root-check", "local@file:./local"])
        .assert()
        .success();
}

#[test]
fn add_accepts_multiple_local_package_selectors() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let fixtures_dir = workspace.join("fixtures");
    for package_name in ["local-a", "local-b"] {
        let package_dir = fixtures_dir.join(package_name);
        std::fs::create_dir_all(&package_dir).expect("create local package directory");
        std::fs::write(
            package_dir.join("package.json"),
            serde_json::json!({ "name": package_name, "version": "1.0.0" }).to_string(),
        )
        .expect("write local package manifest");
    }

    pacquet
        .with_args(["add", "local-a@file:./fixtures/local-a", "local-b@file:./fixtures/local-b"])
        .assert()
        .success();

    assert_eq!(prod_spec(&workspace, "local-a"), "file:./fixtures/local-a");
    assert_eq!(prod_spec(&workspace, "local-b"), "file:./fixtures/local-b");

    let lockfile_text =
        std::fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read pnpm-lock.yaml");
    let lockfile: Lockfile = serde_saphyr::from_str(&lockfile_text)
        .unwrap_or_else(|error| panic!("parse pnpm-lock.yaml: {error}\n{lockfile_text}"));
    let dependencies = lockfile
        .importers
        .get(Lockfile::ROOT_IMPORTER_KEY)
        .and_then(|importer| importer.dependencies.as_ref())
        .expect("root importer dependencies");
    for package_name in ["local-a", "local-b"] {
        let parsed_name: PkgName = package_name.parse().expect("parse local package name");
        assert!(dependencies.contains_key(&parsed_name), "lockfile contains {package_name}");
        assert!(
            workspace.join("node_modules").join(package_name).join("package.json").exists(),
            "{package_name} is installed",
        );
    }

    drop(root); // cleanup
}

/// Covers pnpm/pnpm#14618: `pnpm setup` re-runs from the globally
/// installed `@pnpm/exe`, whose directory is the symlink pnpm's own
/// `node_modules` layout puts there.
#[test]
fn add_installs_a_local_package_reached_through_a_symlinked_directory() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let real_dir = workspace.join("fixtures/real-local");
    std::fs::create_dir_all(&real_dir).expect("create local package directory");
    std::fs::write(
        real_dir.join("package.json"),
        serde_json::json!({ "name": "local", "version": "1.0.0" }).to_string(),
    )
    .expect("write local package manifest");
    std::fs::write(real_dir.join("index.js"), "module.exports = 1\n").expect("write index.js");
    pnpm_fs::symlink_dir(&real_dir, &workspace.join("fixtures/linked-local"))
        .expect("link the local package directory");

    pacquet.with_args(["add", "file:./fixtures/linked-local"]).assert().success();

    assert_eq!(prod_spec(&workspace, "local"), "file:fixtures/linked-local");
    assert!(workspace.join("node_modules/local/index.js").is_file());

    drop(root); // cleanup
}

/// End to end rather than a unit test: a relative `--dir` resolves
/// against the process cwd, which a unit test must not mutate.
#[test]
fn add_workspace_root_tolerates_a_dir_that_does_not_exist() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace_with_local_fixtures(&workspace);

    pacquet
        .with_args([
            "--dir",
            "packages/does-not-exist",
            "add",
            "-D",
            "local-a@file:./fixtures/local-a",
            "-w",
        ])
        .assert()
        .success();

    let root_manifest = workspace
        .join("package.json")
        .pipe(PackageManifest::from_path)
        .expect("read root manifest");
    assert!(
        root_manifest.dependencies([DependencyGroup::Dev]).any(|(key, _)| key == "local-a"),
        "a nonexistent --dir must still redirect the add to the root manifest",
    );

    drop(root); // cleanup
}

/// The counterpart to the tolerated nonexistent `--dir` above.
#[test]
fn add_workspace_root_rejects_a_dir_that_climbs_out_of_the_workspace() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace_with_local_fixtures(&workspace);

    let output = pacquet
        .with_args([
            "--dir",
            "../../outside-does-not-exist",
            "add",
            "-D",
            "local-a@file:./fixtures/local-a",
            "-w",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(
        stderr.contains("ERR_PNPM_NOT_IN_WORKSPACE"),
        "a --dir pointing outside the workspace must not fall back to it: {stderr}",
    );

    drop(root); // cleanup
}

/// `pnpm add -D <pkg> <pkg> -w` run from a workspace subdirectory
/// (pnpm/pnpm#13031).
#[test]
fn add_workspace_root_saves_to_the_root_manifest_from_a_subdir() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let member_dir = write_workspace_with_local_fixtures(&workspace);

    pacquet
        .with_args([
            "--dir",
            "packages/a",
            "add",
            "-D",
            // Relative to the root: a `file:` spec resolves from the
            // manifest that records it, which `-w` makes the root's.
            "local-a@file:./fixtures/local-a",
            "local-b@file:./fixtures/local-b",
            "-w",
        ])
        .assert()
        .success();

    let root_manifest = workspace
        .join("package.json")
        .pipe(PackageManifest::from_path)
        .expect("read root manifest");
    for package_name in ["local-a", "local-b"] {
        assert!(
            root_manifest.dependencies([DependencyGroup::Dev]).any(|(key, _)| key == package_name),
            "--workspace-root must save {package_name} to the root manifest",
        );
    }

    let member_manifest = member_dir
        .join("package.json")
        .pipe(PackageManifest::from_path)
        .expect("read packages/a manifest");
    assert_eq!(
        member_manifest
            .dependencies([
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
                DependencyGroup::Peer,
            ])
            .count(),
        0,
        "--workspace-root must leave the `--dir` project's manifest untouched",
    );

    drop(root); // cleanup
}

#[test]
fn add_lockfile_only_from_workspace_subdir_prints_manifest_summary() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        std::fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    std::fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    let package_dir = workspace.join("packages/a");
    std::fs::create_dir_all(&package_dir).expect("mkdir packages/a");
    std::fs::write(
        package_dir.join("package.json"),
        serde_json::json!({ "name": "a", "version": "1.0.0" }).to_string(),
    )
    .expect("write packages/a/package.json");

    let output = pacquet
        .with_args([
            "--dir",
            "packages/a",
            "--reporter=append-only",
            "add",
            "@pnpm.e2e/hello-world-js-bin",
            "--lockfile-only",
        ])
        .output()
        .expect("run pacquet add");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "add failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        stdout.contains("dependencies:\n+ @pnpm.e2e/hello-world-js-bin ^1.0.0"),
        "add --lockfile-only should print the manifest diff summary for the selected importer\nstdout:\n{stdout}",
    );

    assert_eq!(prod_spec(&package_dir, "@pnpm.e2e/hello-world-js-bin"), "^1.0.0");

    let package_dir = workspace.join("packages/b");
    std::fs::create_dir_all(&package_dir).expect("mkdir packages/b");
    std::fs::write(
        package_dir.join("package.json"),
        serde_json::json!({ "name": "b", "version": "1.0.0" }).to_string(),
    )
    .expect("write packages/b/package.json");

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args([
            "--dir",
            "packages/b",
            "--reporter=ndjson",
            "add",
            "@pnpm.e2e/hello-world-js-bin",
            "--lockfile-only",
        ])
        .output()
        .expect("run pacquet add with ndjson reporter");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "add failed\nstderr:\n{stderr}");
    let records = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .collect::<Vec<_>>();
    let initial_manifest_count = records
        .iter()
        .filter(|record| {
            record.get("name").and_then(|name| name.as_str()) == Some("pnpm:package-manifest")
                && record.get("initial").is_some()
        })
        .count();
    assert_eq!(
        initial_manifest_count, 1,
        "ndjson should emit one initial package manifest\nstderr:\n{stderr}",
    );
    let summary_count = records
        .iter()
        .filter(|record| record.get("name").and_then(|name| name.as_str()) == Some("pnpm:summary"))
        .count();
    assert_eq!(summary_count, 1, "ndjson should emit one pnpm:summary\nstderr:\n{stderr}");

    assert_eq!(prod_spec(&package_dir, "@pnpm.e2e/hello-world-js-bin"), "^1.0.0");
    drop((root, npmrc_info)); // cleanup
}

/// `saveWorkspaceProtocol` decides what `pnpm add <pkg>@workspace:…`
/// writes back. The rolling default drops the version so the range
/// never has to be rewritten when the local package is bumped; `true`
/// pins the workspace package's *actual* version (not the one the user
/// typed); `false` still honors an explicit `workspace:` request.
///
/// Verified against the TypeScript CLI for every row.
#[test]
fn save_workspace_protocol_decides_the_saved_workspace_range() {
    const LIB: &str = "@pnpm.e2e/ws-lib";
    let cases = [
        (None, "workspace:^1.2.3", "1.2.3", true, "workspace:^"),
        (None, "workspace:^1.2.3", "1.2.3", false, "workspace:^"),
        (None, "workspace:~1.2.3", "1.2.3", true, "workspace:~"),
        (None, "workspace:1.2.3", "1.2.3", true, "workspace:*"),
        (None, "workspace:*", "1.2.3", true, "workspace:*"),
        (Some("true"), "workspace:^1.2.3", "1.2.3", true, "workspace:^1.2.3"),
        // The typed `~` loses to the default `^`: the pinned form reads
        // its operator off the previous entry, and there is none here.
        (Some("true"), "workspace:~1.2.3", "1.2.3", true, "workspace:^1.2.3"),
        // The local version wins over the typed range.
        (Some("true"), "workspace:^1.0.0", "2.5.0", true, "workspace:^2.5.0"),
        // A range over a prerelease would not match it, so it is exact.
        (Some("true"), "workspace:^1.0.0", "2.0.0-beta.1", true, "workspace:2.0.0-beta.1"),
        (Some("false"), "workspace:^1.2.3", "1.2.3", true, "workspace:^1.2.3"),
    ];

    for (setting, requested, lib_version, link_workspace_packages, expected) in cases {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        let protocol_line = setting
            .map(|setting| format!("saveWorkspaceProtocol: {setting}\n"))
            .unwrap_or_default();
        let yaml = format!(
            "{HERMETIC_STORE_YAML}packages:\n  - packages/*\nlinkWorkspacePackages: {link_workspace_packages}\n{protocol_line}",
        );
        std::fs::write(workspace.join("pnpm-workspace.yaml"), yaml).expect("write workspace yaml");
        write_json(&workspace.join("package.json"), &serde_json::json!({ "name": "root" }));
        for (dir, manifest) in [
            ("lib", serde_json::json!({ "name": LIB, "version": lib_version })),
            ("app", serde_json::json!({ "name": "ws-app", "version": "1.0.0" })),
        ] {
            let package_dir = workspace.join("packages").join(dir);
            std::fs::create_dir_all(&package_dir).expect("create package dir");
            write_json(&package_dir.join("package.json"), &manifest);
        }

        let app_dir = workspace.join("packages/app");
        Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(&app_dir)
            .with_args(["add", &format!("{LIB}@{requested}"), "--lockfile-only"])
            .assert()
            .success();

        let saved = PackageManifest::from_path(app_dir.join("package.json"))
            .expect("read app manifest")
            .dependencies([DependencyGroup::Prod])
            .find(|(name, _)| *name == LIB)
            .map(|(_, spec)| spec.to_string());
        eprintln!("setting={setting:?} requested={requested} local={lib_version} -> {saved:?}");
        assert_eq!(saved.as_deref(), Some(expected));

        drop(root);
    }
}

#[test]
fn a_bare_workspace_add_uses_the_local_package_and_saved_protocol_setting() {
    const LIB: &str = "@pnpm.e2e/ws-bare";
    for (setting, expected) in
        [(None, "workspace:^"), (Some("true"), "workspace:^1.2.3"), (Some("false"), "^1.2.3")]
    {
        let (root, app_dir) = workspace_with_lib(
            &linking_settings(setting),
            &[(LIB, "1.2.3")],
            "packages/app/package.json",
        );
        add_in(&app_dir, LIB);

        assert_eq!(saved_spec(&app_dir, LIB).as_deref(), Some(expected));
        drop(root);
    }
}

/// `workspace:<target>@<range>` installs `<target>` under the name the
/// selector gave, so the saved specifier has to keep naming the target —
/// dropping it would leave a `workspace:` entry pointing at the install
/// name, which resolves to nothing.
///
/// Verified against the TypeScript CLI.
#[test]
fn an_aliased_workspace_add_keeps_naming_its_target() {
    const TARGET: &str = "@pnpm.e2e/ws-target";
    for (setting, expected) in [
        (None, "workspace:@pnpm.e2e/ws-target@^"),
        (Some("true"), "workspace:@pnpm.e2e/ws-target@^1.2.3"),
    ] {
        let (root, app_dir) = workspace_with_lib(
            &linking_settings(setting),
            &[(TARGET, "1.2.3")],
            "packages/app/package.json",
        );
        add_in(&app_dir, &format!("myalias@workspace:{TARGET}@^1.0.0"));

        assert_eq!(saved_spec(&app_dir, "myalias").as_deref(), Some(expected));
        drop(root);
    }
}

/// The pinned form picks the workspace version by semver, not by string
/// order: a workspace holding both `9.0.0` and `10.0.0` must pin to
/// `10.0.0`, which sorts *before* `9.0.0` lexicographically.
///
/// Verified against the TypeScript CLI.
#[test]
fn the_pinned_form_picks_the_highest_workspace_version_by_semver() {
    const LIB: &str = "@pnpm.e2e/ws-multi";
    let (root, app_dir) = workspace_with_lib(
        &linking_settings(Some("true")),
        &[(LIB, "9.0.0"), (LIB, "10.0.0")],
        "packages/app/package.json",
    );
    add_in(&app_dir, &format!("{LIB}@workspace:*"));

    assert_eq!(saved_spec(&app_dir, LIB).as_deref(), Some("workspace:^10.0.0"));
    drop(root);
}

/// The setting is readable from `PNPM_CONFIG_SAVE_WORKSPACE_PROTOCOL`
/// too, not only `pnpm-workspace.yaml` — pnpm exposes every setting
/// through its env pass.
#[test]
fn the_env_var_drives_the_saved_workspace_range() {
    const LIB: &str = "@pnpm.e2e/ws-env";
    for (value, expected) in
        [("true", "workspace:^1.2.3"), ("rolling", "workspace:^"), ("false", "workspace:^1.2.3")]
    {
        let (root, app_dir) = workspace_with_lib(
            &linking_settings(None),
            &[(LIB, "1.2.3")],
            "packages/app/package.json",
        );
        Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(&app_dir)
            .with_args(["add", &format!("{LIB}@workspace:^1.0.0"), "--lockfile-only"])
            .env("PNPM_CONFIG_SAVE_WORKSPACE_PROTOCOL", value)
            .assert()
            .success();

        eprintln!("PNPM_CONFIG_SAVE_WORKSPACE_PROTOCOL={value} -> {:?}", saved_spec(&app_dir, LIB));
        assert_eq!(saved_spec(&app_dir, LIB).as_deref(), Some(expected));
        drop(root);
    }
}
