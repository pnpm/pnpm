use super::{pacquet, run_sbom_json, run_sbom_json_from_store};
use serde_json::{Value, json};
use std::{fs, path::Path};
use tempfile::TempDir;

const HASH_A: &str = "sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const HASH_B: &str = "sha512-BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB";
const HASH_C: &str = "sha512-CCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCCC";
const HASH_D: &str = "sha512-DDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDDD";
const HASH_E: &str = "sha512-EEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEEE";

#[derive(Clone, Copy)]
enum WorkspaceRegistries {
    Declared,
    Undeclared,
    MalformedSecretUrl,
}

fn write_named_registries_fixture(registries: WorkspaceRegistries) -> TempDir {
    let workspace = TempDir::new().expect("create workspace");
    fs::write(
        workspace.path().join("package.json"),
        json!({
            "name": "named-registries-sbom-app",
            "version": "1.0.0",
            "license": "MIT",
            "dependencies": {
                "pkg-a": "^1.0.0",
                "pkg-a-work": "work:pkg-a@^1.0.0",
                "@scope/pkg": "work:^1.0.0",
                "pkg-peer": "secret:^2.0.0",
                "peer-dep": "^3.0.0",
            },
        })
        .to_string(),
    )
    .expect("write package.json");

    match registries {
        WorkspaceRegistries::Declared => {
            fs::write(
                workspace.path().join("pnpm-workspace.yaml"),
                concat!(
                    "namedRegistries:\n",
                    "  work: https://npm.work.example.com/\n",
                    "  secret: https://build-user:build-token@npm.secret.example.com/team-a/?api_key=top-secret#frag\n",
                ),
            )
            .expect("write pnpm-workspace.yaml");
        }
        WorkspaceRegistries::MalformedSecretUrl => {
            fs::write(
                workspace.path().join("pnpm-workspace.yaml"),
                concat!(
                    "namedRegistries:\n",
                    "  work: https://npm.work.example.com/\n",
                    "  secret: https://build-user:build-token@npm.secret.example.com:bad/team-a/?api_key=top-secret#frag\n",
                ),
            )
            .expect("write pnpm-workspace.yaml");
        }
        WorkspaceRegistries::Undeclared => {}
    }

    fs::write(
        workspace.path().join("pnpm-lock.yaml"),
        format!(
            "lockfileVersion: '9.0'
importers:
  .:
    dependencies:
      '@scope/pkg':
        specifier: work:^1.0.0
        version: work:1.0.0
      peer-dep:
        specifier: ^3.0.0
        version: 3.0.0
      pkg-a:
        specifier: ^1.0.0
        version: 1.0.0
      pkg-a-work:
        specifier: work:pkg-a@^1.0.0
        version: pkg-a@work:1.0.0
      pkg-peer:
        specifier: secret:^2.0.0
        version: secret:2.0.0(peer-dep@3.0.0)
packages:
  '@scope/pkg@work:1.0.0':
    resolution: {{integrity: {HASH_C}}}
  peer-dep@3.0.0:
    resolution: {{integrity: {HASH_E}}}
  pkg-a@1.0.0:
    resolution: {{integrity: {HASH_A}}}
  pkg-a@work:1.0.0:
    resolution: {{integrity: {HASH_B}}}
  pkg-peer@secret:2.0.0:
    resolution: {{integrity: {HASH_D}}}
    peerDependencies:
      peer-dep: ^3.0.0
snapshots:
  '@scope/pkg@work:1.0.0': {{}}
  peer-dep@3.0.0: {{}}
  pkg-a@1.0.0: {{}}
  pkg-a@work:1.0.0: {{}}
  pkg-peer@secret:2.0.0(peer-dep@3.0.0):
    dependencies:
      peer-dep: 3.0.0
",
        ),
    )
    .expect("write pnpm-lock.yaml");

    plant_virtual_store_package(
        workspace.path(),
        "pkg-a@1.0.0",
        "pkg-a",
        &json!({ "name": "pkg-a", "version": "1.0.0", "license": "MIT", "description": "public pkg-a" }),
    );
    plant_virtual_store_package(
        workspace.path(),
        "pkg-a@work+1.0.0",
        "pkg-a",
        &json!({ "name": "pkg-a", "version": "1.0.0", "license": "Apache-2.0", "description": "work pkg-a" }),
    );
    plant_virtual_store_package(
        workspace.path(),
        "@scope+pkg@work+1.0.0",
        "@scope/pkg",
        &json!({ "name": "@scope/pkg", "version": "1.0.0", "license": "BSD-3-Clause" }),
    );
    plant_virtual_store_package(
        workspace.path(),
        "pkg-peer@secret+2.0.0_peer-dep@3.0.0",
        "pkg-peer",
        &json!({ "name": "pkg-peer", "version": "2.0.0", "license": "ISC" }),
    );
    plant_virtual_store_package(
        workspace.path(),
        "peer-dep@3.0.0",
        "peer-dep",
        &json!({ "name": "peer-dep", "version": "3.0.0", "license": "MIT" }),
    );

    workspace
}

