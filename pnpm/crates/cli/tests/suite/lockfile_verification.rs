//! End-to-end CLI integration test for the lockfile-verification
//! gate. Spawns the `pacquet` binary against a
//! pnpm-workspace.yaml that activates the verifier and confirms the
//! gate fires through the real install path — non-zero exit, the
//! upstream-canonical diagnostic code in stderr.
//!
//! Doesn't try to exercise every branch — the unit tests in
//! `pnpm-lockfile-verification` and `pnpm-resolving-npm-resolver`
//! already do that. This file pins the user-visible contract: the
//! gate runs from the CLI, the install fails when policy is
//! tripped, and the error envelope carries the upstream code so
//! `pnpm errors` documentation routes to the right entry.

pub use _utils::*;

use crate::_utils;

use command_extra::CommandExtra;
use pnpm_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path};

/// A project whose only dependency every policy rejects. The mocked
/// registry's packument times are real-world (years old), so a
/// `minimumReleaseAge` of 100 years catches every version regardless of
/// when the mock was populated, and the verification gate fires on any
/// lockfile naming one. The hand-rolled v9 lockfile pins that package
/// with a placeholder integrity: the gate rejects the entry before the
/// tarball is verified, and an install that skips the gate fails
/// downstream on the tarball check instead.
fn write_policy_rejected_project(workspace: &Path) {
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin": "1.0.0",
        },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    set_minimum_release_age(workspace, 60 * 24 * 365 * 100);
    let lockfile = "lockfileVersion: '9.0'\n\
        importers:\n  \
          .:\n    \
            dependencies:\n      \
              '@pnpm.e2e/hello-world-js-bin':\n        \
                specifier: 1.0.0\n        \
                version: 1.0.0\n\
        packages:\n  \
          '@pnpm.e2e/hello-world-js-bin@1.0.0':\n    \
            resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==}\n\
        snapshots:\n  \
          '@pnpm.e2e/hello-world-js-bin@1.0.0': {}\n";
    fs::write(workspace.join("pnpm-lock.yaml"), lockfile).expect("write lockfile");
}

/// `minimumReleaseAge` set to 100 years rejects every version the
/// mocked registry has ever served. The install fails before any
/// tarball is fetched; stderr names the
/// `ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION` code so log consumers
/// and `pnpm errors` URL routing both work.
#[test]
fn install_fails_under_huge_minimum_release_age() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_policy_rejected_project(&workspace);

    let output = pacquet
        .with_args(["install", "--frozen-lockfile"])
        .output()
        .expect("spawn pacquet install");

    assert!(
        !output.status.success(),
        "the gate must reject the install (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION"),
        "stderr must name the upstream-canonical diagnostic code; got:\n{stderr}",
    );

    // No `node_modules/.pnpm/...` slot must materialize for the
    // gated package — proves the failure short-circuits before
    // tarball fetch.
    assert!(
        !workspace.join("node_modules/.pnpm/@pnpm.e2e+hello-world-js-bin@1.0.0").exists(),
        "the gate must fail before any virtual-store materialization",
    );

    drop((root, mock_instance));
}

/// TS: `minimumReleaseAge falls back to immature version when no mature
/// version satisfies the range (non-strict mode)` (`minimumReleaseAge.ts:68`).
/// A 100-year cutoff makes every `@pnpm.e2e/bravo-dep` version immature.
/// Non-strict mode must fall back to the *lowest* version matching the `1.0`
/// range — normal resolution would pick the highest, `1.0.1` — and complete
/// the install instead of aborting. An explicit cutoff turns strict mode on
/// by default, so reaching non-strict mode takes an explicit opt-out.
#[test]
fn non_strict_minimum_release_age_falls_back_when_no_mature_version_matches() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    set_minimum_release_age(&workspace, 60 * 24 * 365 * 100);
    append_workspace_yaml_key(&workspace, "minimumReleaseAgeStrict", false);

    let output = pacquet
        .with_args(["add", "@pnpm.e2e/bravo-dep@1.0"])
        .output()
        .expect("spawn pacquet add");
    assert!(
        output.status.success(),
        "non-strict mode must proceed with the immature fallback (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );

    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
    )
    .expect("parse package.json");
    assert_eq!(
        manifest["dependencies"]["@pnpm.e2e/bravo-dep"],
        serde_json::json!("~1.0.0"),
        "the fallback must pin the lowest matching version",
    );

    let lockfile = read_lockfile(&workspace.join("pnpm-lock.yaml"));
    let snapshots = lockfile.snapshots.as_ref().expect("lockfile has snapshots");
    assert!(
        snapshots
            .keys()
            .any(|key| key.to_string() == "@pnpm.e2e/bravo-dep@1.0.0"),
        "the lockfile must resolve the lowest matching version",
    );

    drop((root, mock_instance));
}

