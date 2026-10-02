use super::{pacquet_at, write_manifest};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{
    bin::CommandTempCwd, diagnostics::assert_diagnostic_contains, fixtures::minimal_tarball,
};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[test]
fn hoisted_reinstall_refreshes_a_custom_resolution_delegated_to_a_directory() {
    assert_delegated_directory_is_refreshed("{ type: 'custom:directory' }");
}

#[test]
fn hoisted_reinstall_refreshes_a_tarball_resolution_delegated_to_a_directory() {
    assert_delegated_directory_is_refreshed("{ tarball: 'file:delegated.tgz' }");
}

#[test]
fn hoisted_reinstall_without_custom_fetchers_keeps_the_fast_path() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    configure_hoisted_tarball(&workspace);
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        "module.exports = { hooks: { readPackage (pkg) { return pkg; } } };\n",
    )
    .expect("write pnpmfile");
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    let output = pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&output.get_output().stdout);
    eprintln!("STDOUT:\n{stdout}\n");
    assert!(stdout.contains("Already up to date"));
    drop(root);
}

#[test]
fn hoisted_reinstall_propagates_a_custom_fetcher_hook_error() {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    configure_hoisted_tarball(&workspace);
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        r"const fs = require('node:fs');
const path = require('node:path');
module.exports = {
  get fetchers () {
    if (fs.existsSync(path.join(__dirname, 'fail-fetchers'))) {
      throw new Error('fetchers unavailable');
    }
    return [];
  },
};
",
    )
    .expect("write pnpmfile");
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();

    // The hook can start failing without its own contents or the lockfile changing.
    fs::write(workspace.join("fail-fetchers"), "fail").expect("make the hook fail");
    for frozen in [false, true] {
        let output = pacquet_at(&workspace)
            .with_arg("install")
            .with_args(frozen.then_some("--frozen-lockfile"))
            .assert()
            .failure();
        let stderr = String::from_utf8_lossy(&output.get_output().stderr);
        assert_diagnostic_contains(&stderr, "ERR_PNPM_PNPMFILE_FAIL");
        assert_diagnostic_contains(&stderr, "fetchers unavailable");
    }
    drop(root);
}

fn configure_hoisted_tarball(workspace: &Path) {
    write_manifest(workspace, "file:dep.tgz");
    fs::write(workspace.join("pnpm-workspace.yaml"), "nodeLinker: hoisted\n")
        .expect("write workspace settings");
    fs::write(
        workspace.join("dep.tgz"),
        minimal_tarball("@pnpm.e2e/dep-of-pkg-with-1-dep", "1.0.0"),
    )
    .expect("write package tarball");
}

fn assert_delegated_directory_is_refreshed(resolution: &str) {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let source = set_up_delegated_directory(&workspace, resolution);
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    let installed = workspace.join("node_modules/@pnpm.e2e/dep-of-pkg-with-1-dep");
    assert_eq!(fs::read_to_string(installed.join("index.js")).unwrap(), "first");

    fs::write(source.join("index.js"), "second").expect("edit source file");
    fs::write(source.join("new.js"), "new").expect("add source file");
    pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .success();
    assert_eq!(fs::read_to_string(installed.join("index.js")).unwrap(), "second");
    assert_eq!(fs::read_to_string(installed.join("new.js")).unwrap(), "new");

    fs::write(source.join("index.js"), "third").expect("edit source file again");
    pacquet_at(&workspace)
        .with_arg("install")
        .assert()
        .success();
    assert_eq!(fs::read_to_string(installed.join("index.js")).unwrap(), "third");
    drop(root);
}

/// Writes a hoisted project whose pnpmfile resolves the dependency to
/// `resolution` and fetches it from the returned `source` directory.
fn set_up_delegated_directory(workspace: &Path, resolution: &str) -> PathBuf {
    write_manifest(workspace, "1.0.0");
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "nodeLinker: hoisted\npackageImportMethod: copy\n",
    )
    .expect("write workspace settings");
    let source = workspace.join("source");
    fs::create_dir(&source).expect("create source");
    fs::write(
        source.join("package.json"),
        r#"{"name":"@pnpm.e2e/dep-of-pkg-with-1-dep","version":"1.0.0"}"#,
    )
    .expect("write source manifest");
    fs::write(source.join("index.js"), "first").expect("write source file");
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        format!(
            r"const path = require('node:path');
module.exports = {{
  resolvers: [{{
    canResolve (wanted) {{ return wanted.alias === '@pnpm.e2e/dep-of-pkg-with-1-dep'; }},
    resolve () {{
      return {{
        id: '@pnpm.e2e/dep-of-pkg-with-1-dep@1.0.0',
        manifest: {{ name: '@pnpm.e2e/dep-of-pkg-with-1-dep', version: '1.0.0' }},
        resolution: {resolution},
      }};
    }},
  }}],
  fetchers: [{{
    canFetch (pkgId) {{ return pkgId === '@pnpm.e2e/dep-of-pkg-with-1-dep@1.0.0'; }},
    fetch () {{
      return {{ delegate: {{ type: 'directory', directory: path.join(__dirname, 'source') }} }};
    }},
  }}],
}};
",
        ),
    )
    .expect("write pnpmfile");

    source
}
