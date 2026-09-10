#[cfg(unix)]
use super::{
    Command, CommandExtra, CommandTempCwd, fs, global_command, global_shim_command,
    prepare_global_home, symlink_entries,
};
#[cfg(unix)]
use pnpm_testing_utils::command_env::CommandTestExt;

/// A `globalShims` entry for the package writes context-aware shims: the
/// generated shim dispatches through the versioned binary next to it, so a project-local
/// version of the same bin wins over the global target, and falls back to
/// the global target outside any providing project.
#[cfg(unix)]
#[test]
fn global_shims_all_prefers_local_bins() {
    use assert_cmd::assert::OutputAssertExt;
    use std::os::unix::fs::PermissionsExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    prepare_global_home(&pnpm_home, &npmrc_info);
    let yaml_path = pnpm_home.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).unwrap();
    fs::write(&yaml_path, format!("{yaml}globalShims: {{'@foo/touch-file-one-bin': true}}\n"))
        .unwrap();

    global_command(&workspace, &pnpm_home)
        .with_arg("add")
        .with_arg("-g")
        .with_arg("@foo/touch-file-one-bin")
        .assert()
        .success();

    let shim_path = global_bin.join("touch-file-one-bin");
    assert!(shim_path.is_file());
    let target = fs::read(global_bin.join(".pnpm-shim-v1-touch-file-one-bin-target"))
        .expect("read the shim target");
    assert!(target.ends_with(b"/cli.js"), "target was: {}", String::from_utf8_lossy(&target));

    fs::write(global_bin.join("pnpm"), "#!/bin/sh\nexit 64\n").unwrap();
    fs::set_permissions(global_bin.join("pnpm"), fs::Permissions::from_mode(0o755)).unwrap();

    let project = root.path().join("project");
    let local_script =
        project.join("node_modules").join("@foo").join("touch-file-one-bin").join("cli.sh");
    fs::create_dir_all(local_script.parent().unwrap()).unwrap();
    fs::write(
        local_script.parent().unwrap().join("package.json"),
        serde_json::json!({ "name": "@foo/touch-file-one-bin", "version": "1.0.0" }).to_string(),
    )
    .unwrap();
    fs::write(&local_script, "#!/bin/sh\necho local\n").unwrap();
    fs::set_permissions(&local_script, fs::Permissions::from_mode(0o755)).unwrap();
    let local_bin = project.join("node_modules").join(".bin").join("touch-file-one-bin");
    fs::create_dir_all(local_bin.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink("../@foo/touch-file-one-bin/cli.sh", &local_bin).unwrap();

    let output = Command::new(&shim_path)
        .without_ambient_pnpm_config()
        .with_current_dir(&project)
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CONFIG_HOME", root.path().join("config"))
        .with_env("PNPM_AUTO_APPROVE_PROJECT_BINS_FOR_TESTS", "1")
        .output()
        .expect("run the generated shim inside the project");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.trim(), "local", "stderr:\n{}", String::from_utf8_lossy(&output.stderr));

    let outside = root.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    Command::new(&shim_path)
        .with_current_dir(&outside)
        .with_env("PNPM_HOME", &pnpm_home)
        .with_env("XDG_STATE_HOME", root.path().join("state"))
        .with_env("XDG_CONFIG_HOME", root.path().join("config"))
        .assert()
        .success();

    drop(npmrc_info);
    drop(root);
}

