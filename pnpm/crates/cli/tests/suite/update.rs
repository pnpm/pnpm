use crate::_utils;

use _utils::{
    append_workspace_yaml_key, bravo_dep_mature_up_to_1_0_1_minimum_release_age,
    lockfile_package_keys, set_ignore_dependencies, set_minimum_release_age,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use std::{ffi::OsStr, fmt::Write as _, fs, path::Path, process::Command};
use tempfile::TempDir;

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const FOO: &str = "@pnpm.e2e/foo";
const BAR: &str = "@pnpm.e2e/bar";
/// Declares `peer-a`, `peer-b`, and `peer-c` as peers, which an install
/// auto-installs.
const ABC: &str = "@pnpm.e2e/abc";
const PEER_A: &str = "@pnpm.e2e/peer-a";
const PEER_C: &str = "@pnpm.e2e/peer-c";
const HAS_PRERELEASE: &str = "@pnpm.e2e/has-prerelease";
/// Depends on `dep-of-pkg-with-1-dep@^100.0.0`, used to exercise
/// indirect-dependency update behavior when the direct dep is ignored.
const PARENT: &str = "@pnpm.e2e/pkg-with-1-dep";

/// Spin up a temp workspace with the mocked registry and return the
/// pieces a multi-step update test needs.
fn setup() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    (root, workspace, npmrc_info)
}

/// [`setup`] over fixture storage this test owns, so it can move dist
/// tags mid-test the way the upstream tests' `addDistTag` does.
fn setup_with_own_registry() -> (TempDir, std::path::PathBuf, AddMockedRegistry) {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry_with_own_storage();
    (root, workspace, npmrc_info)
}

/// Build a fresh `pacquet` command bound to `workspace`. The
/// `assert_cmd` `Command` is single-shot, so each install/update step
/// needs its own.
fn pacquet(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(workspace)
        .with_args(args)
}

fn write_manifest(workspace: &Path, dependencies: &str) {
    let manifest = format!(
        r#"{{ "name": "test-update", "version": "1.0.0", "dependencies": {dependencies} }}"#,
    );
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
}

