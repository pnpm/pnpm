pub mod _utils;

use _utils::{
    bravo_dep_mature_up_to_1_0_1_minimum_release_age, read_current_lockfile,
    set_minimum_release_age,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_lockfile::{Lockfile, PkgName};
use pacquet_package_manifest::{DependencyGroup, PackageManifest};
use pacquet_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::{get_all_folders, get_filenames_in_folder},
};
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
#[cfg(unix)]
use std::fs;
use std::{ffi::OsStr, path::PathBuf, process::Command};
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
fn add_accepts_multiple_local_package_selectors() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let fixtures_dir = workspace.join("fixtures");
    for package_name in ["local-a", "local-b"] {
        let package_dir = fixtures_dir.join(package_name);
        std::fs::create_dir_all(&package_dir).expect("create local package directory");
        std::fs::write(
            package_dir.join("package.json"),
            serde_json::json!({ "name": package_name, "version": "1.0.0" }).to_string(),
        )
        .expect("write local package manifest");
    }

    pacquet
        .with_args(["add", "local-a@file:./fixtures/local-a", "local-b@file:./fixtures/local-b"])
        .assert()
        .success();

    assert_eq!(prod_spec(&workspace, "local-a"), "file:./fixtures/local-a");
    assert_eq!(prod_spec(&workspace, "local-b"), "file:./fixtures/local-b");

    let lockfile_text =
        std::fs::read_to_string(workspace.join(Lockfile::FILE_NAME)).expect("read pnpm-lock.yaml");
    let lockfile: Lockfile = serde_saphyr::from_str(&lockfile_text)
        .unwrap_or_else(|error| panic!("parse pnpm-lock.yaml: {error}\n{lockfile_text}"));
    let dependencies = lockfile
        .importers
        .get(Lockfile::ROOT_IMPORTER_KEY)
        .and_then(|importer| importer.dependencies.as_ref())
        .expect("root importer dependencies");
    for package_name in ["local-a", "local-b"] {
        let parsed_name: PkgName = package_name.parse().expect("parse local package name");
        assert!(dependencies.contains_key(&parsed_name), "lockfile contains {package_name}");
        assert!(
            workspace.join("node_modules").join(package_name).join("package.json").exists(),
            "{package_name} is installed",
        );
    }

    drop(root); // cleanup
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

#[test]
fn add_lockfile_only_from_workspace_subdir_prints_manifest_summary() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        std::fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    std::fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    let package_dir = workspace.join("packages/a");
    std::fs::create_dir_all(&package_dir).expect("mkdir packages/a");
    std::fs::write(
        package_dir.join("package.json"),
        serde_json::json!({ "name": "a", "version": "1.0.0" }).to_string(),
    )
    .expect("write packages/a/package.json");

    let output = pacquet
        .with_args([
            "--dir",
            "packages/a",
            "--reporter=append-only",
            "add",
            "@pnpm.e2e/hello-world-js-bin",
            "--lockfile-only",
        ])
        .output()
        .expect("run pacquet add");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "add failed\nstdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        stdout.contains("dependencies:\n+ @pnpm.e2e/hello-world-js-bin ^1.0.0"),
        "add --lockfile-only should print the manifest diff summary for the selected importer\nstdout:\n{stdout}",
    );

    assert_eq!(prod_spec(&package_dir, "@pnpm.e2e/hello-world-js-bin"), "^1.0.0");

    let package_dir = workspace.join("packages/b");
    std::fs::create_dir_all(&package_dir).expect("mkdir packages/b");
    std::fs::write(
        package_dir.join("package.json"),
        serde_json::json!({ "name": "b", "version": "1.0.0" }).to_string(),
    )
    .expect("write packages/b/package.json");

    let output = Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args([
            "--dir",
            "packages/b",
            "--reporter=ndjson",
            "add",
            "@pnpm.e2e/hello-world-js-bin",
            "--lockfile-only",
        ])
        .output()
        .expect("run pacquet add with ndjson reporter");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "add failed\nstderr:\n{stderr}");
    let records = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .collect::<Vec<_>>();
    let initial_manifest_count = records
        .iter()
        .filter(|record| {
            record.get("name").and_then(|name| name.as_str()) == Some("pnpm:package-manifest")
                && record.get("initial").is_some()
        })
        .count();
    assert_eq!(
        initial_manifest_count, 1,
        "ndjson should emit one initial package manifest\nstderr:\n{stderr}",
    );
    let summary_count = records
        .iter()
        .filter(|record| record.get("name").and_then(|name| name.as_str()) == Some("pnpm:summary"))
        .count();
    assert_eq!(summary_count, 1, "ndjson should emit one pnpm:summary\nstderr:\n{stderr}");

    assert_eq!(prod_spec(&package_dir, "@pnpm.e2e/hello-world-js-bin"), "^1.0.0");
    drop((root, npmrc_info)); // cleanup
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

