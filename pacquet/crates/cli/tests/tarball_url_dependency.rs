//! Integrity preservation for remote https-tarball dependencies.
//!
//! URL/tarball resolvers carry no `integrity` at resolve time — it is
//! only known after the tarball is downloaded and hashed. The risk
//! (pnpm issue [#12001](https://github.com/pnpm/pnpm/issues/12001),
//! fixed upstream in [#12040](https://github.com/pnpm/pnpm/pull/12040))
//! is that installing an *unrelated* package later rewrites the
//! lockfile while the tarball dependency is reused without being
//! re-fetched, dropping its recorded integrity — which then makes the
//! next `--frozen-lockfile` install fail closed.
//!
//! Reproducing that in pacquet requires resolving a dependency through
//! the [`TarballResolver`], which only claims a bare specifier whose
//! URL does *not* start with the configured registry. A registry-host
//! tarball URL is parsed by the npm resolver instead (see
//! `parse_bare_specifier`), so it carries the registry's integrity from
//! metadata and never exercises the reuse path [#12001](https://github.com/pnpm/pnpm/issues/12001) is about.
//!
//! Pacquet doesn't support remote (non-registry) https-tarball *direct
//! dependencies* end to end yet, so the scenario below is a
//! [`known_failures`] entry. See that module for the exact gap.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use known_failures::external_tarball_dependency_unsupported;
use pacquet_testing_utils::{
    allow_known_failure,
    bin::{AddMockedRegistry, CommandTempCwd},
};
use std::{fs, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

/// The `integrity:` recorded for a `packages:` entry, e.g.
/// `is-positive@1.0.0`. `None` when the entry is absent or carries no
/// integrity (the [#12001](https://github.com/pnpm/pnpm/issues/12001) regression).
fn package_integrity(lockfile: &str, package_key: &str) -> Option<String> {
    let header = format!("{package_key}:");
    lockfile
        .lines()
        .skip_while(|line| line.trim() != header)
        .take_while(|line| !line.trim_start().starts_with("snapshots:"))
        .find_map(|line| line.trim().strip_prefix("integrity:").map(|rest| rest.trim().to_string()))
}

/// A remote-tarball dependency keeps its integrity when an unrelated
/// dependency is added and the lockfile is rewritten, so the next
/// `--frozen-lockfile` install still succeeds.
#[test]
fn remote_tarball_integrity_survives_unrelated_install() {
    allow_known_failure!(external_tarball_dependency_unsupported());

    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // The registry is `http://localhost:PORT/`; pointing at the same
    // loopback server via `127.0.0.1` keeps the URL from matching the
    // registry prefix, so the TarballResolver — not the npm resolver —
    // claims it. The tarball is still downloadable from that server.
    let tarball = format!(
        "{}is-positive/-/is-positive-1.0.0.tgz",
        mock_instance.url().replace("localhost", "127.0.0.1"),
    );
    let manifest_path = workspace.join("package.json");
    let lockfile_path = workspace.join("pnpm-lock.yaml");

    fs::write(
        &manifest_path,
        serde_json::json!({ "dependencies": { "is-positive": tarball } }).to_string(),
    )
    .expect("write package.json");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    let integrity = package_integrity(&lockfile, "is-positive@1.0.0").unwrap_or_else(|| {
        panic!("the fresh install must record an integrity for the tarball dep:\n{lockfile}")
    });

    // Install an unrelated package. This rewrites the lockfile while the
    // tarball dependency is reused — the exact #12001 trigger.
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": { "is-positive": tarball, "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }
        })
        .to_string(),
    )
    .expect("rewrite package.json with an unrelated dependency");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    assert!(
        lockfile.contains("@pnpm.e2e/pkg-with-1-dep@100.0.0"),
        "the unrelated dependency must be recorded:\n{lockfile}",
    );
    assert_eq!(
        package_integrity(&lockfile, "is-positive@1.0.0").as_deref(),
        Some(integrity.as_str()),
        "the tarball dependency's integrity must be preserved verbatim:\n{lockfile}",
    );

    // The frozen install is the symptom #12001 reports: it fails closed
    // when the tarball entry has lost its integrity.
    pacquet_at(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    drop((root, mock_instance));
}

mod known_failures {
    //! Subject-under-test not built yet, stubbed through
    //! [`pacquet_testing_utils::allow_known_failure`] so the port exits
    //! early instead of masking a real bug.

    use pacquet_testing_utils::known_failure::{KnownFailure, KnownResult};

    /// A remote, non-registry https-tarball *direct dependency* (e.g.
    /// `https://cdn.example.com/foo-1.0.0.tgz`) cannot be installed yet:
    /// `TarballResolver` returns no `name_ver` (the name/version live in
    /// the tarball's manifest, which pacquet doesn't fetch during
    /// resolution), so `dependencies_graph_to_lockfile` panics with
    /// `MissingSuffix` building the importer dep path. Until the
    /// resolve-time tarball-manifest fetch (and the integrity it
    /// computes) lands, pnpm [#12001](https://github.com/pnpm/pnpm/issues/12001)'s integrity-preservation-on-reuse
    /// isn't reachable here. Registry-host tarball URLs take the npm
    /// resolver path instead and already carry integrity from metadata.
    /// Tracked in <https://github.com/pnpm/pnpm/issues/12053>.
    pub fn external_tarball_dependency_unsupported() -> KnownResult<()> {
        Err(KnownFailure::new(
            "remote non-registry https-tarball direct dependencies are unsupported",
        ))
    }
}
