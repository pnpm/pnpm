use super::{
    AddMockedRegistry, CommandExtra, CommandTempCwd, READ_PACKAGE_PNPMFILE,
    append_workspace_setting, fs, pacquet_in, read_package_hook_applied, remove_workspace_setting,
    write_read_package_pnpmfile,
};
use assert_cmd::assert::OutputAssertExt;

/// pnpm 10 moved the install settings out of `package.json`'s `pnpm`
/// field into `pnpm-workspace.yaml`, and warns about every migrated key
/// a manifest still declares so the setting isn't silently dropped.
/// A repository that hasn't migrated its `pnpm.overrides` would
/// otherwise see only the downstream symptom.
///
/// The message is asserted verbatim, `[WARN]` label included: it is the
/// same string pnpm's `getConfig` prints with `console.warn`. Config-load
/// warnings go to stderr, outside the reporter, so a script capturing
/// stdout never sees them mixed into the command's own output.
#[test]
fn migrated_keys_under_the_package_json_pnpm_field_are_reported() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "private": true,
            // `app` is not a key pnpm ever owned, so it must not be named.
            "pnpm": { "overrides": { "is-number": "6.0.0" }, "app": {} },
        })
        .to_string(),
    )
    .expect("write package.json");

    let assert = pacquet.with_args(["install", "--lockfile-only"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    eprintln!("STDOUT:\n{stdout}\nSTDERR:\n{stderr}");
    assert!(
        stderr.contains(
            "[WARN] The \"pnpm\" field in package.json is no longer read by pnpm. \
             The following keys were ignored: \"pnpm.overrides\". \
             See https://pnpm.io/settings for the new home of each setting.",
        ),
        "expected the ignored-field warning on stderr; got:\n{stderr}",
    );
    assert!(
        !stdout.contains("no longer read by pnpm"),
        "the warning must stay out of stdout; got:\n{stdout}",
    );

    drop(root);
}

/// The up-to-date fast path finishes `install` before the pipeline that
/// carries the warning ever runs, so it has to warn on its own: pnpm
/// warns from config-reading and therefore keeps warning on a repeat
/// install that has nothing to do.
#[test]
fn migrated_keys_are_reported_by_the_up_to_date_fast_path() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "private": true,
            "pnpm": { "overrides": { "is-number": "6.0.0" } },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet.with_arg("install").assert().success();
    let assert = pacquet_in(&workspace).with_arg("install").assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    eprintln!("STDOUT:\n{stdout}\nSTDERR:\n{stderr}");
    assert!(
        stderr.contains("no longer read by pnpm"),
        "the fast path must warn too; got:\n{stderr}",
    );
    assert!(
        stdout.contains("Already up to date"),
        "expected the fast path's own output; got:\n{stdout}",
    );

    drop(root);
}

/// Each emit site reads the root manifest on its own, and an editor may
/// leave a UTF-8 BOM at its head. A reader that trips over one drops the
/// warning without a trace, so both sites are exercised: the install
/// pipeline on the first run, the up-to-date fast path on the second.
#[test]
fn migrated_keys_are_reported_through_a_utf8_bom() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let manifest = serde_json::json!({
        "name": "root",
        "version": "1.0.0",
        "private": true,
        "pnpm": { "overrides": { "is-number": "6.0.0" } },
    });
    fs::write(workspace.join("package.json"), format!("\u{feff}{manifest}"))
        .expect("write package.json");

    let assert = pacquet.with_arg("install").assert().success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    eprintln!("STDERR:\n{stderr}");
    assert!(
        stderr.contains("no longer read by pnpm"),
        "the BOM must not swallow the warning; got:\n{stderr}",
    );

    let assert = pacquet_in(&workspace).with_arg("install").assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    eprintln!("STDOUT:\n{stdout}\nSTDERR:\n{stderr}");
    assert!(
        stderr.contains("no longer read by pnpm"),
        "the fast path reads the manifest on its own; got:\n{stderr}",
    );
    assert!(
        stdout.contains("Already up to date"),
        "expected the fast path's own output; got:\n{stdout}",
    );

    drop(root);
}

