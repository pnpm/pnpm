//! Registry routing for `@jsr/*` lockfile entries during supply-chain policy
//! verification, the shape reported in
//! [pnpm/pnpm#14649](https://github.com/pnpm/pnpm/issues/14649).

use crate::_utils::{flatten_report, pacquet_in};

use assert_cmd::prelude::*;
use std::fs;
use tempfile::{TempDir, tempdir};

const JSR_LOCKFILE: &str = r"lockfileVersion: '9.0'

settings:
  autoInstallPeers: true
  excludeLinksFromLockfile: false

importers:

  .:
    dependencies:
      '@std/csv':
        specifier: jsr:^1.0.6
        version: '@jsr/std__csv@1.0.6'

packages:

  '@jsr/std__csv@1.0.6':
    resolution: {integrity: sha512-9n/SZzjolPQ907gUPksQU3EFURwRl4GA9V6MylA8hpSkpxhcj6J98zDpn9EmDqgE2A4L9MnlUvFhRpeWl+Y/ZQ==, tarball: https://npm.jsr.io/~/11/@jsr/std__csv/1.0.6.tgz}

snapshots:

  '@jsr/std__csv@1.0.6': {}
";

/// A project whose committed lockfile holds a JSR dependency, with an empty
/// cache and a default registry that is not npmjs, so the host the run reaches
/// for is the one under test.
fn jsr_project() -> TempDir {
    let root = tempdir().expect("create temp directory");
    let workspace = root.path();
    fs::write(workspace.join(".npmrc"), "registry=https://registry.example.test/\n")
        .expect("write .npmrc");
    fs::write(workspace.join("pnpm-workspace.yaml"), "storeDir: store\ncacheDir: cache\n")
        .expect("write pnpm-workspace.yaml");
    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@std/csv": "jsr:^1.0.6" } }).to_string(),
    )
    .expect("write project manifest");
    fs::write(workspace.join("pnpm-lock.yaml"), JSR_LOCKFILE).expect("write lockfile");
    root
}

/// The metadata a `@jsr/*` entry is verified against is asked of npm.jsr.io,
/// the built-in route for the scope, not of the default registry, which serves
/// no `@jsr/*` packument. `--offline` names the registry in the mirror path it
/// looked the metadata up in, so the routing is observable without a request.
#[test]
fn a_jsr_lockfile_entry_is_verified_against_the_jsr_registry() {
    let root = jsr_project();

    let assert = pacquet_in(root.path())
        .args(["install", "--frozen-lockfile", "--offline"])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    eprintln!("STDERR:\n{stderr}\n");
    let report = flatten_report(&stderr);
    assert!(
        report.contains("npm.jsr.io"),
        "the JSR metadata must be looked up under npm.jsr.io; got:\n{stderr}",
    );
    assert!(
        !report.contains("registry.example.test"),
        "the default registry must not be asked for an `@jsr/*` packument; got:\n{stderr}",
    );
}
