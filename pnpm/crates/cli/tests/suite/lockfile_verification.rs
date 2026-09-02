//! End-to-end CLI integration test for the lockfile-verification
//! gate ported in Phase 7. Spawns the `pacquet` binary against a
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

use crate::_utils;
pub use _utils::*;

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
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
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
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    set_minimum_release_age(&workspace, 60 * 24 * 365 * 100);
    append_workspace_yaml_key(&workspace, "minimumReleaseAgeStrict", false);

    let output =
        pacquet.with_args(["add", "@pnpm.e2e/bravo-dep@1.0"]).output().expect("spawn pacquet add");
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
        snapshots.keys().any(|key| key.to_string() == "@pnpm.e2e/bravo-dep@1.0.0"),
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
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_policy_rejected_project(&workspace);
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
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    write_policy_rejected_project(&workspace);

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

/// `remove` declares no clap flag for `trustLockfile`; the bare spelling
/// reaches it as a setting. The lockfile is verified after the removal is
/// applied to it, so the provocation keeps a second rejected entry that
/// survives the removal: without the flag the gate fires on that entry and
/// the manifest is left alone, with the flag the removal completes.
#[test]
fn remove_honors_the_bare_trust_lockfile_flag() {
    let CommandTempCwd { pacquet: initial_install, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
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
    let CommandTempCwd { pacquet, root, workspace, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
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