/// Both emit sites resolve the root manifest the way pnpm does — the
/// workspace root when there is one, the current directory otherwise —
/// so a run from a workspace package names the root's keys and never
/// the package's own.
#[test]
fn migrated_keys_are_read_from_the_workspace_root() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "name": "root",
            "version": "1.0.0",
            "private": true,
            "pnpm": { "overrides": { "is-number": "6.0.0" } },
        })
        .to_string(),
    )
    .expect("write package.json");
    let package_dir = workspace.join("packages").join("leaf");
    fs::create_dir_all(&package_dir).expect("create the workspace package");
    fs::write(
        package_dir.join("package.json"),
        serde_json::json!({
            "name": "leaf",
            "version": "1.0.0",
            "pnpm": { "neverBuiltDependencies": [] },
        })
        .to_string(),
    )
    .expect("write the workspace package's package.json");

    let assert =
        pacquet_in(&package_dir).with_args(["install", "--lockfile-only"]).assert().success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    eprintln!("STDERR:\n{stderr}");
    assert!(
        stderr.contains(r#"The following keys were ignored: "pnpm.overrides"."#),
        "the root manifest's keys must be named; got:\n{stderr}",
    );
    assert!(
        !stderr.contains("neverBuiltDependencies"),
        "the workspace package's own `pnpm` field is not the root manifest; got:\n{stderr}",
    );

    drop(root);
}

/// The warning names only keys pnpm migrated, so a manifest carrying a
/// `pnpm` field that third-party tooling owns stays quiet.
#[test]
fn an_unmigrated_package_json_pnpm_field_is_not_reported() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "name": "root", "version": "1.0.0", "private": true, "pnpm": { "app": {} } })
            .to_string(),
    )
    .expect("write package.json");

    let assert = pacquet.with_args(["install", "--lockfile-only"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout).into_owned();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();

    eprintln!("STDOUT:\n{stdout}\nSTDERR:\n{stderr}");
    assert!(
        !stdout.contains("no longer read by pnpm") && !stderr.contains("no longer read by pnpm"),
        "must stay quiet; got stdout:\n{stdout}\nstderr:\n{stderr}",
    );

    drop(root);
}

/// `ignorePnpmfile` is a setting, not only a flag: pnpm reads it from
/// `pnpm-workspace.yaml` and from `PNPM_CONFIG_IGNORE_PNPMFILE`, so a project
/// or a machine can turn hooks off without every command growing the flag.
#[test]
fn ignore_pnpmfile_is_settable_without_the_flag() {
    for source in ["pnpm-workspace.yaml", "PNPM_CONFIG_IGNORE_PNPMFILE"] {
        let CommandTempCwd { root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();
        let AddMockedRegistry { mock_instance, .. } = npmrc_info;

        write_read_package_pnpmfile(&workspace);
        fs::write(
            workspace.join("package.json"),
            r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
        )
        .expect("write package.json");

        let mut command = pacquet_in(&workspace);
        if source == "pnpm-workspace.yaml" {
            append_workspace_setting(&workspace, "ignorePnpmfile: true");
        } else {
            command = command.with_env("PNPM_CONFIG_IGNORE_PNPMFILE", "true");
        }
        command.with_args(["install", "--lockfile-only"]).assert().success();
        assert!(!read_package_hook_applied(&workspace), "{source}: the pnpmfile's hook is skipped");

        // Resolve the same project with the setting absent, so the assertion
        // above cannot pass on a fixture whose hook never worked.
        fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
        if source == "pnpm-workspace.yaml" {
            remove_workspace_setting(&workspace, "ignorePnpmfile");
        }
        pacquet_in(&workspace).with_args(["install", "--lockfile-only"]).assert().success();
        assert!(read_package_hook_applied(&workspace), "{source}: the hook otherwise applies");

        drop((root, mock_instance));
    }
}

/// The global `config.yaml` is not one of the places pnpm reads it from, and it
/// should not be: a pnpmfile belongs to the project that ships it, so honoring
/// this globally would drop a repository's hooks on one machine and resolve a
/// different graph there than everywhere else.
#[test]
fn ignore_pnpmfile_in_the_global_config_does_not_disable_hooks() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_read_package_pnpmfile(&workspace);
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
    )
    .expect("write package.json");
    let config_home = workspace.join(".config");
    fs::create_dir_all(config_home.join("pnpm")).expect("create global config dir");
    fs::write(config_home.join("pnpm/config.yaml"), "ignorePnpmfile: true\n")
        .expect("write global config");

    pacquet_in(&workspace)
        .with_env("XDG_CONFIG_HOME", &config_home)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    assert!(read_package_hook_applied(&workspace), "the hook still runs");

    drop((root, mock_instance));
}

/// The flag ORs on top, so it still turns hooks off for a project whose
/// configuration leaves them on.
#[test]
fn the_ignore_pnpmfile_flag_wins_over_a_configured_false() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_read_package_pnpmfile(&workspace);
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
    )
    .expect("write package.json");
    append_workspace_setting(&workspace, "ignorePnpmfile: false");

    pacquet_in(&workspace)
        .with_args(["install", "--lockfile-only", "--ignore-pnpmfile"])
        .assert()
        .success();
    assert!(!read_package_hook_applied(&workspace), "the flag turns the hook off anyway");

    drop((root, mock_instance));
}

