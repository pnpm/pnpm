use super::{Command, CommandCargoExt, CommandExtra, CommandTempCwd, fs};
#[cfg(unix)]
use super::{
    assert_fixture_paths, dependency_manifest_path, global_command, prepare_global_home,
    seed_global_group, snapshot_tree, symlink_entries,
};

#[cfg(unix)]
#[test]
fn global_update_preflights_incomplete_target_ownership_before_activation() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    let global_pkg_dir = pnpm_home.join("global/v11");
    prepare_global_home(&pnpm_home, &npmrc_info);
    fs::create_dir_all(&global_pkg_dir).expect("create global packages directory");
    assert_fixture_paths(
        root.path(),
        &[&pnpm_home, &global_bin, &global_pkg_dir, &npmrc_info.store_dir, &npmrc_info.cache_dir],
    );

    let target_install =
        seed_global_group(&global_pkg_dir, "target-hash", &[("@pnpm.e2e/print-version", None)]);
    let stale_bin = global_bin.join("stale-version-bin");
    fs::write(&stale_bin, b"old command\n").expect("seed the stale global bin");
    let packages_before = snapshot_tree(&global_pkg_dir);
    let bins_before = snapshot_tree(&global_bin);

    for attempt in 1..=2 {
        let output = global_command(&workspace, &pnpm_home)
            .with_args(["update", "-g", "--latest", "@pnpm.e2e/print-version"])
            .output()
            .expect("run global update with incomplete ownership");
        assert!(
            !output.status.success(),
            "attempt {attempt} must reject incomplete ownership; stdout: {}; stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert_eq!(snapshot_tree(&global_pkg_dir), packages_before);
        assert_eq!(snapshot_tree(&global_bin), bins_before);
    }

    fs::write(
        dependency_manifest_path(&target_install, "@pnpm.e2e/print-version"),
        r#"{"name":"@pnpm.e2e/print-version","version":"1.0.0","bin":{"stale-version-bin":"old.js"}}"#,
    )
    .expect("repair target dependency manifest");

    global_command(&workspace, &pnpm_home)
        .with_args(["update", "-g", "--latest", "@pnpm.e2e/print-version"])
        .assert()
        .success();
    assert!(!stale_bin.exists(), "the repaired retry must remove the genuinely stale bin");
    assert!(global_bin.join("print-version").exists());
    assert!(!target_install.exists());
    assert_eq!(symlink_entries(&global_pkg_dir).len(), 1);

    global_command(&workspace, &pnpm_home)
        .with_args(["update", "-g", "--latest", "@pnpm.e2e/print-version"])
        .assert()
        .success();
    assert!(!stale_bin.exists());
    assert!(global_bin.join("print-version").exists());
    assert_eq!(symlink_entries(&global_pkg_dir).len(), 1);

    drop((root, npmrc_info));
}

