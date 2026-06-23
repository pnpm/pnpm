use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::{get_all_folders, get_filenames_in_folder},
};
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
#[cfg(unix)]
use std::fs;
use std::{ffi::OsStr, path::PathBuf};
use tempfile::TempDir;

fn exec_pacquet_in_temp_cwd<Args>(args: Args) -> (TempDir, PathBuf, AddMockedRegistry)
where
    Args: IntoIterator,
    Args::Item: AsRef<OsStr>,
{
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    pacquet.with_args(args).assert().success();
    (root, workspace, npmrc_info)
}

#[test]
fn should_install_all_dependencies() {
    let (root, workspace, anchor) =
        exec_pacquet_in_temp_cwd(["add", "@pnpm.e2e/hello-world-js-bin-parent"]);

    eprintln!("Directory list");
    insta::assert_debug_snapshot!(get_all_folders(&workspace));

    let manifest_path = workspace.join("package.json");

    eprintln!("Ensure the manifest file ({manifest_path:?}) exists");
    assert!(manifest_path.exists());

    let virtual_store_dir = workspace.join("node_modules").join(".pnpm");

    eprintln!("Ensure virtual store dir ({virtual_store_dir:?}) exists");
    assert!(virtual_store_dir.exists());

    eprintln!("Ensure that @pnpm.e2e/hello-world-js-bin has no other dependencies than itself");
    let path = virtual_store_dir.join("@pnpm.e2e+hello-world-js-bin@1.0.0/node_modules");
    assert_eq!(get_filenames_in_folder(&path), ["@pnpm.e2e"]);
    assert_eq!(get_filenames_in_folder(&path.join("@pnpm.e2e")), ["hello-world-js-bin"]);

    eprintln!("Ensure that @pnpm.e2e/hello-world-js-bin-parent has correct dependencies");
    let path = virtual_store_dir.join("@pnpm.e2e+hello-world-js-bin-parent@1.0.0/node_modules");
    assert_eq!(get_filenames_in_folder(&path), ["@pnpm.e2e"]);
    assert_eq!(
        get_filenames_in_folder(&path.join("@pnpm.e2e")),
        ["hello-world-js-bin", "hello-world-js-bin-parent"],
    );

    drop((root, anchor)); // cleanup
}

#[test]
#[cfg(unix)]
pub fn should_symlink_correctly() {
    let (root, workspace, anchor) =
        exec_pacquet_in_temp_cwd(["add", "@pnpm.e2e/hello-world-js-bin-parent"]);

    eprintln!("Directory list");
    insta::assert_debug_snapshot!(get_all_folders(&workspace));

    let manifest_path = workspace.join("package.json");

    eprintln!("Ensure the manifest file ({manifest_path:?}) exists");
    assert!(manifest_path.exists());

    let virtual_store_dir = workspace.join("node_modules").join(".pnpm");

    eprintln!("Ensure virtual store dir ({virtual_store_dir:?}) exists");
    assert!(virtual_store_dir.exists());

    eprintln!("Make sure the symlinks are correct");
    // pacquet writes the symlink target as a path relative to the
    // link's parent (matching upstream `symlink-dir`), so
    // canonicalize the symlink itself rather than comparing
    // `read_link`'s relative output against an absolute path.
    let symlink_path = virtual_store_dir
        .join("@pnpm.e2e+hello-world-js-bin-parent@1.0.0")
        .join("node_modules")
        .join("@pnpm.e2e")
        .join("hello-world-js-bin");
    let target_path = virtual_store_dir
        .join("@pnpm.e2e+hello-world-js-bin@1.0.0")
        .join("node_modules")
        .join("@pnpm.e2e")
        .join("hello-world-js-bin");
    assert_eq!(
        symlink_path.pipe(fs::canonicalize).expect("canonicalize symlink"),
        target_path.pipe(fs::canonicalize).expect("canonicalize link target"),
    );

    drop((root, anchor)); // cleanup
}

#[test]
fn should_add_to_package_json() {
    let (root, dir, anchor) = exec_pacquet_in_temp_cwd(["add", "@pnpm.e2e/hello-world-js-bin"]);
    let file = PackageManifest::from_path(dir.join("package.json")).unwrap();
    eprintln!("Ensure @pnpm.e2e/hello-world-js-bin is added to package.json#dependencies");
    assert!(
        file.dependencies([DependencyGroup::Prod])
            .any(|(k, _)| k == "@pnpm.e2e/hello-world-js-bin"),
    );
    drop((root, anchor)); // cleanup
}

#[test]
fn add_runs_with_ndjson_and_silent_reporters() {
    for reporter in ["--reporter=ndjson", "--reporter=silent"] {
        let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
            CommandTempCwd::init().add_mocked_registry();

        pacquet.with_args([reporter, "add", "@pnpm.e2e/hello-world-js-bin"]).assert().success();

        let file = PackageManifest::from_path(workspace.join("package.json")).unwrap();
        assert!(
            file.dependencies([DependencyGroup::Prod])
                .any(|(key, _)| key == "@pnpm.e2e/hello-world-js-bin"),
            "dependency should be saved when running add with {reporter}",
        );

        drop((root, npmrc_info)); // cleanup
    }
}

