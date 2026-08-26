use crate::_utils::{with_colors, without_colors};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use console::strip_ansi_codes;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{ffi::OsStr, fs, path::Path, process::Command};
use tempfile::TempDir;

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const HELLO: &str = "@pnpm.e2e/hello-world-js-bin";
const PKG: &str = "@pnpm.e2e/pkg-with-1-dep";

fn setup() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    (root, workspace, npmrc_info)
}

fn pacquet(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_args(args)
}

fn write_manifest(workspace: &Path, dependencies: &str) {
    let manifest =
        format!(r#"{{ "name": "test-why", "version": "1.0.0", "dependencies": {dependencies} }}"#);
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
}

#[test]
fn why_fails_without_package_name() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why"]).output().expect("run pacquet why");
    assert!(!output.status.success(), "why without args should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("requires the package name or --find-by=<finder-name>"),
        "should show error about missing package name: {stderr}",
    );
    assert!(stderr.contains("ERR_PNPM_MISSING_PACKAGE_NAME"), "stderr: {stderr}");
}

#[test]
fn recursive_why_uses_the_active_dedicated_lockfile() {
    let (_root, workspace, _anchor) = setup();
    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write workspace manifest");
    write_manifest(&workspace, "{}");
    let app = workspace.join("packages/app");
    fs::create_dir_all(&app).expect("create app project");
    fs::write(
        app.join("package.json"),
        format!(
            r#"{{ "name": "app", "version": "1.0.0", "dependencies": {{ "{PKG}": "100.0.0" }} }}"#,
        ),
    )
    .expect("write app manifest");
    pacquet(&app, ["install"]).assert().success();

    let output = pacquet(&app, ["-r", "why", PKG]).output().expect("run recursive pacquet why");

    assert!(output.status.success(), "recursive why should succeed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(PKG) && stdout.contains("app@1.0.0"),
        "recursive why should query the active project lockfile: {stdout}",
    );
}

#[test]
fn why_shows_reverse_tree_for_direct_dep() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why", PKG]).output().expect("run pacquet why");
    assert!(output.status.success(), "why should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(PKG), "should mention the package: {stdout}");
    assert!(stdout.contains("100.0.0"), "should show the version: {stdout}");
    assert!(stdout.contains("test-why"), "should show the project as a dependent: {stdout}");
}

#[test]
fn why_shows_reverse_tree_for_transitive_dep() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why", DEP]).output().expect("run pacquet why");
    assert!(output.status.success(), "why should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(DEP), "should mention the package: {stdout}");
    assert!(stdout.contains(PKG), "should show PKG as a dependent: {stdout}");
    assert!(stdout.contains("test-why"), "should show the project as a dependent: {stdout}");
}