/// `--ignore-pnpmfile` disables the hooks the workspace pnpmfile
/// exports, so an install that passes it resolves the manifest as
/// written and drops what a `readPackage` hook injected.
#[test]
fn ignore_pnpmfile_skips_the_read_package_hook() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_read_package_pnpmfile(&workspace);
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
    )
    .expect("write package.json");

    pacquet.with_args(["install", "--lockfile-only", "--ignore-pnpmfile"]).assert().success();
    assert!(
        !read_package_hook_applied(&workspace),
        "--ignore-pnpmfile resolves without the hook's dependency",
    );

    // Resolve the same project again with the hook honored, so the
    // assertion above cannot pass on a fixture that never worked.
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    pacquet_in(&workspace).with_args(["install", "--lockfile-only"]).assert().success();
    assert!(
        read_package_hook_applied(&workspace),
        "without the flag the hook injects its dependency",
    );

    drop((root, mock_instance));
}

/// `add` and `update` each merge their own CLI flags into the config,
/// on a dispatch path `install` never takes.
#[test]
fn ignore_pnpmfile_skips_the_read_package_hook_on_add_and_update() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_read_package_pnpmfile(&workspace);
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");

    pacquet
        .with_args([
            "add",
            "@pnpm.e2e/pkg-with-1-dep@100.0.0",
            "--lockfile-only",
            "--ignore-pnpmfile",
        ])
        .assert()
        .success();
    assert!(!read_package_hook_applied(&workspace), "add resolves without the hook's dependency");

    pacquet_in(&workspace)
        .with_args(["update", "@pnpm.e2e/pkg-with-1-dep", "--lockfile-only", "--ignore-pnpmfile"])
        .assert()
        .success();
    assert!(
        !read_package_hook_applied(&workspace),
        "update resolves without the hook's dependency",
    );

    // Re-resolve with the hook honored, so the assertions above cannot
    // pass on a fixture that never worked.
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    pacquet_in(&workspace)
        .with_args(["update", "@pnpm.e2e/pkg-with-1-dep", "--lockfile-only"])
        .assert()
        .success();
    assert!(
        read_package_hook_applied(&workspace),
        "without the flag the hook injects its dependency",
    );

    drop((root, mock_instance));
}

/// `readPackage` is loaded off the install's own pnpmfile handle, while
/// `updateConfig` runs earlier, off the pnpmfile set the config layer
/// resolves. `--ignore-pnpmfile` has to empty that set too, or the
/// install still runs on a hook-rewritten config.
#[test]
fn ignore_pnpmfile_skips_the_update_config_hook() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { updateConfig (config) { config.autoInstallPeers = false; return config } } }\n",
    )
    .expect("write pnpmfile");
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
    )
    .expect("write package.json");

    let recorded_auto_install_peers = || {
        pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
            .expect("load wanted lockfile")
            .expect("wanted lockfile")
            .settings
            .expect("recorded settings")
            .auto_install_peers
    };

    pacquet.with_args(["install", "--lockfile-only", "--ignore-pnpmfile"]).assert().success();
    assert!(recorded_auto_install_peers(), "--ignore-pnpmfile installs on the unhooked config");

    // Resolve the same project again with the hook honored, so the
    // assertion above cannot pass on a fixture that never worked.
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    pacquet_in(&workspace).with_args(["install", "--lockfile-only"]).assert().success();
    assert!(!recorded_auto_install_peers(), "without the flag the hook rewrites the config");

    drop((root, mock_instance));
}

/// A workspace pnpmfile whose single `readPackage` hook is observable in
/// the lockfile: it gives `@pnpm.e2e/pkg-with-1-dep` a dependency the
/// published package doesn't declare.
/// `globalPnpmfile` names a user-level pnpmfile that runs for projects that
/// ship none of their own. pnpm loads it ahead of the project's, and exposes
/// the setting through `PNPM_CONFIG_GLOBAL_PNPMFILE` like every other key in
/// its schema.
#[test]
fn a_global_pnpmfile_runs_for_a_project_without_one() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let global_pnpmfile = root.path().join("global-pnpmfile.cjs");
    fs::write(&global_pnpmfile, READ_PACKAGE_PNPMFILE).expect("write global pnpmfile");
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
    )
    .expect("write package.json");
    assert!(!workspace.join(".pnpmfile.cjs").exists());

    pacquet_in(&workspace)
        .with_env("PNPM_CONFIG_GLOBAL_PNPMFILE", &global_pnpmfile)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    assert!(
        read_package_hook_applied(&workspace),
        "the global pnpmfile's hook injects its dependency",
    );

    // The same install without the setting resolves the manifest as written,
    // so the assertion above cannot pass on a fixture that never worked.
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    pacquet_in(&workspace).with_args(["install", "--lockfile-only"]).assert().success();
    assert!(!read_package_hook_applied(&workspace));

    drop((root, mock_instance));
}

