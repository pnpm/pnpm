use super::{
    BRAVO_DEP, DEP, DependencyGroup, FOO, PARENT, PEER_A, PEER_C, PackageManifest, TempDir,
    append_workspace_yaml_key, assert_eq, dep_spec, disable_dedupe_peer_dependents, fs,
    list_virtual_store, lockfile_package_keys, pacquet, set_ignore_dependencies, setup,
    setup_with_own_registry, virtual_store_has, write_manifest,
};
use assert_cmd::assert::OutputAssertExt;

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
