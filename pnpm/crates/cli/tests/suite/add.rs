use crate::_utils;

use _utils::{
    bravo_dep_mature_up_to_1_0_1_minimum_release_age, read_current_lockfile,
    set_minimum_release_age,
};
use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pipe_trait::Pipe;
use pnpm_lockfile::{Lockfile, PkgName};
use pnpm_package_manifest::{DependencyGroup, PackageManifest};
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fs::{get_all_folders, get_filenames_in_folder},
    registry::TestRegistry,
};
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

fn cache_foo_index(cache_dir: &Path) {
    let index_dir = cache_dir.join("v11/cargo-index/crates-io/3/f");
    std::fs::create_dir_all(&index_dir).expect("create sparse-index cache");
    std::fs::write(
        index_dir.join("foo"),
        r#"{"name":"foo","vers":"1.0.0","deps":[],"cksum":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","features":{},"yanked":false}"#,
    )
    .expect("cache sparse-index entry");
}

fn cargo_add_project() -> (TempDir, PathBuf) {
    let root = TempDir::new().expect("create Cargo add project");
    let cache_dir = root.path().join("cache");
    cache_foo_index(&cache_dir);
    std::fs::create_dir(root.path().join("src")).expect("create Cargo source directory");
    std::fs::write(root.path().join("src/lib.rs"), "").expect("write Cargo source");
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )
    .expect("write Cargo manifest");
    std::fs::write(
        root.path().join("Cargo.lock"),
        "# stale lockfile that pnpm add must refresh\nversion = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )
    .expect("write stale Cargo lockfile");
    std::fs::write(root.path().join("pnpm-workspace.yaml"), "cargo:\n  enabled: true\n")
        .expect("enable Cargo dependency management");
    (root, cache_dir)
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

/// `--allow-build=!<pkg>` denies the package's build: `allowBuilds` records
/// `<pkg>: false` and the install script does not run.
#[test]
fn add_denies_a_build_with_the_negation_prefix() {
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let registry = mock_instance.url();

    pacquet
        .with_args([
            "add",
            "@pnpm.e2e/pre-and-postinstall-scripts-example@1.0.0",
            "--allow-build=!@pnpm.e2e/pre-and-postinstall-scripts-example",
        ])
        .with_arg(format!("--registry={registry}"))
        .assert()
        .success();

    let pkg_dir = workspace.join(
        "node_modules/.pnpm/@pnpm.e2e+pre-and-postinstall-scripts-example@1.0.0\
         /node_modules/@pnpm.e2e/pre-and-postinstall-scripts-example",
    );
    assert!(
        !pkg_dir.join("generated-by-postinstall.js").exists(),
        "a denied package must not run its postinstall",
    );

    let yaml = std::fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .expect("pnpm-workspace.yaml present");
    assert!(
        yaml.contains("'@pnpm.e2e/pre-and-postinstall-scripts-example': false"),
        "the denial should be persisted, got:\n{yaml}",
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

/// Naming a package manager records which one the project uses instead of
/// installing it — but an alias only borrows the name, so it is an
/// ordinary dependency and must still be installed.
#[test]
fn add_aliasing_a_package_manager_name_installs_the_aliased_package() {
    let (root, dir, anchor) = exec_pacquet_in_temp_cwd([
        "add",
        "yarn@npm:@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0",
        "--lockfile-only",
    ]);
    assert_eq!(prod_spec(&dir, "yarn"), "npm:@pnpm.e2e/dep-of-pkg-with-1-dep@^100.0.0");
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("package.json")).unwrap()).unwrap();
    assert_eq!(manifest.get("devEngines"), None, "{manifest}");
    drop((root, anchor)); // cleanup
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

fn linking_settings(save_workspace_protocol: Option<&str>) -> String {
    let protocol_line = save_workspace_protocol
        .map(|setting| format!("saveWorkspaceProtocol: {setting}\n"))
        .unwrap_or_default();
    format!("linkWorkspacePackages: true\n{protocol_line}")
}

/// Scaffold a workspace whose `packages/*` hold `libs` (one directory
/// per entry, so the same name may appear at several versions) plus an
/// `app` member, with `settings` appended to its `pnpm-workspace.yaml`.
/// Returns the temp root and the app's directory.
fn workspace_with_lib(
    settings: &str,
    libs: &[(&str, &str)],
    app_manifest_path: &str,
) -> (TempDir, PathBuf) {
    let CommandTempCwd { root, workspace, .. } = CommandTempCwd::init();
    std::fs::write(
        workspace.join("pnpm-workspace.yaml"),
        format!("{HERMETIC_STORE_YAML}packages:\n  - packages/*\n{settings}"),
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

/// An alias-less selector — the whole argument is the specifier, with no
/// `<name>@` in front — names a package whose name lives only in its own
/// manifest, so `add` reads it from the directory, the archive, or the
/// checkout rather than from the selector.
///
/// Covers <https://github.com/pnpm/pnpm/issues/14437>.
mod aliasless_selectors;

/// Covers <https://github.com/pnpm/pnpm/issues/14602>.
mod workspace_flag;

mod cargo;

mod version_specifiers;

mod workspace;
