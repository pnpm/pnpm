//! End-to-end `pacquet catalog migrate` tests.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_testing_utils::{bin::CommandTempCwd, command_env::CommandTestExt};
use pretty_assertions::assert_eq;
use std::{fs, path::Path, process::Command};

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
These dependencies were left unchanged because an updateConfig hook would not keep their catalog entry as declared: lodash
No dependencies were moved to the default catalog.",
    );
    assert_eq!(read(&workspace, "packages/foo/package.json"), manifest);
    assert_eq!(
        read(&workspace, "pnpm-workspace.yaml"),
        "packages:\n  - packages/*\ncatalog:\n  react: ^18.0.0\n",
    );
}

#[test]
fn a_pnpmfile_that_leaves_the_catalogs_alone_does_not_block_new_entries() {
    let CommandTempCwd { pacquet, root: _root, workspace, .. } = CommandTempCwd::init();
    write(&workspace, "pnpm-workspace.yaml", "packages:\n  - packages/*\n");
    write(&workspace, "package.json", r#"{ "name": "root", "private": true }"#);
    write(
        &workspace,
        "packages/foo/package.json",
        r#"{ "name": "foo", "dependencies": { "lodash": "^4.0.0" } }"#,
    );
    write(
        &workspace,
        ".pnpmfile.cjs",
        "module.exports = { hooks: { readPackage (pkg) { return pkg } } }",
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
        "Moved 1 dependency to the default catalog. Run `pnpm install` to update the lockfile.",
    );
    assert_eq!(
        read(&workspace, "pnpm-workspace.yaml"),
        "packages:\n  - packages/*\ncatalog:\n  lodash: ^4.0.0\n",
    );
}

#[test]
fn holds_back_entries_a_hook_returning_fixed_catalogs_would_drop() {
    let CommandTempCwd { pacquet, root: _root, workspace, .. } = CommandTempCwd::init();
    write(
        &workspace,
        "pnpm-workspace.yaml",
        "packages:\n  - packages/*\ncatalog:\n  react: ^18.0.0\n",
    );
    write(&workspace, "package.json", r#"{ "name": "root", "private": true }"#);
    write(
        &workspace,
        "packages/foo/package.json",
        r#"{ "name": "foo", "dependencies": { "lodash": "^4.0.0", "react": "^18.0.0" } }"#,
    );
    // The hook's catalogs equal the workspace manifest's today, but it
    // replaces whatever the manifest holds.
    write(
        &workspace,
        ".pnpmfile.cjs",
        "module.exports = { hooks: { updateConfig (config) {\n  config.catalogs = { default: { react: '^18.0.0' } };\n  return config;\n} } }",
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
        "These dependencies were left unchanged because an updateConfig hook would not keep their catalog entry as declared: lodash
Moved 1 dependency to the default catalog. Run `pnpm install` to update the lockfile.",
    );
    assert_eq!(
        read(&workspace, "pnpm-workspace.yaml"),
        "packages:\n  - packages/*\ncatalog:\n  react: ^18.0.0\n",
    );
    assert_eq!(
        read(&workspace, "packages/foo/package.json"),
        r#"{"name":"foo","dependencies":{"lodash":"^4.0.0","react":"catalog:"}}"#,
    );
}

#[test]
fn adds_entries_a_merging_hook_passes_through() {
    let CommandTempCwd { pacquet, root: _root, workspace, .. } = CommandTempCwd::init();
    write(&workspace, "pnpm-workspace.yaml", "packages:\n  - packages/*\n");
    write(&workspace, "package.json", r#"{ "name": "root", "private": true }"#);
    write(
        &workspace,
        "packages/foo/package.json",
        r#"{ "name": "foo", "dependencies": { "lodash": "^4.0.0" } }"#,
    );
    write(
        &workspace,
        ".pnpmfile.cjs",
        "module.exports = { hooks: { updateConfig (config) {\n  config.catalogs = { ...config.catalogs, extra: { react: '^19.0.0' } };\n  return config;\n} } }",
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
        "Moved 1 dependency to the default catalog. Run `pnpm install` to update the lockfile.",
    );
    assert_eq!(
        read(&workspace, "pnpm-workspace.yaml"),
        "packages:\n  - packages/*\ncatalog:\n  lodash: ^4.0.0\n",
    );
}

/// Runs `pacquet catalog migrate` in a workspace whose one project declares
/// `dependencies`, with `hook` as the body of an `updateConfig` hook, and
/// returns stdout.
fn migrate_with_hook(
    workspace_yaml: &str,
    dependencies: &str,
    hook: &str,
) -> (String, CommandTempCwd<()>) {
    let cwd = CommandTempCwd::init();
    let workspace = &cwd.workspace;
    write(workspace, "pnpm-workspace.yaml", workspace_yaml);
    write(workspace, "package.json", r#"{ "name": "root", "private": true }"#);
    write(
        workspace,
        "packages/foo/package.json",
        &format!(r#"{{ "name": "foo", "dependencies": {dependencies} }}"#),
    );
    write(
        workspace,
        ".pnpmfile.cjs",
        &format!(
            "module.exports = {{ hooks: {{ updateConfig (config) {{\n{hook}\n  return config;\n}} }} }}",
        ),
    );
    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .without_ambient_pnpm_config()
        .with_args(["catalog", "migrate"])
        .output()
        .expect("run pacquet");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    eprintln!("STDOUT:\n{stdout}");
    eprintln!("STDERR:\n{}", String::from_utf8_lossy(&output.stderr));
    assert!(output.status.success(), "pacquet catalog migrate failed");
    (stdout, cwd)
}

#[test]
fn checks_the_hooks_an_install_runs_even_when_they_turn_themselves_off() {
    let (stdout, cwd) = migrate_with_hook(
        "packages:\n  - packages/*\n",
        r#"{ "lodash": "^4.0.0" }"#,
        "  config.catalogs = { default: { react: '^19.0.0' } };\n  config.ignorePnpmfile = true;",
    );
    assert_eq!(
        stdout.trim_end(),
        "These dependencies were left unchanged because an updateConfig hook would not keep their catalog entry as declared: lodash
No dependencies were moved to the default catalog.",
    );
    assert_eq!(read(&cwd.workspace, "pnpm-workspace.yaml"), "packages:\n  - packages/*\n");
}

#[test]
fn holds_back_an_existing_entry_a_new_one_makes_the_hook_change() {
    let (stdout, cwd) = migrate_with_hook(
        "packages:\n  - packages/*\ncatalog:\n  react: ^18.0.0\n",
        r#"{ "lodash": "^4.0.0", "react": "^18.0.0" }"#,
        "  if (config.catalogs.default.lodash) config.catalogs.default.react = '^19.0.0';",
    );
    assert_eq!(
        stdout.trim_end(),
        "These dependencies were left unchanged because an updateConfig hook would not keep their catalog entry as declared: react
Moved 1 dependency to the default catalog. Run `pnpm install` to update the lockfile.",
    );
    assert_eq!(
        read(&cwd.workspace, "packages/foo/package.json"),
        r#"{"name":"foo","dependencies":{"lodash":"catalog:","react":"^18.0.0"}}"#,
    );
}

#[test]
fn holds_back_an_entry_the_hook_keeps_only_next_to_one_it_drops() {
    let (stdout, cwd) = migrate_with_hook(
        "packages:\n  - packages/*\n",
        r#"{ "a": "^1.0.0", "b": "^2.0.0" }"#,
        "  const catalog = config.catalogs.default ?? {};\n  if (!catalog.a) delete catalog.b;\n  delete catalog.a;",
    );
    assert_eq!(
        stdout.trim_end(),
        "These dependencies were left unchanged because an updateConfig hook would not keep their catalog entry as declared: a, b
No dependencies were moved to the default catalog.",
    );
    assert_eq!(read(&cwd.workspace, "pnpm-workspace.yaml"), "packages:\n  - packages/*\n");
}