/// Create a sibling workspace project and register its directory in
/// `pnpm-workspace.yaml`'s `packages` list.
fn add_workspace_package(workspace: &Path, name: &str, version: &str) {
    let project = workspace.join(name);
    fs::create_dir_all(&project).expect("mkdir workspace project");
    fs::write(
        project.join("package.json"),
        format!(r#"{{ "name": "{name}", "version": "{version}" }}"#),
    )
    .expect("write workspace project package.json");

    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    if !yaml.contains("packages:") {
        yaml.push_str("packages:\n");
    }
    writeln!(yaml, "  - '{name}'").unwrap();
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
}

/// [`append_workspace_yaml_key`] for `dedupePeerDependents: false` — the
/// setting under which pnpm/pnpm#12456 reproduces on the TypeScript stack.
fn disable_dedupe_peer_dependents(workspace: &Path) {
    append_workspace_yaml_key(workspace, "dedupePeerDependents", false);
}

fn dep_spec(workspace: &Path, name: &str) -> Option<String> {
    let manifest = PackageManifest::from_path(workspace.join("package.json")).unwrap();
    manifest
        .dependencies([DependencyGroup::Prod])
        .find(|(key, _)| *key == name)
        .map(|(_, spec)| spec.to_string())
}

fn virtual_store_has(workspace: &Path, name_at_version: &str) -> bool {
    workspace.join("node_modules").join(".pnpm").join(name_at_version).exists()
}

/// List the `node_modules/.pnpm` entries. Logged before
/// [`virtual_store_has`] assertions so a failing CI run shows what was
/// actually materialized.
fn list_virtual_store(workspace: &Path) -> Vec<String> {
    let dir = workspace.join("node_modules").join(".pnpm");
    std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .filter_map(|entry| {
                    entry.ok().map(|entry| entry.file_name().to_string_lossy().into_owned())
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `pacquet update` re-resolves a dependency to the highest version
/// inside its range, even when the lockfile pins an older one — the
/// behaviour that distinguishes it from a plain `install` (which keeps
/// the pin because it still satisfies the range).
#[test]
fn update_bumps_within_range() {
    let (root, workspace, anchor) = setup();

    // Pin 100.0.0 exactly, then widen the range to `^100.0.0`. A plain
    // install would keep 100.0.0 (it satisfies `^100.0.0`); update must
    // bump to 100.1.0 (101.0.0 is outside the range).
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.0.0"));

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["update"]).assert().success();

    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(
        virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"),
        "update should have bumped the dependency to the highest version in range",
    );
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.1.0"));

    // The rewritten range is what the lockfile importer records, so the
    // lockfile is still frozen-installable.
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// An exact pin is included because it has no room to move.
#[test]
fn update_preserves_the_declared_range_operator() {
    let (root, workspace, anchor) = setup();

    write_manifest(
        &workspace,
        &format!(
            r#"{{ "@pnpm.e2e/bravo-dep": "~1.0.0", "{FOO}": "1.0.0", "{PARENT}": "^100.0.0" }}"#,
        ),
    );
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update"]).assert().success();

    assert_eq!(dep_spec(&workspace, "@pnpm.e2e/bravo-dep").as_deref(), Some("~1.0.1"));
    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("1.0.0"));
    assert_eq!(dep_spec(&workspace, PARENT).as_deref(), Some("^100.1.0"));

    drop((root, anchor));
}

#[test]
fn update_preserves_an_existing_prerelease_range_operator() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{HAS_PRERELEASE}": "3.0.0-rc.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    assert!(
        virtual_store_has(&workspace, "@pnpm.e2e+has-prerelease@3.0.0-rc.0"),
        "virtual store entries: {:?}",
        list_virtual_store(&workspace),
    );

    write_manifest(&workspace, &format!(r#"{{ "{HAS_PRERELEASE}": "^3.0.0-rc.0" }}"#));
    pacquet(&workspace, ["update"]).assert().success();

    assert!(
        virtual_store_has(&workspace, "@pnpm.e2e+has-prerelease@3.0.0-rc.1"),
        "virtual store entries: {:?}",
        list_virtual_store(&workspace),
    );
    assert_eq!(dep_spec(&workspace, HAS_PRERELEASE).as_deref(), Some("^3.0.0-rc.1"));
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// A dist-tag names no version of its own, so there is nothing to rewrite.
#[test]
fn update_keeps_a_dist_tag_specifier() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "latest" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update"]).assert().success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("latest"));

    drop((root, anchor));
}

/// `pnpm update <name>@<version>` records the version under the operator
/// the manifest already pins, the way pnpm 11 does. Regression test for
/// <https://github.com/pnpm/pnpm/issues/14745>.
#[test]
fn update_with_a_requested_version_keeps_the_declared_range_operator() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "100.0.0", "{FOO}": "1.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0", "{FOO}": "~1.0.0" }}"#));

    pacquet(&workspace, ["update", &format!("{DEP}@100.1.0"), &format!("{FOO}@100.1.0")])
        .assert()
        .success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.1.0"));
    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("~100.1.0"));
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+foo@100.1.0"));
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// An exact pin and a dist tag carry no operator to keep, so the requested
/// version is recorded as is.
#[test]
fn update_with_a_requested_version_keeps_an_exact_pin() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "100.0.0", "{FOO}": "latest" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", &format!("{DEP}@100.1.0"), &format!("{FOO}@1.0.0")])
        .assert()
        .success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("100.1.0"));
    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("1.0.0"));
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// The kept range admits newer versions than the one requested, so the
/// lockfile has to record the request rather than re-resolve to the
/// range's highest version.
#[test]
fn update_with_a_requested_version_locks_that_version_inside_the_kept_range() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"));

    pacquet(&workspace, ["update", &format!("{DEP}@100.0.0")]).assert().success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));
    let lock = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(lock.contains("version: 100.0.0"), "the requested version must be locked:\n{lock}");
    assert!(
        !lock.contains("version: 100.1.0"),
        "the range's highest version must not win:\n{lock}",
    );
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// An aliased entry keeps its `npm:<name>@` prefix, and the requested version
/// still reaches the lockfile under the package name the alias resolves to.
#[test]
fn update_with_a_requested_version_keeps_an_npm_alias() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "dep-alias": "npm:{DEP}@^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"));

    pacquet(&workspace, ["update", "dep-alias@100.0.0"]).assert().success();

    assert_eq!(dep_spec(&workspace, "dep-alias").as_deref(), Some(&*format!("npm:{DEP}@^100.0.0")));
    let lock = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        lock.contains(&format!("version: '{DEP}@100.0.0'")),
        "the requested version must be locked:\n{lock}",
    );
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// `--latest` keeps the operator a prerelease range already pins, the same
/// way a plain update does.
#[test]
fn update_latest_keeps_a_prerelease_range_operator() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{HAS_PRERELEASE}": "3.0.0-rc.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    write_manifest(&workspace, &format!(r#"{{ "{HAS_PRERELEASE}": "^3.0.0-rc.0" }}"#));

    pacquet(&workspace, ["update", "--latest"]).assert().success();

    assert_eq!(dep_spec(&workspace, HAS_PRERELEASE).as_deref(), Some("^3.0.0-rc.1"));
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// Dedicated per-project lockfiles anchor importer ids at the project
/// rather than the workspace root, so the range rewrite has to derive them
/// the same way the install does or it silently matches no importer.
#[test]
fn update_rewrites_the_range_with_dedicated_lockfiles() {
    let (root, workspace, anchor) = setup();
    append_workspace_yaml_key(&workspace, "sharedWorkspaceLockfile", false);
    add_workspace_package(&workspace, "a", "1.0.0");
    let project = workspace.join("a");
    fs::write(
        project.join("package.json"),
        format!(
            r#"{{ "name": "a", "version": "1.0.0", "dependencies": {{ "{DEP}": "^100.0.0" }} }}"#,
        ),
    )
    .expect("write project package.json");

    pacquet(&project, ["install"]).assert().success();
    pacquet(&project, ["update"]).assert().success();

    assert_eq!(dep_spec(&project, DEP).as_deref(), Some("^100.1.0"));
    pacquet(&project, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// `--no-save` keeps `package.json` authoritative, so the lockfile moves
/// within the declared range while the range itself stands.
#[test]
fn update_no_save_keeps_the_declared_range() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["update", "--no-save"]).assert().success();

    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"));
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));

    drop((root, anchor));
}

