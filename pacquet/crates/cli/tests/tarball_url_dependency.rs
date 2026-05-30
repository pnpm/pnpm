//! Remote (non-registry) https-tarball *direct* dependencies install
//! end to end, recording the computed integrity in the lockfile.
//!
//! URL/tarball resolvers carry no `name@version`/`integrity` at resolve
//! time — those live in the tarball's `package.json`. pacquet builds
//! the lockfile before the install pass, so the [`TarballResolver`]
//! downloads the tarball during resolution to compute its sha512
//! integrity and read its manifest (see <https://github.com/pnpm/pnpm/issues/12053>).
//!
//! The scenario also guards pnpm issue
//! [#12001](https://github.com/pnpm/pnpm/issues/12001) (fixed upstream
//! in [#12040](https://github.com/pnpm/pnpm/pull/12040)): installing an
//! *unrelated* package rewrites the lockfile while the tarball
//! dependency is re-resolved, and its integrity must survive so the next
//! `--frozen-lockfile` install doesn't fail closed.
//!
//! Reaching the [`TarballResolver`] requires a bare specifier whose URL
//! does *not* start with the configured registry — a registry-host
//! tarball URL is parsed by the npm resolver instead (see
//! `parse_bare_specifier`) and carries the registry's integrity from
//! metadata. The test points at the loopback registry via `localhost`
//! while it's configured as `127.0.0.1` so the URL prefix doesn't match.

use assert_cmd::prelude::*;
use command_extra::CommandExtra;
use pacquet_testing_utils::bin::{AddMockedRegistry, CommandTempCwd};
use std::{fs, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pacquet").expect("find the pacquet binary").with_current_dir(workspace)
}

/// The `integrity:` recorded for a `packages:` entry keyed by
/// `package_key` (e.g. `is-positive@<tarball-url>`). `None` when the
/// entry is absent or carries no integrity (the
/// <https://github.com/pnpm/pnpm/issues/12001> regression).
fn package_integrity(lockfile: &str, package_key: &str) -> Option<String> {
    // The `packages:` key for a tarball-URL dep contains `://` and a
    // `:port`, which the YAML emitter wraps in double quotes; the lookup
    // tolerates either the quoted or bare form.
    let is_header = |line: &str| {
        let trimmed = line.trim().trim_end_matches(':');
        trimmed == package_key || trimmed.trim_matches('"') == package_key
    };
    let mut lines = lockfile.lines().skip_while(|line| !is_header(line));
    let header = lines.next()?;
    let header_indent = header.len() - header.trim_start().len();

    // Stop at the next sibling entry (a key at the header's indent or
    // shallower, e.g. the next `packages:` member or `snapshots:`) so a
    // tarball entry that lost its own `integrity:` can't borrow another
    // package's.
    lines
        .take_while(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("snapshots:")
                && (!trimmed.ends_with(':') || (line.len() - trimmed.len()) > header_indent)
        })
        .find_map(|line| line.trim().strip_prefix("integrity:").map(|rest| rest.trim().to_string()))
}

/// A remote-tarball dependency keeps its integrity when an unrelated
/// dependency is added and the lockfile is rewritten, so the next
/// `--frozen-lockfile` install still succeeds.
#[test]
fn remote_tarball_integrity_survives_unrelated_install() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    // The mocked registry is `http://127.0.0.1:PORT/`; pointing at the
    // same loopback server via `localhost` keeps the URL from matching
    // the registry prefix, so the TarballResolver — not the npm resolver
    // — claims it. `localhost` resolves to 127.0.0.1, so the tarball is
    // still downloadable from that server.
    let tarball = format!(
        "{}is-positive/-/is-positive-1.0.0.tgz",
        mock_instance.url().replace("127.0.0.1", "localhost"),
    );
    // A non-registry tarball is keyed by `name@<url>` (the version lives
    // in `resolution.tarball` + the `version:` field), not `name@1.0.0`.
    // Mirrors pnpm — see `installing/deps-installer/test/lockfile.ts`
    // ("packages installed via tarball URL ... are normalized").
    let package_key = format!("is-positive@{tarball}");
    let manifest_path = workspace.join("package.json");
    let lockfile_path = workspace.join("pnpm-lock.yaml");

    fs::write(
        &manifest_path,
        serde_json::json!({ "dependencies": { "is-positive": tarball } }).to_string(),
    )
    .expect("write package.json");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    let integrity = package_integrity(&lockfile, &package_key).unwrap_or_else(|| {
        panic!("the fresh install must record an integrity for the tarball dep:\n{lockfile}")
    });

    // Install an unrelated package. This rewrites the lockfile while the
    // tarball dependency is re-resolved — the exact
    // <https://github.com/pnpm/pnpm/issues/12001> trigger.
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
        package_integrity(&lockfile, &package_key).as_deref(),
        Some(integrity.as_str()),
        "the tarball dependency's integrity must be preserved verbatim:\n{lockfile}",
    );

    // The frozen install is the symptom
    // <https://github.com/pnpm/pnpm/issues/12001> reports: it fails
    // closed when the tarball entry has lost its integrity.
    pacquet_at(&workspace).with_args(["install", "--frozen-lockfile"]).assert().success();

    drop((root, mock_instance));
}