fn plant_virtual_store_package(workspace: &Path, slot: &str, pkg_name: &str, manifest: &Value) {
    let pkg_dir = workspace
        .join("node_modules/.pnpm")
        .join(slot)
        .join("node_modules")
        .join(pkg_name);
    fs::create_dir_all(&pkg_dir).expect("create virtual store package dir");
    fs::write(pkg_dir.join("package.json"), manifest.to_string()).expect("write package.json");
}

fn find_by_purl<'a>(items: &'a [Value], purl_field: &str, purl: &str) -> &'a Value {
    items
        .iter()
        .find(|item| item[purl_field] == purl)
        .unwrap_or_else(|| panic!("expected component with purl {purl} in {items:#?}"))
}

fn distribution_url(component: &Value) -> &str {
    component["externalReferences"]
        .as_array()
        .expect("externalReferences")
        .iter()
        .find(|ext_ref| ext_ref["type"] == "distribution")
        .expect("distribution reference")["url"]
        .as_str()
        .expect("distribution url string")
}

#[test]
fn sbom_cyclonedx_resolves_named_registries_and_strips_credentials() {
    let workspace = write_named_registries_fixture(WorkspaceRegistries::Declared);
    let bom = run_sbom_json_from_store(workspace.path(), "cyclonedx");
    let raw = serde_json::to_string(&bom).expect("serialize bom");
    for secret in ["build-user", "build-token", "top-secret", "frag"] {
        assert!(!raw.contains(secret), "secret {secret} leaked into CycloneDX SBOM: {raw}");
    }

    let components = bom["components"].as_array().expect("components");
    let public_a = find_by_purl(components, "purl", "pkg:npm/pkg-a@1.0.0");
    assert_eq!(public_a["name"], "pkg-a");
    assert_eq!(public_a["version"], "1.0.0");
    assert_eq!(public_a["description"], "public pkg-a");
    assert_eq!(public_a["licenses"][0]["license"]["id"], "MIT");
    assert_eq!(distribution_url(public_a), "https://registry.npmjs.org/pkg-a/-/pkg-a-1.0.0.tgz");

    let work_a = find_by_purl(
        components,
        "purl",
        "pkg:npm/pkg-a@1.0.0?repository_url=https%3A%2F%2Fnpm.work.example.com%2F",
    );
    assert_eq!(work_a["name"], "pkg-a");
    assert_eq!(work_a["version"], "1.0.0");
    assert_eq!(work_a["description"], "work pkg-a");
    assert_eq!(work_a["licenses"][0]["license"]["id"], "Apache-2.0");
    assert_eq!(distribution_url(work_a), "https://npm.work.example.com/pkg-a/-/pkg-a-1.0.0.tgz");

    let scoped = find_by_purl(
        components,
        "purl",
        "pkg:npm/%40scope/pkg@1.0.0?repository_url=https%3A%2F%2Fnpm.work.example.com%2F",
    );
    assert_eq!(scoped["group"], "@scope");
    assert_eq!(scoped["name"], "pkg");
    assert_eq!(scoped["version"], "1.0.0");
    assert_eq!(distribution_url(scoped), "https://npm.work.example.com/@scope/pkg/-/pkg-1.0.0.tgz");

    let peer_purl =
        "pkg:npm/pkg-peer@2.0.0?repository_url=https%3A%2F%2Fnpm.secret.example.com%2Fteam-a%2F";
    let pkg_peer = find_by_purl(components, "purl", peer_purl);
    assert_eq!(pkg_peer["name"], "pkg-peer");
    assert_eq!(pkg_peer["version"], "2.0.0");
    assert_eq!(
        distribution_url(pkg_peer),
        "https://npm.secret.example.com/team-a/pkg-peer/-/pkg-peer-2.0.0.tgz",
    );

    let peer_deps = bom["dependencies"]
        .as_array()
        .expect("dependencies")
        .iter()
        .find(|entry| entry["ref"] == peer_purl)
        .expect("pkg-peer dependency entry");
    assert_eq!(peer_deps["dependsOn"], json!(["pkg:npm/peer-dep@3.0.0"]));
}

