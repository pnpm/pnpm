//! End-to-end tests for the `packageProvider` setting. Mirrors
//! `pnpm11/installing/deps-installer/test/packageProvider.ts` — the
//! fake provider implements the same protocol-v1 contract as the
//! nix-provider: it materializes every requested depPath as a
//! directory whose `node_modules` holds the package next to symlinks
//! to its dependencies, and records the request for assertions.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const FAKE_PROVIDER: &str = r"#!/usr/bin/env node
const fs = require('fs')
const path = require('path')
let input = ''
process.stdin.on('data', (chunk) => { input += chunk })
process.stdin.on('end', () => {
  const request = JSON.parse(input)
  fs.writeFileSync(path.join(__dirname, 'request.json'), JSON.stringify(request))
  const subdir = (depPath) => depPath.replace(/[^A-Za-z0-9._@-]/g, '+')
  const setTreeMode = (root, mode) => {
    // Walk without following symlinks: chmod on a symlink would affect
    // the (possibly shared) target tree.
    const stack = [root]
    while (stack.length > 0) {
      const dir = stack.pop()
      for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        const entryPath = path.join(dir, entry.name)
        if (entry.isSymbolicLink()) continue
        if (entry.isDirectory()) stack.push(entryPath)
        fs.chmodSync(entryPath, entry.isDirectory() ? mode | 0o111 : mode)
      }
    }
    fs.chmodSync(root, mode | 0o111)
  }
  const paths = {}
  const skipped = []
  for (const [depPath, node] of Object.entries(request.nodes)) {
    // simulate an optional package whose build fails
    if (node.optional === true) {
      skipped.push(depPath)
      continue
    }
    const dir = path.join(__dirname, 'store', subdir(depPath))
    // a repeat request may reuse a tree this provider froze earlier
    if (fs.existsSync(dir)) setTreeMode(dir, 0o644)
    fs.mkdirSync(path.join(dir, 'node_modules', node.name), { recursive: true })
    fs.writeFileSync(path.join(dir, 'node_modules', node.name, 'package.json'), JSON.stringify({ name: node.name, version: node.version }))
    paths[depPath] = dir
  }
  for (const [depPath, node] of Object.entries(request.nodes)) {
    if (paths[depPath] == null) continue
    for (const [alias, dep] of Object.entries(node.deps)) {
      if (paths[dep.depPath] == null) continue
      const link = path.join(paths[depPath], 'node_modules', alias)
      fs.mkdirSync(path.dirname(link), { recursive: true })
      try {
        fs.symlinkSync(path.join(paths[dep.depPath], 'node_modules', dep.name), link)
      } catch (err) {
        if (err.code !== 'EEXIST') throw err
      }
    }
  }
  // Freeze the returned trees: a real provider (the Nix store) hands
  // out read-only directories, so installer writes must fail here too.
  for (const dir of Object.values(paths)) setTreeMode(dir, 0o444)
  process.stdout.write(JSON.stringify({ protocol: 1, paths, skipped }))
})
";

const IS_POSITIVE_PATCH: &str = include_str!(
    "../../../../pnpm11/installing/deps-installer/test/fixtures/patch-pkg/is-positive@1.0.0.patch"
);

