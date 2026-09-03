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

/// The unmatched dependency also has a newer version in range, so its
/// untouched declaration is the selector's doing rather than a no-op.
#[test]
fn update_with_selector_only_rewrites_the_matched_dependency() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0", "{FOO}": "^1.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", DEP]).assert().success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.1.0"));
    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("^1.0.0"));

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

/// Mixing a transitive selector with a direct dependency selector must
/// still update the matching transitive package. Ports pnpm's regression
/// test for <https://github.com/pnpm/pnpm/issues/12103>, where a direct
/// selector wrongly suppressed recursive transitive updates. pacquet
/// matches every bare-name selector against direct deps and locked
/// package names alike, so the direct selector never gates the
/// transitive one.
#[test]
fn update_transitive_mixed_with_direct_selector() {
    let (root, workspace, anchor) = setup();

    // Pin the transitive dep-of-pkg-with-1-dep at 100.0.0 (via a direct
    // exact entry), then drop it to a pure transitive of pkg-with-1-dep.
    write_manifest(
        &workspace,
        &format!(r#"{{ "{FOO}": "1.0.0", "{PARENT}": "100.0.0", "{DEP}": "100.0.0" }}"#),
    );
    pacquet(&workspace, ["install"]).assert().success();
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.0.0"));

    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "1.0.0", "{PARENT}": "100.0.0" }}"#));

    // DEP is a transitive selector; FOO is a direct dependency selector.
    pacquet(&workspace, ["update", DEP, FOO]).assert().success();

    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(
        virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"),
        "the transitive selector should bump even alongside a direct selector",
    );

    drop((root, anchor));
}

/// The glob form of the mixed-selector case — the shape from
/// <https://github.com/pnpm/pnpm/issues/12103> (`pnpm up "@babel/*" uuid`).
/// A glob that names only a transitive
/// dependency must still bump it when a direct selector rides alongside.
/// The glob is matched against locked package names through the same
/// `create_matcher` path as a bare name, so the direct selector cannot
/// gate it.
#[test]
fn update_transitive_glob_mixed_with_direct_selector() {
    let (root, workspace, anchor) = setup();

    write_manifest(
        &workspace,
        &format!(r#"{{ "{FOO}": "1.0.0", "{PARENT}": "100.0.0", "{DEP}": "100.0.0" }}"#),
    );
    pacquet(&workspace, ["install"]).assert().success();
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.0.0"));

    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "1.0.0", "{PARENT}": "100.0.0" }}"#));

    // "@pnpm.e2e/dep-of-*" matches the transitive dep-of-pkg-with-1-dep
    // only; FOO is a direct dependency selector.
    pacquet(&workspace, ["update", "@pnpm.e2e/dep-of-*", FOO]).assert().success();

    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(
        virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"),
        "the transitive glob selector should bump even alongside a direct selector",
    );

    drop((root, anchor));
}

/// `pacquet update <pkg>@<version>` on a package that is only present as a
/// transitive dependency has no manifest entry to write the version into, and
/// an update resolves the target the way a fresh install would — so the
/// version could only reach the lockfile as an entry nothing backs. The
/// command fails and points at `overrides`, the mechanism that does pin a
/// transitive dependency.
#[test]
fn update_transitive_rejects_a_requested_version() {
    let (root, workspace, anchor) = setup();

    // Pin the transitive dep-of-pkg-with-1-dep at 100.0.0 (via a direct
    // exact entry), then drop it to a pure transitive of pkg-with-1-dep.
    write_manifest(&workspace, &format!(r#"{{ "{PARENT}": "100.0.0", "{DEP}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.0.0"));

    write_manifest(&workspace, &format!(r#"{{ "{PARENT}": "100.0.0" }}"#));

    let output = pacquet(&workspace, ["update", &format!("{DEP}@100.0.0")])
        .output()
        .expect("run pacquet update");
    let rendered = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    eprintln!("STATUS: {}\nOUTPUT:\n{rendered}", output.status);

    assert!(!output.status.success(), "a version that cannot be recorded should fail");
    assert!(
        rendered.contains("ERR_PNPM_UPDATE_VERSION_ON_INDIRECT_DEP"),
        "the failure must carry the UPDATE_VERSION_ON_INDIRECT_DEP code",
    );

    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(
        !virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"),
        "a rejected update must not have resolved anything",
    );

    drop((root, anchor));
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

/// `--latest` must not rewrite a `workspace:` dependency that points at a
/// local path. Resolving it against the registry would either fail (the
/// package is workspace-only, not published) or replace the path — which can
/// target a publish directory — with a version range. Regression test for
/// <https://github.com/pnpm/pnpm/issues/3902>.
#[test]
fn update_latest_preserves_workspace_local_path_specifier() {
    let (root, workspace, anchor) = setup();

    // A workspace-only sibling package, not published to the mocked
    // registry, referenced by a `workspace:` local path.
    add_workspace_package(&workspace, "local-dep", "1.0.0");

    write_manifest(&workspace, r#"{ "local-dep": "workspace:./local-dep" }"#);
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest"]).assert().success();

    assert_eq!(dep_spec(&workspace, "local-dep").as_deref(), Some("workspace:./local-dep"));

    drop((root, anchor));
}

#[test]
fn update_patches_preserves_an_implicit_workspace_dependency() {
    let (root, workspace, anchor) = setup();

    add_workspace_package(&workspace, "workspace-only", "1.0.0");
    append_workspace_yaml_key(&workspace, "linkWorkspacePackages", true);
    write_manifest(&workspace, r#"{ "workspace-only": "^1.0.0" }"#);
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--patches"]).assert().success();

    let dependency = workspace.join("node_modules/workspace-only");
    assert!(dependency.exists(), "workspace dependency should remain linked");
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile.contains(
            "workspace-only:\n        specifier: ^1.0.0\n        version: link:workspace-only"
        ),
        "{lockfile}",
    );

    drop((root, anchor));
}

/// A package selector only updates the matched dependency; others keep
/// their manifest ranges.
#[test]
fn update_latest_with_selector_is_scoped() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0", "{FOO}": "^1.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest", FOO]).assert().success();

    // foo's latest is 100.1.0; dep-of-pkg-with-1-dep is untouched.
    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("^100.1.0"));
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));

    drop((root, anchor));
}

/// A negation selector (`!@scope/*`) updates everything *except* the
/// matched packages — ports pnpm's "update with negation pattern" test.
#[test]
fn update_latest_with_negation_selector() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0", "{FOO}": "^1.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    // Update everything except dep-of-pkg-with-1-dep.
    pacquet(&workspace, ["update", "--latest", &format!("!{DEP}")]).assert().success();

    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("^100.1.0"));
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));

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