/// The global pnpmfile loads ahead of the project's, so `readPackage` reaches
/// it first and the project's hook sees what it returned. Chaining is the whole
/// point of the order pnpm pins by pushing the global entry first, and nothing
/// else in this file would notice if the two swapped.
#[test]
fn a_global_pnpmfile_runs_before_the_project_pnpmfile() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let global_pnpmfile = root.path().join("global-pnpmfile.cjs");
    fs::write(
        &global_pnpmfile,
        r"module.exports = { hooks: { readPackage: (pkg) => {
            pkg.greetedByGlobalPnpmfile = true;
            return pkg;
        } } }",
    )
    .expect("write global pnpmfile");
    // Injects only what the global hook already put on the manifest, so the
    // dependency appears if and only if the global hook ran first.
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"module.exports = { hooks: { readPackage: (pkg) => {
            if (pkg.greetedByGlobalPnpmfile && pkg.name === '@pnpm.e2e/pkg-with-1-dep') {
                pkg.dependencies['is-positive'] = '1.0.0';
            }
            return pkg;
        } } }",
    )
    .expect("write project pnpmfile");
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
    )
    .expect("write package.json");

    pacquet_in(&workspace)
        .with_env("PNPM_CONFIG_GLOBAL_PNPMFILE", &global_pnpmfile)
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    assert!(
        read_package_hook_applied(&workspace),
        "the project hook saw the global hook's manifest",
    );

    drop((root, mock_instance));
}

/// The global pnpmfile is excluded from `pnpmfileChecksum`, matching the
/// `includeInChecksum: false` entry pnpm's `requireHooks` pushes for it.
/// Editing it must therefore leave the lockfile's checksum alone — the field
/// answers for the project's pnpmfiles, and claiming otherwise would let a
/// user-level file silently decide whether a lockfile is still current.
#[test]
fn a_global_pnpmfile_stays_out_of_the_pnpmfile_checksum() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let global_pnpmfile = root.path().join("global-pnpmfile.cjs");
    fs::write(&global_pnpmfile, "module.exports = { hooks: { readPackage: (pkg) => pkg } }")
        .expect("write global pnpmfile");
    write_read_package_pnpmfile(&workspace);
    fs::write(
        workspace.join("package.json"),
        r#"{"dependencies":{"@pnpm.e2e/pkg-with-1-dep":"100.0.0"}}"#,
    )
    .expect("write package.json");

    let install = || {
        // Each measurement resolves from scratch. Reading back a lockfile an
        // up-to-date check declined to rewrite would compare a value to itself.
        drop(fs::remove_file(workspace.join("pnpm-lock.yaml")));
        pacquet_in(&workspace)
            .with_env("PNPM_CONFIG_GLOBAL_PNPMFILE", &global_pnpmfile)
            .with_args(["install", "--lockfile-only"])
            .assert()
            .success();
        pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
            .expect("load wanted lockfile")
            .expect("wanted lockfile")
            .pnpmfile_checksum
    };

    let with_global = install();
    assert!(with_global.is_some(), "the project pnpmfile is checksummed");

    fs::write(
        &global_pnpmfile,
        "module.exports = { hooks: { readPackage: (pkg) => { void 0; return pkg } } }",
    )
    .expect("rewrite global pnpmfile");
    assert_eq!(install(), with_global, "editing the global pnpmfile leaves the checksum alone");

    // The value itself has to match what the project alone records, not merely
    // stay stable: `pnpmfileChecksum` is shared with pnpm, so a project that
    // happens to have a global pnpmfile must not hash to something pnpm would
    // disagree with.
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    pacquet_in(&workspace).with_args(["install", "--lockfile-only"]).assert().success();
    let without_global = pnpm_lockfile::Lockfile::load_wanted_from_dir(&workspace)
        .expect("load wanted lockfile")
        .expect("wanted lockfile")
        .pnpmfile_checksum;
    assert_eq!(with_global, without_global, "a global pnpmfile does not alter the recorded value");

    drop((root, mock_instance));
}
