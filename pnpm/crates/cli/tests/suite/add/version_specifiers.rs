use super::{
    CommandExtra, CommandTempCwd, DependencyGroup, PackageManifest, Pipe, add_with_save_settings,
    add_with_settings, assert_eq, bravo_dep_mature_up_to_1_0_1_minimum_release_age,
    exec_pacquet_in_temp_cwd, prod_spec, set_minimum_release_age,
};
use assert_cmd::assert::OutputAssertExt;

#[test]
fn save_prefix_defaults_to_caret() {
    let (root, dir, anchor) = exec_pacquet_in_temp_cwd(["add", "@pnpm.e2e/hello-world-js-bin"]);
    let spec = prod_spec(&dir, "@pnpm.e2e/hello-world-js-bin");
    eprintln!("SPEC: {spec}");
    assert_eq!(spec, "^1.0.0");
    drop((root, anchor)); // cleanup
}

#[test]
fn save_prefix_tilde_writes_tilde_range() {
    let (root, dir, anchor) =
        exec_pacquet_in_temp_cwd(["add", "@pnpm.e2e/hello-world-js-bin", "--save-prefix=~"]);
    let spec = prod_spec(&dir, "@pnpm.e2e/hello-world-js-bin");
    eprintln!("SPEC: {spec}");
    assert_eq!(spec, "~1.0.0");
    drop((root, anchor)); // cleanup
}

#[test]
fn save_prefix_empty_writes_exact_version() {
    let (root, dir, anchor) =
        exec_pacquet_in_temp_cwd(["add", "@pnpm.e2e/hello-world-js-bin", "--save-prefix="]);
    let spec = prod_spec(&dir, "@pnpm.e2e/hello-world-js-bin");
    eprintln!("SPEC: {spec}");
    assert_eq!(spec, "1.0.0");
    drop((root, anchor)); // cleanup
}

#[test]
fn save_exact_overrides_save_prefix() {
    let (root, dir, anchor) = exec_pacquet_in_temp_cwd([
        "add",
        "@pnpm.e2e/hello-world-js-bin",
        "--save-prefix=~",
        "--save-exact",
    ]);
    let spec = prod_spec(&dir, "@pnpm.e2e/hello-world-js-bin");
    eprintln!("SPEC: {spec}");
    assert_eq!(spec, "1.0.0");
    drop((root, anchor)); // cleanup
}

#[test]
fn save_exact_writes_exact_version() {
    let (root, dir, anchor) =
        exec_pacquet_in_temp_cwd(["add", "@pnpm.e2e/hello-world-js-bin", "--save-exact"]);
    let spec = prod_spec(&dir, "@pnpm.e2e/hello-world-js-bin");
    eprintln!("SPEC: {spec}");
    assert_eq!(spec, "1.0.0");
    drop((root, anchor)); // cleanup
}

#[test]
fn add_prerelease_resolved_version_keeps_no_prefix() {
    // `@pnpm.e2e/beta-version`'s only published version is the prerelease
    // `1.0.0-beta.0`, so `latest` resolves to it. A prerelease range is
    // written verbatim, with no `^`, matching pnpm.
    let (root, dir, anchor) = exec_pacquet_in_temp_cwd(["add", "@pnpm.e2e/beta-version"]);
    let spec = prod_spec(&dir, "@pnpm.e2e/beta-version");
    eprintln!("SPEC: {spec}");
    assert_eq!(spec, "1.0.0-beta.0");
    drop((root, anchor)); // cleanup
}

/// `pacquet add <existing-dep>` without a version keeps the dependency's
/// declared range verbatim instead of bumping it to `^<latest>`, matching
/// `pnpm add <existing>`. The latest published version is `101.0.0`, which a
/// bump would have written.
#[test]
fn add_existing_dependency_without_version_keeps_tilde_range() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    std::fs::write(
        workspace.join("package.json"),
        r#"{ "name": "p", "version": "1.0.0", "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "~100.0.0" } }"#,
    )
    .unwrap();

    pacquet
        .with_args(["add", "@pnpm.e2e/dep-of-pkg-with-1-dep", "--lockfile-only"])
        .assert()
        .success();

    assert_eq!(prod_spec(&workspace, "@pnpm.e2e/dep-of-pkg-with-1-dep"), "~100.0.0");
    drop((root, npmrc_info)); // cleanup
}

/// The same applies to an exact pin: a re-add keeps it exact rather than
/// widening it to the default caret.
#[test]
fn add_existing_dependency_without_version_keeps_exact_pin() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    std::fs::write(
        workspace.join("package.json"),
        r#"{ "name": "p", "version": "1.0.0", "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0" } }"#,
    )
    .unwrap();

    pacquet
        .with_args(["add", "@pnpm.e2e/dep-of-pkg-with-1-dep", "--lockfile-only"])
        .assert()
        .success();

    assert_eq!(prod_spec(&workspace, "@pnpm.e2e/dep-of-pkg-with-1-dep"), "100.0.0");
    drop((root, npmrc_info)); // cleanup
}