fn prod_spec(dir: &std::path::Path, name: &str) -> String {
    let manifest = dir.join("package.json").pipe(PackageManifest::from_path).unwrap();
    let (_, spec) = manifest
        .dependencies([DependencyGroup::Prod])
        .find(|(key, _)| *key == name)
        .unwrap_or_else(|| panic!("{name} should be in dependencies"));
    spec.to_string()
}

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

/// When the same package exists in more than one dependency bucket with
/// different specs, a versionless re-add preserves the specifier of the
/// *targeted* group (here `--save-dev`), not whichever group happens to be
/// scanned first, and leaves the other bucket untouched.
#[test]
fn add_existing_dependency_preserves_target_group_specifier() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    std::fs::write(
        workspace.join("package.json"),
        r#"{ "name": "p", "version": "1.0.0", "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "~100.0.0" }, "devDependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "^100.0.0" } }"#,
    )
    .unwrap();

    pacquet
        .with_args(["add", "@pnpm.e2e/dep-of-pkg-with-1-dep", "--save-dev", "--lockfile-only"])
        .assert()
        .success();

    let manifest = PackageManifest::from_path(workspace.join("package.json")).unwrap();
    let group_spec = |group| {
        manifest
            .dependencies([group])
            .find(|(key, _)| *key == "@pnpm.e2e/dep-of-pkg-with-1-dep")
            .map(|(_, spec)| spec.to_string())
    };
    assert_eq!(group_spec(DependencyGroup::Dev).as_deref(), Some("^100.0.0"));
    assert_eq!(group_spec(DependencyGroup::Prod).as_deref(), Some("~100.0.0"));
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
/// the pin: `which_version_is_pinned` forward-scans for a version substring,
/// so a `file:` tarball path with an embedded `x.y.z` could otherwise force
/// an exact pin. Re-adding over `file:../…-100.0.0.tgz` with `@^100.0.0`
/// keeps the caret (`^100.1.0`), not an exact `100.1.0`.
#[test]
fn add_explicit_range_ignores_pin_from_non_registry_prev() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    std::fs::write(
        workspace.join("package.json"),
        r#"{ "name": "p", "version": "1.0.0", "dependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "file:../dep-of-pkg-with-1-dep-100.0.0.tgz" } }"#,
    )
    .unwrap();

    pacquet
        .with_args(["add", "@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0", "--lockfile-only"])
        .assert()
        .success();

    assert_eq!(prod_spec(&workspace, "@pnpm.e2e/dep-of-pkg-with-1-dep"), "^100.1.0");
    drop((root, npmrc_info)); // cleanup
}

/// A registry-host tarball URL parses as a registry `Version` spec, but it
/// must be written verbatim — resolving it would rewrite an explicit URL
/// dependency into a semver range.
#[test]
fn add_registry_tarball_url_is_kept_verbatim() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    std::fs::write(workspace.join("package.json"), r#"{ "name": "p", "version": "1.0.0" }"#)
        .unwrap();

    let url = format!(
        "{}@pnpm.e2e/dep-of-pkg-with-1-dep/-/dep-of-pkg-with-1-dep-100.0.0.tgz",
        npmrc_info.mock_instance.url(),
    );
    pacquet
        .with_args(["add", &format!("@pnpm.e2e/dep-of-pkg-with-1-dep@{url}"), "--lockfile-only"])
        .assert()
        .success();

    assert_eq!(prod_spec(&workspace, "@pnpm.e2e/dep-of-pkg-with-1-dep"), url);
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

#[test]
fn should_add_dev_dependency() {
    let (root, dir, anchor) =
        exec_pacquet_in_temp_cwd(["add", "@pnpm.e2e/hello-world-js-bin", "--save-dev"]);
    let file = PackageManifest::from_path(dir.join("package.json")).unwrap();
    eprintln!("Ensure @pnpm.e2e/hello-world-js-bin is added to package.json#devDependencies");
    assert!(
        file.dependencies([DependencyGroup::Dev]).any(|(k, _)| k == "@pnpm.e2e/hello-world-js-bin"),
    );
    drop((root, anchor)); // cleanup
}

#[test]
fn should_add_peer_dependency() {
    let (root, dir, anchor) =
        exec_pacquet_in_temp_cwd(["add", "@pnpm.e2e/hello-world-js-bin", "--save-peer"]);
    let file = PackageManifest::from_path(dir.join("package.json")).unwrap();
    eprintln!("Ensure @pnpm.e2e/hello-world-js-bin is added to package.json#devDependencies");
    assert!(
        file.dependencies([DependencyGroup::Dev]).any(|(k, _)| k == "@pnpm.e2e/hello-world-js-bin"),
    );
    eprintln!("Ensure @pnpm.e2e/hello-world-js-bin is added to package.json#peerDependencies");
    assert!(
        file.dependencies([DependencyGroup::Peer])
            .any(|(k, _)| k == "@pnpm.e2e/hello-world-js-bin"),
    );
    drop((root, anchor)); // cleanup
}