#[test]
fn update_runs_with_ndjson_and_silent_reporters() {
    for reporter in ["--reporter=ndjson", "--reporter=silent"] {
        let (root, workspace, anchor) = setup();

        write_manifest(&workspace, &format!(r#"{{ "{DEP}": "100.0.0" }}"#));
        pacquet(&workspace, ["install"]).assert().success();
        write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));

        pacquet(&workspace, [reporter, "update"]).assert().success();

        assert!(
            virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"),
            "update should bump the dependency when running with {reporter}",
        );

        drop((root, anchor));
    }
}

/// `pacquet update --latest` ignores the manifest range, bumps to the
/// `latest` dist-tag, and rewrites `package.json`.
#[test]
fn update_latest_rewrites_manifest() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"));

    pacquet(&workspace, ["update", "--latest"]).assert().success();

    // latest tag is the max published version, 101.0.0.
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@101.0.0"));
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^101.0.0"));

    drop((root, anchor));
}

/// `--latest` keeps the range operator the dependency already used, even
/// when `--save-exact` is passed: a pre-existing pin takes precedence over
/// the config default, matching pnpm's `calcRange`. (`pnpm update --latest
/// --save-exact` on `^1.0.0` writes `^<latest>`, not the exact version.)
#[test]
fn update_latest_save_exact_preserves_existing_caret() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest", "--save-exact"]).assert().success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^101.0.0"));

    drop((root, anchor));
}

/// `--latest` preserves a tilde range instead of widening it to the default
/// caret. Ports the prefix-preservation half of pnpm's `calcRange`.
#[test]
fn update_latest_preserves_tilde() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "~100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest"]).assert().success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("~101.0.0"));

    drop((root, anchor));
}

/// A dist-tag already reaches the latest version, so `--latest` has nothing
/// to rewrite either.
#[test]
fn update_latest_keeps_a_dist_tag_specifier() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "latest" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest"]).assert().success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("latest"));

    drop((root, anchor));
}

/// `--latest` preserves an exact pin (no range operator) without needing
/// `--save-exact`.
#[test]
fn update_latest_preserves_exact() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest"]).assert().success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("101.0.0"));

    drop((root, anchor));
}