/// `update <pkg> --depth 0` where the package is not a direct dependency
/// fails with `ERR_PNPM_NO_PACKAGE_IN_DEPENDENCIES`.
#[test]
fn update_depth_zero_unknown_package_errors() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["update", "--depth", "0", "@pnpm.e2e/not-a-dependency"])
        .output()
        .expect("run pacquet update");
    assert!(!output.status.success(), "depth-0 update of a non-dependency should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("None of the specified packages were found in the dependencies"),
        "stderr did not mention NO_PACKAGE_IN_DEPENDENCIES: {stderr}",
    );

    drop((root, anchor));
}

/// `--depth 0` reaches direct dependencies only: a transitive
/// dependency keeps its locked resolution even though the same update
/// without the flag bumps it.
#[test]
fn update_depth_zero_leaves_transitive_dependencies_locked() {
    let (root, workspace, anchor) = setup();

    // Pin the transitive dep-of-pkg-with-1-dep at 100.0.0 through a
    // direct exact entry, then drop it to a pure transitive of
    // pkg-with-1-dep, whose ^100.0.0 range a fresh resolve answers with
    // 100.1.0.
    write_manifest(&workspace, &format!(r#"{{ "{PARENT}": "100.0.0", "{DEP}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    write_manifest(&workspace, &format!(r#"{{ "{PARENT}": "100.0.0" }}"#));

    pacquet(&workspace, ["update", "--depth", "0"]).assert().success();

    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(
        !virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"),
        "a depth-0 update should not reach a transitive dependency",
    );

    pacquet(&workspace, ["update"]).assert().success();

    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(
        virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"),
        "the default unlimited depth should reach the transitive dependency",
    );

    drop((root, anchor));
}

/// `updateConfig.ignoreDependencies` excludes the listed packages from a
/// no-selector update — ports pnpm's "ignore packages in
/// updateConfig.ignoreDependencies" test (adapted to static fixtures).
#[test]
fn update_latest_honors_ignore_dependencies() {
    let (root, workspace, anchor) = setup();
    set_ignore_dependencies(&workspace, &[DEP]);

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0", "{FOO}": "^1.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest"]).assert().success();

    // foo is updated to its latest; the ignored dep keeps its range.
    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("^100.1.0"));
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));

    drop((root, anchor));
}

/// A compatible (non-`--latest`) update honors `ignoreDependencies`: the
/// ignored dep keeps its lockfile pin while the rest re-resolve.
#[test]
fn update_compatible_honors_ignore_dependencies() {
    let (root, workspace, anchor) = setup();
    set_ignore_dependencies(&workspace, &[FOO]);

    // Pin both exactly, then widen the ranges. A plain `update` would
    // bump both to the highest in range; ignoring foo must keep it pinned.
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "100.0.0", "{FOO}": "1.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0", "{FOO}": "^1.0.0" }}"#));
    pacquet(&workspace, ["update"]).assert().success();

    // dep re-resolved to the highest in range; foo kept its old pin.
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+foo@1.0.0"));
    assert!(!virtual_store_has(&workspace, "@pnpm.e2e+foo@1.3.0"));

    drop((root, anchor));
}