#[test]
fn why_with_glob_pattern() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0", "{DEP}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why", "@pnpm.e2e/*"]).output().expect("run pacquet why");
    assert!(output.status.success(), "why with glob should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(PKG), "should mention pkg-with-1-dep: {stdout}");
    assert!(stdout.contains(DEP), "should mention dep-of-pkg-with-1-dep: {stdout}");
}

#[test]
fn why_without_lockfile_returns_empty() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));

    let output = pacquet(&workspace, ["why", PKG]).output().expect("run pacquet why");
    assert!(output.status.success(), "why without lockfile should succeed like pnpm: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.is_empty(), "should produce no output without lockfile: {stdout}");
}

#[test]
fn why_depth_limits_output() {
    let (_root, workspace, _anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output_full =
        pacquet(&workspace, ["why", DEP]).output().expect("run pacquet why --depth unset");
    let output_depth1 = pacquet(&workspace, ["why", DEP, "--depth", "1"])
        .output()
        .expect("run pacquet why --depth 1");

    let full_stdout = String::from_utf8_lossy(&output_full.stdout);
    let depth1_stdout = String::from_utf8_lossy(&output_depth1.stdout);
    assert!(full_stdout.contains("test-why"), "full output shows project: {full_stdout}");
    assert!(depth1_stdout.contains(DEP), "depth=1 output still shows the target: {depth1_stdout}");
    assert!(depth1_stdout.contains(PKG), "depth=1 output shows direct parent: {depth1_stdout}");
}

/// `why` is recursive by default inside a workspace, so from a member it
/// reports the whole workspace — including a dependency only a sibling
/// declares. `recursiveInstall: false` is what narrows it to the current
/// project and its dependencies (pnpm's `{.}...` filter); both halves are
/// checked against `pnpm why` in pnpm 11.
#[test]
fn why_from_a_workspace_member_covers_the_workspace_until_recursive_install_is_off() {
    let (_root, workspace, _anchor) = setup();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    write_manifest(&workspace, "{}");

    let app = workspace.join("packages/app");
    let linked = workspace.join("packages/linked");
    let sibling = workspace.join("packages/sibling");
    for project in [&app, &linked, &sibling] {
        fs::create_dir_all(project).expect("create workspace project");
    }
    fs::write(
        app.join("package.json"),
        r#"{ "name": "app", "version": "1.0.0", "dependencies": { "linked": "workspace:*" } }"#,
    )
    .expect("write app package.json");
    fs::write(
        linked.join("package.json"),
        format!(
            r#"{{ "name": "linked", "version": "1.0.0", "dependencies": {{ "{PKG}": "100.0.0" }} }}"#,
        ),
    )
    .expect("write linked package.json");
    fs::write(
        sibling.join("package.json"),
        format!(
            r#"{{ "name": "sibling", "version": "1.0.0", "dependencies": {{ "{HELLO}": "1.0.0" }} }}"#,
        ),
    )
    .expect("write sibling package.json");
    pacquet(&workspace, ["install"]).assert().success();

    let sibling_output = pacquet(&app, ["why", HELLO]).output().expect("query sibling dependency");
    assert!(sibling_output.status.success(), "why should succeed: {sibling_output:?}");
    let sibling_stdout = String::from_utf8_lossy(&sibling_output.stdout);
    assert!(
        sibling_stdout.contains("sibling@1.0.0"),
        "a recursive why reports the sibling's own dependency: {sibling_stdout}",
    );

    let linked_output = pacquet(&app, ["why", PKG]).output().expect("query linked dependency");
    assert!(linked_output.status.success(), "why should succeed: {linked_output:?}");
    let linked_stdout = String::from_utf8_lossy(&linked_output.stdout);
    assert!(linked_stdout.contains(PKG), "linked dependency should be reported: {linked_stdout}");
    assert!(
        linked_stdout.contains("linked@1.0.0"),
        "the forward workspace-link closure should be retained: {linked_stdout}",
    );

    fs::write(
        workspace.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\nrecursiveInstall: false\n",
    )
    .expect("rewrite workspace manifest");

    let scoped_sibling =
        pacquet(&app, ["why", HELLO]).output().expect("query sibling dependency when scoped");
    assert!(scoped_sibling.status.success(), "why should succeed: {scoped_sibling:?}");
    let scoped_sibling_stdout = String::from_utf8_lossy(&scoped_sibling.stdout);
    assert!(
        scoped_sibling_stdout.is_empty(),
        "a scoped why leaves the sibling out: {scoped_sibling_stdout}",
    );

    let scoped_linked =
        pacquet(&app, ["why", PKG]).output().expect("query linked dependency when scoped");
    assert!(scoped_linked.status.success(), "why should succeed: {scoped_linked:?}");
    let scoped_linked_stdout = String::from_utf8_lossy(&scoped_linked.stdout);
    assert!(
        scoped_linked_stdout.contains("linked@1.0.0"),
        "a scoped why keeps the forward workspace-link closure: {scoped_linked_stdout}",
    );
}

#[test]
fn filtered_why_excludes_unselected_workspace_siblings() {
    let (_root, workspace, _anchor) = setup();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    write_manifest(&workspace, "{}");

    let app = workspace.join("packages/app");
    let sibling = workspace.join("packages/sibling");
    fs::create_dir_all(&app).expect("create app project");
    fs::create_dir_all(&sibling).expect("create sibling project");
    fs::write(
        app.join("package.json"),
        format!(
            r#"{{ "name": "app", "version": "1.0.0", "dependencies": {{ "{PKG}": "100.0.0" }} }}"#,
        ),
    )
    .expect("write app package.json");
    fs::write(
        sibling.join("package.json"),
        format!(
            r#"{{ "name": "sibling", "version": "1.0.0", "dependencies": {{ "{HELLO}": "1.0.0" }} }}"#,
        ),
    )
    .expect("write sibling package.json");
    pacquet(&workspace, ["install"]).assert().success();

    let excluded = pacquet(&workspace, ["--filter", "app", "why", HELLO])
        .output()
        .expect("query unselected sibling dependency");
    assert!(excluded.status.success(), "filtered why should succeed: {excluded:?}");
    assert!(
        String::from_utf8_lossy(&excluded.stdout).is_empty(),
        "a dependency reachable only from an unselected sibling must not be reported: {}",
        String::from_utf8_lossy(&excluded.stdout),
    );

    let included = pacquet(&workspace, ["--filter", "app", "why", PKG])
        .output()
        .expect("query selected dependency");
    assert!(included.status.success(), "filtered why should succeed: {included:?}");
    assert!(
        String::from_utf8_lossy(&included.stdout).contains(PKG),
        "a selected dependency should be reported: {}",
        String::from_utf8_lossy(&included.stdout),
    );
}

#[test]
fn why_is_recursive_by_default_inside_a_workspace() {
    let (_root, workspace, _anchor) = setup();
    fs::write(workspace.join("pnpm-workspace.yaml"), "packages:\n  - packages/*\n")
        .expect("write workspace manifest");
    write_manifest(&workspace, "{}");
    let sibling = workspace.join("packages/sibling");
    fs::create_dir_all(&sibling).expect("create workspace project");
    fs::write(
        sibling.join("package.json"),
        format!(
            r#"{{ "name": "sibling", "version": "1.0.0", "dependencies": {{ "{HELLO}": "1.0.0" }} }}"#,
        ),
    )
    .expect("write sibling package.json");
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why", HELLO])
        .output()
        .expect("query default-recursive workspace dependency");

    assert!(output.status.success(), "recursive why should succeed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(HELLO), "recursive why should include sibling dependencies: {stdout}");
    assert!(
        stdout.contains("sibling@1.0.0"),
        "recursive why should include the sibling importer: {stdout}",
    );
}

// --- ports of the TypeScript why command tests --------------------------------
//
// Source: pnpm11/deps/inspection/commands/test/listing/why.ts.

/// Port of upstream's `"why" should show reverse dependency tree for a
/// non-direct dependency`.
#[test]
fn why_shows_reverse_dependency_tree_for_a_non_direct_dependency() {
    let (_root, workspace, _anchor) = setup();
    fs::write(
        workspace.join("package.json"),
        format!(
            r#"{{ "name": "project", "version": "0.0.0", "dependencies": {{ "{DEP}": "100.0.0", "{PKG}": "100.0.0" }} }}"#,
        ),
    )
    .expect("write package.json");
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why", "--prod", DEP]).output().expect("run pacquet why");
    assert!(output.status.success(), "why should succeed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines[0], format!("{DEP}@100.0.0"), "root is the searched package: {stdout}");
    assert!(
        lines.iter().any(|line| line.contains("project@0.0.0")),
        "shows project as a direct dependent: {stdout}",
    );
    assert!(
        lines.iter().any(|line| line.contains(&format!("{PKG}@100.0.0"))),
        "shows the transitive path: {stdout}",
    );
}

/// Port of upstream's `"why" should find packages by alias name when
/// using npm: protocol`.
#[test]
fn why_finds_packages_by_alias_name_when_using_npm_protocol() {
    let (_root, workspace, _anchor) = setup();
    fs::write(
        workspace.join("package.json"),
        format!(
            r#"{{ "name": "project", "version": "0.0.0", "dependencies": {{ "foo": "npm:{PKG}@100.0.0" }} }}"#,
        ),
    )
    .expect("write package.json");
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why", "--prod", "foo"]).output().expect("run pacquet why");
    assert!(output.status.success(), "why should succeed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines[0], format!("{PKG}@100.0.0"), "root shows the canonical name: {stdout}");
    assert!(
        lines.iter().any(|line| line.contains("project@0.0.0")),
        "shows the project as dependent: {stdout}",
    );
}

/// Port of upstream's `"why" should find packages by actual package
/// name when using npm: protocol`.
#[test]
fn why_finds_packages_by_actual_name_when_using_npm_protocol() {
    let (_root, workspace, _anchor) = setup();
    fs::write(
        workspace.join("package.json"),
        format!(
            r#"{{ "name": "project", "version": "0.0.0", "dependencies": {{ "foo": "npm:{PKG}@100.0.0" }} }}"#,
        ),
    )
    .expect("write package.json");
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why", "--prod", PKG]).output().expect("run pacquet why");
    assert!(output.status.success(), "why should succeed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines[0], format!("{PKG}@100.0.0"), "root shows the canonical name: {stdout}");
    assert!(
        lines.iter().any(|line| line.contains("project@0.0.0")),
        "shows the project as dependent: {stdout}",
    );
}

/// Port of upstream's `"why" should display parseable output`.
#[test]
fn why_displays_parseable_output() {
    let (_root, workspace, _anchor) = setup();
    fs::write(
        workspace.join("package.json"),
        format!(
            r#"{{ "name": "project", "version": "0.0.0", "dependencies": {{ "{DEP}": "100.0.0", "{PKG}": "100.0.0" }} }}"#,
        ),
    )
    .expect("write package.json");
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why", "--parseable", "--prod", DEP])
        .output()
        .expect("run pacquet why");
    assert!(output.status.success(), "why should succeed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(
        lines.contains(&format!("project@0.0.0 > {DEP}@100.0.0").as_str()),
        "direct path is importer-first: {stdout}",
    );
    assert!(
        lines.contains(&format!("project@0.0.0 > {PKG}@100.0.0 > {DEP}@100.0.0").as_str()),
        "transitive path is importer-first: {stdout}",
    );
}

/// Port of upstream's `"why" should display finder message in tree
/// output`.
#[test]
fn why_displays_finder_message_in_tree_output() {
    let (_root, workspace, _anchor) = setup();
    write_finder_pnpmfile(&workspace, "'Found: has 1 dep'");
    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output =
        pacquet(&workspace, ["why", "--find-by=test-finder"]).output().expect("run pacquet why");
    assert!(output.status.success(), "why should succeed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines[0], format!("{PKG}@100.0.0"), "stdout: {stdout}");
    assert_eq!(lines[1], "\u{2502} Found: has 1 dep", "stdout: {stdout}");
}

/// Port of upstream's `"why" should display finder message in JSON
/// output`.
#[test]
fn why_displays_finder_message_in_json_output() {
    let (_root, workspace, _anchor) = setup();
    write_finder_pnpmfile(&workspace, "'custom message'");
    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why", "--json", "--find-by=test-finder"])
        .output()
        .expect("run pacquet why");
    assert!(output.status.success(), "why should succeed: {output:?}");
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).expect("parse why JSON");
    let matched = parsed
        .as_array()
        .expect("array output")
        .iter()
        .find(|result| result["name"] == PKG)
        .expect("the finder-matched package is present");
    assert_eq!(matched["searchMessage"], "custom message");
}

/// Port of upstream's `"why" finder can read manifest from store`.
#[test]
fn why_finder_can_read_manifest_from_store() {
    let (_root, workspace, _anchor) = setup();
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        format!(
            r"
module.exports = {{ finders: {{ 'manifest-reader': (ctx) => {{
  const manifest = ctx.readManifest()
  if (manifest && manifest.name === '{PKG}') {{
    return 'description: ' + (manifest.description ?? 'none')
  }}
  return false
}} }} }}
",
        ),
    )
    .expect("write .pnpmfile.cjs");
    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why", "--json", "--find-by=manifest-reader"])
        .output()
        .expect("run pacquet why");
    assert!(output.status.success(), "why should succeed: {output:?}");
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).expect("parse why JSON");
    let matched = parsed
        .as_array()
        .expect("array output")
        .iter()
        .find(|result| result["name"] == PKG)
        .expect("the finder-matched package is present");
    let message = matched["searchMessage"].as_str().expect("searchMessage string");
    assert!(message.starts_with("description: "), "searchMessage: {message}");
}

/// Port of upstream's `"why" should find file: protocol local packages`.
#[test]
fn why_finds_file_protocol_local_packages() {
    let (_root, workspace, _anchor) = setup();
    let local_pkg = workspace.join("local-pkg");
    fs::create_dir_all(&local_pkg).expect("create local-pkg");
    fs::write(local_pkg.join("package.json"), r#"{ "name": "my-local-pkg", "version": "1.0.0" }"#)
        .expect("write local-pkg package.json");
    fs::write(
        workspace.join("package.json"),
        r#"{ "name": "project", "version": "0.0.0", "dependencies": { "my-alias": "file:./local-pkg" } }"#,
    )
    .expect("write package.json");
    pacquet(&workspace, ["install"]).assert().success();

    let output =
        pacquet(&workspace, ["why", "--prod", "my-local-pkg"]).output().expect("run pacquet why");
    assert!(output.status.success(), "why should succeed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines[0].contains("my-local-pkg"), "finds the local package: {stdout}");
    assert!(
        lines.iter().any(|line| line.contains("project@0.0.0")),
        "shows the project as dependent: {stdout}",
    );
}

/// The importer leaf carries the dependency field it declares the chain
/// in, and the output ends with the `Found …` summary (mirrors the
/// `renderDependentsTree` contract exercised end to end).
#[test]
fn why_marks_importer_dep_field_and_prints_summary() {
    let (_root, workspace, _anchor) = setup();
    fs::write(
        workspace.join("package.json"),
        format!(
            r#"{{ "name": "project", "version": "0.0.0", "devDependencies": {{ "{PKG}": "100.0.0" }} }}"#,
        ),
    )
    .expect("write package.json");
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["why", PKG]).output().expect("run pacquet why");
    assert!(output.status.success(), "why should succeed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        stdout,
        format!(
            "{PKG}@100.0.0\n\u{2514}\u{2500}\u{2500} project@0.0.0 (devDependencies)\n\nFound 1 version of {PKG}\n"
        ),
    );
}

#[test]
fn why_styles_the_tree_without_corrupting_it() {
    let (_root, workspace, _anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let plain =
        without_colors(pacquet(&workspace, ["why", PKG])).output().expect("run pacquet why");
    assert!(plain.status.success(), "why should succeed: {plain:?}");
    let colored = with_colors(pacquet(&workspace, ["why", PKG]))
        .output()
        .expect("run pacquet why with colors");
    assert!(colored.status.success(), "colored why should succeed: {colored:?}");

    let plain_stdout = String::from_utf8_lossy(&plain.stdout);
    let colored_stdout = String::from_utf8_lossy(&colored.stdout);
    eprintln!("PLAIN:\n{plain_stdout}\nCOLORED:\n{colored_stdout}\n");
    assert!(colored_stdout.contains('\u{1b}'), "colors should be on");
    assert_eq!(strip_ansi_codes(&colored_stdout), plain_stdout);
}

#[test]
fn color_modes_override_terminal_environment_hints() {
    let (_root, workspace, _anchor) = setup();
    write_manifest(&workspace, &format!(r#"{{ "{PKG}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let always = without_colors(pacquet(&workspace, ["--color=always", "why", PKG]))
        .output()
        .expect("run color=always");
    assert!(always.status.success(), "color=always should succeed: {always:?}");
    assert!(String::from_utf8_lossy(&always.stdout).contains('\u{1b}'));

    let never = with_colors(pacquet(&workspace, ["--color=never", "why", PKG]))
        .output()
        .expect("run color=never");
    assert!(never.status.success(), "color=never should succeed: {never:?}");
    assert!(!String::from_utf8_lossy(&never.stdout).contains('\u{1b}'));
}

fn write_finder_pnpmfile(workspace: &Path, message_expr: &str) {
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        format!(
            r"
module.exports = {{ finders: {{ 'test-finder': (ctx) => {{
  if (ctx.name === '{PKG}') {{
    return {message_expr}
  }}
  return false
}} }} }}
",
        ),
    )
    .expect("write .pnpmfile.cjs");
}
