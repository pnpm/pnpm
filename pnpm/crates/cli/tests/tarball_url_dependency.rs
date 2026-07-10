//! Remote (non-registry) https-tarball *direct* dependencies install
//! end to end, recording the computed integrity in the lockfile.
//!
//! URL/tarball resolvers carry no `name@version`/`integrity` at resolve
//! time — those live in the tarball's `package.json`. pacquet builds
//! the lockfile before the install pass, so the [`TarballResolver`]
//! downloads the tarball during resolution to compute its sha512
//! integrity and read its manifest (see <https://github.com/pnpm/pnpm/issues/12053>).
//!
//! The scenario also guards pnpm issues
//! [#12001](https://github.com/pnpm/pnpm/issues/12001) (fixed upstream
//! in [#12040](https://github.com/pnpm/pnpm/pull/12040)) and
//! [#12067](https://github.com/pnpm/pnpm/issues/12067) (fixed upstream in
//! [#12096](https://github.com/pnpm/pnpm/pull/12096)): installing an
//! *unrelated* package rewrites the lockfile while the tarball
//! dependency is re-resolved, and its integrity must survive so the next
//! `--frozen-lockfile` install doesn't fail closed.
//!
//! Both upstream bugs stem from pnpm's URL/tarball resolver returning no
//! integrity (it's learned only on download) and a later fetch step being
//! skipped on a warm store — so pnpm has to carry the previous lockfile
//! entry's integrity forward. pacquet has no such gap: because it builds
//! the lockfile before the install pass, the [`TarballResolver`] learns the
//! integrity from the tarball's bytes (a download, or — on a re-resolve
//! where the prior lockfile already recorded the URL + integrity — a reuse
//! of the warm store extraction, see the no-refetch test below). Either way
//! a re-resolved entry can never lose its integrity, so pacquet needs no
//! carry-forward equivalent of pnpm's `packageRequester` / `updateLockfile`
//! fixes.
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
use std::{fs, path::Path, process::Command, thread::sleep, time::Duration};

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
        // `integrity:` appears either on its own line (block style) or inside
        // the single-line `resolution: {integrity: ..., tarball: ...}` flow map,
        // so extract the value up to the next `,` / `}` / end-of-line. Match
        // only the YAML key token (start-of-line, indent, `{`, or `,` before
        // it) so a tarball URL/path containing the substring can't masquerade
        // as the field and hide a genuinely missing `integrity`.
        .find_map(|line| {
            let key_at = line.match_indices("integrity:").find(|(idx, _)| {
                matches!(line[..*idx].chars().next_back(), None | Some(' ' | '{' | ','))
            })?;
            let rest = line[key_at.0 + "integrity:".len()..].trim_start();
            let end = rest.find([',', '}']).unwrap_or(rest.len());
            Some(rest[..end].trim().to_string())
        })
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
    // Ensure the manifest mtime is observably newer than the first
    // install's workspace-state validation timestamp; otherwise the
    // optimistic repeat-install shortcut can legitimately skip resolution.
    sleep(Duration::from_millis(20));
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

/// Build a minimal gzipped npm tarball carrying just a `package.json`
/// under the conventional top-level `package/` directory (which the
/// extractor strips). Enough for the resolver to learn the package's
/// name/version + compute an integrity, without a real registry.
fn minimal_tarball(name: &str, version: &str) -> Vec<u8> {
    use std::io::Write;
    let manifest = serde_json::json!({ "name": name, "version": version }).to_string();
    let manifest = manifest.as_bytes();

    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_path("package/package.json").expect("set tar entry path");
    header.set_size(manifest.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append(&header, manifest).expect("append package.json to tar");
    let tar_bytes = builder.into_inner().expect("finish tar");

    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar_bytes).expect("gzip tar");
    encoder.finish().expect("finish gzip")
}

/// On re-resolution, a remote tarball already recorded in the lockfile is
/// reused from the warm store instead of being downloaded again.
///
/// pnpm's URL/tarball resolver never downloads at resolve time, so a
/// re-resolve against a warm store skips the fetch entirely. pacquet
/// downloads during resolution to learn the integrity + manifest, so
/// without reuse it would re-fetch the tarball on every re-resolution
/// ([PR #12096](https://github.com/pnpm/pnpm/pull/12096)). The resolver now consults the prior lockfile + store
/// index and, on a hit, reuses the cached integrity + bundled manifest
/// without touching the network.
///
/// The proof serves the tarball from a throwaway HTTP server, then tears
/// it down before the re-resolve: a fresh install warms the store and
/// records the integrity; `pacquet update` then forces a re-resolution
/// with the server gone. It can only succeed if the resolver reused the
/// warm store entry instead of re-fetching the (now unreachable) tarball.
#[test]
fn remote_tarball_reresolves_from_warm_store_without_refetch() {
    // `add_mocked_registry` supplies the npmrc/registry the install needs;
    // the tarball itself is served separately so its host can be torn down
    // independently of the (process-global) mock registry.
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let tarball_path = "/pkg-from-tarball-1.0.0.tgz";
    let tarball = minimal_tarball("pkg-from-tarball", "1.0.0");
    let mut tarball_server = mockito::Server::new();
    let head_mock = tarball_server.mock("HEAD", tarball_path).with_status(200).create();
    let get_mock =
        tarball_server.mock("GET", tarball_path).with_status(200).with_body(tarball).create();
    // A host distinct from the configured registry, so the URL is treated
    // as a remote (non-registry) tarball and claimed by the TarballResolver.
    let tarball_url = format!("{}{tarball_path}", tarball_server.url());
    let package_key = format!("pkg-from-tarball@{tarball_url}");
    let manifest_path = workspace.join("package.json");
    let lockfile_path = workspace.join("pnpm-lock.yaml");

    fs::write(
        &manifest_path,
        serde_json::json!({ "dependencies": { "pkg-from-tarball": &tarball_url } }).to_string(),
    )
    .expect("write package.json");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let lockfile = fs::read_to_string(&lockfile_path).expect("read pnpm-lock.yaml");
    package_integrity(&lockfile, &package_key).unwrap_or_else(|| {
        panic!("the fresh install must record an integrity for the tarball dep:\n{lockfile}")
    });

    // Tear the tarball server down. Any re-fetch attempt now fails.
    drop((head_mock, get_mock, tarball_server));

    // `pacquet update` re-resolves the tarball dependency. With the server
    // gone it can only succeed by reusing the warm store entry rather than
    // re-downloading.
    pacquet_at(&workspace).with_arg("update").assert().success();

    drop((root, mock_instance));
}