/// `--latest` treats a `=` pin (`=100.0.0`) as an exact pin instead of
/// widening it to the default caret range, and keeps the explicit `=`
/// operator when writing the new version back. Regression test for
/// <https://github.com/pnpm/pnpm/issues/12745> and
/// <https://github.com/pnpm/pnpm/issues/13168>.
#[test]
fn update_latest_preserves_equals_pin() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "=100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest"]).assert().success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("=101.0.0"));

    drop((root, anchor));
}

/// `--no-save` bumps the lockfile but leaves `package.json` untouched —
/// ports pnpm's "update --no-save should not update package.json" test.
/// The bump stays inside the kept range: `--latest` would reach 101.0.0,
/// which the retained `^100.0.0` cannot record.
#[test]
fn update_latest_no_save_keeps_manifest() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.0.0"));
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));

    let output = pacquet(&workspace, ["update", "--latest", "--no-save"])
        .output()
        .expect("run update --latest --no-save");
    assert!(output.status.success(), "update --latest --no-save failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(r#"Ignoring "--latest""#),
        "the ignored --latest must be reported to the user: {stdout}",
    );

    // package.json range is unchanged...
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));
    // ...and the lockfile/store re-resolved to the highest version it admits.
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"));
    assert!(!virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@101.0.0"));
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// `up` and `upgrade` are accepted as aliases of `update`.
#[test]
fn update_aliases_work() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["up", "--latest"]).assert().success();
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^101.0.0"));

    drop((root, anchor));
}

/// `--latest` combined with a versioned selector is rejected, matching
/// pnpm's `ERR_PNPM_LATEST_WITH_SPEC`.
#[test]
fn update_latest_with_spec_is_rejected() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["update", "--latest", &format!("{DEP}@2")])
        .output()
        .expect("run pacquet update");
    assert!(!output.status.success(), "update --latest with a spec should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Specs are not allowed to be used with --latest"),
        "stderr did not mention the LATEST_WITH_SPEC error: {stderr}",
    );

    drop((root, anchor));
}

/// The failing half of a `pacquet update` run: the command must exit
/// non-zero and its stderr must mention `needle`.
fn assert_update_fails(workspace: &Path, args: &[&str], needle: &str) {
    let output = pacquet(workspace, args).output().expect("run pacquet update");
    assert!(!output.status.success(), "`pacquet {}` should fail", args.join(" "));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(needle), "stderr did not mention {needle:?}: {stderr}");
}

/// Append `catalogMode: strict` and a default `catalog:` with the given
/// `(name, specifier)` entries to the harness-written
/// `pnpm-workspace.yaml`.
fn set_strict_catalog(workspace: &Path, entries: &[(&str, &str)]) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str("catalogMode: strict\ncatalog:\n");
    for (name, spec) in entries {
        writeln!(yaml, r#"  "{name}": "{spec}""#).unwrap();
    }
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
}

/// Append a named `catalogs:` block (default `manual` catalogMode) to the
/// harness-written `pnpm-workspace.yaml`.
fn set_named_catalog(workspace: &Path, catalog: &str, entries: &[(&str, &str)]) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    writeln!(yaml, "catalogs:\n  {catalog}:").unwrap();
    for (name, spec) in entries {
        writeln!(yaml, r#"    "{name}": "{spec}""#).unwrap();
    }
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
}

/// Append an `overrides:` block with the given `(name, specifier)` entries
/// to the harness-written `pnpm-workspace.yaml`.
fn set_overrides(workspace: &Path, entries: &[(&str, &str)]) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str("overrides:\n");
    for (name, spec) in entries {
        writeln!(yaml, r#"  "{name}": "{spec}""#).unwrap();
    }
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
}

fn read_workspace_yaml(workspace: &Path) -> String {
    fs::read_to_string(workspace.join("pnpm-workspace.yaml")).expect("read pnpm-workspace.yaml")
}

/// The alias name does not exist in the mock registry.
#[test]
fn update_latest_npm_alias_resolves_aliased_package() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "dep-alias": "npm:{DEP}@~100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.0.0"));

    pacquet(&workspace, ["update", "--latest"]).assert().success();

    assert_eq!(dep_spec(&workspace, "dep-alias").as_deref(), Some(&*format!("npm:{DEP}@~101.0.0")));
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@101.0.0"));

    drop((root, anchor));
}