fn pacquet(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

/// Write the fake provider script into `<root>/provider/provider.js`.
/// Returns `(provider_bin, provider_dir)` — the provider records the
/// request at `<provider_dir>/request.json` and materializes packages
/// under `<provider_dir>/store/`.
fn write_provider(root: &Path, script: &str) -> (PathBuf, PathBuf) {
    let provider_dir = root.join("provider");
    fs::create_dir_all(&provider_dir).expect("create provider dir");
    let provider_bin = provider_dir.join("provider.js");
    fs::write(&provider_bin, script).expect("write provider script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&provider_bin, fs::Permissions::from_mode(0o755))
            .expect("mark provider script executable");
    }
    (provider_bin, provider_dir)
}

fn append_workspace_yaml(workspace: &Path, extra: &str) {
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let mut yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    if !yaml.ends_with('\n') {
        yaml.push('\n');
    }
    yaml.push_str(extra);
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
}

fn set_package_provider(workspace: &Path, provider_bin: &Path) {
    append_workspace_yaml(
        workspace,
        &format!("packageProvider: '{}'\n", provider_bin.to_string_lossy()),
    );
}

fn read_request(provider_dir: &Path) -> Value {
    let text = fs::read_to_string(provider_dir.join("request.json")).expect("read request.json");
    serde_json::from_str(&text).expect("parse request.json")
}

/// Canonicalized realpath of an installed `node_modules` entry.
fn realpath(workspace: &Path, entry: &str) -> PathBuf {
    fs::canonicalize(workspace.join("node_modules").join(entry))
        .unwrap_or_else(|error| panic!("canonicalize node_modules/{entry}: {error}"))
}

fn assert_in_provider_store(dir: &Path, provider_dir: &Path) {
    let store = fs::canonicalize(provider_dir.join("store")).expect("canonicalize provider store");
    assert!(dir.starts_with(&store), "{} must be inside {}", dir.display(), store.display());
}

/// Entries of the project's virtual store dir (`node_modules/.pnpm`),
/// which must never gain a package slot when a provider materializes
/// the install. `lock.yaml` (the current-lockfile bookkeeping file) is
/// still expected.
fn virtual_store_entries(workspace: &Path) -> Vec<String> {
    let virtual_store = workspace.join("node_modules").join(".pnpm");
    if !virtual_store.exists() {
        return Vec::new();
    }
    fs::read_dir(virtual_store)
        .expect("read node_modules/.pnpm")
        .map(|entry| entry.expect("read dir entry").file_name().to_string_lossy().into_owned())
        .collect()
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "the fake provider is a Unix shebang script")]
fn packages_are_materialized_through_the_provider_and_symlinked() {
    let CommandTempCwd { pacquet: install, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let (provider_bin, provider_dir) = write_provider(root.path(), FAKE_PROVIDER);
    set_package_provider(&workspace, &provider_bin);

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    install.with_arg("install").assert().success();

    // The direct dependency resolves into the provider's store,
    // through an absolute symlink (provider directories outlive the
    // project location).
    let direct_link =
        fs::read_link(workspace.join("node_modules").join("@pnpm.e2e").join("pkg-with-1-dep"))
            .expect("read direct dep symlink");
    assert!(direct_link.is_absolute(), "direct dep link: {}", direct_link.display());
    let real_dir = realpath(&workspace, "@pnpm.e2e/pkg-with-1-dep");
    assert_in_provider_store(&real_dir, &provider_dir);

    // The transitive dependency is reachable as a sibling inside the
    // provider's store.
    let transitive =
        fs::canonicalize(real_dir.join("../..").join("@pnpm.e2e").join("dep-of-pkg-with-1-dep"))
            .expect("canonicalize transitive dep");
    assert_in_provider_store(&transitive, &provider_dir);

    // Hoisted links are absolute as well.
    let hoisted_link = fs::read_link(
        workspace
            .join("node_modules/.pnpm/node_modules")
            .join("@pnpm.e2e")
            .join("dep-of-pkg-with-1-dep"),
    )
    .expect("read hoisted dep symlink");
    assert!(hoisted_link.is_absolute(), "hoisted link: {}", hoisted_link.display());

    // The provider received a closed graph with resolutions and the gc
    // root location.
    let request = read_request(&provider_dir);
    dbg!(&request);
    assert_eq!(request["protocol"], 1);
    let gc_root = Path::new(request["gcRootDir"].as_str().expect("gcRootDir string"));
    assert!(gc_root.ends_with("node_modules/.pnpm-nix"), "gcRootDir: {}", gc_root.display());
    let workspace_from_gc_root =
        gc_root.parent().and_then(Path::parent).expect("gcRootDir has two ancestors");
    assert_eq!(
        fs::canonicalize(workspace_from_gc_root).expect("canonicalize gcRootDir workspace"),
        fs::canonicalize(&workspace).expect("canonicalize workspace"),
    );
    let nodes = request["nodes"].as_object().expect("nodes object");
    let direct_node = nodes
        .values()
        .find(|node| node["name"] == "@pnpm.e2e/pkg-with-1-dep")
        .expect("direct dep node");
    assert!(direct_node["tarball"].as_str().is_some_and(|url| !url.is_empty()));
    assert!(direct_node["integrity"].as_str().expect("integrity").starts_with("sha"));
    assert!(direct_node["engine"].as_str().expect("engine").contains(";node"));
    let dep_alias = &direct_node["deps"]["@pnpm.e2e/dep-of-pkg-with-1-dep"];
    let dep_path = dep_alias["depPath"].as_str().expect("dep depPath");
    assert_eq!(nodes[dep_path]["name"], "@pnpm.e2e/dep-of-pkg-with-1-dep");

    // Nothing was imported into the virtual store.
    let entries = virtual_store_entries(&workspace);
    dbg!(&entries);
    assert!(!entries.iter().any(|entry| entry.contains("pkg-with-1-dep")));

    // The lockfile is written as usual.
    let lockfile =
        fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read pnpm-lock.yaml");
    assert!(lockfile.contains("'@pnpm.e2e/pkg-with-1-dep'"), "lockfile:\n{lockfile}");

    drop((root, mock_instance));
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "the fake provider is a Unix shebang script")]
fn a_repeat_install_keeps_resolving_from_the_provider() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let (provider_bin, provider_dir) = write_provider(root.path(), FAKE_PROVIDER);
    set_package_provider(&workspace, &provider_bin);
    fs::write(workspace.join("package.json"), "{}").expect("write package.json");

    pacquet(&workspace).with_args(["add", "@pnpm.e2e/foo@100.0.0"]).assert().success();
    pacquet(&workspace).with_args(["add", "@pnpm.e2e/bar@100.0.0"]).assert().success();

    for entry in ["@pnpm.e2e/foo", "@pnpm.e2e/bar"] {
        let real_dir = realpath(&workspace, entry);
        assert_in_provider_store(&real_dir, &provider_dir);
    }

    drop((root, mock_instance));
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "the fake provider is a Unix shebang script")]
fn patched_dependencies_are_sent_with_their_patch_content() {
    let CommandTempCwd { pacquet: install, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let (provider_bin, provider_dir) = write_provider(root.path(), FAKE_PROVIDER);
    set_package_provider(&workspace, &provider_bin);

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "is-positive": "1.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::create_dir_all(workspace.join("patches")).expect("create patches dir");
    fs::write(workspace.join("patches").join("is-positive@1.0.0.patch"), IS_POSITIVE_PATCH)
        .expect("write patch file");
    append_workspace_yaml(
        &workspace,
        "patchedDependencies:\n  is-positive@1.0.0: patches/is-positive@1.0.0.patch\n",
    );

    install.with_arg("install").assert().success();

    let request = read_request(&provider_dir);
    dbg!(&request);
    let node = request["nodes"]
        .as_object()
        .expect("nodes object")
        .values()
        .find(|node| node["name"] == "is-positive")
        .expect("is-positive node");
    assert!(node["patch"]["content"].as_str().expect("patch content").contains("patched"));
    assert!(node["patch"]["hash"].as_str().is_some_and(|hash| !hash.is_empty()));

    drop((root, mock_instance));
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "the fake provider is a Unix shebang script")]
fn a_frozen_install_materializes_through_the_provider() {
    let CommandTempCwd { pacquet: install, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let (provider_bin, provider_dir) = write_provider(root.path(), FAKE_PROVIDER);
    set_package_provider(&workspace, &provider_bin);

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/pkg-with-1-dep": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    install.with_arg("install").assert().success();
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    pacquet(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    let direct_link =
        fs::read_link(workspace.join("node_modules").join("@pnpm.e2e").join("pkg-with-1-dep"))
            .expect("read direct dep symlink");
    assert!(direct_link.is_absolute(), "direct dep link: {}", direct_link.display());
    let real_dir = realpath(&workspace, "@pnpm.e2e/pkg-with-1-dep");
    assert_in_provider_store(&real_dir, &provider_dir);
    let transitive =
        fs::canonicalize(real_dir.join("../..").join("@pnpm.e2e").join("dep-of-pkg-with-1-dep"))
            .expect("canonicalize transitive dep");
    assert_in_provider_store(&transitive, &provider_dir);
    let entries = virtual_store_entries(&workspace);
    dbg!(&entries);
    assert!(!entries.iter().any(|entry| entry.contains("pkg-with-1-dep")));

    drop((root, mock_instance));
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "the fake provider is a Unix shebang script")]
fn local_directory_dependencies_are_sent_as_absolute_directories() {
    let CommandTempCwd { pacquet: install, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let (provider_bin, provider_dir) = write_provider(root.path(), FAKE_PROVIDER);
    set_package_provider(&workspace, &provider_bin);

    fs::create_dir_all(workspace.join("local-pkg")).expect("create local-pkg");
    fs::write(
        workspace.join("local-pkg").join("package.json"),
        serde_json::json!({ "name": "local-pkg", "version": "1.0.0" }).to_string(),
    )
    .expect("write local-pkg package.json");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "local-pkg": "file:./local-pkg" },
        })
        .to_string(),
    )
    .expect("write package.json");

    install.with_arg("install").assert().success();

    let request = read_request(&provider_dir);
    dbg!(&request);
    let node = request["nodes"]
        .as_object()
        .expect("nodes object")
        .values()
        .find(|node| node["name"] == "local-pkg")
        .expect("local-pkg node");
    let directory = Path::new(node["directory"].as_str().expect("directory string"));
    assert!(directory.is_absolute(), "directory must be absolute: {}", directory.display());
    assert_eq!(
        fs::canonicalize(directory).expect("canonicalize sent directory"),
        fs::canonicalize(workspace.join("local-pkg")).expect("canonicalize local-pkg"),
    );
    assert!(node.get("tarball").is_none());
    let real_dir = realpath(&workspace, "local-pkg");
    assert_in_provider_store(&real_dir, &provider_dir);

    drop((root, mock_instance));
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "the fake provider is a Unix shebang script")]
fn optional_packages_the_provider_cannot_build_are_skipped() {
    let CommandTempCwd { pacquet: install, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let (provider_bin, provider_dir) = write_provider(root.path(), FAKE_PROVIDER);
    set_package_provider(&workspace, &provider_bin);

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/foo": "100.0.0" },
            "optionalDependencies": { "@pnpm.e2e/bar": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    install.with_arg("install").assert().success();

    assert!(workspace.join("node_modules").join("@pnpm.e2e").join("foo").exists());
    assert!(!workspace.join("node_modules").join("@pnpm.e2e").join("bar").exists());

    let request = read_request(&provider_dir);
    dbg!(&request);
    let bar_node = request["nodes"]
        .as_object()
        .expect("nodes object")
        .values()
        .find(|node| node["name"] == "@pnpm.e2e/bar")
        .expect("optional dep node");
    assert_eq!(bar_node["optional"], true);

    // The skip is recorded like an installability skip, so a later
    // install seeds it from `.modules.yaml`.
    let modules_yaml = fs::read_to_string(workspace.join("node_modules").join(".modules.yaml"))
        .expect("read .modules.yaml");
    eprintln!("MODULES.YAML:\n{modules_yaml}\n");
    assert!(modules_yaml.contains("@pnpm.e2e/bar@100.0.0"));

    drop((root, mock_instance));
}

#[test]
#[cfg_attr(target_os = "windows", ignore = "the fake provider is a Unix shebang script")]
fn the_install_aborts_when_the_provider_fails() {
    let CommandTempCwd { pacquet: install, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let (provider_bin, _provider_dir) =
        write_provider(root.path(), "#!/usr/bin/env node\nprocess.exit(1)\n");
    set_package_provider(&workspace, &provider_bin);

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/foo": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    let output = install.with_arg("install").output().expect("spawn pacquet install");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    eprintln!("STDERR:\n{stderr}\n");
    assert!(!output.status.success(), "the install must abort when the provider fails");
    assert!(stderr.contains("ERR_PNPM_PACKAGE_PROVIDER_FAILED"));
    // miette wraps the message, so match the two halves separately.
    assert!(stderr.contains("The package provider at"));
    assert!(stderr.contains("with code 1"));

    drop((root, mock_instance));
}

#[test]
fn package_provider_requires_the_isolated_node_linker() {
    let CommandTempCwd { pacquet: install, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    append_workspace_yaml(
        &workspace,
        "packageProvider: /nonexistent-provider\nnodeLinker: hoisted\n",
    );

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/foo": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    let output = install.with_arg("install").output().expect("spawn pacquet install");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    eprintln!("STDERR:\n{stderr}\n");
    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_CONFIG_CONFLICT_PACKAGE_PROVIDER_NODE_LINKER"));
    assert!(stderr.contains("packageProvider requires node-linker=isolated"));

    drop((root, mock_instance));
}

#[test]
fn package_provider_conflicts_with_the_global_virtual_store() {
    let CommandTempCwd { pacquet: install, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let yaml_path = workspace.join("pnpm-workspace.yaml");
    let yaml = fs::read_to_string(&yaml_path).expect("read pnpm-workspace.yaml");
    let yaml = yaml.replace("enableGlobalVirtualStore: false", "enableGlobalVirtualStore: true");
    fs::write(&yaml_path, yaml).expect("write pnpm-workspace.yaml");
    append_workspace_yaml(&workspace, "packageProvider: /nonexistent-provider\n");

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({
            "dependencies": { "@pnpm.e2e/foo": "100.0.0" },
        })
        .to_string(),
    )
    .expect("write package.json");

    let output = install.with_arg("install").output().expect("spawn pacquet install");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    eprintln!("STDERR:\n{stderr}\n");
    assert!(!output.status.success());
    assert!(stderr.contains("ERR_PNPM_CONFIG_CONFLICT_PACKAGE_PROVIDER_GLOBAL_VIRTUAL_STORE"));

    drop((root, mock_instance));
}