#[cfg(unix)]
#[test]
fn global_add_preflights_incomplete_survivor_ownership_before_activation() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    let global_pkg_dir = pnpm_home.join("global/v11");
    prepare_global_home(&pnpm_home, &npmrc_info);
    fs::create_dir_all(&global_pkg_dir).expect("create global packages directory");
    assert_fixture_paths(
        root.path(),
        &[&pnpm_home, &global_bin, &global_pkg_dir, &npmrc_info.store_dir, &npmrc_info.cache_dir],
    );

    let target_install = seed_global_group(
        &global_pkg_dir,
        "target-hash",
        &[(
            "@foo/touch-file-one-bin",
            Some(
                r#"{"name":"@foo/touch-file-one-bin","version":"0.0.0","bin":{"shared":"shared.js","stale":"stale.js"}}"#,
            ),
        )],
    );
    let survivor_install =
        seed_global_group(&global_pkg_dir, "survivor-hash", &[("keeper", Some("{"))]);
    let shared_bin = global_bin.join("shared");
    let stale_bin = global_bin.join("stale");
    fs::write(&shared_bin, b"keeper command\n").expect("seed survivor-owned bin");
    fs::write(&stale_bin, b"target command\n").expect("seed target-owned bin");
    let packages_before = snapshot_tree(&global_pkg_dir);
    let bins_before = snapshot_tree(&global_bin);

    for attempt in 1..=2 {
        let output = global_command(&workspace, &pnpm_home)
            .with_args(["add", "-g", "@foo/touch-file-one-bin"])
            .output()
            .expect("run global add with incomplete survivor ownership");
        assert!(
            !output.status.success(),
            "attempt {attempt} must reject incomplete survivor ownership; stdout: {}; stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert_eq!(snapshot_tree(&global_pkg_dir), packages_before);
        assert_eq!(snapshot_tree(&global_bin), bins_before);
    }

    fs::write(
        dependency_manifest_path(&survivor_install, "keeper"),
        r#"{"name":"keeper","version":"1.0.0","bin":{"shared":"keeper.js"}}"#,
    )
    .expect("repair survivor dependency manifest");

    global_command(&workspace, &pnpm_home)
        .with_args(["add", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    assert_eq!(fs::read(&shared_bin).expect("read survivor-owned bin"), b"keeper command\n");
    assert!(!stale_bin.exists());
    assert!(global_bin.join("touch-file-one-bin").exists());
    assert!(!target_install.exists());
    assert!(survivor_install.exists());
    assert_eq!(symlink_entries(&global_pkg_dir).len(), 2);

    global_command(&workspace, &pnpm_home)
        .with_args(["add", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    assert_eq!(fs::read(&shared_bin).expect("read survivor-owned bin"), b"keeper command\n");
    assert!(!stale_bin.exists());
    assert!(global_bin.join("touch-file-one-bin").exists());
    assert_eq!(symlink_entries(&global_pkg_dir).len(), 2);

    drop((root, npmrc_info));
}

#[cfg(unix)]
#[test]
fn global_remove_preflights_all_targets_before_mutating_any_group() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    let global_pkg_dir = pnpm_home.join("global/v11");
    fs::create_dir_all(&global_bin).expect("create global bin directory");
    fs::create_dir_all(&global_pkg_dir).expect("create global packages directory");
    assert_fixture_paths(root.path(), &[&pnpm_home, &global_bin, &global_pkg_dir]);

    let first_install = seed_global_group(
        &global_pkg_dir,
        "first-hash",
        &[(
            "victim-a",
            Some(r#"{"name":"victim-a","version":"1.0.0","bin":{"victim-a-bin":"cli.js"}}"#),
        )],
    );
    let second_install = seed_global_group(&global_pkg_dir, "second-hash", &[("victim-b", None)]);
    let first_bin = global_bin.join("victim-a-bin");
    let second_bin = global_bin.join("victim-b-bin");
    fs::write(&first_bin, b"first command\n").expect("seed first target bin");
    fs::write(&second_bin, b"second command\n").expect("seed second target bin");
    let packages_before = snapshot_tree(&global_pkg_dir);
    let bins_before = snapshot_tree(&global_bin);

    for attempt in 1..=2 {
        let output = global_command(&workspace, &pnpm_home)
            .with_args(["remove", "-g", "victim-a", "victim-b"])
            .output()
            .expect("run multi-target global remove");
        assert!(
            !output.status.success(),
            "attempt {attempt} must reject an incomplete target; stdout: {}; stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert_eq!(snapshot_tree(&global_pkg_dir), packages_before);
        assert_eq!(snapshot_tree(&global_bin), bins_before);
    }

    fs::write(
        dependency_manifest_path(&second_install, "victim-b"),
        r#"{"name":"victim-b","version":"1.0.0","bin":{"victim-b-bin":"cli.js"}}"#,
    )
    .expect("repair second target dependency manifest");

    global_command(&workspace, &pnpm_home)
        .with_args(["remove", "-g", "victim-a", "victim-b"])
        .assert()
        .success();
    assert!(!first_install.exists());
    assert!(!second_install.exists());
    assert!(!first_bin.exists());
    assert!(!second_bin.exists());
    assert!(snapshot_tree(&global_pkg_dir).is_empty());
    let removed_packages = snapshot_tree(&global_pkg_dir);
    let removed_bins = snapshot_tree(&global_bin);

    let output = global_command(&workspace, &pnpm_home)
        .with_args(["remove", "-g", "victim-a", "victim-b"])
        .output()
        .expect("repeat completed multi-target remove");
    assert!(!output.status.success(), "the existing not-found contract is retained");
    assert_eq!(snapshot_tree(&global_pkg_dir), removed_packages);
    assert_eq!(snapshot_tree(&global_bin), removed_bins);

    drop(root);
}

#[cfg(unix)]
#[test]
fn global_remove_preflights_survivors_before_mutating_targets() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    let global_pkg_dir = pnpm_home.join("global/v11");
    fs::create_dir_all(&global_bin).expect("create global bin directory");
    fs::create_dir_all(&global_pkg_dir).expect("create global packages directory");
    assert_fixture_paths(root.path(), &[&pnpm_home, &global_bin, &global_pkg_dir]);

    let target_install = seed_global_group(
        &global_pkg_dir,
        "target-hash",
        &[(
            "victim",
            Some(
                r#"{"name":"victim","version":"1.0.0","bin":{"shared":"shared.js","stale":"stale.js"}}"#,
            ),
        )],
    );
    let survivor_install =
        seed_global_group(&global_pkg_dir, "survivor-hash", &[("keeper", Some("{"))]);
    let shared_bin = global_bin.join("shared");
    let stale_bin = global_bin.join("stale");
    fs::write(&shared_bin, b"keeper command\n").expect("seed survivor-owned bin");
    fs::write(&stale_bin, b"target command\n").expect("seed target-owned bin");
    let packages_before = snapshot_tree(&global_pkg_dir);
    let bins_before = snapshot_tree(&global_bin);

    for attempt in 1..=2 {
        let output = global_command(&workspace, &pnpm_home)
            .with_args(["remove", "-g", "victim"])
            .output()
            .expect("run global remove with incomplete survivor ownership");
        assert!(
            !output.status.success(),
            "attempt {attempt} must reject incomplete survivor ownership; stdout: {}; stderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
        assert_eq!(snapshot_tree(&global_pkg_dir), packages_before);
        assert_eq!(snapshot_tree(&global_bin), bins_before);
    }

    fs::write(
        dependency_manifest_path(&survivor_install, "keeper"),
        r#"{"name":"keeper","version":"1.0.0","bin":{"shared":"keeper.js"}}"#,
    )
    .expect("repair survivor dependency manifest");

    global_command(&workspace, &pnpm_home).with_args(["remove", "-g", "victim"]).assert().success();
    assert!(!target_install.exists());
    assert!(survivor_install.exists());
    assert_eq!(fs::read(&shared_bin).expect("read survivor-owned bin"), b"keeper command\n");
    assert!(!stale_bin.exists());
    let removed_packages = snapshot_tree(&global_pkg_dir);
    let removed_bins = snapshot_tree(&global_bin);

    let output = global_command(&workspace, &pnpm_home)
        .with_args(["remove", "-g", "victim"])
        .output()
        .expect("repeat completed global remove");
    assert!(!output.status.success(), "the existing not-found contract is retained");
    assert_eq!(snapshot_tree(&global_pkg_dir), removed_packages);
    assert_eq!(snapshot_tree(&global_bin), removed_bins);

    drop(root);
}

/// `pacquet add -g pnpm` is rejected — pnpm is managed via `self-update`. An
/// `npm:` alias installs pnpm under another name, but the package still carries
/// pnpm's own `pnpm` bin, so it is rejected the same way. A comma-separated
/// group is a request to install each of its tokens, so pnpm hiding inside one
/// is caught as well.
#[test]
fn global_add_pnpm_is_rejected() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm-home");
    fs::create_dir_all(pnpm_home.join("bin")).expect("create global bin dir");

    for selector in [
        "pnpm",
        "@pnpm/exe",
        "pnpm@12",
        "my-pnpm@npm:pnpm@12",
        "pnpm,lodash",
        "lodash,my-pnpm@npm:pnpm@12",
    ] {
        let output = Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(&workspace)
            .with_env("PNPM_HOME", &pnpm_home)
            .with_arg("add")
            .with_arg("-g")
            .with_arg(selector)
            .output()
            .expect("run add -g");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "add -g {selector} must fail, got: {stderr}");
        assert!(
            stderr.contains("ERR_PNPM_GLOBAL_PNPM_INSTALL")
                && stderr
                    .contains(r#"Use the "pnpm self-update" command to install or update pnpm"#),
            "add -g {selector} must report the self-update diagnostic, got: {stderr}",
        );
    }

    drop(root);
}

/// `pnpm update -g pnpm` is rejected — pnpm is managed via `self-update`. The
/// interactive form goes through its own selection path, so it is covered too.
#[cfg(unix)]
#[test]
fn global_update_pnpm_is_rejected() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let pnpm_home = root.path().join("pnpm-home");
    fs::create_dir_all(pnpm_home.join("bin")).expect("create global bin dir");

    for selector in ["pnpm", "@pnpm/exe", "pnpm@12", "my-pnpm@npm:pnpm@12"] {
        for extra_args in [&[][..], &["-i"][..]] {
            let output = global_command(&workspace, &pnpm_home)
                .with_args(["update", "-g", selector])
                .with_args(extra_args)
                .output()
                .expect("run update -g");

            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(!output.status.success(), "update -g {selector} must fail, got: {stderr}");
            assert!(
                stderr.contains("ERR_PNPM_GLOBAL_PNPM_INSTALL")
                    && stderr.contains(
                        r#"Use the "pnpm self-update" command to install or update pnpm"#
                    ),
                "update -g {selector} must report the self-update diagnostic, got: {stderr}",
            );
        }
    }

    drop(root);
}

/// `pnpm self-update` owns the pnpm CLI's global install: it is what points
/// the pnpm home's bins at a release. Reinstalling that group from `update -g`
/// would resolve pnpm from the `latest` dist-tag and relink the bins, rolling
/// the running pnpm back to whatever `latest` points at (pnpm/pnpm#14270).
#[cfg(unix)]
#[test]
fn global_update_leaves_the_pnpm_cli_group_to_self_update() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    prepare_global_home(&pnpm_home, &npmrc_info);

    // The group `self-update` leaves behind: a hash symlink to an install dir
    // whose only dependency is the pnpm CLI wrapper.
    let global_pkg_dir = pnpm_home.join("global/v11");
    let install_dir = global_pkg_dir.join("pnpm-cli-install");
    fs::create_dir_all(&install_dir).expect("create the pnpm CLI install dir");
    fs::write(install_dir.join("package.json"), r#"{"dependencies":{"@pnpm/exe":"11.24.0"}}"#)
        .expect("write the pnpm CLI group manifest");
    std::os::unix::fs::symlink(&install_dir, global_pkg_dir.join("hash-pnpm-cli"))
        .expect("link the pnpm CLI group");

    let output = global_command(&workspace, &pnpm_home)
        .with_args(["update", "-g", "--latest"])
        .output()
        .expect("run update -g --latest");

    assert!(
        output.status.success(),
        "update -g should succeed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No global packages to update"),
        "the pnpm CLI group must be left to self-update, got: {stdout}",
    );
    assert_eq!(
        fs::read_to_string(install_dir.join("package.json")).expect("read the group manifest"),
        r#"{"dependencies":{"@pnpm/exe":"11.24.0"}}"#,
        "the pnpm CLI group must be left untouched",
    );

    drop((root, npmrc_info));
}