/// See [`_utils::bravo_dep_mature_up_to_1_0_1_minimum_release_age`] for the
/// publish dates the `minimumReleaseAge` tests below rely on.
const BRAVO_DEP: &str = "@pnpm.e2e/bravo-dep";

/// Covers <https://github.com/pnpm/pnpm/issues/11165>: a compatible update
/// under an active `minimumReleaseAge` re-resolves to the newest *mature*
/// in-range version instead of the raw highest one.
#[test]
fn update_respects_minimum_release_age() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{BRAVO_DEP}": "1.0.0" }}"#));
    set_minimum_release_age(&workspace, bravo_dep_mature_up_to_1_0_1_minimum_release_age());
    pacquet(&workspace, ["install"]).assert().success();
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+bravo-dep@1.0.0"));

    // Widen the range so the update has newer versions to consider: 1.0.1
    // is mature under the cutoff, the newest in-range version (1.1.0) is
    // not.
    write_manifest(&workspace, &format!(r#"{{ "{BRAVO_DEP}": "^1.0.0" }}"#));
    pacquet(&workspace, ["update"]).assert().success();

    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+bravo-dep@1.0.1"));
    assert!(!virtual_store_has(&workspace, "@pnpm.e2e+bravo-dep@1.1.0"));

    drop((root, anchor));
}

/// Covers <https://github.com/pnpm/pnpm/issues/11165>: `update --latest`
/// under an active `minimumReleaseAge` writes the newest *mature* version
/// into `package.json`, not the raw `latest` dist-tag.
#[test]
fn update_latest_respects_minimum_release_age() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{BRAVO_DEP}": "^1.0.0" }}"#));
    set_minimum_release_age(&workspace, bravo_dep_mature_up_to_1_0_1_minimum_release_age());
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest"]).assert().success();

    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert_eq!(dep_spec(&workspace, BRAVO_DEP).as_deref(), Some("^1.0.1"));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+bravo-dep@1.0.1"));
    assert!(!virtual_store_has(&workspace, "@pnpm.e2e+bravo-dep@1.1.0"));

    drop((root, anchor));
}

/// Covers <https://github.com/pnpm/pnpm/issues/14835>: `update --no-save`
/// under a strict `minimumReleaseAge` runs to completion as long as every
/// pick is mature. Tools that refresh a lockfile without touching
/// manifests, Renovate among them, update this way.
#[test]
fn update_no_save_succeeds_when_every_pick_is_mature() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{BRAVO_DEP}": "1.0.0" }}"#));
    set_minimum_release_age(&workspace, bravo_dep_mature_up_to_1_0_1_minimum_release_age());
    pacquet(&workspace, ["install"]).assert().success();

    write_manifest(&workspace, &format!(r#"{{ "{BRAVO_DEP}": "^1.0.0" }}"#));
    pacquet(&workspace, ["update", "--no-save"]).assert().success();

    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert_eq!(dep_spec(&workspace, BRAVO_DEP).as_deref(), Some("^1.0.0"));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+bravo-dep@1.0.1"));

    drop((root, anchor));
}

/// `update --no-save` is still refused once a pick is immature: approving
/// it would have to be recorded in `minimumReleaseAgeExclude`, which
/// `--no-save` forbids.
#[test]
fn update_no_save_is_refused_when_a_pick_is_immature() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{BRAVO_DEP}": "1.1.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    set_minimum_release_age(&workspace, bravo_dep_mature_up_to_1_0_1_minimum_release_age());

    let output = pacquet(&workspace, ["update", "--no-save"]).assert().failure();
    let stderr = String::from_utf8_lossy(&output.get_output().stderr).into_owned();

    assert!(stderr.contains("ERR_PNPM_STRICT_MIN_RELEASE_AGE_REQUIRES_SAVE"), "{stderr}");

    drop((root, anchor));
}

/// An invalid `minimumReleaseAgeExclude` must not preempt command
/// validation: `update <name>@<spec> --latest` still fails with the
/// versioned-selector rejection, matching the TypeScript CLI, which
/// parses the excludes only once resolution starts.
#[test]
fn update_latest_spec_rejection_wins_over_invalid_minimum_release_age_exclude() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{BRAVO_DEP}": "^1.0.0" }}"#));
    append_workspace_yaml_key(
        &workspace,
        "minimumReleaseAgeExclude",
        format!(r#"["{BRAVO_DEP}@^1.0.0"]"#),
    );

    let output = pacquet(&workspace, ["update", "--latest", &format!("{BRAVO_DEP}@1.0.1")])
        .output()
        .expect("run pacquet update");
    assert!(!output.status.success(), "update --latest with a spec should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Specs are not allowed to be used with --latest"),
        "stderr did not mention the LATEST_WITH_SPEC error: {stderr}",
    );

    drop((root, anchor));
}