/// `--prod` scopes the update to production dependencies, and
/// `ignoreDependencies` still excludes names within that scope. A
/// devDependency is left untouched even though it has a newer version.
#[test]
fn update_prod_scopes_and_honors_ignore() {
    let (root, workspace, anchor) = setup();
    set_ignore_dependencies(&workspace, &[FOO]);

    let manifest = format!(
        r#"{{ "name": "test-update", "version": "1.0.0", "dependencies": {{ "{DEP}": "^100.0.0", "{FOO}": "^1.0.0" }}, "devDependencies": {{ "@pnpm.e2e/peer-c": "^1.0.0" }} }}"#,
    );
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--prod", "--latest"]).assert().success();

    // dep (prod, not ignored) → latest; foo (prod, ignored) unchanged;
    // peer-c (dev, excluded by --prod) unchanged.
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^101.0.0"));
    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("^1.0.0"));
    let manifest = PackageManifest::from_path(workspace.join("package.json")).unwrap();
    let peer_c = manifest
        .dependencies([DependencyGroup::Dev])
        .find(|(k, _)| *k == "@pnpm.e2e/peer-c")
        .map(|(_, spec)| spec.to_string());
    assert_eq!(peer_c.as_deref(), Some("^1.0.0"));

    drop((root, anchor));
}

