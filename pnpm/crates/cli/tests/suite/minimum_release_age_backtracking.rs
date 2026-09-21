use assert_cmd::prelude::*;
use pnpm_testing_utils::bin::CommandTempCwd;
use serde_json::json;
use std::fs;

#[test]
fn install_add_and_latest_update_back_off_from_immature_exact_pins() {
    let mut server = mockito::Server::new();
    let _parent = server
        .mock("GET", "/parent")
        .with_status(200)
        .with_body(packument("parent", true).to_string())
        .create();
    let _child = server
        .mock("GET", "/child")
        .with_status(200)
        .with_body(packument("child", false).to_string())
        .create();
    for args in [vec!["install"], vec!["add", "parent"], vec!["update", "parent", "--latest"]] {
        let CommandTempCwd { mut pacquet, root, workspace, .. } = CommandTempCwd::init();
        let manifest = if args[0] == "add" {
            json!({ "name": "test-project", "version": "1.0.0" })
        } else {
            json!({ "name": "test-project", "version": "1.0.0", "dependencies": { "parent": "*" } })
        };
        fs::write(workspace.join("package.json"), manifest.to_string()).unwrap();
        fs::write(workspace.join(".npmrc"), format!("registry={}/\n", server.url())).unwrap();
        fs::write(workspace.join("pnpm-workspace.yaml"), "minimumReleaseAge: 1440\nminimumReleaseAgeStrict: true\nstoreDir: ../store\ncacheDir: ../cache\nenableGlobalVirtualStore: false\n").unwrap();
        pacquet
            .args(&args)
            .args(["--lockfile-only", "--ignore-scripts"])
            .assert()
            .success();
        let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).unwrap();
        assert!(lockfile.contains("parent@1.0.0:"));
        assert!(lockfile.contains("child@1.0.0:"));
        assert!(!lockfile.contains("parent@2.0.0:"));
        drop(root);
    }
}

fn packument(name: &str, parent: bool) -> serde_json::Value {
    let mut versions = serde_json::Map::new();
    for version in ["1.0.0", "2.0.0"] {
        let mut manifest = json!({
            "name": name, "version": version,
            "dist": {
                "tarball": format!("https://registry.example/{name}-{version}.tgz"),
                "integrity": ssri::Integrity::from(b"fixture".as_slice()).to_string(),
            }
        });
        if parent {
            manifest["dependencies"] = json!({ "child": version });
        }
        versions.insert(version.to_string(), manifest);
    }
    json!({
        "name": name, "dist-tags": { "latest": "2.0.0" }, "versions": versions,
        "time": {
            "1.0.0": "2020-01-01T00:00:00Z",
            "2.0.0": if parent { "2020-01-01T00:00:00Z" } else { "2099-01-01T00:00:00Z" },
        }
    })
}
