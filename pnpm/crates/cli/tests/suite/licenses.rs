use crate::_utils;

use _utils::{enable_gvs_in_workspace_yaml, pacquet_in};
use assert_cmd::prelude::*;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[test]
fn licenses_normalizes_metadata_and_orders_groups_by_package() {
    let workspace = tempfile::tempdir().expect("create workspace");
    fs::write(
        workspace.path().join("package.json"),
        json!({
            "dependencies": {
                "a-b": "1.0.0",
                "a_b": "1.0.0",
                "alpha": "1.0.0",
                "zeta": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.path().join("pnpm-lock.yaml"),
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      a-b:
        specifier: 1.0.0
        version: 1.0.0
      a_b:
        specifier: 1.0.0
        version: 1.0.0
      alpha:
        specifier: 1.0.0
        version: 1.0.0
      zeta:
        specifier: 1.0.0
        version: 1.0.0
packages:
  a-b@1.0.0:
    resolution: {integrity: sha512-a-b}
  a_b@1.0.0:
    resolution: {integrity: sha512-a_b}
  alpha@1.0.0:
    resolution: {integrity: sha512-alpha}
  zeta@1.0.0:
    resolution: {integrity: sha512-zeta}
snapshots:
  a-b@1.0.0: {}
  a_b@1.0.0: {}
  alpha@1.0.0: {}
  zeta@1.0.0: {}
",
    )
    .expect("write lockfile");
    let virtual_store = workspace.path().join("node_modules/.pnpm");
    let a_dash_b_dir = virtual_store.join("a-b@1.0.0/node_modules/a-b");
    let a_underscore_b_dir = virtual_store.join("a_b@1.0.0/node_modules/a_b");
    let alpha_dir = virtual_store.join("alpha@1.0.0/node_modules/alpha");
    let zeta_dir = virtual_store.join("zeta@1.0.0/node_modules/zeta");
    fs::create_dir_all(&a_dash_b_dir).expect("create a-b directory");
    fs::create_dir_all(&a_underscore_b_dir).expect("create a_b directory");
    fs::create_dir_all(&alpha_dir).expect("create alpha directory");
    fs::create_dir_all(&zeta_dir).expect("create zeta directory");
    for (directory, name) in [(&a_dash_b_dir, "a-b"), (&a_underscore_b_dir, "a_b")] {
        fs::write(
            directory.join("package.json"),
            json!({
                "name": name,
                "version": "1.0.0",
                "license": "MIT",
            })
            .to_string(),
        )
        .expect("write collation fixture manifest");
    }
    fs::write(
        alpha_dir.join("package.json"),
        json!({
            "name": "alpha",
            "version": "1.0.0",
            "license": "Zlib",
            "author": "Alpha Team <alpha@example.com> (https://example.com/team)",
            "repository": "github:example/alpha",
        })
        .to_string(),
    )
    .expect("write alpha manifest");
    fs::write(
        zeta_dir.join("package.json"),
        json!({
            "name": "zeta",
            "version": "1.0.0",
            "license": "MIT",
        })
        .to_string(),
    )
    .expect("write zeta manifest");

    let output = pacquet_in(workspace.path())
        .args(["licenses", "list", "--json"])
        .output()
        .expect("run licenses");
    assert!(
        output.status.success(),
        "licenses should succeed: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("parse licenses JSON");
    assert_eq!(
        report
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["MIT", "Zlib"],
    );
    assert_eq!(report["Zlib"][0]["author"], "Alpha Team");
    assert_eq!(report["Zlib"][0]["homepage"], "https://github.com/example/alpha#readme");

    let output = pacquet_in(workspace.path())
        .args(["licenses", "list"])
        .output()
        .expect("run licenses");
    assert!(
        output.status.success(),
        "licenses should succeed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let table = String::from_utf8(output.stdout).expect("licenses table is UTF-8");
    assert!(
        table.find("a_b").expect("a_b row") < table.find("a-b").expect("a-b row"),
        "table should use JavaScript-compatible package collation:\n{table}",
    );
}

#[test]
fn licenses_reads_global_store_metadata_with_a_manifest_selected_runtime() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let config_home = root.path().join("config");
    fs::create_dir(&config_home).expect("create empty config home");

    enable_gvs_in_workspace_yaml(
        &workspace,
        "allowBuilds:\n  '@pnpm.e2e/install-script-example': true\n",
    );
    fs::write(
        workspace.join("package.json"),
        json!({
            "devEngines": {
                "runtime": {
                    "name": "node",
                    "version": "1",
                    "onFail": "ignore",
                },
            },
            "dependencies": {
                // This engine-constrained dependency makes installation resolve the GVS engine
                // from the manifest-selected runtime instead of deferring to the host Node.
                "@pnpm.e2e/for-legacy-node": "1.0.0",
                "@pnpm.e2e/install-script-example": "1.0.0",
                "@pnpm.e2e/legacy-license": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    let mut pacquet = pacquet;
    pacquet
        .env("XDG_CONFIG_HOME", &config_home)
        .arg("install")
        .assert()
        .success();

    for subcommand in ["list", "ls"] {
        let mut licenses_command = pacquet_in(&workspace);
        let output = licenses_command
            .env("XDG_CONFIG_HOME", &config_home)
            .args(["licenses", subcommand, "--json"])
            .output()
            .expect("spawn pacquet licenses");
        assert!(
            output.status.success(),
            "licenses {subcommand} should succeed: {}",
            String::from_utf8_lossy(&output.stderr),
        );

        let licenses: Value = serde_json::from_slice(&output.stdout).expect("parse licenses JSON");
        let packages = licenses["MIT"].as_array().expect("MIT license group");
        assert_eq!(
            packages
                .iter()
                .map(|package| package["name"].as_str().expect("package name"))
                .collect::<Vec<_>>(),
            [
                "@pnpm.e2e/for-legacy-node",
                "@pnpm.e2e/install-script-example",
                "@pnpm.e2e/legacy-license",
            ],
        );
        for package in packages {
            assert_eq!(package["versions"], json!(["1.0.0"]));
            assert!(
                package["paths"]
                    .as_array()
                    .expect("package paths")
                    .iter()
                    .all(|path| Path::new(path.as_str().expect("path string")).exists()),
                "reported package paths should exist: {}",
                package["paths"],
            );
        }
    }

    drop((root, mock_instance));
}

#[test]
fn licenses_lists_only_the_project_in_the_current_directory() {
    let workspace = tempfile::tempdir().expect("create workspace");
    fs::write(workspace.path().join("pnpm-workspace.yaml"), "packages:\n  - foo\n  - bar\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(workspace.path().join("package.json"), json!({ "private": true }).to_string())
        .expect("write root package.json");
    for (project, dependency) in [("foo", "alpha"), ("bar", "zeta")] {
        let project_dir = workspace.path().join(project);
        fs::create_dir_all(&project_dir).expect("create project directory");
        fs::write(
            project_dir.join("package.json"),
            json!({ "name": project, "dependencies": { dependency: "1.0.0" } }).to_string(),
        )
        .expect("write project package.json");
        let package_dir = workspace
            .path()
            .join(format!("node_modules/.pnpm/{dependency}@1.0.0/node_modules/{dependency}"));
        fs::create_dir_all(&package_dir).expect("create package directory");
        fs::write(
            package_dir.join("package.json"),
            json!({ "name": dependency, "version": "1.0.0", "license": "MIT" }).to_string(),
        )
        .expect("write package manifest");
    }
    fs::write(
        workspace.path().join("pnpm-lock.yaml"),
        r"
lockfileVersion: '9.0'
importers:
  .: {}
  foo:
    dependencies:
      alpha:
        specifier: 1.0.0
        version: 1.0.0
  bar:
    dependencies:
      zeta:
        specifier: 1.0.0
        version: 1.0.0
packages:
  alpha@1.0.0:
    resolution: {integrity: sha512-alpha}
  zeta@1.0.0:
    resolution: {integrity: sha512-zeta}
snapshots:
  alpha@1.0.0: {}
  zeta@1.0.0: {}
",
    )
    .expect("write lockfile");

    let listed_names = |args: &[&str]| -> Vec<String> {
        let output = pacquet_in(&workspace.path().join("bar"))
            .args(args)
            .output()
            .expect("run licenses");
        assert!(
            output.status.success(),
            "licenses should succeed: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        let report: Value = serde_json::from_slice(&output.stdout).expect("parse licenses JSON");
        report["MIT"]
            .as_array()
            .expect("MIT group")
            .iter()
            .map(|package| {
                package["name"]
                    .as_str()
                    .expect("package name")
                    .to_string()
            })
            .collect()
    };

    assert_eq!(listed_names(&["licenses", "list", "--json"]), ["zeta"]);
    assert_eq!(listed_names(&["--recursive", "licenses", "list", "--json"]), ["alpha", "zeta"]);
}

fn hoisted_project(recorded_dep_path: &str) -> tempfile::TempDir {
    let workspace = tempfile::tempdir().expect("create workspace");
    fs::write(
        workspace.path().join("package.json"),
        json!({ "dependencies": { "alpha": "1.0.0", "peer": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    fs::write(workspace.path().join("pnpm-workspace.yaml"), "nodeLinker: hoisted\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.path().join("pnpm-lock.yaml"),
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      alpha:
        specifier: 1.0.0
        version: 1.0.0(peer@1.0.0)
      peer:
        specifier: 1.0.0
        version: 1.0.0
packages:
  alpha@1.0.0:
    resolution: {integrity: sha512-alpha}
    peerDependencies:
      peer: '*'
  peer@1.0.0:
    resolution: {integrity: sha512-peer}
snapshots:
  alpha@1.0.0(peer@1.0.0):
    dependencies:
      peer: 1.0.0
  peer@1.0.0: {}
",
    )
    .expect("write lockfile");
    for name in ["alpha", "peer"] {
        let package_dir = workspace
            .path()
            .join("node_modules")
            .join(name);
        fs::create_dir_all(&package_dir).expect("create package directory");
        fs::write(
            package_dir.join("package.json"),
            json!({ "name": name, "version": "1.0.0", "license": "MIT" }).to_string(),
        )
        .expect("write package manifest");
    }
    fs::write(
        workspace.path().join("node_modules/.modules.yaml"),
        json!({
            "layoutVersion": 5,
            "nodeLinker": "hoisted",
            "hoistedLocations": {
                recorded_dep_path: ["node_modules/alpha"],
                "peer@1.0.0": ["node_modules/peer"],
            },
        })
        .to_string(),
    )
    .expect("write .modules.yaml");
    workspace
}

fn listed_paths(workspace: &tempfile::TempDir) -> Value {
    let output = pacquet_in(workspace.path())
        .args(["licenses", "list", "--json"])
        .output()
        .expect("run licenses");
    assert!(
        output.status.success(),
        "licenses should succeed: {}",
        String::from_utf8_lossy(&output.stderr),
    );
    let report: Value = serde_json::from_slice(&output.stdout).expect("parse licenses JSON");
    Value::Array(
        report["MIT"]
            .as_array()
            .expect("MIT group")
            .iter()
            .map(|package| package["paths"][0].clone())
            .collect(),
    )
}

#[test]
fn licenses_reads_packages_where_the_hoisted_linker_placed_them() {
    let workspace = hoisted_project("alpha@1.0.0(peer@1.0.0)");
    let modules_dir =
        dunce::canonicalize(workspace.path()).expect("canonicalize workspace").join("node_modules");
    assert_eq!(
        listed_paths(&workspace),
        json!([modules_dir.join("alpha"), modules_dir.join("peer")]),
    );
}

#[test]
fn licenses_reads_a_collapsed_peer_variant_where_the_hoisted_linker_placed_it() {
    let workspace = hoisted_project("alpha@1.0.0(peer@2.0.0)");
    let modules_dir =
        dunce::canonicalize(workspace.path()).expect("canonicalize workspace").join("node_modules");
    assert_eq!(
        listed_paths(&workspace),
        json!([modules_dir.join("alpha"), modules_dir.join("peer")]),
    );
}

#[test]
fn licenses_reads_the_lockfile_of_each_project_with_dedicated_lockfiles() {
    let workspace = tempfile::tempdir().expect("create workspace");
    fs::write(
        workspace.path().join("pnpm-workspace.yaml"),
        "packages:\n  - foo\n  - bar\nsharedWorkspaceLockfile: false\n",
    )
    .expect("write pnpm-workspace.yaml");
    fs::write(workspace.path().join("package.json"), json!({ "private": true }).to_string())
        .expect("write root package.json");
    for (project, dependency) in [("foo", "alpha"), ("bar", "zeta")] {
        let project_dir = workspace.path().join(project);
        fs::create_dir_all(&project_dir).expect("create project directory");
        fs::write(
            project_dir.join("package.json"),
            json!({ "name": project, "dependencies": { dependency: "1.0.0" } }).to_string(),
        )
        .expect("write project package.json");
        let package_dir = project_dir.join(format!(
            "node_modules/.pnpm/{dependency}@1.0.0/node_modules/{dependency}",
        ));
        fs::create_dir_all(&package_dir).expect("create package directory");
        fs::write(
            package_dir.join("package.json"),
            json!({ "name": dependency, "version": "1.0.0", "license": "MIT" }).to_string(),
        )
        .expect("write package manifest");
        fs::write(
            project_dir.join("pnpm-lock.yaml"),
            format!(
                "
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      {dependency}:
        specifier: 1.0.0
        version: 1.0.0
packages:
  {dependency}@1.0.0:
    resolution: {{integrity: sha512-{dependency}}}
snapshots:
  {dependency}@1.0.0: {{}}
",
            ),
        )
        .expect("write lockfile");
    }
    fs::write(
        workspace.path().join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\nimporters:\n  .: {}\n",
    )
    .expect("write root lockfile");

    let listed = |dir: &str, args: &[&str]| -> Vec<(String, String)> {
        let output = pacquet_in(&workspace.path().join(dir))
            .args(args)
            .output()
            .expect("run licenses");
        assert!(
            output.status.success(),
            "licenses should succeed: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        let report: Value = serde_json::from_slice(&output.stdout).expect("parse licenses JSON");
        report["MIT"]
            .as_array()
            .expect("MIT group")
            .iter()
            .map(|package| {
                let name = package["name"]
                    .as_str()
                    .expect("package name")
                    .to_string();
                let path = package["paths"][0]
                    .as_str()
                    .expect("package path")
                    .to_string();
                (name, path)
            })
            .collect()
    };
    let workspace_dir = dunce::canonicalize(workspace.path()).expect("canonicalize workspace");
    let installed_at = |project: &str, dependency: &str| {
        workspace_dir
            .join(project)
            .join("node_modules")
            .join(".pnpm")
            .join(format!("{dependency}@1.0.0"))
            .join("node_modules")
            .join(dependency)
            .to_string_lossy()
            .into_owned()
    };

    assert_eq!(
        listed("bar", &["licenses", "list", "--json"]),
        [("zeta".to_string(), installed_at("bar", "zeta"))],
    );
    assert_eq!(
        listed(".", &["--recursive", "licenses", "list", "--json"]),
        [
            ("alpha".to_string(), installed_at("foo", "alpha")),
            ("zeta".to_string(), installed_at("bar", "zeta")),
        ],
    );
}

#[test]
fn licenses_lists_all_valid_copies_of_a_collapsed_hoisted_variant() {
    let workspace = hoisted_project("alpha@1.0.0(peer@2.0.0)");
    let locations = ["node_modules/a/node_modules/alpha", "node_modules/b/node_modules/alias"];
    for location in locations {
        let package_dir = workspace.path().join(location);
        fs::create_dir_all(&package_dir).expect("create package directory");
        fs::write(
            package_dir.join("package.json"),
            json!({ "name": "alpha", "version": "1.0.0", "license": "MIT" }).to_string(),
        )
        .expect("write manifest");
    }
    fs::write(workspace.path().join("node_modules/.modules.yaml"), json!({
        "layoutVersion": 5,
        "nodeLinker": "hoisted",
        "hoistedLocations": {
            "alpha@1.0.0(peer@2.0.0)": ["../outside/alpha", locations[0], locations[1], locations[0]],
            "peer@1.0.0": ["node_modules/peer"],
        },
    }).to_string()).expect("write modules manifest");
    let output = pacquet_in(workspace.path())
        .args(["licenses", "list", "--json"])
        .output()
        .expect("run licenses");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report: Value = serde_json::from_slice(&output.stdout).expect("parse licenses JSON");
    let root = dunce::canonicalize(workspace.path()).expect("canonicalize workspace");
    let license_groups: Vec<_> = report
        .as_object()
        .expect("report object")
        .keys()
        .collect();
    assert_eq!(license_groups, ["MIT"], "the location outside the project must not be read");
    assert_eq!(report["MIT"][0]["name"], "alpha");
    assert_eq!(report["MIT"][0]["versions"], json!(["1.0.0"]));
    let expected_paths: Vec<_> = locations
        .iter()
        .map(|location| {
            dunce::canonicalize(root.join(location)).expect("canonicalize installed package")
        })
        .collect();
    assert_eq!(report["MIT"][0]["paths"], json!(expected_paths));
}

#[test]
fn licenses_lists_distinct_isolated_peer_installations_of_one_version() {
    let workspace = hoisted_project("alpha@1.0.0(peer@1.0.0)");
    fs::write(workspace.path().join("pnpm-workspace.yaml"), "nodeLinker: isolated\n")
        .expect("write workspace config");
    let lockfile_path = workspace.path().join("pnpm-lock.yaml");
    let lockfile = fs::read_to_string(&lockfile_path).expect("read lockfile")
        .replace("      peer:\n", "      alternate:\n        specifier: npm:alpha@1.0.0\n        version: alpha@1.0.0(peer@2.0.0)\n      peer:\n")
        .replace("  peer@1.0.0: {}", "  alpha@1.0.0(peer@2.0.0): {}\n  peer@1.0.0: {}");
    fs::write(lockfile_path, lockfile).expect("write lockfile");
    let locations = [
        "node_modules/.pnpm/alpha@1.0.0_peer@1.0.0/node_modules/alpha",
        "node_modules/.pnpm/alpha@1.0.0_peer@2.0.0/node_modules/alpha",
    ];
    for location in locations {
        let package_dir = workspace.path().join(location);
        fs::create_dir_all(&package_dir).expect("create package directory");
        fs::write(
            package_dir.join("package.json"),
            json!({ "name": "alpha", "version": "1.0.0", "license": "MIT" }).to_string(),
        )
        .expect("write manifest");
    }
    let output = pacquet_in(workspace.path())
        .args(["licenses", "list", "--json"])
        .output()
        .expect("run licenses");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report: Value = serde_json::from_slice(&output.stdout).expect("parse licenses JSON");
    let root = dunce::canonicalize(workspace.path()).expect("canonicalize workspace");
    assert_eq!(report["MIT"][0]["name"], "alpha");
    assert_eq!(report["MIT"][0]["versions"], json!(["1.0.0"]));
    let expected_paths: Vec<_> = locations
        .iter()
        .map(|location| {
            dunce::canonicalize(root.join(location)).expect("canonicalize installed package")
        })
        .collect();
    assert_eq!(report["MIT"][0]["paths"], json!(expected_paths));
}

#[test]
fn licenses_reports_distinct_paths_for_conflicting_versions_under_shamefully_hoist() {
    let workspace = tempfile::tempdir().expect("create workspace");
    fs::write(
        workspace.path().join("package.json"),
        json!({
            "dependencies": {
                "is-positive": "3.1.0",
                "wrapper": "1.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.path().join("pnpm-lock.yaml"),
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      is-positive:
        specifier: 3.1.0
        version: 3.1.0
      wrapper:
        specifier: 1.0.0
        version: 1.0.0
packages:
  is-positive@1.0.0:
    resolution: {integrity: sha512-is-positive-1}
  is-positive@3.1.0:
    resolution: {integrity: sha512-is-positive-3}
  wrapper@1.0.0:
    resolution: {integrity: sha512-wrapper}
snapshots:
  is-positive@1.0.0: {}
  is-positive@3.1.0: {}
  wrapper@1.0.0:
    dependencies:
      is-positive: 1.0.0
",
    )
    .expect("write lockfile");
    fs::write(workspace.path().join("pnpm-workspace.yaml"), "shamefullyHoist: true\n")
        .expect("write pnpm-workspace.yaml");

    let vs_v1 =
        workspace.path().join("node_modules/.pnpm/is-positive@1.0.0/node_modules/is-positive");
    let vs_v3 =
        workspace.path().join("node_modules/.pnpm/is-positive@3.1.0/node_modules/is-positive");
    fs::create_dir_all(&vs_v1).expect("create vs v1");
    fs::create_dir_all(&vs_v3).expect("create vs v3");
    fs::write(
        vs_v1.join("package.json"),
        json!({
            "name": "is-positive",
            "version": "1.0.0",
            "license": "MIT",
        })
        .to_string(),
    )
    .expect("write v1 manifest");
    fs::write(
        vs_v3.join("package.json"),
        json!({
            "name": "is-positive",
            "version": "3.1.0",
            "license": "MIT",
        })
        .to_string(),
    )
    .expect("write v3 manifest");

    let root_hoisted = workspace.path().join("node_modules/is-positive");
    pnpm_fs::symlink_dir(&vs_v3, &root_hoisted).expect("symlink v3 to root node_modules");

    fs::write(
        workspace.path().join("node_modules/.modules.yaml"),
        r"
shamefullyHoist: true
publicHoistPattern:
  - '*'
",
    )
    .expect("write .modules.yaml");

    let output = pacquet_in(workspace.path())
        .args(["licenses", "list", "--json"])
        .output()
        .expect("run licenses");
    assert!(
        output.status.success(),
        "licenses should succeed: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("parse licenses JSON");
    let mit_packages = report["MIT"].as_array().expect("MIT license group");
    let is_positive = mit_packages
        .iter()
        .find(|pkg| pkg["name"] == "is-positive")
        .expect("find is-positive");

    let versions = is_positive["versions"].as_array().expect("versions array");
    assert_eq!(versions.len(), 2);

    let paths = is_positive["paths"].as_array().expect("paths array");
    assert_eq!(paths.len(), 2);
    assert_ne!(paths[0], paths[1], "both versions must have distinct paths");

    let has_virtual_store = paths
        .iter()
        .any(|path| path.as_str().unwrap().contains(".pnpm"));
    let has_root_hoisted = paths
        .iter()
        .any(|path| !path.as_str().unwrap().contains(".pnpm"));
    assert!(has_virtual_store, "version 1.0.0 must point to virtual store path");
    assert!(has_root_hoisted, "version 3.1.0 must point to hoisted node_modules path");
}

#[test]
fn licenses_reports_hoisted_package_paths_under_hoisted_node_linker() {
    let workspace = tempfile::tempdir().expect("create workspace");
    fs::write(
        workspace.path().join("package.json"),
        json!({
            "dependencies": {
                "is-positive": "3.1.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");
    fs::write(
        workspace.path().join("pnpm-lock.yaml"),
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      is-positive:
        specifier: 3.1.0
        version: 3.1.0
packages:
  is-positive@3.1.0:
    resolution: {integrity: sha512-is-positive}
snapshots:
  is-positive@3.1.0: {}
",
    )
    .expect("write lockfile");
    fs::write(workspace.path().join("pnpm-workspace.yaml"), "nodeLinker: hoisted\n")
        .expect("write pnpm-workspace.yaml");

    let pkg_dir = workspace.path().join("node_modules/is-positive");
    fs::create_dir_all(&pkg_dir).expect("create is-positive directory");
    fs::write(
        pkg_dir.join("package.json"),
        json!({
            "name": "is-positive",
            "version": "3.1.0",
            "license": "MIT",
            "author": "Test Author",
        })
        .to_string(),
    )
    .expect("write is-positive manifest");

    fs::write(
        workspace.path().join("node_modules/.modules.yaml"),
        r#"
nodeLinker: hoisted
hoistedLocations:
  "is-positive@3.1.0":
    - node_modules/is-positive
"#,
    )
    .expect("write .modules.yaml");

    let output = pacquet_in(workspace.path())
        .args(["licenses", "list", "--json"])
        .output()
        .expect("run licenses");
    assert!(
        output.status.success(),
        "licenses should succeed: {}",
        String::from_utf8_lossy(&output.stderr),
    );

    let report: Value = serde_json::from_slice(&output.stdout).expect("parse licenses JSON");
    let mit_packages = report["MIT"].as_array().expect("MIT license group");
    assert_eq!(mit_packages.len(), 1);
    let pkg = &mit_packages[0];
    assert_eq!(pkg["name"], "is-positive");
    assert_eq!(pkg["versions"], json!(["3.1.0"]));
    assert_eq!(pkg["author"], "Test Author");

    let paths = pkg["paths"].as_array().expect("paths array");
    assert_eq!(paths.len(), 1);
    let reported_path = paths[0].as_str().expect("path string");
    assert!(
        !reported_path.contains(".pnpm"),
        "reported path should not contain .pnpm: {reported_path}",
    );
    let canonical_reported =
        dunce::canonicalize(reported_path).expect("canonicalize reported path");
    let canonical_pkg_dir = dunce::canonicalize(&pkg_dir).expect("canonicalize pkg dir");
    assert_eq!(
        canonical_reported, canonical_pkg_dir,
        "reported path should match the hoisted directory",
    );
    assert!(Path::new(reported_path).exists(), "reported path should exist on disk");
}

#[test]
fn licenses_falls_back_to_a_version_matched_root_package_without_recorded_hoisted_locations() {
    let workspace = tempfile::tempdir().expect("create workspace");
    fs::write(
        workspace.path().join("package.json"),
        json!({ "dependencies": { "is-positive": "3.1.0", "wrapper": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    fs::write(workspace.path().join("pnpm-workspace.yaml"), "nodeLinker: hoisted\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.path().join("pnpm-lock.yaml"),
        r"
lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      is-positive:
        specifier: 3.1.0
        version: 3.1.0
      wrapper:
        specifier: 1.0.0
        version: 1.0.0
packages:
  is-positive@1.0.0:
    resolution: {integrity: sha512-is-positive-1}
  is-positive@3.1.0:
    resolution: {integrity: sha512-is-positive-3}
  wrapper@1.0.0:
    resolution: {integrity: sha512-wrapper}
snapshots:
  is-positive@1.0.0: {}
  is-positive@3.1.0: {}
  wrapper@1.0.0:
    dependencies:
      is-positive: 1.0.0
",
    )
    .expect("write lockfile");
    for (location, name, version) in [
        ("node_modules/is-positive", "is-positive", "3.1.0"),
        ("node_modules/wrapper", "wrapper", "1.0.0"),
        ("node_modules/wrapper/node_modules/is-positive", "is-positive", "1.0.0"),
    ] {
        let package_dir = workspace.path().join(location);
        fs::create_dir_all(&package_dir).expect("create package directory");
        fs::write(
            package_dir.join("package.json"),
            json!({ "name": name, "version": version, "license": "MIT" }).to_string(),
        )
        .expect("write manifest");
    }
    fs::write(
        workspace.path().join("node_modules/.modules.yaml"),
        json!({ "layoutVersion": 5, "nodeLinker": "hoisted" }).to_string(),
    )
    .expect("write .modules.yaml");

    let output = pacquet_in(workspace.path())
        .args(["licenses", "list", "--json"])
        .output()
        .expect("run licenses");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report: Value = serde_json::from_slice(&output.stdout).expect("parse licenses JSON");
    let root = dunce::canonicalize(workspace.path()).expect("canonicalize workspace");
    let root_is_positive = root.join("node_modules").join("is-positive");
    let is_positive_paths: Vec<(String, PathBuf)> = report
        .as_object()
        .expect("license groups")
        .values()
        .flat_map(|group| group.as_array().expect("license group"))
        .filter(|pkg| pkg["name"] == "is-positive")
        .flat_map(|pkg| {
            let versions = pkg["versions"]
                .as_array()
                .expect("versions")
                .clone();
            let paths = pkg["paths"]
                .as_array()
                .expect("paths")
                .clone();
            versions
                .into_iter()
                .zip(paths)
                .map(|(version, path)| {
                    (version.as_str().unwrap().to_string(), PathBuf::from(path.as_str().unwrap()))
                })
        })
        .collect();

    let path_of = |version: &str| {
        is_positive_paths
            .iter()
            .find(|(listed, _)| listed == version)
            .map_or_else(
                || panic!("is-positive@{version} missing from {is_positive_paths:?}"),
                |(_, path)| path.clone(),
            )
    };
    assert_eq!(
        dunce::canonicalize(path_of("3.1.0")).expect("canonicalize reported path"),
        root_is_positive,
        "the root package matches the locked version",
    );
    let nested_path = path_of("1.0.0");
    assert!(
        nested_path != root_is_positive
            && nested_path
                != workspace
                    .path()
                    .join("node_modules")
                    .join("is-positive"),
        "the root package holds another version, so it must not be reported for 1.0.0",
    );
}
