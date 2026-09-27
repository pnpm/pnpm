use super::{pacquet_at, write_manifest};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use std::fs;

#[test]
fn hoisted_reinstall_refreshes_a_custom_resolution_delegated_to_a_directory() {
    assert_delegated_directory_is_refreshed("{ type: 'custom:directory' }");
}

#[test]
fn hoisted_reinstall_refreshes_a_tarball_resolution_delegated_to_a_directory() {
    assert_delegated_directory_is_refreshed("{ tarball: 'file:delegated.tgz' }");
}

fn assert_delegated_directory_is_refreshed(resolution: &str) {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    write_manifest(&workspace, "1.0.0");
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