/// `add <pkg>@<range>` records the range resolved to a concrete version
/// with the input's operator, matching pnpm. `^100.0.0` resolves to the
/// highest in-range version (100.1.0; 101.0.0 is a different major), so the
/// manifest gets `^100.1.0` — not the verbatim `^100.0.0`.
#[test]
fn add_explicit_range_resolves_to_concrete_version() {
    let (root, dir, anchor) = exec_pacquet_in_temp_cwd([
        "add",
        "@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0",
        "--lockfile-only",
    ]);
    assert_eq!(prod_spec(&dir, "@pnpm.e2e/dep-of-pkg-with-1-dep"), "^100.1.0");
    drop((root, anchor)); // cleanup
}

/// A narrower range is not widened: `~100.0.0` resolves to the highest
/// `100.0.x` (here `100.0.0`) and keeps the tilde — it is not bumped to the
/// `latest` tag (`101.0.0`).
#[test]
fn add_explicit_tilde_range_is_not_widened_to_latest() {
    let (root, dir, anchor) = exec_pacquet_in_temp_cwd([
        "add",
        "@pnpm.e2e/dep-of-pkg-with-1-dep@~100.0.0",
        "--lockfile-only",
    ]);
    assert_eq!(prod_spec(&dir, "@pnpm.e2e/dep-of-pkg-with-1-dep"), "~100.0.0");
    drop((root, anchor)); // cleanup
}

/// A dist-tag spec resolves to that tag's version, pinned with the default
/// caret (the tag carries no operator). `latest` is 101.0.0.
#[test]
fn add_explicit_dist_tag_resolves_with_caret() {
    let (root, dir, anchor) = exec_pacquet_in_temp_cwd([
        "add",
        "@pnpm.e2e/dep-of-pkg-with-1-dep@latest",
        "--lockfile-only",
    ]);
    assert_eq!(prod_spec(&dir, "@pnpm.e2e/dep-of-pkg-with-1-dep"), "^101.0.0");
    drop((root, anchor)); // cleanup
}

#[test]
fn readding_a_dev_dependency_at_a_dist_tag_keeps_its_group() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let name = "@pnpm.e2e/dep-of-pkg-with-1-dep";
    std::fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "devDependencies": { (name): "^100.0.0" } }).to_string(),
    )
    .expect("write package.json");

    pacquet.with_args(["add", &format!("{name}@latest"), "--lockfile-only"]).assert().success();

    let manifest =
        PackageManifest::from_path(workspace.join("package.json")).expect("read package.json");
    assert_eq!(
        manifest.dependencies([DependencyGroup::Dev]).collect::<Vec<_>>(),
        vec![(name, "^101.0.0")],
    );
    assert!(
        manifest.dependencies([DependencyGroup::Prod]).all(|(dependency, _)| dependency != name),
    );

    drop((root, npmrc_info));
}

/// On a re-add with an explicit version, the existing entry biases the pick
/// (it is a preferred version): re-adding `~100.0.0` with `@^100.0.0` keeps
/// the existing `100.0.0` rather than bumping to the highest in range
/// (`100.1.0`), and the existing operator wins over the spec's — matching
/// pnpm, which dedups to and keeps the already-declared version.
#[test]
fn add_explicit_range_respects_existing_operator() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    std::fs::write(
        workspace.join("package.json"),
        r#"{ "name": "p", "version": "1.0.0", "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "~100.0.0" } }"#,
    )
    .unwrap();

    pacquet
        .with_args(["add", "@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0", "--lockfile-only"])
        .assert()
        .success();

    assert_eq!(prod_spec(&workspace, "@pnpm.e2e/dep-of-pkg-with-1-dep"), "~100.0.0");
    drop((root, npmrc_info)); // cleanup
}

/// An `npm:` alias specifier is written verbatim — never resolved (which
/// would risk dropping the aliased target name).
#[test]
fn add_npm_alias_spec_is_kept_verbatim() {
    let (root, dir, anchor) = exec_pacquet_in_temp_cwd([
        "add",
        "my-alias@npm:@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0",
        "--lockfile-only",
    ]);
    assert_eq!(prod_spec(&dir, "my-alias"), "npm:@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0");
    drop((root, anchor)); // cleanup
}

/// A previous specifier that is a non-registry path/URL must not influence
/// the pin: `infer_range_spec_style` scans for a version anywhere in the
/// spec, so a `file:` tarball path whose only range-like element is an
/// `x.y.z` classifies as an exact pin. Re-adding over
/// `file:../deps/100.0.0.tgz` with `@^100.0.0` keeps the caret
/// (`^100.1.0`), not an exact `100.1.0`.
#[test]
fn add_explicit_range_ignores_pin_from_non_registry_prev() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    std::fs::write(
        workspace.join("package.json"),
        r#"{ "name": "p", "version": "1.0.0", "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "file:../deps/100.0.0.tgz" } }"#,
    )
    .unwrap();

    pacquet
        .with_args(["add", "@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0", "--lockfile-only"])
        .assert()
        .success();

    assert_eq!(prod_spec(&workspace, "@pnpm.e2e/dep-of-pkg-with-1-dep"), "^100.1.0");
    drop((root, npmrc_info)); // cleanup
}

