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
    registry::TestRegistry,
};
use pipe_trait::Pipe;
use pretty_assertions::assert_eq;
#[cfg(unix)]
use std::fs;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Command,
};
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

/// Regression test for the Tag release operator's invocation (pnpm/pnpm#13242).
#[test]
fn add_accepts_dir_allow_build_and_registry_after_the_subcommand() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let registry = mock_instance.url();

    pacquet
        .with_args([
            "add",
            "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0",
            "--dir",
            ".",
            "--allow-build=@pnpm.e2e/pre-and-postinstall-scripts-example",
        ])
        .with_arg(format!("--registry={registry}"))
        .assert()
        .success();

    let pkg_dir = workspace.join(
        "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
         /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example",
    );
    assert!(
        pkg_dir.join("generated-by-postinstall.js").exists(),
        "the --allow-build package should have run its postinstall",
    );

    let yaml = std::fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .expect("pnpm-workspace.yaml present");
    assert!(
        yaml.contains("@pnpm.e2e/pre-and-postinstall-scripts-example"),
        "allowBuilds entry should be persisted, got:\n{yaml}",
    );

    drop((root, mock_instance));
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

/// A one-member workspace whose `fixtures/` packages let a `-w` add use
/// `file:` specs instead of reaching the registry. Returns the member's
/// directory.
fn write_workspace_with_local_fixtures(workspace: &Path) -> PathBuf {
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml = std::fs::read_to_string(&workspace_yaml_path).unwrap_or_default();
    if !workspace_yaml.is_empty() && !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str("packages:\n  - 'packages/*'\n");
    std::fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    for package_name in ["local-a", "local-b"] {
        let package_dir = workspace.join("fixtures").join(package_name);
        std::fs::create_dir_all(&package_dir).expect("create local package directory");
        std::fs::write(
            package_dir.join("package.json"),
            serde_json::json!({ "name": package_name, "version": "1.0.0" }).to_string(),
        )
        .expect("write local package manifest");
    }

    let member_dir = workspace.join("packages/a");
    std::fs::create_dir_all(&member_dir).expect("mkdir packages/a");
    std::fs::write(
        member_dir.join("package.json"),
        serde_json::json!({ "name": "a", "version": "1.0.0" }).to_string(),
    )
    .expect("write packages/a/package.json");
    member_dir
}

/// End to end rather than a unit test: a relative `--dir` resolves
/// against the process cwd, which a unit test must not mutate.
#[test]
fn add_workspace_root_tolerates_a_dir_that_does_not_exist() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace_with_local_fixtures(&workspace);

    pacquet
        .with_args([
            "--dir",
            "packages/does-not-exist",
            "add",
            "-D",
            "local-a@file:./fixtures/local-a",
            "-w",
        ])
        .assert()
        .success();

    let root_manifest = workspace
        .join("package.json")
        .pipe(PackageManifest::from_path)
        .expect("read root manifest");
    assert!(
        root_manifest.dependencies([DependencyGroup::Dev]).any(|(key, _)| key == "local-a"),
        "a nonexistent --dir must still redirect the add to the root manifest",
    );

    drop(root); // cleanup
}

/// The counterpart to the tolerated nonexistent `--dir` above.
#[test]
fn add_workspace_root_rejects_a_dir_that_climbs_out_of_the_workspace() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    write_workspace_with_local_fixtures(&workspace);

    let output = pacquet
        .with_args([
            "--dir",
            "../../outside-does-not-exist",
            "add",
            "-D",
            "local-a@file:./fixtures/local-a",
            "-w",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&output.get_output().stderr);
    assert!(
        stderr.contains("ERR_PNPM_NOT_IN_WORKSPACE"),
        "a --dir pointing outside the workspace must not fall back to it: {stderr}",
    );

    drop(root); // cleanup
}

