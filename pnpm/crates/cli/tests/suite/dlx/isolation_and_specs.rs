use super::pacquet_in;
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::CommandTempCwd, command_env::CommandTestExt, fixtures::tarball_entries,
};
use pretty_assertions::assert_eq;
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write executable script");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("chmod executable script");
}

fn build_postinstall_cli_tarball(name: &str, bin_name: &str) -> Vec<u8> {
    let manifest = serde_json::json!({
        "name": name,
        "version": "1.0.0",
        "bin": { bin_name: "bin.js" },
        "scripts": {
            "postinstall": r#"node -e "require('fs').writeFileSync('built.txt', 'ok')""#,
        },
    })
    .to_string();
    let bin = concat!(
        "#!/usr/bin/env node\n",
        "const fs = require('fs');\n",
        "const path = require('path');\n",
        "const ok = fs.existsSync(path.join(__dirname, 'built.txt'));\n",
        "console.log(ok ? 'BUILT_OK' : 'MISSING_BUILD');\n",
    );
    tarball_entries(&[
        ("package/package.json", manifest.as_bytes()),
        ("package/bin.js", bin.as_bytes()),
    ])
}

fn write_local_cli_dir(dir: &Path, name: &str, message: &str) {
    fs::create_dir_all(dir).expect("create local cli dir");
    fs::write(
        dir.join("package.json"),
        serde_json::json!({
            "name": name,
            "version": "1.0.0",
            "bin": { name: "bin.js" },
        })
        .to_string(),
    )
    .expect("write local cli manifest");
    write_executable(
        &dir.join("bin.js"),
        &format!("#!/usr/bin/env node\nconsole.log({message:?});\n"),
    );
}

#[test]
fn dlx_pnx_and_create_ignore_untrusted_workspace_execution_settings() {
    let CommandTempCwd { root, npmrc_info, .. } = CommandTempCwd::init().add_mocked_registry();
    let repo_dir = root.path().join("untrusted-repo");
    fs::create_dir_all(repo_dir.join("node_modules/.bin")).expect("create repo bin dir");

    let hook_marker = root.path().join("pwned-hook");
    let shell_marker = root.path().join("pwned-shell");
    let bin_marker = root.path().join("pwned-bin");

    let hook_code = format!(
        "require('fs').writeFileSync({hook_marker:?}, 'hook');\nmodule.exports = {{ hooks: {{}} }};\n",
    );
    for filename in [".pnpmfile.cjs", "hook.cjs", "global-hook.cjs"] {
        fs::write(repo_dir.join(filename), &hook_code).expect("write untrusted hook");
    }
    write_executable(
        &repo_dir.join("evil-shell.sh"),
        &format!("#!/bin/sh\necho shell > {shell_marker:?}\nexit 99\n"),
    );
    write_executable(
        &repo_dir.join("node_modules/.bin/hijack-cmd"),
        &format!("#!/bin/sh\necho bin > {bin_marker:?}\n"),
    );

    fs::write(
        repo_dir.join("pnpm-workspace.yaml"),
        r"pnpmfile: ./hook.cjs
globalPnpmfile: ./global-hook.cjs
configDependencies:
  evil-plugin: 1.0.0+sha512-deadbeef
scriptShell: ./evil-shell.sh
shellEmulator: true
cacheDir: ./evil-cache
storeDir: ./evil-store
",
    )
    .expect("write untrusted pnpm-workspace.yaml");

    pacquet_in(&repo_dir, &npmrc_info)
        .with_args(["dlx", "@foo/touch-file-one-bin"])
        .assert()
        .success();

    pacquet_in(&repo_dir, &npmrc_info)
        .with_args(["dlx", "--package=@foo/touch-file-one-bin", "-c", "echo shell-ok"])
        .assert()
        .success();

    let hijack = pacquet_in(&repo_dir, &npmrc_info)
        .with_args(["dlx", "--package=@foo/touch-file-one-bin", "-c", "hijack-cmd"])
        .output()
        .expect("run dlx shell command checking workspace bin isolation");
    assert!(!hijack.status.success(), "workspace node_modules/.bin must not be on PATH");

    let pnpm_cmd = Command::cargo_bin("pnpm").expect("find pnpm binary");
    let pnx_bin = root.path().join("pnx");
    fs::hard_link(pnpm_cmd.get_program(), &pnx_bin)
        .or_else(|_| fs::copy(pnpm_cmd.get_program(), &pnx_bin).map(|_| ()))
        .expect("create pnx binary alias");
    Command::new(&pnx_bin)
        .with_current_dir(&repo_dir)
        .without_ambient_pnpm_config()
        .with_env("PNPM_CONFIG_REGISTRY", npmrc_info.mock_instance.url())
        .with_env("PNPM_CONFIG_STORE_DIR", &npmrc_info.store_dir)
        .with_env("PNPM_CONFIG_CACHE_DIR", &npmrc_info.cache_dir)
        .with_env("PNPM_CONFIG_ENABLE_GLOBAL_VIRTUAL_STORE", "false")
        .with_arg("@foo/touch-file-one-bin")
        .assert()
        .success();

    pacquet_in(&repo_dir, &npmrc_info)
        .with_args(["create", "touch-file-one-bin"])
        .assert()
        .success();

    assert!(!hook_marker.exists(), "untrusted pnpmfile / globalPnpmfile must never execute");
    assert!(!shell_marker.exists(), "untrusted scriptShell must never execute");
    assert!(!bin_marker.exists(), "untrusted workspace node_modules/.bin must not run");
    assert!(!repo_dir.join("evil-cache").exists(), "untrusted cacheDir must be ignored");
    assert!(!repo_dir.join("evil-store").exists(), "untrusted storeDir must be ignored");

    drop((root, npmrc_info));
}

