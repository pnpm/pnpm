//! Workspace settings that change what the server resolves, and the install
//! flags that change how it treats the input lockfile.

use super::{
    AddMockedRegistry,
    CommandExtra,
    CommandTempCwd,
    WORKSPACE_HELLO,
    configure_pnpr_auth,
    configure_workspace,
    fs,
    pacquet_at,
    read_workspace_lockfile,
    start_pnpr,
    workspace_importer,
    workspace_importer_version,
};
use crate::_utils::append_workspace_yaml_key;
use assert_cmd::assert::OutputAssertExt;
use std::path::Path;

fn write_manifest(dir: &Path, manifest: &serde_json::Value) {
    fs::create_dir_all(dir).expect("create project dir");
    fs::write(
        dir.join("package.json"),
        serde_json::to_string_pretty(manifest).expect("serialize package.json"),
    )
    .expect("write package.json");
}

/// `app` depends on the workspace project `lib`, with `extra` merged into
/// `app`'s manifest.
fn write_app_and_lib(workspace: &Path, extra: &serde_json::Value) {
    let mut app = serde_json::json!({
        "name": "app",
        "version": "1.0.0",
        "dependencies": { "lib": "workspace:*" },
    });
    if let (Some(app), Some(extra)) = (app.as_object_mut(), extra.as_object()) {
        app.extend(extra.clone());
    }
    write_manifest(&workspace.join("packages/app"), &app);
    write_manifest(
        &workspace.join("packages/lib"),
        &serde_json::json!({ "name": "lib", "version": "1.0.0" }),
    );
}

fn install_via_pnpr(
    workspace: &Path,
    registry: &str,
    pnpr_url: &str,
) -> assert_cmd::assert::Assert {
    pacquet_at(workspace)
        .with_env("PNPM_CONFIG_REGISTRY", registry)
        .with_args(["install", "--pnpr-server", pnpr_url])
        .assert()
}

#[test]
fn install_via_pnpr_injects_workspace_packages() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    append_workspace_yaml_key(&workspace, "injectWorkspacePackages", true);
    write_app_and_lib(&workspace, &serde_json::json!({}));
    let (pnpr_url, token) = start_pnpr(mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    install_via_pnpr(&workspace, mock_instance.url(), &pnpr_url).success();

    let lockfile = read_workspace_lockfile(&workspace);
    assert!(lockfile.settings.is_some_and(|settings| settings.inject_workspace_packages));

    drop((root, mock_instance));
}

#[test]
fn install_via_pnpr_honors_dependencies_meta() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    write_app_and_lib(
        &workspace,
        &serde_json::json!({ "dependenciesMeta": { "lib": { "injected": true } } }),
    );
    let (pnpr_url, token) = start_pnpr(mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    install_via_pnpr(&workspace, mock_instance.url(), &pnpr_url).success();

    let lockfile = read_workspace_lockfile(&workspace);
    assert_eq!(
        workspace_importer(&lockfile, "packages/app").dependencies_meta,
        Some(serde_json::json!({ "lib": { "injected": true } })),
    );

    drop((root, mock_instance));
}

#[test]
fn install_via_pnpr_skips_ignored_optional_dependencies() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    append_workspace_yaml_key(
        &workspace,
        "ignoredOptionalDependencies",
        format!("['{WORKSPACE_HELLO}']"),
    );
    write_manifest(
        &workspace.join("packages/app"),
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "optionalDependencies": { WORKSPACE_HELLO: "1.0.0" },
        }),
    );
    let (pnpr_url, token) = start_pnpr(mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    install_via_pnpr(&workspace, mock_instance.url(), &pnpr_url).success();

    let lockfile = read_workspace_lockfile(&workspace);
    assert_eq!(lockfile.ignored_optional_dependencies, Some(vec![WORKSPACE_HELLO.to_string()]));
    assert!(workspace_importer(&lockfile, "packages/app").optional_dependencies.is_none());

    drop((root, mock_instance));
}