/// `trustLockfile: true` short-circuits the verification gate so a
/// lockfile that would otherwise trip the policy
/// (`minimumReleaseAge: 100 years` rejects every published version)
/// bypasses the verification step. Confirms the opt-out path runs
/// end-to-end through the CLI and that no
/// `MINIMUM_RELEASE_AGE_VIOLATION` error leaks into stderr — the test
/// stops short of asserting full install success, see the inline
/// comment above the assertion below.
#[test]
fn trust_lockfile_skips_verification() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_policy_rejected_project(&workspace);
    append_workspace_yaml_key(&workspace, "fetchRetryMintimeout", 1);
    append_workspace_yaml_key(&workspace, "fetchRetryMaxtimeout", 1);
    append_workspace_yaml_key(&workspace, "trustLockfile", true);

    let output = pacquet
        .with_args(["install", "--frozen-lockfile"])
        .output()
        .expect("spawn pacquet install");

    // Asserting only on the absence of the verifier error code, not
    // `output.status.success()`: the test fixture's `pnpm-lock.yaml`
    // is hand-rolled with a placeholder integrity hash, so the
    // install fails the tarball integrity check downstream of the
    // verification pass. That's irrelevant to what's being tested —
    // the contract here is "the supply-chain gate doesn't fire",
    // not "the install completes". Asserting success would require a
    // real lockfile generated against the mocked registry first
    // (see hoist.rs's `generate_lockfile` pattern); not worth the
    // extra wiring for a smoke test of the opt-out switch.
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        !stderr.contains("ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION"),
        "trustLockfile must skip the verification gate; got:\n{stderr}",
    );

    drop((root, mock_instance));
}

/// `--trust-lockfile` CLI flag short-circuits the verification gate
/// the same way `trustLockfile: true` in `pnpm-workspace.yaml` does.
/// Same provocation as the yaml-based test above, with the yaml
/// override removed so the gate would normally fire — the flag is
/// what makes the verification gate skip. Like that test, this only
/// asserts the absence of `MINIMUM_RELEASE_AGE_VIOLATION` (not full
/// install success); see the inline comment above the assertion.
#[test]
fn trust_lockfile_cli_flag_skips_verification() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_policy_rejected_project(&workspace);
    append_workspace_yaml_key(&workspace, "fetchRetryMintimeout", 1);
    append_workspace_yaml_key(&workspace, "fetchRetryMaxtimeout", 1);

    let output = pacquet
        .with_args(["install", "--frozen-lockfile", "--trust-lockfile"])
        .output()
        .expect("spawn pacquet install");

    // Same reasoning as the yaml-opt-out test above: not asserting
    // `output.status.success()` because the hand-rolled lockfile's
    // placeholder integrity trips the downstream tarball check. The
    // contract being tested is gate-skipped, not install-succeeded.
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        !stderr.contains("ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION"),
        "--trust-lockfile must skip the verification gate; got:\n{stderr}",
    );

    drop((root, mock_instance));
}