#[test]
fn dlx_preserves_user_global_config_and_explicit_policies() {
    let CommandTempCwd { root, npmrc_info, .. } = CommandTempCwd::init().add_mocked_registry();
    let xdg_config = root.path().join("xdg-config");
    let user_cache = root.path().join("user-cache");
    let user_store = root.path().join("user-store");
    let user_hook = root.path().join("user-global-hook.cjs");
    let user_hook_marker = root.path().join("user-hook-ran");
    let repo_dir = root.path().join("repo");
    fs::create_dir_all(xdg_config.join("pnpm")).expect("create xdg pnpm dir");
    fs::create_dir_all(&repo_dir).expect("create repo dir");

    fs::write(
        &user_hook,
        format!(
            "module.exports = {{ hooks: {{ readPackage(pkg) {{ require('fs').writeFileSync({user_hook_marker:?}, 'ok'); return pkg; }} }} }};\n",
        ),
    )
    .expect("write user global pnpmfile");
    fs::write(
        xdg_config.join("pnpm/config.yaml"),
        format!(
            "cacheDir: {}\nstoreDir: {}\nblockExoticSubdeps: true\nglobalPnpmfile: {}\n",
            user_cache.display(),
            user_store.display(),
            user_hook.display(),
        ),
    )
    .expect("write user global config.yaml");
    fs::write(
        repo_dir.join("pnpm-workspace.yaml"),
        "cacheDir: ./evil-cache\nstoreDir: ./evil-store\nblockExoticSubdeps: false\n",
    )
    .expect("write repo pnpm-workspace.yaml");

    let run_with_user_config = |args: &[&str]| {
        Command::cargo_bin("pnpm")
            .expect("find pnpm binary")
            .with_current_dir(&repo_dir)
            .without_ambient_pnpm_config()
            .with_env("XDG_CONFIG_HOME", &xdg_config)
            .with_env("PNPM_CONFIG_REGISTRY", npmrc_info.mock_instance.url())
            .with_env("PNPM_CONFIG_ENABLE_GLOBAL_VIRTUAL_STORE", "false")
            .with_args(args)
            .output()
            .expect("run dlx with user global config")
    };

    let basic = run_with_user_config(&["dlx", "@foo/touch-file-one-bin"]);
    assert!(basic.status.success(), "stderr: {}", String::from_utf8_lossy(&basic.stderr));
    assert!(user_cache.join("dlx").is_dir(), "user global cacheDir must be honored");
    assert!(user_hook_marker.exists(), "user globalPnpmfile must be honored");
    assert!(!repo_dir.join("evil-cache").exists());

    let mut server = mockito::Server::new();
    let exotic_sub_tgz =
        tarball_entries(&[("package/package.json", br#"{"name":"exotic-sub","version":"1.0.0"}"#)]);
    let _sub_mock = server
        .mock("GET", "/exotic-sub.tgz")
        .with_status(200)
        .with_body(exotic_sub_tgz)
        .create();
    let wrapper_manifest = serde_json::json!({
        "name": "wrapper-cli",
        "version": "1.0.0",
        "bin": { "wrapper-cli": "bin.js" },
        "dependencies": {
            "exotic-sub": format!("{}/exotic-sub.tgz", server.url()),
        },
    })
    .to_string();
    let wrapper_tgz = tarball_entries(&[
        ("package/package.json", wrapper_manifest.as_bytes()),
        ("package/bin.js", b"#!/usr/bin/env node\nconsole.log('ok');\n"),
    ]);
    let _wrapper_mock = server
        .mock("GET", "/wrapper-cli.tgz")
        .with_status(200)
        .with_body(wrapper_tgz)
        .create();
    let wrapper_url = format!("{}/wrapper-cli.tgz", server.url());

    let blocked = run_with_user_config(&["dlx", wrapper_url.as_str()]);
    let blocked_stderr = String::from_utf8_lossy(&blocked.stderr);
    assert!(!blocked.status.success(), "global blockExoticSubdeps=true must block exotic subdep");
    assert!(blocked_stderr.contains("ERR_PNPM_EXOTIC_SUBDEP"), "stderr:\n{blocked_stderr}");

    fs::write(
        xdg_config.join("pnpm/config.yaml"),
        format!(
            "cacheDir: {}\nstoreDir: {}\nblockExoticSubdeps: false\n",
            user_cache.display(),
            user_store.display(),
        ),
    )
    .expect("update user global config.yaml with blockExoticSubdeps: false");
    let allowed = run_with_user_config(&["dlx", wrapper_url.as_str()]);
    assert!(
        allowed.status.success(),
        "global blockExoticSubdeps=false must be preserved; stderr: {}",
        String::from_utf8_lossy(&allowed.stderr),
    );

    let ext_output = pacquet_in(&repo_dir, &npmrc_info)
        .with_env(
            "PNPM_CONFIG_PACKAGE_EXTENSIONS",
            r#"{"@pnpm.e2e/bravo":{"dependencies":{"@pnpm.e2e/romeo":"1.0.0"}}}"#,
        )
        .with_args([
            "dlx",
            "--package=@pnpm.e2e/bravo@1.0.0",
            "-c",
            r#"node -p "const p=require('path'),nm=p.dirname(process.env.PATH.split(p.delimiter)[0]),b=require.resolve('@pnpm.e2e/bravo/package.json',{paths:[nm]});require(require.resolve('@pnpm.e2e/romeo/package.json',{paths:[b]})).version""#,
        ])
        .output()
        .expect("run dlx with PNPM_CONFIG_PACKAGE_EXTENSIONS");
    assert!(ext_output.status.success(), "stderr: {}", String::from_utf8_lossy(&ext_output.stderr));
    assert_eq!(String::from_utf8_lossy(&ext_output.stdout).trim(), "1.0.0");

    drop((root, npmrc_info));
}

#[test]
fn dlx_isolates_workspace_ignore_scripts_and_keys_cache_by_explicit_ignore_scripts() {
    let CommandTempCwd { root, npmrc_info, .. } = CommandTempCwd::init().add_mocked_registry();
    let ws_ignore_dir = root.path().join("ws-ignore");
    let clean_dir = root.path().join("clean");
    fs::create_dir_all(&ws_ignore_dir).expect("create ws-ignore dir");
    fs::create_dir_all(&clean_dir).expect("create clean dir");
    fs::write(ws_ignore_dir.join("pnpm-workspace.yaml"), "ignoreScripts: true\n")
        .expect("write workspace ignoreScripts: true");

    let mut server = mockito::Server::new();
    let tgz = build_postinstall_cli_tarball("dlx-cache-tool", "dlx-cache-tool");
    let _mock = server
        .mock("GET", "/dlx-cache-tool.tgz")
        .with_status(200)
        .with_body(tgz)
        .expect_at_least(2)
        .create();
    let url = format!("{}/dlx-cache-tool.tgz", server.url());

    let skipped = pacquet_in(&clean_dir, &npmrc_info)
        .with_args(["dlx", "--ignore-scripts", url.as_str()])
        .output()
        .expect("run dlx with --ignore-scripts");
    assert!(skipped.status.success(), "stderr: {}", String::from_utf8_lossy(&skipped.stderr));
    assert_eq!(String::from_utf8_lossy(&skipped.stdout).trim(), "MISSING_BUILD");

    let from_ws = pacquet_in(&ws_ignore_dir, &npmrc_info)
        .with_args(["dlx", url.as_str()])
        .output()
        .expect("run dlx in workspace with ignoreScripts: true");
    assert!(from_ws.status.success(), "stderr: {}", String::from_utf8_lossy(&from_ws.stderr));
    assert_eq!(
        String::from_utf8_lossy(&from_ws.stdout).trim(),
        "BUILT_OK",
        "neither --ignore-scripts cache nor workspace ignoreScripts: true may suppress build",
    );

    let from_clean = pacquet_in(&clean_dir, &npmrc_info)
        .with_args(["dlx", url.as_str()])
        .output()
        .expect("run dlx in clean dir");
    assert!(from_clean.status.success(), "stderr: {}", String::from_utf8_lossy(&from_clean.stderr));
    assert_eq!(String::from_utf8_lossy(&from_clean.stdout).trim(), "BUILT_OK");

    let cache_entries =
        fs::read_dir(npmrc_info.cache_dir.join("dlx")).expect("read dlx cache").count();
    assert_eq!(cache_entries, 2, "built and --ignore-scripts runs must occupy distinct cache keys");

    drop((root, npmrc_info));
}

#[test]
fn dlx_resolves_relative_local_directory_and_tarball_specs_against_caller_cwd() {
    let CommandTempCwd { root, npmrc_info, .. } = CommandTempCwd::init().add_mocked_registry();
    let dir_a = root.path().join("dir-a");
    let dir_b = root.path().join("dir-b");
    write_local_cli_dir(&dir_a.join("pkg"), "local-cli", "from-a");
    write_local_cli_dir(&dir_b.join("pkg"), "local-cli", "from-b");

    let tgz_manifest = serde_json::json!({
        "name": "tarball-cli",
        "version": "1.0.0",
        "bin": { "tarball-cli": "bin.js" },
    })
    .to_string();
    let tgz_bytes = tarball_entries(&[
        ("package/package.json", tgz_manifest.as_bytes()),
        ("package/bin.js", b"#!/usr/bin/env node\nconsole.log('from-tarball');\n"),
    ]);
    fs::write(dir_a.join("dlx-tool.tgz"), tgz_bytes).expect("write local tarball");

    let run_in = |cwd: &Path, spec: &str| {
        let out = pacquet_in(cwd, &npmrc_info)
            .with_args(["dlx", spec])
            .output()
            .expect("run dlx on relative local spec");
        assert!(out.status.success(), "{spec} stderr:\n{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    assert_eq!(run_in(&dir_a, "./pkg"), "from-a");
    assert_eq!(
        run_in(&dir_b, "./pkg"),
        "from-b",
        "relative ./pkg in dir_b must not collide with dir_a's ./pkg in the dlx cache",
    );
    assert_eq!(run_in(&dir_b, "../dir-a/pkg"), "from-a");
    assert_eq!(run_in(&dir_a, "./dlx-tool.tgz"), "from-tarball");

    drop((root, npmrc_info));
}

#[test]
fn dlx_and_create_run_postinstall_for_direct_url_and_local_tarball_specs() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let mut server = mockito::Server::new();
    let dlx_tgz = build_postinstall_cli_tarball("dlx-url-tool", "dlx-url-tool");
    let create_tgz = build_postinstall_cli_tarball("create-url-tool", "create-url-tool");
    let _dlx_mock = server
        .mock("GET", "/dlx-url-tool.tgz")
        .with_status(200)
        .with_body(dlx_tgz.clone())
        .create();
    let _create_mock = server
        .mock("GET", "/create-url-tool.tgz")
        .with_status(200)
        .with_body(create_tgz)
        .create();

    fs::write(workspace.join("local-postinstall.tgz"), dlx_tgz)
        .expect("write local postinstall tgz");
    let dlx_url = format!("{}/dlx-url-tool.tgz", server.url());
    let create_spec = format!("url-tool@{}/create-url-tool.tgz", server.url());

    let denied = pacquet_in(&workspace, &npmrc_info)
        .with_args(["dlx", "--allow-build=!dlx-url-tool", dlx_url.as_str()])
        .output()
        .expect("run dlx with negated --allow-build on URL tarball");
    assert!(denied.status.success(), "stderr: {}", String::from_utf8_lossy(&denied.stderr));
    assert_eq!(String::from_utf8_lossy(&denied.stdout).trim(), "MISSING_BUILD");

    let url_built = pacquet_in(&workspace, &npmrc_info)
        .with_args(["dlx", dlx_url.as_str()])
        .output()
        .expect("run dlx on URL tarball");
    assert!(url_built.status.success(), "stderr: {}", String::from_utf8_lossy(&url_built.stderr));
    assert_eq!(String::from_utf8_lossy(&url_built.stdout).trim(), "BUILT_OK");

    let local_built = pacquet_in(&workspace, &npmrc_info)
        .with_args(["dlx", "./local-postinstall.tgz"])
        .output()
        .expect("run dlx on local tarball with postinstall");
    assert!(
        local_built.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&local_built.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&local_built.stdout).trim(), "BUILT_OK");

    let create_built = pacquet_in(&workspace, &npmrc_info)
        .with_args(["create", create_spec.as_str()])
        .output()
        .expect("run create on URL tarball");
    assert!(
        create_built.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&create_built.stderr),
    );
    assert_eq!(String::from_utf8_lossy(&create_built.stdout).trim(), "BUILT_OK");

    drop((root, npmrc_info));
}
