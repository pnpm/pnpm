//! Custom resolvers from a `.pnpmfile.cjs` `resolvers` export: chain
//! precedence over the built-in resolvers, `shouldRefreshResolution`
//! forcing re-resolution past the prefer-frozen fast path, and hook
//! failures aborting the install. Mirrors pnpm's
//! `installing/deps-installer/test/install/customResolvers.ts`.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

fn write_manifest(workspace: &Path) {
    let manifest = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0",
        },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
}

/// A resolver that claims `@pnpm.e2e/dep-of-pkg-with-1-dep` and resolves
/// it to `100.1.0` — a version the built-in npm resolver would never pick
/// for the exact `100.0.0` specifier, so its presence in the result
/// proves the custom resolver ran.
fn overriding_pnpmfile(registry_url: &str, should_refresh: &str) -> String {
    format!(
        r"module.exports = {{
  resolvers: [
    {{
      canResolve (wanted) {{
        return wanted.alias === '@pnpm.e2e/dep-of-pkg-with-1-dep';
      }},
      async resolve () {{
        const response = await fetch('{registry_url}@pnpm.e2e%2Fdep-of-pkg-with-1-dep');
        const meta = await response.json();
        const dist = meta.versions['100.1.0'].dist;
        return {{
          id: '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0',
          resolution: {{ tarball: dist.tarball, integrity: dist.integrity }},
        }};
      }},
      shouldRefreshResolution () {{
        return {should_refresh};
      }},
    }},
  ],
}}
",
    )
}

fn installed_version(workspace: &Path) -> String {
    let manifest_path = workspace.join("node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep/package.json");
    let manifest: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(manifest_path).expect("read installed manifest"))
            .expect("parse installed manifest");
    manifest["version"].as_str().expect("version is a string").to_string()
}

#[test]
fn custom_resolver_takes_precedence_over_builtin_resolvers() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace);
    fs::write(workspace.join(".pnpmfile.cjs"), overriding_pnpmfile(&mock_instance.url(), "false"))
        .expect("write pnpmfile");

    pacquet.with_arg("install").assert().success();

    assert_eq!(installed_version(&workspace), "100.1.0");
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0"),
        "lockfile records the custom resolution: {lockfile}",
    );

    drop((root, mock_instance)); // cleanup
}

#[test]
fn should_refresh_resolution_forces_re_resolution_past_the_frozen_path() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace);
    // First install: the resolver implements only
    // `shouldRefreshResolution`, so it never joins the resolver chain
    // and the built-in npm resolver pins 100.0.0.
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { resolvers: [{ shouldRefreshResolution: () => false }] }\n",
    )
    .expect("write pnpmfile");
    pacquet.with_arg("install").assert().success();
    assert_eq!(installed_version(&workspace), "100.0.0");

    // Second install: manifest and lockfile are unchanged, so without
    // the hook the install would go frozen and re-resolve nothing.
    // `shouldRefreshResolution` returning true must force the
    // fresh-resolve path, where the custom resolver now overrides the
    // pinned version.
    fs::write(workspace.join(".pnpmfile.cjs"), overriding_pnpmfile(&mock_instance.url(), "true"))
        .expect("rewrite pnpmfile");
    pacquet_at(&workspace).with_arg("install").assert().success();

    assert_eq!(installed_version(&workspace), "100.1.0");
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile.contains("@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0"),
        "lockfile records the refreshed resolution: {lockfile}",
    );

    drop((root, mock_instance)); // cleanup
}

#[test]
fn failing_should_refresh_resolution_aborts_the_install() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace);
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { resolvers: [{ shouldRefreshResolution: () => false }] }\n",
    )
    .expect("write pnpmfile");
    pacquet.with_arg("install").assert().success();

    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { resolvers: [{ shouldRefreshResolution () { throw new Error('refresh check crashed'); } }] }\n",
    )
    .expect("rewrite pnpmfile");

    let output = pacquet_at(&workspace).with_arg("install").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    assert!(stderr.contains("refresh check crashed"), "stderr: {stderr}");

    drop((root, mock_instance)); // cleanup
}