/// An invalid `minimumReleaseAgeExclude` that a `--latest` rewrite does
/// hit fails with `ERR_PNPM_INVALID_MINIMUM_RELEASE_AGE_EXCLUDE`, the
/// same code the install path and the TypeScript CLI report.
#[test]
fn update_latest_reports_invalid_minimum_release_age_exclude() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{BRAVO_DEP}": "^1.0.0" }}"#));
    append_workspace_yaml_key(
        &workspace,
        "minimumReleaseAgeExclude",
        format!(r#"["{BRAVO_DEP}@^1.0.0"]"#),
    );

    let output = pacquet(&workspace, ["update", "--latest"]).output().expect("run pacquet update");
    assert!(!output.status.success(), "update --latest with an invalid exclude should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Invalid value in minimumReleaseAgeExclude"),
        "stderr did not mention the invalid exclude: {stderr}",
    );

    drop((root, anchor));
}

/// `pnpm update --latest` must not resolve a local dependency against the
/// registry. `workspace:`, `file:`, and `link:` all point at a local package
/// that may be unpublished, so there is no registry "latest" to fetch; each is
/// preserved verbatim. Mirrors the TS `isLocalRef` guard (`link:`/`file:`/`workspace:`)
/// in `@pnpm/outdated`. Regression for the pnpm/pnpm update-lockfile job, whose
/// `@pnpm-private/*` deps are `workspace:*`.
#[test]
fn update_latest_preserves_local_protocol_dependencies() {
    let (root, workspace, anchor) = setup();

    fs::write(
        workspace.join("package.json"),
        r#"{ "name": "root", "version": "1.0.0", "private": true }"#,
    )
    .expect("write root package.json");

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    // Package `a` links three local, unpublished packages — `b` via `workspace:*`,
    // `c` via `file:`, and `d` via `link:` — alongside a real registry dependency
    // so `--latest` has work to do. `c` and `d` live under `packages/a/fixtures`,
    // which the `packages/*` glob does not match, so they are plain local deps
    // rather than workspace members.
    fs::create_dir_all(workspace.join("packages/a/fixtures/c")).expect("mkdir fixtures/c");
    fs::create_dir_all(workspace.join("packages/a/fixtures/d")).expect("mkdir fixtures/d");
    fs::write(
        workspace.join("packages/a/package.json"),
        format!(
            r#"{{ "name": "@test/a", "version": "1.0.0", "dependencies": {{ "@test/b": "workspace:*", "@test/c": "file:./fixtures/c", "@test/d": "link:./fixtures/d", "{DEP}": "^100.0.0" }} }}"#,
        ),
    )
    .expect("write packages/a/package.json");
    fs::write(
        workspace.join("packages/a/fixtures/c/package.json"),
        r#"{ "name": "@test/c", "version": "1.0.0" }"#,
    )
    .expect("write fixtures/c package.json");
    fs::write(
        workspace.join("packages/a/fixtures/d/package.json"),
        r#"{ "name": "@test/d", "version": "1.0.0" }"#,
    )
    .expect("write fixtures/d package.json");
    fs::create_dir_all(workspace.join("packages/b")).expect("mkdir packages/b");
    fs::write(
        workspace.join("packages/b/package.json"),
        r#"{ "name": "@test/b", "version": "1.0.0" }"#,
    )
    .expect("write packages/b/package.json");

    pacquet(&workspace, ["-r", "install"]).assert().success();
    // Before the fix this failed with ERR_PNPM_PACKAGE_MANAGER_UPDATE_RESOLVE_LATEST
    // trying to fetch the unpublished @test/b, @test/c, and @test/d from the registry.
    pacquet(&workspace, ["-r", "update", "--latest"]).assert().success();

    let a_manifest = fs::read_to_string(workspace.join("packages/a/package.json"))
        .expect("read packages/a/package.json");
    for (dep, spec) in [
        ("@test/b", "workspace:*"),
        ("@test/c", "file:./fixtures/c"),
        ("@test/d", "link:./fixtures/d"),
    ] {
        assert!(
            a_manifest.contains(&format!(r#""{dep}":"{spec}""#)),
            "the spec for {dep} should be preserved verbatim as {spec}: {a_manifest}",
        );
    }

    drop((root, anchor));
}

/// A versioned selector under `--no-save` is skipped when the requested
/// version falls outside the range the manifest keeps: recording it would
/// produce a lockfile the next frozen install rejects. Regression test for
/// <https://github.com/pnpm/pnpm/issues/12764>.
#[test]
fn update_no_save_skips_version_outside_kept_range() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"));

    let output = pacquet(&workspace, ["update", "--no-save", &format!("{DEP}@101.0.0")])
        .output()
        .expect("run update --no-save");
    assert!(output.status.success(), "update --no-save failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!(r#"Skipping "{DEP}@101.0.0""#)),
        "the skipped dependency must be reported to the user: {stdout}",
    );

    // package.json keeps its range, and the dependency stays untouched.
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));
    let lock = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        lock.contains("specifier: ^100.0.0"),
        "the lockfile importer entry must keep the manifest's specifier",
    );
    assert!(
        !lock.contains("dep-of-pkg-with-1-dep@101.0.0"),
        "the out-of-range requested version must not be recorded",
    );
    // The lockfile still satisfies the manifest.
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