#[cfg(unix)]
#[test]
fn global_install_preserves_virtual_shim_ownership_and_restores_it_on_remove() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    let shim_path = global_bin.join("touch-file-one-bin");
    prepare_global_home(&pnpm_home, &npmrc_info);
    let registry = npmrc_info.mock_instance.url();

    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["shim", "add", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    let target_file = global_bin.join(".pnpm-shim-v1-touch-file-one-bin-target");
    let virtual_target = fs::read(&target_file).expect("read virtual shim target");
    assert_eq!(virtual_target, b"pkg:@foo/touch-file-one-bin");

    let unrelated = root.path().join("unrelated");
    fs::create_dir_all(&unrelated).expect("create unrelated package");
    fs::write(
        unrelated.join("package.json"),
        serde_json::json!({
            "name": "unrelated",
            "version": "1.0.0",
            "bin": { "touch-file-one-bin": "cli.js" },
        })
        .to_string(),
    )
    .expect("write unrelated manifest");
    fs::write(unrelated.join("cli.js"), "#!/usr/bin/env node\n").expect("write unrelated bin");
    let unrelated_selector = format!("file:{}", unrelated.display());
    let collision = global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["add", "-g", &unrelated_selector])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&collision.get_output().stderr);
    assert!(stderr.contains("ERR_PNPM_GLOBAL_BIN_CONFLICT"), "{stderr}");
    assert!(stderr.contains("pnpm shim rm @foo/touch-file-one-bin"), "{stderr}");
    assert_eq!(fs::read(&target_file).unwrap(), virtual_target);

    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["add", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    let backed_target = fs::read(&target_file).expect("read backed shim target");
    assert!(
        !backed_target.starts_with(b"pkg:"),
        "target was: {}",
        String::from_utf8_lossy(&backed_target),
    );

    let collision = global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["add", "-g", &unrelated_selector])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&collision.get_output().stderr);
    assert!(stderr.contains("pnpm shim rm @foo/touch-file-one-bin"), "{stderr}");
    assert_eq!(fs::read(&target_file).unwrap(), backed_target);

    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["remove", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    assert_eq!(
        fs::read(global_bin.join(".pnpm-shim-v1-touch-file-one-bin-target"))
            .expect("read restored virtual shim target"),
        b"pkg:@foo/touch-file-one-bin",
    );
    assert!(shim_path.is_file(), "the restored shim must be in place");

    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["add", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["shim", "rm", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["remove", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    assert!(!shim_path.exists(), "shim rm must cancel restoration after global removal");
    assert!(!target_file.exists(), "shim rm must drop the recorded target");

    drop(npmrc_info);
    drop(root);
}

#[cfg(unix)]
#[test]
fn global_replacement_restores_a_virtual_shim_for_a_dropped_bin() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    let shim_path = global_bin.join("touch-file-one-bin");
    prepare_global_home(&pnpm_home, &npmrc_info);
    let registry = npmrc_info.mock_instance.url();

    let new_package = root.path().join("new-package");
    fs::create_dir_all(&new_package).expect("create new package");
    fs::write(
        new_package.join("package.json"),
        serde_json::json!({
            "name": "@foo/touch-file-one-bin",
            "version": "2.0.0",
        })
        .to_string(),
    )
    .expect("write new package manifest");

    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["shim", "add", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["add", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    assert!(
        !fs::read(global_bin.join(".pnpm-shim-v1-touch-file-one-bin-target"))
            .unwrap()
            .starts_with(b"pkg:"),
    );

    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["add", "-g", &format!("file:{}", new_package.display())])
        .assert()
        .success();

    assert_eq!(
        fs::read(global_bin.join(".pnpm-shim-v1-touch-file-one-bin-target"))
            .expect("read restored virtual shim target"),
        b"pkg:@foo/touch-file-one-bin",
    );
    assert!(shim_path.is_file(), "the restored shim must be in place");

    drop((root, npmrc_info));
}

#[cfg(unix)]
#[test]
fn failed_virtual_shim_restoration_leaves_global_removal_retryable() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let pnpm_home = root.path().join("pnpm-home");
    let global_bin = pnpm_home.join("bin");
    let global_pkg_dir = pnpm_home.join("global").join("v11");
    let shim_path = global_bin.join("touch-file-one-bin");
    prepare_global_home(&pnpm_home, &npmrc_info);
    let registry = npmrc_info.mock_instance.url();

    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["shim", "add", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["add", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();

    fs::remove_file(&shim_path).expect("remove the global shim");
    fs::create_dir(&shim_path).expect("occupy the shim path with a directory");
    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["remove", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .failure();
    assert_eq!(
        symlink_entries(&global_pkg_dir).len(),
        1,
        "a restoration failure must leave the global package installed",
    );

    fs::remove_dir(&shim_path).expect("release the shim path");
    global_shim_command(&workspace, &pnpm_home, root.path(), &registry)
        .with_args(["remove", "-g", "@foo/touch-file-one-bin"])
        .assert()
        .success();
    assert_eq!(
        fs::read(global_bin.join(".pnpm-shim-v1-touch-file-one-bin-target"))
            .expect("read restored virtual shim target"),
        b"pkg:@foo/touch-file-one-bin",
    );
    assert!(shim_path.is_file(), "the restored shim must be in place");
    assert!(
        symlink_entries(&global_pkg_dir).is_empty(),
        "the successful retry must remove the global package",
    );

    drop(npmrc_info);
    drop(root);
}

/// Ordinary packages use the plain direct-exec format in `auto` mode.
#[cfg(unix)]
#[test]
fn global_shims_auto_writes_direct_shims_for_ordinary_packages() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let pnpm_home = root.path().join("pnpm-home");
    prepare_global_home(&pnpm_home, &npmrc_info);
    global_command(&workspace, &pnpm_home)
        .with_arg("add")
        .with_arg("-g")
        .with_arg("@foo/touch-file-one-bin")
        .assert()
        .success();

    let shim = fs::read_to_string(pnpm_home.join("bin").join("touch-file-one-bin"))
        .expect("read the generated global shim");
    assert!(!shim.contains("--shim"), "shim should exec directly, was:\n{shim}");
    assert!(!pnpm_home.join("bin").join(".pnpm-shim-v1-touch-file-one-bin-target").exists());

    drop(npmrc_info);
    drop(root);
}

#[cfg(unix)]
#[test]
fn global_shims_auto_writes_native_dispatcher_for_node_runtime() {
    use assert_cmd::assert::OutputAssertExt;

    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let mut server = mockito::Server::new();
    let version = "24.0.0-rc.4";
    let _mocks = crate::install_runtimes::mock_node_release(&mut server, version);

    let pnpm_home = root.path().join("pnpm-home");
    prepare_global_home(&pnpm_home, &npmrc_info);
    let yaml_path = pnpm_home.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).unwrap();
    fs::write(&yaml_path, format!("{yaml}nodeDownloadMirrors:\n  rc: '{}/'\n", server.url()))
        .unwrap();

    global_command(&workspace, &pnpm_home)
        .with_args(["runtime", "set", "node", version, "--global"])
        .assert()
        .success();

    let global_bin = pnpm_home.join("bin");
    let node = global_bin.join("node");
    assert_eq!(
        fs::metadata(&node).unwrap().len(),
        fs::metadata(assert_cmd::cargo::cargo_bin("pnpm")).unwrap().len(),
        "node should be a copy of the pnpm executable",
    );
    let target = fs::read(global_bin.join(".pnpm-shim-v1-node-target")).unwrap();
    assert!(target.ends_with(b"/bin/node"), "target was: {}", String::from_utf8_lossy(&target));
    assert!(!global_bin.join(".pnpm-shim-v1").exists());

    drop(npmrc_info);
    drop(root);
}
