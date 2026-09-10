use super::{
    BAR, DEP, FOO, add_workspace_package, append_workspace_yaml_key, assert_eq,
    assert_update_fails, dep_spec, fs, list_virtual_store, lockfile_package_keys, pacquet,
    read_workspace_yaml, set_ignore_dependencies, set_named_catalog, set_overrides,
    set_strict_catalog, setup, setup_with_own_registry, virtual_store_has, write_manifest,
};
use assert_cmd::assert::OutputAssertExt;
use std::fmt::Write as _;

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