/// pnpr does not fetch exotic subdependencies, so it refuses
/// `blockExoticSubdeps: false` and pnpm resolves locally.
#[test]
fn install_via_pnpr_resolves_locally_when_exotic_subdeps_are_allowed() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    append_workspace_yaml_key(&workspace, "blockExoticSubdeps", false);
    write_app_and_lib(&workspace, &serde_json::json!({}));
    let (pnpr_url, token) = start_pnpr(mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let output = install_via_pnpr(&workspace, mock_instance.url(), &pnpr_url).success();

    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout).into_owned();
    assert!(
        format!("{stdout}{stderr}").contains("blockExoticSubdeps"),
        "stdout: {stdout}\nstderr: {stderr}",
    );
    let lockfile = read_workspace_lockfile(&workspace);
    assert_eq!(workspace_importer_version(&lockfile, "packages/app", "lib"), "link:../lib");

    drop((root, mock_instance));
}

const WRONG_INTEGRITY: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==";

#[test]
fn install_via_pnpr_updates_checksums() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    write_manifest(
        &workspace.join("packages/app"),
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { WORKSPACE_HELLO: "1.0.0" },
        }),
    );
    let (pnpr_url, token) = start_pnpr(mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);
    install_via_pnpr(&workspace, mock_instance.url(), &pnpr_url).success();
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let resolved = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    let integrity = resolved
        .split_whitespace()
        .find(|word| word.starts_with("sha512-"))
        .expect("the lockfile records an integrity")
        .trim_end_matches(['}', ',']);
    fs::write(&lockfile_path, resolved.replace(integrity, WRONG_INTEGRITY))
        .expect("write pnpm-lock.yaml");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--update-checksums", "--pnpr-server", &pnpr_url])
        .assert()
        .success();

    let updated = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(updated.contains(integrity), "{updated}");
    assert!(!updated.contains(WRONG_INTEGRITY), "{updated}");

    drop((root, mock_instance));
}

#[test]
fn install_via_pnpr_rejects_update_checksums_with_frozen_lockfile() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    configure_workspace(&workspace);
    write_app_and_lib(&workspace, &serde_json::json!({}));
    let (pnpr_url, token) = start_pnpr(mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);
    install_via_pnpr(&workspace, mock_instance.url(), &pnpr_url).success();

    let output = pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args([
            "install",
            "--frozen-lockfile",
            "--update-checksums",
            "--pnpr-server",
            &pnpr_url,
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_CONFIG_CONFLICT_FROZEN_LOCKFILE_WITH_UPDATE_CHECKSUMS"),
        "{stderr}",
    );

    drop((root, mock_instance));
}

/// pnpr resolves into the v9 format, so a v13 project keeps resolving
/// locally and its lockfile is left as it was.
#[test]
fn update_patches_via_pnpr_does_not_downgrade_a_v13_lockfile() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { npmrc_path, mock_instance, .. } = npmrc_info;
    append_workspace_yaml_key(&workspace, "lockfile", "\n  format: v13");
    write_manifest(
        &workspace,
        &serde_json::json!({
            "name": "app",
            "version": "1.0.0",
            "dependencies": { WORKSPACE_HELLO: "1.0.0" },
        }),
    );
    pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["install", "--lockfile-only"])
        .assert()
        .success();
    let lockfile_path = workspace.join("pnpm-lock.yaml");
    let local = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    let (pnpr_url, token) = start_pnpr(mock_instance.url());
    configure_pnpr_auth(&npmrc_path, &pnpr_url, &token);

    let output = pacquet_at(&workspace)
        .with_env("PNPM_CONFIG_REGISTRY", mock_instance.url())
        .with_args(["update", "--patches", "--pnpr-server", &pnpr_url])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    assert!(stderr.contains("ERR_PNPM_RESOLUTION_SETTINGS_WITH_PNPR_SERVER"), "{stderr}");
    assert_eq!(fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml"), local);

    drop((root, mock_instance));
}
