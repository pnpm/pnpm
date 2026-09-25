//! Registry routing for `@jsr/*` lockfile entries during supply-chain policy
//! verification, the shape reported in
//! [pnpm/pnpm#14649](https://github.com/pnpm/pnpm/issues/14649).

use crate::_utils::{flatten_report, pacquet_in};

use assert_cmd::prelude::*;
use std::fs;
use tempfile::{TempDir, tempdir};

/// The integrity is a placeholder: the run fails at the metadata lookup this
/// test is about, so no artifact is ever fetched or hashed. What matters is
/// the entry's name and its `npm.jsr.io` tarball.
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
    resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==, tarball: https://npm.jsr.io/~/11/@jsr/std__csv/1.0.6.tgz}

snapshots:

  '@jsr/std__csv@1.0.6': {}
";

/// The default registry is deliberately neither npmjs nor a reachable host, so
/// the only registry the run can succeed at naming is the built-in JSR route.
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

/// `--offline` names the registry in the metadata mirror path it failed to
/// read, which is what makes the route observable without a request.
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
