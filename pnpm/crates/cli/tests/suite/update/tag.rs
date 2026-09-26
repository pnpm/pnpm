//! `pacquet update --tag <tag>` (pnpm/pnpm#3534): like `--latest`, but the
//! matched direct dependencies move to the version behind the named
//! dist-tag rather than `latest`.
use super::{
    DEP, FOO, PEER_A, assert_eq, dep_spec, lockfile_package_keys, pacquet, setup_with_own_registry,
    write_manifest,
};
use assert_cmd::assert::OutputAssertExt;

/// `--tag` ignores the manifest range, resolves the named tag, and rewrites
/// `package.json` to the tag's version, keeping the declared operator. The
/// `latest` tag pointing higher is not consulted.
#[test]
fn update_tag_rewrites_manifest_to_the_tag_version() {
    let (root, workspace, anchor) = setup_with_own_registry();
    anchor.set_dist_tag(DEP, "101.0.0", "latest");
    anchor.set_dist_tag(DEP, "100.1.0", "next");

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--tag", "next"]).assert().success();

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.1.0"));
    let packages = lockfile_package_keys(&workspace);
    assert!(packages.contains(&format!("{DEP}@100.1.0")), "{packages:?}");
    assert!(!packages.contains(&format!("{DEP}@101.0.0")), "{packages:?}");

    drop((root, anchor));
}

/// A selector scopes the tag update to the matched dependency; the
/// unmatched one keeps its declaration.
#[test]
fn update_tag_with_selector_only_rewrites_the_matched_dependency() {
    let (root, workspace, anchor) = setup_with_own_registry();
    anchor.set_dist_tag(FOO, "100.1.0", "latest");
    anchor.set_dist_tag(FOO, "100.0.0", "next");
    anchor.set_dist_tag(PEER_A, "1.0.1", "next");

    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "^1.0.0", "{PEER_A}": "^1.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--tag", "next", FOO]).assert().success();

    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("^100.0.0"));
    assert_eq!(dep_spec(&workspace, PEER_A).as_deref(), Some("^1.0.0"));

    drop((root, anchor));
}

/// A manifest entry that already tracks a dist tag keeps tracking one: the
/// flag's tag replaces it rather than being resolved into a range.
#[test]
fn update_tag_replaces_a_declared_tag() {
    let (root, workspace, anchor) = setup_with_own_registry();
    anchor.set_dist_tag(FOO, "100.1.0", "latest");
    anchor.set_dist_tag(FOO, "100.0.0", "canary");

    write_manifest(&workspace, &format!(r#"{{ "{FOO}": "latest" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    pacquet(&workspace, ["update", "--tag", "canary"]).assert().success();

    assert_eq!(dep_spec(&workspace, FOO).as_deref(), Some("canary"));
    let packages = lockfile_package_keys(&workspace);
    assert!(packages.contains(&format!("{FOO}@100.0.0")), "{packages:?}");

    drop((root, anchor));
}

/// `--no-save` leaves `package.json` untouched, so the tag reaches past the
/// kept range only in the warning: the update degrades to a bump inside the
/// declared range, the way `--latest --no-save` does.
#[test]
fn update_tag_no_save_keeps_manifest() {
    let (root, workspace, anchor) = setup_with_own_registry();
    anchor.set_dist_tag(DEP, "101.0.0", "next");

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["update", "--tag", "next", "--no-save"])
        .output()
        .expect("run update --tag next --no-save");
    assert!(output.status.success(), "update --tag next --no-save failed: {output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains(r#"Ignoring "--tag next""#),
        "the ignored --tag must be reported to the user: {stdout}",
    );

    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));
    let packages = lockfile_package_keys(&workspace);
    assert!(packages.contains(&format!("{DEP}@100.1.0")), "{packages:?}");
    assert!(!packages.contains(&format!("{DEP}@101.0.0")), "{packages:?}");

    drop((root, anchor));
}

/// `--tag` combined with a versioned selector is rejected, the way
/// `--latest` rejects one: the selector's version and the flag's tag name
/// two different targets.
#[test]
fn update_tag_with_spec_is_rejected() {
    let (root, workspace, anchor) = setup_with_own_registry();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["update", "--tag", "next", &format!("{DEP}@2")])
        .output()
        .expect("run pacquet update");
    assert!(!output.status.success(), "update --tag with a spec should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Specs are not allowed to be used with --tag"),
        "stderr did not mention the TAG_WITH_SPEC error: {stderr}",
    );

    drop((root, anchor));
}

/// `--latest` and `--tag` are two names for the same slot, so clap rejects
/// the combination before any resolution happens.
#[test]
fn update_tag_conflicts_with_latest() {
    let (root, workspace, anchor) = setup_with_own_registry();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));

    let output = pacquet(&workspace, ["update", "--latest", "--tag", "next"])
        .output()
        .expect("run pacquet update");
    assert!(!output.status.success(), "update --latest --tag should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--tag"), "stderr did not mention the conflicting flag: {stderr}");

    drop((root, anchor));
}

/// A tag the registry does not publish for a matched dependency is an
/// error, the same way an unknown `<name>@<tag>` selector is one.
#[test]
fn update_tag_unknown_tag_fails() {
    let (root, workspace, anchor) = setup_with_own_registry();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));
    pacquet(&workspace, ["install"]).assert().success();

    let output = pacquet(&workspace, ["update", "--tag", "no-such-tag"])
        .output()
        .expect("run pacquet update");
    assert!(!output.status.success(), "update --tag with an unknown tag should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!("Failed to resolve {DEP}@no-such-tag")),
        "stderr did not name the unresolvable tag: {stderr}",
    );

    drop((root, anchor));
}

/// A value that is no dist-tag name at all is rejected before anything
/// runs: reaching the rewrite, it would resolve through no resolver in the
/// tag chain and the manifest would take the raw string as the
/// dependency's new specifier.
#[test]
fn update_tag_rejects_a_value_that_is_not_a_dist_tag() {
    let (root, workspace, anchor) = setup_with_own_registry();

    write_manifest(&workspace, &format!(r#"{{ "{DEP}": "^100.0.0" }}"#));

    let output = pacquet(&workspace, ["update", "--tag", "file:../payload"])
        .output()
        .expect("run pacquet update");
    assert!(!output.status.success(), "update --tag with a non-tag value should fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Invalid dist-tag: file:../payload"),
        "stderr did not name the invalid tag: {stderr}",
    );
    assert_eq!(dep_spec(&workspace, DEP).as_deref(), Some("^100.0.0"));

    drop((root, anchor));
}
