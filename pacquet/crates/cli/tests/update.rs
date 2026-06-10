use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use pretty_assertions::assert_eq;
use std::{ffi::OsStr, fmt::Write as _, fs, path::Path, process::Command};
use tempfile::TempDir;

const DEP: &str = "@pnpm.e2e/dep-of-pkg-with-1-dep";
const FOO: &str = "@pnpm.e2e/foo";
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

/// Build a fresh `pacquet` command bound to `workspace`. The
/// `assert_cmd` `Command` is single-shot, so each install/update step
/// needs its own.
fn pacquet(workspace: &Path, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Command {
    Command::cargo_bin("pacquet")
        .expect("find the pacquet binary")
        .with_current_dir(workspace)
        .with_args(args)
}

fn write_manifest(workspace: &Path, dependencies: &str) {
    let manifest = format!(
        r#"{{ "name": "test-update", "version": "1.0.0", "dependencies": {dependencies} }}"#,
    );
    fs::write(workspace.join("package.json"), manifest).expect("write package.json");
}

/// Append an `updateConfig.ignoreDependencies` block to the
/// `pnpm-workspace.yaml` the harness already wrote.
fn set_ignore_dependencies(workspace: &Path, names: &[&str]) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    // Fail loudly if the harness ever starts writing `updateConfig` —
    // appending a second top-level mapping key produces invalid YAML.
    assert!(
        !yaml.contains("updateConfig:"),
        "pnpm-workspace.yaml already has an `updateConfig:` key — update this helper",
    );
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str("updateConfig:\n  ignoreDependencies:\n");
    for name in names {
        writeln!(yaml, "    - \"{name}\"").unwrap();
    }
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
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
/// `virtual_store_has` assertions so a failing CI run shows what was
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
    // Compatible updates do not rewrite the manifest range.
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));

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

/// `--save-exact` writes the bumped version without a range operator.
#[test]
fn update_latest_save_exact() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--latest", "--save-exact"]).assert().success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("101.0.0"));

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
#[test]
fn update_latest_no_save_keeps_manifest() {
    let (root, workspace, anchor) = setup();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@100.1.0"));

    pacquet(&workspace, ["update", "--latest", "--no-save"]).assert().success();

    // package.json range is unchanged...
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));
    // ...but the lockfile/store was re-resolved (101.0.0 is latest; the
    // in-memory `^101.0.0` drove resolution even though it wasn't saved).
    eprintln!("virtual store contents: {:?}", list_virtual_store(&workspace));
    assert!(virtual_store_has(&workspace, "@pnpm.e2e+dep-of-pkg-with-1-dep@101.0.0"));

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
        writeln!(yaml, "  \"{name}\": \"{spec}\"").unwrap();
    }
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
}

/// `pacquet update --lockfile-only <pkg>@<version>` under
/// `catalogMode: strict`, where the catalog entry for `<pkg>` is a
/// *range*, rejects with `ERR_PNPM_CATALOG_VERSION_MISMATCH` instead of
/// crashing. This is the exact `Renovate` scenario ported from
/// [pnpm#11706](https://github.com/pnpm/pnpm/pull/11706): before the fix,
/// passing a range to the exact-version comparison threw `Invalid
/// Version`.
#[test]
fn update_strict_catalog_range_mismatch_errors() {
    let (root, workspace, anchor) = setup();
    set_strict_catalog(&workspace, &[(DEP, "^100.0.0")]);
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