/// `pnpm add -D <pkg> <pkg> -w` run from a workspace subdirectory
/// (pnpm/pnpm#13031).
#[test]
fn add_workspace_root_saves_to_the_root_manifest_from_a_subdir() {
    let CommandTempCwd { pacquet, root, workspace, .. } = CommandTempCwd::init();
    let member_dir = write_workspace_with_local_fixtures(&workspace);

    pacquet
        .with_args([
            "--dir",
            "packages/a",
            "add",
            "-D",
            // Relative to the root: a `file:` spec resolves from the
            // manifest that records it, which `-w` makes the root's.
            "local-a@file:./fixtures/local-a",
            "local-b@file:./fixtures/local-b",
            "-w",
        ])
        .assert()
        .success();

    let root_manifest = workspace
        .join("package.json")
        .pipe(PackageManifest::from_path)
        .expect("read root manifest");
    for package_name in ["local-a", "local-b"] {
        assert!(
            root_manifest.dependencies([DependencyGroup::Dev]).any(|(key, _)| key == package_name),
            "--workspace-root must save {package_name} to the root manifest",
        );
    }

    let member_manifest = member_dir
        .join("package.json")
        .pipe(PackageManifest::from_path)
        .expect("read packages/a manifest");
    assert_eq!(
        member_manifest
            .dependencies([
                DependencyGroup::Prod,
                DependencyGroup::Dev,
                DependencyGroup::Optional,
                DependencyGroup::Peer,
            ])
            .count(),
        0,
        "--workspace-root must leave the `--dir` project's manifest untouched",
    );

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

// Regression test for pnpm/pnpm#13108
#[test]
fn add_existing_dependency_ignores_pin_from_peer_range() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    std::fs::write(
        workspace.join("package.json"),
        r#"{ "name": "p", "version": "1.0.0", "devDependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "100.0.0" }, "peerDependencies": { "@pnpm.e2e/dep-of-pkg-with-1-dep": "^100.0.0" } }"#,
    )
    .unwrap();

    pacquet
        .with_args([
            "add",
            "@pnpm.e2e/dep-of-pkg-with-1-dep@100.1.0",
            "--save-dev",
            "--lockfile-only",
        ])
        .assert()
        .success();

    let manifest = PackageManifest::from_path(workspace.join("package.json")).unwrap();
    let group_spec = |group| {
        manifest
            .dependencies([group])
            .find(|(key, _)| *key == "@pnpm.e2e/dep-of-pkg-with-1-dep")
            .map(|(_, spec)| spec.to_string())
    };
    assert_eq!(group_spec(DependencyGroup::Dev).as_deref(), Some("100.1.0"));
    assert_eq!(group_spec(DependencyGroup::Peer).as_deref(), Some("^100.0.0"));
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
    run_add(&["--save-prod", "@pnpm.e2e/bar@^100.0.0"]);
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

    run_add(&[
        "--save-prod",
        "@pnpm.e2e/bar@^100.0.0",
        "@pnpm.e2e/foo@^100.0.0",
        "@pnpm.e2e/qar@^100.0.0",
    ]);
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

/// TS: `dependencies should be updated in the fields where they already
/// are` (`updatingPkgJson.ts:88`): `add name@version` without a save flag
/// updates each entry in the group it already occupies instead of moving
/// it to `dependencies`.
#[test]
fn add_updates_dependency_in_the_group_it_already_occupies() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    std::fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "devDependencies": { "@pnpm.e2e/foo": "^100.0.0" },
            "optionalDependencies": { "@pnpm.e2e/bar": "^100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    pacquet
        .with_args(["add", "@pnpm.e2e/foo@100.1.0", "@pnpm.e2e/bar@100.1.0", "--lockfile-only"])
        .assert()
        .success();

    let manifest =
        PackageManifest::from_path(workspace.join("package.json")).expect("read package.json");
    let group_spec = |group: DependencyGroup, name: &str| {
        manifest
            .dependencies([group])
            .find(|(dep, _)| *dep == name)
            .map(|(_, spec)| spec.to_string())
    };
    assert_eq!(group_spec(DependencyGroup::Dev, "@pnpm.e2e/foo").as_deref(), Some("^100.1.0"));
    assert_eq!(group_spec(DependencyGroup::Optional, "@pnpm.e2e/bar").as_deref(), Some("^100.1.0"));
    assert_eq!(group_spec(DependencyGroup::Prod, "@pnpm.e2e/foo"), None);
    assert_eq!(group_spec(DependencyGroup::Prod, "@pnpm.e2e/bar"), None);

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

/// Run `pnpm add @pnpm.e2e/hello-world-js-bin` in a workspace whose
/// `pnpm-workspace.yaml` sets `savePrefix: '~'` and `savePeer: true`.
fn add_with_save_settings(args: &[&str]) -> (TempDir, PathBuf, TestRegistry) {
    add_with_settings("savePrefix: '~'\nsavePeer: true\n", args)
}

fn add_with_settings(settings: &str, args: &[&str]) -> (TempDir, PathBuf, TestRegistry) {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut workspace_yaml =
        std::fs::read_to_string(&workspace_yaml_path).expect("read pnpm-workspace.yaml");
    if !workspace_yaml.ends_with('\n') {
        workspace_yaml.push('\n');
    }
    workspace_yaml.push_str(settings);
    std::fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    pacquet.with_args(args).with_arg("@pnpm.e2e/hello-world-js-bin").assert().success();

    (root, workspace, mock_instance)
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

/// `saveWorkspaceProtocol` decides what `pnpm add <pkg>@workspace:…`
/// writes back. The rolling default drops the version so the range
/// never has to be rewritten when the local package is bumped; `true`
/// pins the workspace package's *actual* version (not the one the user
/// typed); `false` still honors an explicit `workspace:` request.
///
/// Verified against the TypeScript CLI for every row.
#[test]
fn save_workspace_protocol_decides_the_saved_workspace_range() {
    const LIB: &str = "@pnpm.e2e/ws-lib";
    let cases = [
        (None, "workspace:^1.2.3", "1.2.3", true, "workspace:^"),
        (None, "workspace:^1.2.3", "1.2.3", false, "workspace:^"),
        (None, "workspace:~1.2.3", "1.2.3", true, "workspace:~"),
        (None, "workspace:1.2.3", "1.2.3", true, "workspace:*"),
        (None, "workspace:*", "1.2.3", true, "workspace:*"),
        (Some("true"), "workspace:^1.2.3", "1.2.3", true, "workspace:^1.2.3"),
        // The typed `~` loses to the default `^`: the pinned form reads
        // its operator off the previous entry, and there is none here.
        (Some("true"), "workspace:~1.2.3", "1.2.3", true, "workspace:^1.2.3"),
        // The local version wins over the typed range.
        (Some("true"), "workspace:^1.0.0", "2.5.0", true, "workspace:^2.5.0"),
        // A range over a prerelease would not match it, so it is exact.
        (Some("true"), "workspace:^1.0.0", "2.0.0-beta.1", true, "workspace:2.0.0-beta.1"),
        (Some("false"), "workspace:^1.2.3", "1.2.3", true, "workspace:^1.2.3"),
    ];

    for (setting, requested, lib_version, link_workspace_packages, expected) in cases {
        let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
        let protocol_line = setting
            .map(|setting| format!("saveWorkspaceProtocol: {setting}\n"))
            .unwrap_or_default();
        let yaml = format!(
            "{HERMETIC_STORE_YAML}packages:\n  - packages/*\nlinkWorkspacePackages: {link_workspace_packages}\n{protocol_line}",
        );
        std::fs::write(workspace.join("pnpm-workspace.yaml"), yaml).expect("write workspace yaml");
        write_json(&workspace.join("package.json"), &serde_json::json!({ "name": "root" }));
        for (dir, manifest) in [
            ("lib", serde_json::json!({ "name": LIB, "version": lib_version })),
            ("app", serde_json::json!({ "name": "ws-app", "version": "1.0.0" })),
        ] {
            let package_dir = workspace.join("packages").join(dir);
            std::fs::create_dir_all(&package_dir).expect("create package dir");
            write_json(&package_dir.join("package.json"), &manifest);
        }

        let app_dir = workspace.join("packages/app");
        Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(&app_dir)
            .with_args(["add", &format!("{LIB}@{requested}"), "--lockfile-only"])
            .assert()
            .success();

        let saved = PackageManifest::from_path(app_dir.join("package.json"))
            .expect("read app manifest")
            .dependencies([DependencyGroup::Prod])
            .find(|(name, _)| *name == LIB)
            .map(|(_, spec)| spec.to_string());
        eprintln!("setting={setting:?} requested={requested} local={lib_version} -> {saved:?}");
        assert_eq!(saved.as_deref(), Some(expected));

        drop(root);
    }
}

#[test]
fn a_bare_workspace_add_uses_the_local_package_and_saved_protocol_setting() {
    const LIB: &str = "@pnpm.e2e/ws-bare";
    for (setting, expected) in
        [(None, "workspace:^"), (Some("true"), "workspace:^1.2.3"), (Some("false"), "^1.2.3")]
    {
        let (root, app_dir) =
            workspace_with_lib(setting, &[(LIB, "1.2.3")], "packages/app/package.json");
        add_in(&app_dir, LIB);

        assert_eq!(saved_spec(&app_dir, LIB).as_deref(), Some(expected));
        drop(root);
    }
}

/// `workspace:<target>@<range>` installs `<target>` under the name the
/// selector gave, so the saved specifier has to keep naming the target —
/// dropping it would leave a `workspace:` entry pointing at the install
/// name, which resolves to nothing.
///
/// Verified against the TypeScript CLI.
#[test]
fn an_aliased_workspace_add_keeps_naming_its_target() {
    const TARGET: &str = "@pnpm.e2e/ws-target";
    for (setting, expected) in [
        (None, "workspace:@pnpm.e2e/ws-target@^"),
        (Some("true"), "workspace:@pnpm.e2e/ws-target@^1.2.3"),
    ] {
        let (root, app_dir) =
            workspace_with_lib(setting, &[(TARGET, "1.2.3")], "packages/app/package.json");
        add_in(&app_dir, &format!("myalias@workspace:{TARGET}@^1.0.0"));

        assert_eq!(saved_spec(&app_dir, "myalias").as_deref(), Some(expected));
        drop(root);
    }
}

/// The pinned form picks the workspace version by semver, not by string
/// order: a workspace holding both `9.0.0` and `10.0.0` must pin to
/// `10.0.0`, which sorts *before* `9.0.0` lexicographically.
///
/// Verified against the TypeScript CLI.
#[test]
fn the_pinned_form_picks_the_highest_workspace_version_by_semver() {
    const LIB: &str = "@pnpm.e2e/ws-multi";
    let (root, app_dir) = workspace_with_lib(
        Some("true"),
        &[(LIB, "9.0.0"), (LIB, "10.0.0")],
        "packages/app/package.json",
    );
    add_in(&app_dir, &format!("{LIB}@workspace:*"));

    assert_eq!(saved_spec(&app_dir, LIB).as_deref(), Some("workspace:^10.0.0"));
    drop(root);
}

/// The setting is readable from `PNPM_CONFIG_SAVE_WORKSPACE_PROTOCOL`
/// too, not only `pnpm-workspace.yaml` — pnpm exposes every setting
/// through its env pass.
#[test]
fn the_env_var_drives_the_saved_workspace_range() {
    const LIB: &str = "@pnpm.e2e/ws-env";
    for (value, expected) in
        [("true", "workspace:^1.2.3"), ("rolling", "workspace:^"), ("false", "workspace:^1.2.3")]
    {
        let (root, app_dir) =
            workspace_with_lib(None, &[(LIB, "1.2.3")], "packages/app/package.json");
        Command::cargo_bin("pnpm")
            .expect("find the pnpm binary")
            .with_current_dir(&app_dir)
            .with_args(["add", &format!("{LIB}@workspace:^1.0.0"), "--lockfile-only"])
            .env("PNPM_CONFIG_SAVE_WORKSPACE_PROTOCOL", value)
            .assert()
            .success();

        eprintln!("PNPM_CONFIG_SAVE_WORKSPACE_PROTOCOL={value} -> {:?}", saved_spec(&app_dir, LIB));
        assert_eq!(saved_spec(&app_dir, LIB).as_deref(), Some(expected));
        drop(root);
    }
}

/// Scaffold a workspace whose `packages/*` hold `libs` (one directory
/// per entry, so the same name may appear at several versions) plus an
/// `app` member. Returns the temp root and the app's directory.
fn workspace_with_lib(
    save_workspace_protocol: Option<&str>,
    libs: &[(&str, &str)],
    app_manifest_path: &str,
) -> (TempDir, PathBuf) {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    let protocol_line = save_workspace_protocol
        .map(|setting| format!("saveWorkspaceProtocol: {setting}\n"))
        .unwrap_or_default();
    std::fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("{HERMETIC_STORE_YAML}packages:\n  - packages/*\nlinkWorkspacePackages: true\n{protocol_line}"),
    )
    .expect("write workspace yaml");
    write_json(&workspace.join("package.json"), &serde_json::json!({ "name": "root" }));
    for (index, (name, version)) in libs.iter().enumerate() {
        let package_dir = workspace.join("packages").join(format!("lib{index}"));
        std::fs::create_dir_all(&package_dir).expect("create lib dir");
        write_json(
            &package_dir.join("package.json"),
            &serde_json::json!({ "name": name, "version": version }),
        );
    }
    let app_dir = workspace.join(app_manifest_path).parent().expect("app dir").to_path_buf();
    std::fs::create_dir_all(&app_dir).expect("create app dir");
    write_json(
        &app_dir.join("package.json"),
        &serde_json::json!({ "name": "ws-app", "version": "1.0.0" }),
    );
    (root, app_dir)
}

fn add_in(dir: &Path, selector: &str) {
    Command::cargo_bin("pnpm")
        .expect("find the pnpm binary")
        .with_current_dir(dir)
        .with_args(["add", selector, "--lockfile-only"])
        .assert()
        .success();
}

fn saved_spec(dir: &Path, name: &str) -> Option<String> {
    PackageManifest::from_path(dir.join("package.json"))
        .expect("read manifest")
        .dependencies([DependencyGroup::Prod])
        .find(|(dep_name, _)| *dep_name == name)
        .map(|(_, spec)| spec.to_string())
}

/// Store and cache directories pinned inside the test's own temp root,
/// and the global virtual store pinned off.
///
/// `CommandTempCwd::add_mocked_registry` writes these for tests that
/// need a registry. The workspace-protocol tests resolve only local
/// packages, so they skip the registry — but they still have to pin the
/// directories, or they race every other test over the developer's real
/// store.
const HERMETIC_STORE_YAML: &str =
    "storeDir: ../pacquet-store\ncacheDir: ../pacquet-cache\nenableGlobalVirtualStore: false\n";

fn write_json(path: &Path, value: &serde_json::Value) {
    std::fs::write(path, value.to_string()).expect("write manifest");
}