#[test]
fn update_no_save_keeps_importer_specifier_for_admitted_version() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));

    let output =
        pacquet(&workspace, ["update", "--no-save", "--lockfile-only", &format!("{DEP}@100.1.0")])
            .output()
            .expect("run update --no-save");
    assert!(output.status.success(), "update --no-save failed: {output:?}");

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));
    let lock = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(lock.contains("version: 100.1.0"), "the requested admitted version must be resolved");
    assert!(
        lock.contains("specifier: ^100.0.0"),
        "the lockfile importer entry must keep the manifest's specifier: {lock}",
    );
    assert!(
        !lock.contains("specifier: 100.1.0"),
        "the requested version must not replace the importer specifier: {lock}",
    );
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

#[test]
fn update_no_save_applies_read_package_to_kept_importer_specifier() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        format!(
            "module.exports = {{ hooks: {{ readPackage (pkg) {{\n  if (pkg.name === 'test-update' && pkg.dependencies && pkg.dependencies[{DEP:?}]) {{\n    pkg.dependencies[{DEP:?}] = '100.1.0';\n  }}\n  return pkg;\n}} }} }}\n",
        ),
    )
    .expect("write pnpmfile");

    let output =
        pacquet(&workspace, ["update", "--no-save", "--lockfile-only", &format!("{DEP}@100.1.0")])
            .output()
            .expect("run update --no-save");
    assert!(output.status.success(), "update --no-save failed: {output:?}");

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));
    let lock = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        lock.contains("specifier: 100.1.0"),
        "the lockfile importer entry must follow readPackage's kept specifier: {lock}",
    );
    assert!(
        !lock.contains("specifier: ^100.0.0"),
        "the raw package.json specifier must not bypass readPackage: {lock}",
    );
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

#[test]
fn update_no_save_runs_read_package_once_for_kept_importer_specifier() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        format!(
            "const fs = require('fs');\nconst path = require('path');\nmodule.exports = {{ hooks: {{ readPackage (pkg) {{\n  if (pkg.name === 'test-update') {{\n    fs.appendFileSync(path.join(__dirname, 'read-package.log'), `${{pkg.dependencies && pkg.dependencies[{DEP:?}]}}\\n`);\n  }}\n  if (pkg.name === 'test-update' && pkg.dependencies && pkg.dependencies[{DEP:?}]) {{\n    pkg.dependencies[{DEP:?}] = '100.1.0';\n  }}\n  return pkg;\n}} }} }}\n",
        ),
    )
    .expect("write pnpmfile");

    let output =
        pacquet(&workspace, ["update", "--no-save", "--lockfile-only", &format!("{DEP}@100.1.0")])
            .output()
            .expect("run update --no-save");
    assert!(output.status.success(), "update --no-save failed: {output:?}");

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));
    let hook_log =
        fs::read_to_string(workspace.join("read-package.log")).expect("read readPackage log");
    let root_hook_inputs = hook_log.lines().collect::<Vec<_>>();
    assert_eq!(
        root_hook_inputs,
        vec!["^100.0.0"],
        "readPackage should see the kept importer manifest exactly once",
    );
    let lock = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        lock.contains("specifier: 100.1.0"),
        "the lockfile importer entry must use the transformed kept specifier: {lock}",
    );

    drop((root, anchor));
}