/// `remove` declares clap flags for `--trust-lockfile` and `--no-trust-lockfile`.
/// The lockfile is verified after the removal is applied to it, so the
/// provocation keeps a second rejected entry that survives the removal:
/// without the flag the gate fires on that entry and the manifest is left
/// alone, with the flag the removal completes.
#[test]
fn remove_honors_the_bare_trust_lockfile_flag() {
    let CommandTempCwd {
        pacquet: initial_install,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/foo": "100.0.0",
            "@pnpm.e2e/hello-world-js-bin": "1.0.0",
        },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    let output = initial_install
        .with_args(["install", "--ignore-scripts"])
        .output()
        .expect("spawn pacquet install");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    set_minimum_release_age(&workspace, 60 * 24 * 365 * 100);

    let output = pacquet_in(&workspace)
        .with_args(["remove", "@pnpm.e2e/foo"])
        .output()
        .expect("spawn pacquet remove");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        !output.status.success() && stderr.contains("ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION"),
        "remove must verify the lockfile it leaves behind; got:\n{stderr}",
    );
    assert_eq!(
        read_manifest(&workspace),
        package_json,
        "a rejected removal must not touch the manifest",
    );

    let output = pacquet_in(&workspace)
        .with_args(["remove", "@pnpm.e2e/foo", "--trust-lockfile"])
        .output()
        .expect("spawn pacquet remove");
    assert!(
        output.status.success(),
        "--trust-lockfile must skip the verification gate; got:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );
    assert_eq!(
        read_manifest(&workspace),
        serde_json::json!({ "dependencies": { "@pnpm.e2e/hello-world-js-bin": "1.0.0" } }),
    );

    drop((root, mock_instance));
}

#[test]
fn remove_honors_no_trust_lockfile_flag_overriding_workspace_config() {
    let CommandTempCwd {
        pacquet: initial_install,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/foo": "100.0.0",
            "@pnpm.e2e/hello-world-js-bin": "1.0.0",
        },
    });
    fs::write(workspace.join("package.json"), package_json.to_string())
        .expect("write package.json");
    let output = initial_install
        .with_args(["install", "--ignore-scripts"])
        .output()
        .expect("spawn pacquet install");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    set_minimum_release_age(&workspace, 60 * 24 * 365 * 100);
    append_workspace_yaml_key(&workspace, "trustLockfile", true);

    let output = pacquet_in(&workspace)
        .with_args(["remove", "@pnpm.e2e/foo", "--no-trust-lockfile"])
        .output()
        .expect("spawn pacquet remove");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        !output.status.success() && stderr.contains("ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION"),
        "--no-trust-lockfile must override trustLockfile: true; got:\n{stderr}",
    );
    assert_eq!(
        read_manifest(&workspace),
        package_json,
        "a rejected removal must not touch the manifest",
    );

    let output = pacquet_in(&workspace)
        .with_args(["remove", "@pnpm.e2e/foo"])
        .output()
        .expect("spawn pacquet remove");
    assert!(
        output.status.success(),
        "without --no-trust-lockfile, trustLockfile: true allows removal; got:\n{}",
        String::from_utf8_lossy(&output.stderr),
    );

    drop((root, mock_instance));
}

fn read_manifest(workspace: &Path) -> serde_json::Value {
    serde_json::from_str(
        &fs::read_to_string(workspace.join("package.json")).expect("read package.json"),
    )
    .expect("parse package.json")
}

