//! Custom fetchers from a `.pnpmfile.cjs` `fetchers` export, end to
//! end: a custom resolver writes a custom-typed resolution into the
//! lockfile, and the sibling fetcher claims it and delegates to the
//! built-in tarball path — on the fresh-resolve install, on a frozen
//! reinstall, and failing loudly when the fetcher is removed.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
}

fn write_manifest(workspace: &Path) {
    let manifest = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0",
        },
    });
    fs::write(workspace.join("package.json"), manifest.to_string()).expect("write package.json");
}

/// A resolver that claims `@pnpm.e2e/dep-of-pkg-with-1-dep` and
/// resolves it to a `custom:e2e` resolution carrying the registry's
/// real tarball URL and integrity, plus a fetcher that delegates that
/// resolution back to the built-in tarball path. The fetcher uses the
/// TS-parity hook signature `fetch(cafs, resolution, opts, fetchers)`.
fn custom_type_pnpmfile(registry_url: &str, with_fetcher: bool) -> String {
    let fetchers = if with_fetcher {
        r"
  fetchers: [
    {
      canFetch (pkgId, resolution) {
        return resolution.type === 'custom:e2e';
      },
      fetch (cafs, resolution, opts, fetchers) {
        return { delegate: { tarball: resolution.url, integrity: resolution.integrity } };
      },
    },
  ],
"
    } else {
        ""
    };
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
        const picked = meta.versions['100.1.0'];
        return {{
          id: '@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0',
          manifest: picked,
          resolution: {{
            type: 'custom:e2e',
            url: picked.dist.tarball,
            integrity: picked.dist.integrity,
          }},
        }};
      }},
    }},
  ],{fetchers}
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
fn custom_fetcher_delegates_a_custom_typed_resolution_on_fresh_and_frozen_installs() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace);
    fs::write(workspace.join(".pnpmfile.cjs"), custom_type_pnpmfile(&mock_instance.url(), true))
        .expect("write pnpmfile");

    // Fresh resolve: the custom resolver writes the custom-typed
    // resolution, and the fetcher must delegate it during the same
    // install's fetch phase.
    pacquet.with_arg("install").assert().success();
    assert_eq!(installed_version(&workspace), "100.1.0");
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile.contains("type: custom:e2e"),
        "lockfile records the custom-typed resolution: {lockfile}",
    );

    // Frozen reinstall: the lockfile is the source of truth now, so the
    // fetcher (loaded by the frozen path) is the only way to
    // materialize the custom-typed entry.
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    pacquet_at(&workspace).with_arg("install").assert().success();
    assert_eq!(installed_version(&workspace), "100.1.0");

    drop((root, mock_instance)); // cleanup
}

#[test]
fn custom_typed_resolution_without_a_fetcher_fails_the_install() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace);
    fs::write(workspace.join(".pnpmfile.cjs"), custom_type_pnpmfile(&mock_instance.url(), false))
        .expect("write pnpmfile");

    let output = pacquet.with_arg("install").assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    assert!(
        stderr.contains(r#"Cannot fetch dependency with custom resolution type "custom:e2e""#),
        "stderr: {stderr}",
    );

    drop((root, mock_instance)); // cleanup
}

/// `pnpm fetch` takes `--ignore-pnpmfile` too, and its frozen path is
/// the one that loads the custom fetchers. Without a fetcher the
/// custom-typed resolution the lockfile records cannot be materialized,
/// so the flag turns a working fetch into the same loud failure a
/// missing `fetchers` export produces.
#[test]
fn ignore_pnpmfile_skips_the_custom_fetcher_on_fetch() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace);
    fs::write(workspace.join(".pnpmfile.cjs"), custom_type_pnpmfile(&mock_instance.url(), true))
        .expect("write pnpmfile");
    pacquet.with_arg("install").assert().success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    let output =
        pacquet_at(&workspace).with_args(["fetch", "--ignore-pnpmfile"]).assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();
    assert!(
        stderr.contains(r#"Cannot fetch dependency with custom resolution type "custom:e2e""#),
        "stderr: {stderr}",
    );

    // The same fetch with the pnpmfile honored, so the assertion above
    // cannot pass on a fixture that could never fetch.
    pacquet_at(&workspace).with_arg("fetch").assert().success();

    drop((root, mock_instance)); // cleanup
}

/// The resolver half of the same pnpmfile: with `--ignore-pnpmfile` the
/// custom resolver never claims the dependency, so the install resolves
/// the version the manifest asks for instead of the one the resolver
/// substitutes.
#[test]
fn ignore_pnpmfile_skips_the_custom_resolver_on_install() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_manifest(&workspace);
    fs::write(workspace.join(".pnpmfile.cjs"), custom_type_pnpmfile(&mock_instance.url(), true))
        .expect("write pnpmfile");

    pacquet.with_args(["install", "--ignore-pnpmfile"]).assert().success();
    assert_eq!(installed_version(&workspace), "100.0.0");
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        !lockfile.contains("type: custom:e2e"),
        "the custom resolver must not reach the lockfile: {lockfile}",
    );

    // Resolve again with the pnpmfile honored, so the assertions above
    // cannot pass on a fixture whose resolver never worked.
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    fs::remove_file(workspace.join("pnpm-lock.yaml")).expect("remove pnpm-lock.yaml");
    pacquet_at(&workspace).with_arg("install").assert().success();
    assert_eq!(installed_version(&workspace), "100.1.0");

    drop((root, mock_instance)); // cleanup
}