/// A requested range names no version until resolution runs, so the specifier
/// the manifest keeps decides — `>=101.0.0` cannot pull the lockfile past
/// `^100.0.0`. Regression test for
/// <https://github.com/pnpm/pnpm/issues/12764>.
#[test]
fn update_no_save_resolves_a_requested_range_within_the_kept_range() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));

    let output = pacquet(&workspace, ["update", "--no-save", &format!("{DEP}@>=101.0.0")])
        .output()
        .expect("run update --no-save");
    assert!(output.status.success(), "update --no-save failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(&format!(r#"Ignoring "{DEP}@>=101.0.0""#)),
        "the superseded selector must be reported to the user: {stdout}",
    );

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"));
    assert!(!virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@101.0.0"));
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// Ports `update to latest should not touch the automatically installed
/// peer dependencies`.
#[test]
fn update_latest_leaves_auto_installed_peers_alone() {
    let (root, workspace, anchor) = setup_with_own_registry();
    anchor.set_dist_tag(PEER_A, "1.0.0", "latest");
    anchor.set_dist_tag(PEER_C, "1.0.0", "latest");

    write_manifest(&workspace, &format!(r#"{{ "{ABC}": "1.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    anchor.set_dist_tag(PEER_A, "1.0.1", "latest");
    anchor.set_dist_tag(PEER_C, "1.0.1", "latest");
    anchor.set_dist_tag(ABC, "2.0.0", "latest");

    pacquet(&workspace, ["update", "--latest", ABC]).assert().success();

    let packages = lockfile_package_keys(&workspace);
    assert!(packages.contains(&format!("{ABC}@2.0.0")), "{packages:?}");
    assert!(packages.contains(&format!("{PEER_A}@1.0.0")), "{packages:?}");
    assert!(!packages.contains(&format!("{PEER_A}@1.0.1")), "{packages:?}");
    assert!(packages.contains(&format!("{PEER_C}@1.0.0")), "{packages:?}");
    assert!(!packages.contains(&format!("{PEER_C}@1.0.1")), "{packages:?}");

    drop((root, anchor));
}

#[test]
fn update_withholds_the_old_pin_of_an_auto_installed_peer() {
    let (root, workspace, anchor) = setup();
    let consumer = "@pnpm.e2e/wants-peer-c-1";
    write_manifest(&workspace, &format!(r#"{{ "{consumer}": "1.0.0", "{PEER_C}": "1.0.0" }}"#));
    pacquet(&workspace, ["install", "--lockfile-only"]).assert().success();
    write_manifest(&workspace, &format!(r#"{{ "{consumer}": "1.0.0" }}"#));

    pacquet(&workspace, ["update", "--lockfile-only"]).assert().success();
    let lockfile = _utils::read_lockfile(&workspace.join("pnpm-lock.yaml"));
    assert_eq!(_utils::importer_version(&lockfile, ".", consumer), "1.0.0(@pnpm.e2e/peer-c@1.0.1)");
    drop((root, anchor));
}

/// Ports `should not update tag version when --latest not set`.
#[test]
fn update_keeps_every_dist_tag_specifier_without_latest() {
    let (root, workspace, anchor) = setup_with_own_registry();
    anchor.set_dist_tag(PEER_A, "1.0.1", "latest");
    anchor.set_dist_tag(PEER_C, "2.0.0", "canary");
    anchor.set_dist_tag(FOO, "2.0.0", "latest");

    write_manifest(
        &workspace,
        &format!(r#"{{ "{PEER_A}": "latest", "{PEER_C}": "canary", "{FOO}": "1.0.0" }}"#),
    );
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update"]).assert().success();

    assert_eq!(dep_spec(&workspace, PEER_A).as_deref(), Some("latest"));
    assert_eq!(dep_spec(&workspace, PEER_C).as_deref(), Some("canary"));
    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("1.0.0"));

    drop((root, anchor));
}

mod workspace;

mod selectors;