#[test]
fn sbom_spdx_resolves_named_registries_and_keeps_same_name_version_distinct() {
    let workspace = write_named_registries_fixture(WorkspaceRegistries::Declared);
    let doc = run_sbom_json(workspace.path(), "spdx", &[]);
    let raw = serde_json::to_string(&doc).expect("serialize spdx");
    for secret in ["build-user", "build-token", "top-secret", "frag"] {
        assert!(!raw.contains(secret), "secret {secret} leaked into SPDX SBOM: {raw}");
    }

    let packages = doc["packages"].as_array().expect("packages");
    let find_spdx_by_purl = |purl: &str| {
        packages
            .iter()
            .find(|pkg| pkg["externalRefs"][0]["referenceLocator"] == purl)
            .unwrap_or_else(|| panic!("expected SPDX package with purl {purl} in {packages:#?}"))
    };

    let public_a = find_spdx_by_purl("pkg:npm/pkg-a@1.0.0");
    assert_eq!(public_a["name"], "pkg-a");
    assert_eq!(public_a["versionInfo"], "1.0.0");
    assert_eq!(public_a["downloadLocation"], "https://registry.npmjs.org/pkg-a/-/pkg-a-1.0.0.tgz");

    let work_a = find_spdx_by_purl(
        "pkg:npm/pkg-a@1.0.0?repository_url=https%3A%2F%2Fnpm.work.example.com%2F",
    );
    assert_eq!(work_a["name"], "pkg-a");
    assert_eq!(work_a["versionInfo"], "1.0.0");
    assert_eq!(work_a["downloadLocation"], "https://npm.work.example.com/pkg-a/-/pkg-a-1.0.0.tgz");
    assert_ne!(public_a["SPDXID"], work_a["SPDXID"]);

    let scoped = find_spdx_by_purl(
        "pkg:npm/%40scope/pkg@1.0.0?repository_url=https%3A%2F%2Fnpm.work.example.com%2F",
    );
    assert_eq!(scoped["versionInfo"], "1.0.0");
    assert_eq!(
        scoped["downloadLocation"],
        "https://npm.work.example.com/@scope/pkg/-/pkg-1.0.0.tgz",
    );

    let pkg_peer = find_spdx_by_purl(
        "pkg:npm/pkg-peer@2.0.0?repository_url=https%3A%2F%2Fnpm.secret.example.com%2Fteam-a%2F",
    );
    assert_eq!(pkg_peer["versionInfo"], "2.0.0");
    assert_eq!(
        pkg_peer["downloadLocation"],
        "https://npm.secret.example.com/team-a/pkg-peer/-/pkg-peer-2.0.0.tgz",
    );
}

#[test]
fn sbom_fails_with_missing_named_registry_when_prefix_is_undeclared() {
    let workspace = write_named_registries_fixture(WorkspaceRegistries::Undeclared);
    for format in ["cyclonedx", "spdx"] {
        let output =
            pacquet(workspace.path(), ["sbom", "--sbom-format", format, "--lockfile-only"])
                .output()
                .expect("run pacquet sbom");
        assert!(!output.status.success(), "expected {format} sbom to fail when prefix is missing");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("ERR_PNPM_MISSING_NAMED_REGISTRY"), "stderr:\n{stderr}");
    }
}

#[test]
fn sbom_fails_without_leaking_credentials_when_named_registry_url_is_malformed() {
    let workspace = write_named_registries_fixture(WorkspaceRegistries::MalformedSecretUrl);
    for format in ["cyclonedx", "spdx"] {
        for extra_args in [&["--lockfile-only"][..], &[][..]] {
            let mut cmd = pacquet(workspace.path(), ["sbom", "--sbom-format", format]);
            cmd.args(extra_args);
            let output = cmd.output().expect("run pacquet sbom");
            assert!(
                !output.status.success(),
                "expected {format} sbom ({extra_args:?}) to fail when named registry URL is malformed",
            );
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(stderr.contains("ERR_PNPM_INVALID_NAMED_REGISTRY_URL"), "stderr:\n{stderr}");
            for secret in
                ["build-user", "build-token", "top-secret", "frag", "npm.secret.example.com"]
            {
                assert!(!stdout.contains(secret), "secret {secret} leaked to stdout:\n{stdout}");
                assert!(!stderr.contains(secret), "secret {secret} leaked to stderr:\n{stderr}");
            }
        }
    }
}

#[test]
fn sbom_omits_tarball_url_without_leaking_credentials_when_default_registry_is_malformed() {
    let workspace = write_named_registries_fixture(WorkspaceRegistries::Declared);
    for format in ["cyclonedx", "spdx"] {
        let output = pacquet(workspace.path(), ["sbom", "--sbom-format", format])
            .env(
                "PNPM_CONFIG_REGISTRY",
                "https://default-user:default-token@npm.default.example.com:bad/",
            )
            .output()
            .expect("run pacquet sbom");
        assert!(
            output.status.success(),
            "expected {format} sbom to succeed while omitting malformed default registry URL, stderr:\n{}",
            String::from_utf8_lossy(&output.stderr),
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        for secret in ["default-user", "default-token", "npm.default.example.com"] {
            assert!(!stdout.contains(secret), "secret {secret} leaked to stdout:\n{stdout}");
            assert!(!stderr.contains(secret), "secret {secret} leaked to stderr:\n{stderr}");
        }
    }
}