/// When every included *direct* dep is ignored, `update --latest` is a
/// full no-op — it must not re-resolve the non-ignored *indirect* deps.
/// Mirrors pnpm's early `if (opts.latest) return`.
#[test]
fn update_latest_all_direct_ignored_does_not_touch_indirect() {
    let (root, workspace, anchor) = setup();
    set_ignore_dependencies(&workspace, &[PARENT]);

    // Pin the transitive dep-of-pkg-with-1-dep at 100.0.0 (via a direct
    // exact entry), then drop it to a pure transitive of pkg-with-1-dep.
    write_manifest(&workspace, &format!(r#"{{ "{PARENT}": "100.0.0", "{DEP}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.0.0"));

    write_manifest(&workspace, &format!(r#"{{ "{PARENT}": "100.0.0" }}"#));
    pacquet(&workspace, ["update", "--latest"]).assert().success();

    // No-op: the indirect dep stays pinned at 100.0.0.
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.0.0"));
    assert!(!virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"));

    drop((root, anchor));
}

/// The non-`--latest` counterpart: when the only direct dep is ignored,
/// a plain `update` still re-resolves the non-ignored indirect deps to
/// the highest in range. Mirrors pnpm's "updating indirect dependencies
/// only" branch — and guards against narrowing the `--latest` no-op
/// guard into an unconditional one.
#[test]
fn update_compatible_all_direct_ignored_still_updates_indirect() {
    let (root, workspace, anchor) = setup();
    set_ignore_dependencies(&workspace, &[PARENT]);

    write_manifest(&workspace, &format!(r#"{{ "{PARENT}": "100.0.0", "{DEP}": "100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    write_manifest(&workspace, &format!(r#"{{ "{PARENT}": "100.0.0" }}"#));
    pacquet(&workspace, ["update"]).assert().success();

    // The indirect dep bumps within range (100.0.0 -> 100.1.0).
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"));

    drop((root, anchor));
}

/// When every dependency is ignored, `update --latest` is a no-op —
/// ports pnpm's "do not update anything if all the dependencies are
/// ignored" test.
#[test]
fn update_latest_all_ignored_is_noop() {
    let (root, workspace, anchor) = setup();
    set_ignore_dependencies(&workspace, &[FOO]);

    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "^1.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest"]).assert().success();

    // The only dependency is ignored, so its range is untouched.
    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("^1.0.0"));

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

/// `--workspace` re-points a dependency that a workspace project
/// publishes at the local copy. Under the default `rolling`
/// `saveWorkspaceProtocol`, an exactly-pinned dependency becomes
/// `workspace:*` — a specifier the sibling's next release does not
/// invalidate.
#[test]
fn update_workspace_links_to_the_local_package() {
    let (root, workspace, anchor) = setup();

    add_workspace_package(&workspace, "sibling", "2.0.0");
    write_manifest(&workspace, r#"{ "sibling": "0.0.0" }"#);

    pacquet(&workspace, ["update", "--workspace"]).assert().success();

    assert_eq!(dep_spec(&workspace, "sibling").as_deref(), Some("workspace:*"));

    drop((root, anchor));
}

/// A caret-ranged dependency keeps its operator when it is linked.
#[test]
fn update_workspace_keeps_the_declared_range_operator() {
    let (root, workspace, anchor) = setup();

    add_workspace_package(&workspace, "sibling", "2.0.0");
    write_manifest(&workspace, r#"{ "sibling": "^1.0.0" }"#);

    pacquet(&workspace, ["update", "--workspace", "sibling"]).assert().success();

    assert_eq!(dep_spec(&workspace, "sibling").as_deref(), Some("workspace:^"));

    drop((root, anchor));
}

/// With `saveWorkspaceProtocol: false` the linked version is written out
/// in full — the protocol itself is kept regardless, since dropping it
/// would send the dependency back to the registry.
#[test]
fn update_workspace_writes_the_version_when_not_rolling() {
    let (root, workspace, anchor) = setup();

    add_workspace_package(&workspace, "sibling", "2.0.0");
    append_workspace_yaml_key(&workspace, "saveWorkspaceProtocol", false);
    write_manifest(&workspace, r#"{ "sibling": "0.0.0" }"#);

    pacquet(&workspace, ["update", "--workspace"]).assert().success();

    assert_eq!(dep_spec(&workspace, "sibling").as_deref(), Some("workspace:2.0.0"));

    drop((root, anchor));
}

/// A dependency no workspace project publishes is left alone by a
/// selector-less `--workspace`.
#[test]
fn update_workspace_leaves_registry_dependencies_alone() {
    let (root, workspace, anchor) = setup();

    add_workspace_package(&workspace, "sibling", "2.0.0");
    write_manifest(&workspace, &format!(r#"{{ "sibling": "0.0.0", "{DEP}": "^100.0.0" }}"#));

    pacquet(&workspace, ["update", "--workspace"]).assert().success();

    assert_eq!(dep_spec(&workspace, "sibling").as_deref(), Some("workspace:*"));
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));

    drop((root, anchor));
}

/// `--workspace` that links nothing is an ordinary selector-less
/// update, so it stays a *full* install and runs the project's own
/// lifecycle scripts. Only the dependencies it actually re-points make
/// the run partial.
#[test]
fn update_workspace_that_links_nothing_still_runs_project_scripts() {
    let (root, workspace, anchor) = setup();

    // A workspace sibling exists, but nothing depends on it, so
    // `--workspace` has no link target.
    add_workspace_package(&workspace, "sibling", "2.0.0");
    fs::write(
        workspace.join("package.json"),
        format!(
            r#"{{ "name": "test-update", "version": "1.0.0",
                  "scripts": {{ "postinstall": "node -e \"require('fs').writeFileSync('postinstall-ran', '')\"" }},
                  "dependencies": {{ "{DEP}": "^100.0.0" }} }}"#,
        ),
    )
    .expect("write package.json");

    pacquet(&workspace, ["update", "--workspace"]).assert().success();

    assert!(
        workspace.join("postinstall-ran").exists(),
        "a --workspace update with nothing to link should run the project's own scripts",
    );

    drop((root, anchor));
}

#[test]
fn update_ignore_scripts_skips_project_scripts() {
    let root = TempDir::new().expect("create temp directory");
    let workspace = root.path().to_path_buf();

    fs::write(
        workspace.join("package.json"),
        r#"{ "name": "test-update", "version": "1.0.0",
              "scripts": { "postinstall": "node -e \"require('fs').writeFileSync('postinstall-ran', '')\"" } }"#,
    )
    .expect("write package.json");

    pacquet(&workspace, ["update", "--ignore-scripts"]).assert().success();

    assert!(
        !workspace.join("postinstall-ran").exists(),
        "--ignore-scripts should skip the project's lifecycle scripts",
    );

    drop(root);
}

/// Naming a dependency that no workspace project publishes fails, since
/// there is nothing to link it to.
#[test]
fn update_workspace_rejects_a_dependency_outside_the_workspace() {
    let (root, workspace, anchor) = setup();

    add_workspace_package(&workspace, "sibling", "2.0.0");
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));

    assert_update_fails(&workspace, &["update", "--workspace", DEP], "not found in the workspace");

    drop((root, anchor));
}

/// A `--workspace` selector that matches no direct dependency links
/// nothing — the run falls back to an ordinary update of that selector,
/// rather than linking every workspace dependency the user never named.
#[test]
fn update_workspace_with_an_unmatched_selector_links_nothing() {
    let (root, workspace, anchor) = setup();

    add_workspace_package(&workspace, "sibling", "2.0.0");
    append_workspace_yaml_key(&workspace, "linkWorkspacePackages", true);
    write_manifest(&workspace, r#"{ "sibling": "^2.0.0" }"#);
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--workspace", "@pnpm.e2e/not-a-dependency"]).assert().success();

    assert_eq!(dep_spec(&workspace, "sibling").as_deref(), Some("^2.0.0"));

    drop((root, anchor));
}

/// `--latest` rewrites ranges from the registry and `--workspace` from
/// the workspace, so the two cannot both apply.
#[test]
fn update_workspace_with_latest_is_rejected() {
    let (root, workspace, anchor) = setup();

    add_workspace_package(&workspace, "sibling", "2.0.0");
    write_manifest(&workspace, r#"{ "sibling": "0.0.0" }"#);

    assert_update_fails(
        &workspace,
        &["update", "--workspace", "--latest"],
        "Cannot use --latest with --workspace simultaneously",
    );

    drop((root, anchor));
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

/// An unmatched `--latest` selector is a no-op and must not read or parse
/// the workspace catalogs: a malformed catalog config (here, the default
/// catalog defined through both `catalog:` and `catalogs.default`) does not
/// make the no-op fail.
#[test]
fn update_latest_unmatched_selector_does_not_read_catalogs() {
    let (root, workspace, anchor) = setup();

    // A valid `catalog:` dependency (so the eager read would have triggered)
    // alongside a default catalog defined twice (which a catalog read rejects
    // with ERR_PNPM_..._CONFIGURATION).
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "catalog:grp1" }}"#));
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    write!(
        yaml,
        "catalog:\n  \"a\": \"^1.0.0\"\ncatalogs:\n  default:\n    \"b\": \"^1.0.0\"\n  grp1:\n    \"{DEP}\": \"~100.0.0\"\n",
    )
    .unwrap();
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");

    // The selector matches no direct dependency, so the update returns early
    // without ever reading the (malformed) catalogs.
    pacquet(&workspace, ["update", "--latest", "not-a-dependency"]).assert().success();

    drop((root, anchor));
}

/// `pacquet update --latest` on a `catalog:` dependency keeps the
/// `catalog:` reference in `package.json` and bumps the catalog entry to
/// the latest version, preserving the entry's own range operator — even
/// under the default `manual` catalogMode (which does not auto-catalog).
#[test]
fn update_latest_catalog_preserves_reference_and_operator() {
    let (root, workspace, anchor) = setup();

    set_named_catalog(&workspace, "grp1", &[(DEP, "~100.0.0")]);
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "catalog:grp1" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest"]).assert().success();

    // The manifest still references the catalog, untouched.
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("catalog:grp1"));

    // The catalog entry is bumped to the latest version with its tilde
    // operator preserved (not widened to the default caret).
    let yaml = read_workspace_yaml(&workspace);
    assert!(yaml.contains("~101.0.0"), "catalog entry should be bumped to ~101.0.0: {yaml}");
    assert!(!yaml.contains("100.0.0"), "stale catalog entry should be gone: {yaml}");

    drop((root, anchor));
}

/// The catalog entry owns the range a `catalog:` dependency declares, so it
/// is the entry that moves and the entry that bounds the bump.
#[test]
fn update_catalog_bumps_the_entry_within_its_range() {
    let (root, workspace, anchor) = setup();

    set_named_catalog(&workspace, "grp1", &[(DEP, "^100.0.0")]);
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "catalog:grp1" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update"]).assert().success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("catalog:grp1"));
    let yaml = read_workspace_yaml(&workspace);
    assert!(yaml.contains("^100.1.0"), "catalog entry should be bumped to ^100.1.0: {yaml}");
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// A dependency declared through the catalog and listed in `overrides`
/// reaches the resolver with the override's specifier, so the version the
/// run resolves must not be written back over the `catalog:` reference
/// (pnpm/pnpm#12115).
#[test]
fn update_keeps_the_catalog_reference_of_an_overridden_dependency() {
    let (root, workspace, anchor) = setup();

    set_named_catalog(&workspace, "grp1", &[(DEP, "^100.0.0")]);
    set_overrides(&workspace, &[(DEP, "^100.0.0"), (FOO, "100.0.0")]);
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "catalog:grp1", "{FOO}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update"]).assert().success();

    // Both declarations are the overrides' input, not their output: neither
    // the `catalog:` reference nor the declared range may be replaced by the
    // version the override resolved to.
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("catalog:grp1"));
    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("^100.0.0"));
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// An override claims a dependency even when it repeats the range the project
/// declares, so the declared range is not the update's to move: the overrides
/// hook rewrites it back before the resolver reads it, and the lockfile would
/// then record a specifier the manifest never shows (pnpm/pnpm#14224).
#[test]
fn update_keeps_a_declared_range_an_override_repeats() {
    let (root, workspace, anchor) = setup();

    set_overrides(&workspace, &[(DEP, "^100.0.0")]);
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update"]).assert().success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// `--latest` reaches the manifest through its own pre-install rewrite, so
/// it needs the `catalog:` reference to survive an override of its own.
#[test]
fn update_latest_keeps_the_catalog_reference_of_an_overridden_dependency() {
    let (root, workspace, anchor) = setup();

    set_named_catalog(&workspace, "grp1", &[(DEP, "^100.0.0")]);
    set_overrides(&workspace, &[(DEP, "^100.0.0")]);
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "catalog:grp1" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest"]).assert().success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("catalog:grp1"));
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// `--latest --no-save` on a `catalog:` dependency leaves `package.json`
/// and `pnpm-workspace.yaml` untouched, but still re-resolves the lockfile.
/// The catalog entry is what the dependency keeps, so it bounds the bump the
/// same way a range in `package.json` does.
#[test]
fn update_latest_no_save_catalog_bumps_lockfile_only() {
    let (root, workspace, anchor) = setup();

    set_named_catalog(&workspace, "grp1", &[(DEP, "100.0.0")]);
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "catalog:grp1" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.0.0"));

    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let widened = read_workspace_yaml(&workspace).replace(r#""100.0.0""#, r#""^100.0.0""#);
    fs::write(&yaml_path, widened).expect("widen the catalog entry");

    pacquet(&workspace, ["update", "--latest", "--no-save"]).assert().success();

    // package.json and the workspace catalog are untouched...
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("catalog:grp1"));
    let yaml = read_workspace_yaml(&workspace);
    assert!(yaml.contains("^100.0.0"), "catalog entry must be untouched under --no-save: {yaml}");

    // ...and the lockfile/store re-resolved to the highest version the catalog
    // entry admits, not to the 101.0.0 `--latest` would otherwise reach.
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"));
    assert!(!virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@101.0.0"));
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// A versioned `npm:` selector targets the package the alias installs, not
/// the alias it is written at: update targets are keyed by the resolved
/// package name, so keying them by the alias leaves the pin in place.
#[test]
fn update_npm_alias_selector_targets_the_aliased_package() {
    let (root, workspace, anchor) = setup();

    // Pin the aliased package at 100.0.0 through a direct exact entry,
    // then drop the entry so the alias is the only thing holding it — its
    // ^100.0.0 range a fresh resolve answers with 100.1.0.
    write_manifest(
        &workspace,
        &format!(r#"{{ "dep-alias": "npm:{DEP}@^100.0.0", "{DEP}": "100.0.0" }}"#),
    );
    pacquet(&workspace, ["install"]).assert().success();
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.0.0"));
    write_manifest(&workspace, &format!(r#"{{ "dep-alias": "npm:{DEP}@^100.0.0" }}"#));

    pacquet(&workspace, ["update", &format!("dep-alias@npm:{DEP}@^100.0.0")]).assert().success();

    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(
        virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"),
        "the selector should have withheld the aliased package's pin",
    );

    drop((root, anchor));
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

/// The alias name does not exist in the mock registry.
#[test]
fn update_latest_catalog_npm_alias_resolves_aliased_package() {
    let (root, workspace, anchor) = setup();

    set_named_catalog(&workspace, "grp1", &[("dep-alias", &format!("npm:{DEP}@~100.0.0"))]);
    write_manifest(&workspace, r#"{ "dep-alias": "catalog:grp1" }"#);
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest"]).assert().success();

    assert_eq!(dep_spec(&workspace, "dep-alias").as_deref(), Some("catalog:grp1"));

    let yaml = read_workspace_yaml(&workspace);
    assert!(
        yaml.contains(&format!("npm:{DEP}@~101.0.0")),
        "catalog entry should be bumped to npm:{DEP}@~101.0.0: {yaml}",
    );
    assert!(!yaml.contains("100.0.0"), "stale catalog entry should be gone: {yaml}");

    drop((root, anchor));
}

/// The same preservation applies to the default catalog (`catalog:`).
#[test]
fn update_latest_default_catalog_preserves_reference() {
    let (root, workspace, anchor) = setup();

    set_named_catalog(&workspace, "default", &[(DEP, "^100.0.0")]);
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "catalog:" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest"]).assert().success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("catalog:"));

    let yaml = read_workspace_yaml(&workspace);
    assert!(yaml.contains("^101.0.0"), "catalog entry should be bumped to ^101.0.0: {yaml}");
    assert!(!yaml.contains("100.0.0"), "stale catalog entry should be gone: {yaml}");

    drop((root, anchor));
}

/// `pacquet update --lockfile-only <pkg>@<version>` under
/// `catalogMode: strict`, where the wanted version falls outside the
/// catalog entry's range, rejects with
/// `ERR_PNPM_CATALOG_VERSION_MISMATCH` instead of crashing
/// ([pnpm#11706](https://github.com/pnpm/pnpm/pull/11706): before that
/// fix, passing a range to the exact-version comparison threw `Invalid
/// Version`).
#[test]
fn update_strict_catalog_range_mismatch_errors() {
    let (root, workspace, anchor) = setup();
    set_strict_catalog(&workspace, &[(DEP, "^101.0.0")]);
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "catalog:" }}"#));

    let output = pacquet(&workspace, ["update", "--lockfile-only", &format!("{DEP}@100.0.0")])
        .output()
        .expect("run pacquet update");
    assert!(!output.status.success(), "a strict catalog range mismatch must fail the update");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Wanted dependency outside the version range defined in catalog"),
        "stderr did not mention the catalog version mismatch: {stderr}",
    );
    assert!(
        stderr.contains("ERR_PNPM_CATALOG_VERSION_MISMATCH"),
        "stderr did not carry the error code: {stderr}",
    );

    drop((root, anchor));
}

/// A wanted version the catalog range already covers is taken from the
/// catalog instead of being rejected, so the dependency keeps its
/// `catalog:` reference. This is the `Renovate` scenario from
/// [pnpm#13715](https://github.com/pnpm/pnpm/issues/13715).
#[test]
fn update_strict_catalog_range_covering_the_wanted_version_succeeds() {
    let (root, workspace, anchor) = setup();
    set_strict_catalog(&workspace, &[(DEP, "^100.0.0")]);
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "catalog:" }}"#));
    pacquet(&workspace, ["install", "--lockfile-only"]).assert().success();

    pacquet(&workspace, ["update", "--lockfile-only", &format!("{DEP}@100.1.0")])
        .assert()
        .success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("catalog:"));
    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile.contains(&format!("{DEP}@100.1.0")),
        "the update should have moved the catalog resolution to 100.1.0:\n{lockfile}",
    );

    drop((root, anchor));
}

/// Updating one dependency must not drop the transitive snapshots of an
/// unrelated, non-targeted dependency when `dedupePeerDependents` is
/// disabled. Parity guard for pnpm/pnpm#12456: on the TypeScript stack
/// the already-linked resolver shortcut fired below the update-depth
/// boundary, so a partial update made a reused parent snapshot appear
/// childless. pacquet reuses the whole subtree of a non-targeted package
/// from the lockfile ([`UpdateReuseScope::Except`]), so the transitive
/// edge survives — this test locks that in.
///
/// The TypeScript regression updates a package absent from the manifest;
/// pacquet rejects that with `NO_PACKAGE_IN_DEPENDENCIES`, so the update
/// here targets `foo`, already pinned at its latest so the update is a
/// no-op. The reused parent is `pkg-with-1-dep`, whose transitive
/// `dep-of-pkg-with-1-dep` must remain in the lockfile.
#[test]
fn update_preserves_unrelated_transitives_without_peer_dedupe() {
    let (root, workspace, anchor) = setup();
    disable_dedupe_peer_dependents(&workspace);

    write_manifest(&workspace, &format!(r#"{{ "{PARENT}": "100.0.0", "{FOO}": "100.1.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let lockfile_before =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert!(
        lockfile_before.contains(DEP),
        "the parent's transitive dependency should be in the lockfile after install:\n{lockfile_before}",
    );

    pacquet(&workspace, ["update", FOO]).assert().success();

    let lockfile_after =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read lockfile");
    assert_eq!(
        lockfile_after, lockfile_before,
        "a no-op update of an unrelated package must leave the lockfile — and the reused parent's transitive edges — untouched",
    );

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

/// An invalid `minimumReleaseAgeExclude` must not fail the
/// unmatched-selector no-op: `update <unmatched> --latest` still
/// succeeds, matching the TypeScript CLI.
#[test]
fn update_latest_unmatched_noop_ignores_invalid_minimum_release_age_exclude() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{BRAVO_DEP}": "^1.0.0" }}"#));
    append_workspace_yaml_key(
        &workspace,
        "minimumReleaseAgeExclude",
        format!(r#"["{BRAVO_DEP}@^1.0.0"]"#),
    );

    pacquet(&workspace, ["update", "--latest", "@pnpm.e2e/does-not-exist"]).assert().success();

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

/// A root `update --no-save` in a workspace pre-hooks only the root manifest,
/// so the install layer must still run `readPackage` over the workspace
/// projects it discovers itself — exactly once each. Regression test for the
/// review finding on <https://github.com/pnpm/pnpm/pull/13812>.
#[test]
fn update_no_save_applies_read_package_to_workspace_projects() {
    let (root, workspace, anchor) = setup();

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "100.0.0" }}"#));
    fs::create_dir_all(workspace.join("packages/a")).expect("mkdir packages/a");
    fs::write(
        workspace.join("packages/a/package.json"),
        format!(r#"{{ "name": "@test/a", "version": "1.0.0", "dependencies": {{ "{DEP}": "100.0.0" }} }}"#),
    )
    .expect("write packages/a/package.json");
    pacquet(&workspace, ["install"]).assert().success();
    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    fs::write(
        workspace.join(".pnpmfile.cjs"),
        format!(
            "const fs = require('fs');\nconst path = require('path');\nmodule.exports = {{ hooks: {{ readPackage (pkg) {{\n  if (pkg.name === 'test-update' || pkg.name === '@test/a') {{\n    fs.appendFileSync(path.join(__dirname, 'read-package.log'), `${{pkg.name}}:${{pkg.dependencies && pkg.dependencies[{DEP:?}]}}\\n`);\n  }}\n  if (pkg.name === '@test/a' && pkg.dependencies && pkg.dependencies[{DEP:?}]) {{\n    pkg.dependencies[{DEP:?}] = '100.1.0';\n  }}\n  return pkg;\n}} }} }}\n",
        ),
    )
    .expect("write pnpmfile");

    let output =
        pacquet(&workspace, ["update", "--no-save", "--lockfile-only", &format!("{DEP}@100.1.0")])
            .output()
            .expect("run update --no-save");
    assert!(output.status.success(), "update --no-save failed: {output:?}");

    let hook_log =
        fs::read_to_string(workspace.join("read-package.log")).expect("read readPackage log");
    let mut hook_inputs = hook_log.lines().collect::<Vec<_>>();
    hook_inputs.sort_unstable();
    assert_eq!(
        hook_inputs,
        vec!["@test/a:100.0.0", "test-update:^100.0.0"],
        "readPackage should see each project manifest exactly once",
    );
    let lock = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(
        lock.contains("specifier: 100.1.0"),
        "the workspace project's importer entry must follow readPackage's rewrite: {lock}",
    );
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

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

/// Ports `update with "*" pattern`.
#[test]
fn update_latest_with_glob_selector_is_scoped() {
    let (root, workspace, anchor) = setup_with_own_registry();
    anchor.set_dist_tag(PEER_A, "1.0.1", "latest");
    anchor.set_dist_tag(PEER_C, "2.0.0", "latest");
    anchor.set_dist_tag(FOO, "2.0.0", "latest");

    write_manifest(
        &workspace,
        &format!(r#"{{ "{PEER_A}": "1.0.0", "{PEER_C}": "1.0.0", "{FOO}": "1.0.0" }}"#),
    );
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest", "@pnpm.e2e/peer-*"]).assert().success();

    let packages = lockfile_package_keys(&workspace);
    assert!(packages.contains(&format!("{PEER_A}@1.0.1")), "{packages:?}");
    assert!(packages.contains(&format!("{PEER_C}@2.0.0")), "{packages:?}");
    assert!(packages.contains(&format!("{FOO}@1.0.0")), "{packages:?}");

    drop((root, anchor));
}

/// Ports `update should work normal when set empty string version`
/// (<https://github.com/pnpm/pnpm/issues/4196>).
#[test]
fn update_latest_star_selector_updates_an_empty_specifier() {
    let (root, workspace, anchor) = setup_with_own_registry();
    anchor.set_dist_tag(PEER_A, "1.0.1", "latest");
    anchor.set_dist_tag(PEER_C, "2.0.0", "latest");
    anchor.set_dist_tag(FOO, "2.0.0", "latest");

    fs::write(
        workspace.join("package.json"),
        format!(
            r#"{{ "name": "test-update", "version": "1.0.0",
              "dependencies": {{ "{PEER_A}": "1.0.0" }},
              "devDependencies": {{ "{FOO}": "", "{PEER_C}": "" }} }}"#,
        ),
    )
    .expect("write package.json");
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest", "*"]).assert().success();

    let packages = lockfile_package_keys(&workspace);
    assert!(packages.contains(&format!("{PEER_A}@1.0.1")), "{packages:?}");
    assert!(packages.contains(&format!("{PEER_C}@2.0.0")), "{packages:?}");
    assert!(packages.contains(&format!("{FOO}@2.0.0")), "{packages:?}");

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

/// Ports `not ignore packages if these are specified in parameter even
/// if these are listed in ... ignoreDependencies`.
#[test]
fn update_selectors_override_ignore_dependencies() {
    let (root, workspace, anchor) = setup_with_own_registry();
    anchor.set_dist_tag(FOO, "100.0.0", "latest");
    anchor.set_dist_tag(BAR, "100.0.0", "latest");

    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "100.0.0", "{BAR}": "^100.0.0" }}"#));
    set_ignore_dependencies(&workspace, &[FOO]);
    pacquet(&workspace, ["install"]).assert().success();

    let packages = lockfile_package_keys(&workspace);
    assert!(packages.contains(&format!("{FOO}@100.0.0")), "{packages:?}");
    assert!(packages.contains(&format!("{BAR}@100.0.0")), "{packages:?}");

    anchor.set_dist_tag(FOO, "100.1.0", "latest");
    anchor.set_dist_tag(BAR, "100.1.0", "latest");

    pacquet(&workspace, ["update", &format!("{FOO}@latest"), &format!("{BAR}@latest")])
        .assert()
        .success();

    let packages = lockfile_package_keys(&workspace);
    assert!(packages.contains(&format!("{FOO}@100.1.0")), "{packages:?}");
    assert!(packages.contains(&format!("{BAR}@100.1.0")), "{packages:?}");
    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("100.1.0"));
    assert_eq!(dep_spec(&workspace, BAR).as_deref(), Some("^100.1.0"));

    drop((root, anchor));
}

/// A `catalog:` entry declares a reference, not a range, so it is not
/// something a resolved version can replace. The catalog entry keeps
/// bounding the dependency, and the update moves the lockfile within it.
#[test]
fn update_tag_selector_preserves_catalog_reference() {
    let (root, workspace, anchor) = setup_with_own_registry();
    anchor.set_dist_tag(FOO, "100.0.0", "latest");
    set_named_catalog(&workspace, "grp1", &[(FOO, "^100.0.0")]);
    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "catalog:grp1" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let packages = lockfile_package_keys(&workspace);
    assert!(packages.contains(&format!("{FOO}@100.0.0")), "{packages:?}");

    anchor.set_dist_tag(FOO, "100.1.0", "latest");
    pacquet(&workspace, ["update", &format!("{FOO}@latest")]).assert().success();

    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("catalog:grp1"));
    let yaml = read_workspace_yaml(&workspace);
    assert!(yaml.contains("^100.0.0"), "catalog entry should be untouched: {yaml}");
    let packages = lockfile_package_keys(&workspace);
    assert!(packages.contains(&format!("{FOO}@100.1.0")), "{packages:?}");
    pacquet(&workspace, ["install", "--frozen-lockfile"]).assert().success();

    drop((root, anchor));
}

/// The selector names the tag to resolve, which need not be the tag the
/// registry publishes as `latest`, nor the highest published version.
#[test]
fn update_tag_selector_resolves_the_named_tag() {
    let (root, workspace, anchor) = setup_with_own_registry();
    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "^1.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    anchor.set_dist_tag(FOO, "100.0.0", "canary");
    pacquet(&workspace, ["update", &format!("{FOO}@canary")]).assert().success();

    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("^100.0.0"));
    let packages = lockfile_package_keys(&workspace);
    assert!(packages.contains(&format!("{FOO}@100.0.0")), "{packages:?}");

    drop((root, anchor));
}

/// A manifest that already tracks a dist tag keeps tracking one, so the
/// selector's tag replaces it rather than being resolved into a range.
#[test]
fn update_tag_selector_replaces_a_declared_tag() {
    let (root, workspace, anchor) = setup_with_own_registry();
    anchor.set_dist_tag(FOO, "100.1.0", "latest");
    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "latest" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    anchor.set_dist_tag(FOO, "100.0.0", "canary");
    pacquet(&workspace, ["update", &format!("{FOO}@canary")]).assert().success();

    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("canary"));
    let packages = lockfile_package_keys(&workspace);
    assert!(packages.contains(&format!("{FOO}@100.0.0")), "{packages:?}");

    drop((root, anchor));
}
