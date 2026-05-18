//! End-to-end CLI integration test for the lockfile-verification
//! gate ported in Phase 7. Spawns the `pacquet` binary against a
//! pnpm-workspace.yaml that activates the verifier and confirms the
//! gate fires through the real install path — non-zero exit, the
//! upstream-canonical diagnostic code in stderr.
//!
//! Doesn't try to exercise every branch — the unit tests in
//! `pacquet-lockfile-verification` and `pacquet-resolving-npm-resolver`
//! already do that. This file pins the user-visible contract: the
//! gate runs from the CLI, the install fails when policy is
//! tripped, and the error envelope carries the upstream code so
//! `pnpm errors` documentation routes to the right entry.

pub mod _utils;
pub use _utils::*;

use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::fs;

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

    let manifest_path = workspace.join("package.json");
    let package_json = serde_json::json!({
        "dependencies": {
            "@pnpm.e2e/hello-world-js-bin": "1.0.0",
        },
    });
    fs::write(&manifest_path, package_json.to_string()).expect("write package.json");

    // The mocked registry's packument times are real-world (years
    // old), so a `minimumReleaseAge` set in the millions of minutes
    // catches every version regardless of when the mock was
    // populated. The yaml entry shape matches upstream's
    // pnpm-workspace.yaml settings keys byte-for-byte.
    let workspace_yaml_path = workspace.join("pnpm-workspace.yaml");
    let workspace_yaml = format!(
        "{}\nminimumReleaseAge: {}\n",
        fs::read_to_string(&workspace_yaml_path).expect("read workspace yaml seed"),
        60 * 24 * 365 * 100,
    );
    fs::write(&workspace_yaml_path, workspace_yaml).expect("write pnpm-workspace.yaml");

    // Hand-rolled minimal v9 lockfile pinning the same package the
    // manifest above declares. The placeholder integrity is fine:
    // the gate rejects the entry before the tarball is verified.
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