#[test]
fn save_prefix_arbitrary_value_falls_back_to_caret() {
    let (root, dir, anchor) =
        exec_pacquet_in_temp_cwd(["add", "@pnpm.e2e/hello-world-js-bin", "--save-prefix=foo"]);
    let spec = prod_spec(&dir, "@pnpm.e2e/hello-world-js-bin");
    eprintln!("SPEC: {spec}");
    assert_eq!(spec, "^1.0.0");
    drop((root, anchor)); // cleanup
}

/// Covers <https://github.com/pnpm/pnpm/issues/11165>: `add <name>` (no
/// version) under an active `minimumReleaseAge` pins the newest *mature*
/// version, not the raw `latest` dist-tag.
#[test]
fn add_without_version_respects_minimum_release_age() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    set_minimum_release_age(&workspace, bravo_dep_mature_up_to_1_0_1_minimum_release_age());

    pacquet.with_args(["add", "@pnpm.e2e/bravo-dep"]).assert().success();

    assert_eq!(prod_spec(&workspace, "@pnpm.e2e/bravo-dep"), "^1.0.1");

    drop((root, npmrc_info)); // cleanup
}

/// The `savePrefix` and `savePeer` settings drive `pnpm add` the same
/// way `--save-prefix` / `--save-peer` do.
#[test]
fn save_prefix_and_save_peer_settings_drive_add() {
    let (root, workspace, mock_instance) = add_with_save_settings(&["add"]);

    let manifest =
        workspace.join("package.json").pipe(PackageManifest::from_path).expect("read manifest");
    let peer_spec = manifest
        .dependencies([DependencyGroup::Peer])
        .find(|(name, _)| *name == "@pnpm.e2e/hello-world-js-bin")
        .map(|(_, spec)| spec.to_string());
    let dev_spec = manifest
        .dependencies([DependencyGroup::Dev])
        .find(|(name, _)| *name == "@pnpm.e2e/hello-world-js-bin")
        .map(|(_, spec)| spec.to_string());
    eprintln!("PEER: {peer_spec:?}, DEV: {dev_spec:?}");
    assert_eq!(peer_spec.as_deref(), Some("~1.0.0"), "savePeer must add a peerDependencies entry");
    assert_eq!(dev_spec.as_deref(), Some("~1.0.0"), "savePeer also saves it as a dev dependency");

    drop((root, mock_instance));
}

/// `--save-prefix` and `--no-save-peer` overrule the `savePrefix` and
/// `savePeer` settings, in the usual CLI-beats-config order.
#[test]
fn save_flags_overrule_the_save_settings() {
    let (root, workspace, mock_instance) =
        add_with_save_settings(&["add", "--save-prefix", "^", "--no-save-peer"]);

    let manifest =
        workspace.join("package.json").pipe(PackageManifest::from_path).expect("read manifest");
    let prod_spec = manifest
        .dependencies([DependencyGroup::Prod])
        .find(|(name, _)| *name == "@pnpm.e2e/hello-world-js-bin")
        .map(|(_, spec)| spec.to_string());
    let peer_spec = manifest
        .dependencies([DependencyGroup::Peer])
        .find(|(name, _)| *name == "@pnpm.e2e/hello-world-js-bin")
        .map(|(_, spec)| spec.to_string());
    eprintln!("PROD: {prod_spec:?}, PEER: {peer_spec:?}");
    assert_eq!(prod_spec.as_deref(), Some("^1.0.0"), "--save-prefix must overrule savePrefix");
    assert_eq!(peer_spec, None, "--no-save-peer must overrule savePeer");

    drop((root, mock_instance));
}

/// The `saveExact` setting drives `pnpm add` without the `--save-exact`
/// flag, and a `savePrefix` of `=` keeps the explicit operator.
#[test]
fn save_exact_and_equals_prefix_settings_drive_add() {
    let (root, workspace, mock_instance) = add_with_settings("saveExact: true\n", &["add"]);
    let spec = prod_spec(&workspace, "@pnpm.e2e/hello-world-js-bin");
    eprintln!("SPEC: {spec}");
    assert_eq!(spec, "1.0.0", "the saveExact setting must save the bare version");
    drop((root, mock_instance));

    let (root, workspace, mock_instance) = add_with_settings("savePrefix: '='\n", &["add"]);
    let spec = prod_spec(&workspace, "@pnpm.e2e/hello-world-js-bin");
    eprintln!("SPEC: {spec}");
    assert_eq!(spec, "=1.0.0", "a savePrefix of = must keep the explicit operator");
    drop((root, mock_instance));
}
