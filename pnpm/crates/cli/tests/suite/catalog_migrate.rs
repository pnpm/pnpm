//! End-to-end `pacquet catalog migrate` tests.

use command_extra::CommandExtra;
use pnpm_testing_utils::bin::CommandTempCwd;
use pretty_assertions::assert_eq;
use std::{fs, path::Path};

fn write(dir: &Path, file: &str, contents: &str) {
    let path = dir.join(file);
    fs::create_dir_all(path.parent().unwrap()).expect("create the parent directory");
    fs::write(path, contents).unwrap_or_else(|_| panic!("write {file}"));
}

fn read(dir: &Path, file: &str) -> String {
    let contents = fs::read_to_string(dir.join(file)).unwrap_or_else(|_| panic!("read {file}"));
    eprintln!("{file}:\n{contents}");
    contents
}

#[test]
fn moves_agreed_specifiers_to_the_default_catalog() {
    let CommandTempCwd { pacquet, root: _root, workspace, .. } = CommandTempCwd::init();
    write(
        &workspace,
        "pnpm-workspace.yaml",
        "# the workspace projects\npackages:\n  - packages/*\ncatalog:\n  lodash: ^4.17.0\n",
    );
    write(
        &workspace,
        "package.json",
        r#"{
  "name": "root",
  "private": true,
  "devDependencies": {
    "typescript": "^5.0.0"
  }
}
"#,
    );
    write(
        &workspace,
        "packages/foo/package.json",
        r#"{
  "name": "foo",
  "dependencies": {
    "is-positive": "^1.0.0",
    "lodash": "^4.17.0",
    "react": "^18.0.0"
  },
  "devDependencies": {
    "typescript": "^5.0.0"
  }
}
"#,
    );
    write(
        &workspace,
        "packages/bar/package.json",
        r#"{
  "name": "bar",
  "dependencies": {
    "foo": "workspace:*",
    "is-positive": "^1.0.0",
    "local": "file:../local",
    "react": "^19.0.0"
  }
}
"#,
    );

    let output = pacquet
        .with_args(["catalog", "migrate"])
        .output()
        .expect("run pacquet");
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("STDOUT:\n{stdout}");
    eprintln!("STDERR:\n{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "pacquet catalog migrate failed");
    assert_eq!(
        stdout.trim_end(),
        "These dependencies were left unchanged because the projects or the default catalog disagree on their specifier:
  react: ^18.0.0, ^19.0.0
Moved 3 dependencies to the default catalog. Run `pnpm install` to update the lockfile.",
    );

    assert_eq!(
        read(&workspace, "pnpm-workspace.yaml"),
        "# the workspace projects\npackages:\n  - packages/*\ncatalog:\n  is-positive: ^1.0.0\n  lodash: ^4.17.0\n  typescript: ^5.0.0\n",
    );
    assert_eq!(
        read(&workspace, "package.json"),
        r#"{
  "name": "root",
  "private": true,
  "devDependencies": {
    "typescript": "catalog:"
  }
}
"#,
    );
    assert_eq!(
        read(&workspace, "packages/foo/package.json"),
        r#"{
  "name": "foo",
  "dependencies": {
    "is-positive": "catalog:",
    "lodash": "catalog:",
    "react": "^18.0.0"
  },
  "devDependencies": {
    "typescript": "catalog:"
  }
}
"#,
    );
    assert_eq!(
        read(&workspace, "packages/bar/package.json"),
        r#"{
  "name": "bar",
  "dependencies": {
    "foo": "workspace:*",
    "is-positive": "catalog:",
    "local": "file:../local",
    "react": "^19.0.0"
  }
}
"#,
    );
}

#[test]
fn fails_outside_a_workspace() {
    let CommandTempCwd { pacquet, root: _root, workspace, .. } = CommandTempCwd::init();
    write(&workspace, "package.json", r#"{ "name": "solo", "dependencies": { "a": "^1.0.0" } }"#);

    let output = pacquet
        .with_args(["catalog", "migrate"])
        .output()
        .expect("run pacquet");
    let stderr = String::from_utf8_lossy(&output.stderr);
    eprintln!("STDERR:\n{stderr}");
    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_CATALOG_NO_WORKSPACE"));
}

#[test]
fn compares_against_the_catalog_an_update_config_hook_sets() {
    let CommandTempCwd { pacquet, root: _root, workspace, .. } = CommandTempCwd::init();
    write(
        &workspace,
        "pnpm-workspace.yaml",
        "packages:\n  - packages/*\ncatalog:\n  react: ^18.0.0\n",
    );
    write(&workspace, "package.json", r#"{ "name": "root", "private": true }"#);
    let manifest =
        r#"{ "name": "foo", "dependencies": { "lodash": "^4.0.0", "react": "^18.0.0" } }"#;
    write(&workspace, "packages/foo/package.json", manifest);
    write(
        &workspace,
        ".pnpmfile.cjs",
        "module.exports = { hooks: { updateConfig (config) {\n  config.catalogs = { default: { react: '^19.0.0' } };\n  return config;\n} } }",
    );

    let output = pacquet
        .with_args(["catalog", "migrate"])
        .output()
        .expect("run pacquet");
    let stdout = String::from_utf8_lossy(&output.stdout);
    eprintln!("STDOUT:\n{stdout}");
    eprintln!("STDERR:\n{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "pacquet catalog migrate failed");
    assert_eq!(
        stdout.trim_end(),
        "These dependencies were left unchanged because the projects or the default catalog disagree on their specifier:
  react: ^18.0.0 (catalog: ^19.0.0)
These dependencies were left unchanged because the catalogs come from an updateConfig hook that has no entry for them: lodash
No dependencies were moved to the default catalog.",
    );
    assert_eq!(read(&workspace, "packages/foo/package.json"), manifest);
    assert_eq!(
        read(&workspace, "pnpm-workspace.yaml"),
        "packages:\n  - packages/*\ncatalog:\n  react: ^18.0.0\n",
    );
}