/// Regression test for the crafted-lockfile path-traversal advisory
/// (GHSA-2rx9-3g3h-c2jv). A dependency name/alias that isn't a valid npm
/// package name — here a `../../escaped-link` snapshot key — would be
/// joined into a filesystem path at install time and escape the project.
/// The dependency-name check must run **even under `--trust-lockfile`**,
/// which disables the resolution-policy verification fan-out: the two
/// tests above prove that flag skips the *policy* gate, so this test pins
/// the complementary contract that the *structural* name check is not
/// skipped with it. The install must fail with
/// `ERR_PNPM_INVALID_DEPENDENCY_NAME` before any virtual-store slot is
/// materialized.
#[test]
fn trust_lockfile_still_rejects_traversal_dependency_name() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // A legit direct dependency keeps the frozen-lockfile freshness
    // check happy; the crafted entry is an extra `snapshots:` /
    // `packages:` key whose name is a path traversal. The verifier
    // walks every snapshot, so an unreferenced hostile key is still
    // rejected.
    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    let lockfile = "lockfileVersion: '9.0'\n\
        importers:\n  \
          .:\n    \
            dependencies:\n      \
              '@pnpm.e2e/hello-world-js-bin':\n        \
                specifier: 1.0.0\n        \
                version: 1.0.0\n\
        packages:\n  \
          '@pnpm.e2e/hello-world-js-bin@1.0.0':\n    \
            resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==}\n  \
          '../../escaped-link@1.0.0':\n    \
            resolution: {integrity: sha512-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA==}\n\
        snapshots:\n  \
          '@pnpm.e2e/hello-world-js-bin@1.0.0': {}\n  \
          '../../escaped-link@1.0.0': {}\n";
    fs::write(workspace.join("pnpm-lock.yaml"), lockfile).expect("write lockfile");

    let output = pacquet
        .with_args(["install", "--frozen-lockfile", "--trust-lockfile"])
        .output()
        .expect("spawn pacquet install");

    assert!(
        !output.status.success(),
        "the name check must reject the install even under --trust-lockfile (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_INVALID_DEPENDENCY_NAME"),
        "stderr must name the invalid-dependency-name code; got:\n{stderr}",
    );
    // The rejection happens before materialization, so nothing is
    // extracted — neither the legit slot nor any escaped directory.
    assert!(
        !workspace.join("node_modules/.pnpm").exists(),
        "the check must fail before any virtual-store materialization",
    );
    // Belt-and-suspenders: the traversal target of the crafted alias
    // (`<workspace>/node_modules/../../escaped-link`) must not exist.
    if let Some(parent) = workspace.parent() {
        assert!(
            !parent.join("escaped-link").exists(),
            "no link may be created outside the project",
        );
    }

    drop((root, mock_instance));
}

/// A lockfile containing a path traversal in a package's explicit version
/// must fail the slot containment check before any directory is created in
/// the global virtual store — even under `--trust-lockfile`.
#[test]
fn install_rejects_a_traversal_version_under_global_virtual_store() {
    let CommandTempCwd {
        pacquet,
        root,
        workspace,
        npmrc_info,
        ..
    } = CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, store_dir, .. } = npmrc_info;

    let config_text = fs::read_to_string(workspace.join("pnpm-workspace.yaml"))
        .unwrap_or_default()
        .replace("enableGlobalVirtualStore: false", "enableGlobalVirtualStore: true");
    fs::write(workspace.join("pnpm-workspace.yaml"), config_text)
        .expect("write pnpm-workspace.yaml");

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "tar-dep": "file:vendor/dep.tgz",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    let lockfile = "lockfileVersion: '9.0'\n\
        importers:\n  \
          .:\n    \
            dependencies:\n      \
              tar-dep:\n        \
                specifier: file:vendor/dep.tgz\n        \
                version: file:vendor/dep.tgz\n\
        packages:\n  \
          'tar-dep@file:vendor/dep.tgz':\n    \
            resolution: {tarball: file:vendor/dep.tgz}\n    \
            version: ../../escaped\n\
        snapshots:\n  \
          'tar-dep@file:vendor/dep.tgz': {}\n";
    fs::write(workspace.join("pnpm-lock.yaml"), lockfile).expect("write lockfile");

    let output = pacquet
        .with_args(["install", "--frozen-lockfile", "--trust-lockfile"])
        .output()
        .expect("spawn pacquet install");

    assert!(
        !output.status.success(),
        "traversal version in global virtual store must be rejected (stderr: {})",
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        stderr.contains("ERR_PNPM_INVALID_DEPENDENCY_NAME"),
        "stderr must name the invalid-dependency-name code; got:\n{stderr}",
    );

    let gvs_root = store_dir.join("v11").join("links");
    assert!(
        !gvs_root.join("escaped").exists(),
        "escaped directory must not be created under GVS root",
    );

    drop((root, mock_instance));
}