/// A dependency has one manifest home: a versionless re-add with an
/// explicit save target moves the entry into that group and drops it from
/// the others, carrying the first-found specifier in pnpm's `findSpec`
/// order (`optionalDependencies`, `dependencies`, `devDependencies`,
/// `peerDependencies`) — so `--save-dev` here adopts the `dependencies`
/// spec, matching pnpm's `updateProjectManifestObject`.
#[test]
fn add_existing_dependency_moves_it_to_the_target_group() {
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
    assert_eq!(group_spec(DependencyGroup::Dev).as_deref(), Some("~100.0.0"));
    assert_eq!(group_spec(DependencyGroup::Prod), None);
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
/// the pin: `which_version_is_pinned` scans for a version anywhere in the
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

/// `add` saves into one dependency group, but its install must keep every
/// group: the added package's transitive optionals must be materialized in
/// the virtual store and recorded in the current lockfile, and the alias
/// symlink inside the dependent package must resolve. A missing slot here
/// is what breaks a globally installed bin at runtime with "Missing
/// optional dependency" (e.g. `@openai/codex`'s platform binary).
#[test]
fn add_materializes_transitive_optional_dependencies() {
    let (root, workspace, anchor) =
        exec_pacquet_in_temp_cwd(["add", "@pnpm.e2e/pkg-with-good-optional"]);

    let virtual_store = workspace.join("node_modules").join(".pnpm");
    assert!(
        virtual_store.join("is-positive@1.0.0").exists(),
        "the transitive optional dependency must be materialized",
    );
    assert!(
        virtual_store
            .join("@pnpm.e2e+pkg-with-good-optional@1.0.0/node_modules/is-positive/package.json")
            .exists(),
        "the optional dependency alias symlink must resolve",
    );

    let current_lockfile = std::fs::read_to_string(virtual_store.join("lock.yaml"))
        .expect("read the current lockfile");
    assert!(
        current_lockfile.contains("is-positive@1.0.0"),
        "the current lockfile must record the materialized optional:\n{current_lockfile}",
    );

    drop((root, anchor)); // cleanup
}

/// TS: `dependency should be removed from the old field when installing it
/// as a different type of dependency` (`updatingPkgJson.ts:112`).
/// Sequential adds move each entry to its new manifest group without
/// erasing the other groups' entries, and the current lockfile importer
/// tracks the final grouping.
#[test]
fn add_moves_dependency_to_new_group_and_keeps_other_groups() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    std::fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/foo": "^100.0.0" },
            "devDependencies": { "@pnpm.e2e/bar": "^100.0.0" },
            "optionalDependencies": { "@pnpm.e2e/qar": "^100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    let run_add = |args: &[&str]| {
        Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(&workspace)
            .with_arg("add")
            .with_args(args)
            .assert()
            .success();
    };
    pacquet.with_args(["add", "--save-optional", "@pnpm.e2e/foo@^100.0.0"]).assert().success();
    run_add(&["@pnpm.e2e/bar@^100.0.0"]);
    run_add(&["--save-dev", "@pnpm.e2e/qar@^100.0.0"]);

    let group_members = |group: DependencyGroup| -> Vec<String> {
        let manifest =
            PackageManifest::from_path(workspace.join("package.json")).expect("read package.json");
        let mut members: Vec<String> =
            manifest.dependencies([group]).map(|(name, _)| name.to_string()).collect();
        members.sort();
        members
    };
    assert_eq!(group_members(DependencyGroup::Prod), ["@pnpm.e2e/bar"]);
    assert_eq!(group_members(DependencyGroup::Dev), ["@pnpm.e2e/qar"]);
    assert_eq!(group_members(DependencyGroup::Optional), ["@pnpm.e2e/foo"]);

    run_add(&["@pnpm.e2e/bar@^100.0.0", "@pnpm.e2e/foo@^100.0.0", "@pnpm.e2e/qar@^100.0.0"]);
    assert_eq!(
        group_members(DependencyGroup::Prod),
        ["@pnpm.e2e/bar", "@pnpm.e2e/foo", "@pnpm.e2e/qar"],
    );
    assert_eq!(group_members(DependencyGroup::Dev), Vec::<String>::new());
    assert_eq!(group_members(DependencyGroup::Optional), Vec::<String>::new());

    let current = read_current_lockfile(&workspace);
    let importer = current
        .importers
        .get(Lockfile::ROOT_IMPORTER_KEY)
        .expect("current lockfile has the root importer");
    let mut dependencies: Vec<String> = importer
        .dependencies
        .as_ref()
        .expect("root importer has dependencies")
        .keys()
        .map(ToString::to_string)
        .collect();
    dependencies.sort();
    assert_eq!(dependencies, ["@pnpm.e2e/bar", "@pnpm.e2e/foo", "@pnpm.e2e/qar"]);

    drop((root, npmrc_info)); // cleanup
}

/// `add` into one dependency group must leave the other groups' entries in
/// the wanted lockfile and `node_modules`: a prod `add` must not erase the
/// project's devDependencies from either.
#[test]
fn add_keeps_entries_of_other_dependency_groups() {
    let (root, workspace, anchor) =
        exec_pacquet_in_temp_cwd(["add", "--save-dev", "@pnpm.e2e/hello-world-js-bin"]);

    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(&workspace)
        .with_args(["add", "@pnpm.e2e/hello-world-js-bin-parent"])
        .assert()
        .success();

    let lockfile = std::fs::read_to_string(workspace.join("pnpm-lock.yaml"))
        .expect("read the wanted lockfile");
    assert!(
        lockfile.contains("devDependencies"),
        "the wanted lockfile must keep the dev dependency after a prod add:\n{lockfile}",
    );
    assert!(
        workspace.join("node_modules/@pnpm.e2e/hello-world-js-bin/package.json").exists(),
        "the dev dependency's node_modules link must survive a prod add",
    );

    drop((root, anchor)); // cleanup
}