fn acme_packument(registry_url: &str, name: &str, integrity: &str, published_at: &str) -> String {
    let bare = name.rsplit('/').next().unwrap_or(name);
    serde_json::json!({
        "name": name,
        "modified": published_at,
        "dist-tags": { "latest": "1.0.0" },
        "time": { "1.0.0": published_at },
        "versions": {
            "1.0.0": {
                "name": name,
                "version": "1.0.0",
                "dist": {
                    "integrity": integrity,
                    "shasum": "0000000000000000000000000000000000000000",
                    "tarball": format!("{registry_url}/{name}/-/{bare}-1.0.0.tgz"),
                }
            }
        }
    })
    .to_string()
}

/// Switching `--registry` or `@scope:registry` must miss
/// `<cacheDir>/lockfile-verified.jsonl` and re-verify against the new
/// registry rather than reusing a verdict recorded for the previous one.
#[test]
fn changing_default_or_scoped_registry_invalidates_lockfile_verified_cache() {
    let CommandTempCwd { root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let mut registry_a = mockito::Server::new();
    let mut registry_b = mockito::Server::new();
    let tarball =
        gzipped_tarball(&[("package/package.json", r#"{"name":"@corp/acme","version":"1.0.0"}"#)]);
    let integrity = ssri::Integrity::from(&tarball).to_string();
    let mature = "2020-01-01T00:00:00.000Z";
    let immature = chrono::Utc::now().to_rfc3339();

    let meta_a = registry_a
        .mock("GET", "/@corp%2Facme")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(acme_packument(&registry_a.url(), "@corp/acme", &integrity, mature))
        .expect(1)
        .create();
    let _tarball_a = registry_a
        .mock("GET", "/@corp/acme/-/acme-1.0.0.tgz")
        .with_status(200)
        .with_body(tarball)
        .expect(1)
        .create();
    let meta_b = registry_b
        .mock("GET", "/@corp%2Facme")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(acme_packument(&registry_b.url(), "@corp/acme", &integrity, &immature))
        .expect_at_least(2)
        .create();

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "@corp/acme": "1.0.0" } }).to_string(),
    )
    .expect("write package.json");
    set_minimum_release_age(&workspace, 1440);
    let lockfile = format!(
        "lockfileVersion: '9.0'\n\
         importers:\n  \
           .:\n    \
             dependencies:\n      \
               '@corp/acme':\n        \
                 specifier: 1.0.0\n        \
                 version: 1.0.0\n\
         packages:\n  \
           '@corp/acme@1.0.0':\n    \
             resolution: {{integrity: {integrity}}}\n\
         snapshots:\n  \
           '@corp/acme@1.0.0': {{}}\n",
    );
    fs::write(workspace.join("pnpm-lock.yaml"), lockfile).expect("write lockfile");

    let reg_a_arg = format!("--registry={}/", registry_a.url());
    let first = pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile", &reg_a_arg])
        .output()
        .expect("first install");
    assert_success(&first);

    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");
    let cached = pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile", &reg_a_arg])
        .output()
        .expect("cached install");
    assert_success(&cached);
    meta_a.assert();

    let reg_b_arg = format!("--registry={}/", registry_b.url());
    let changed_default = pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile", &reg_b_arg])
        .output()
        .expect("install with changed default registry");
    let default_stderr = String::from_utf8_lossy(&changed_default.stderr);
    assert!(
        !changed_default.status.success()
            && default_stderr.contains("ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION"),
        "changing default registry must invalidate lockfile-verified.jsonl; got:\n{default_stderr}",
    );

    fs::write(workspace.join(".npmrc"), format!("@corp:registry={}/\n", registry_b.url()))
        .expect("write .npmrc");
    let changed_scope = pacquet_in(&workspace)
        .with_args(["install", "--frozen-lockfile", &reg_a_arg])
        .output()
        .expect("install with changed @corp scope registry");
    let scope_stderr = String::from_utf8_lossy(&changed_scope.stderr);
    assert!(
        !changed_scope.status.success()
            && scope_stderr.contains("ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION"),
        "changing @scope:registry must invalidate lockfile-verified.jsonl; got:\n{scope_stderr}",
    );
    meta_b.assert();

    drop((root, mock_instance));
}
