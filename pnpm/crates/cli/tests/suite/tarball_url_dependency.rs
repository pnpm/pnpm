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
use pnpm_testing_utils::{
    bin::{AddMockedRegistry, CommandTempCwd},
    fixtures::minimal_tarball,
    fs::bump_mtime,
};
use std::{fs, path::Path, process::Command};

fn pacquet_at(workspace: &Path) -> Command {
    Command::cargo_bin("pnpm").expect("find the pnpm binary").with_current_dir(workspace)
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
    fs::write(
        &manifest_path,
        serde_json::json!({
            "dependencies": { "is-positive": tarball, "@pnpm.e2e/pkg-with-1-dep": "100.0.0" }
        })
        .to_string(),
    )
    .expect("rewrite package.json with an unrelated dependency");
    bump_mtime(&manifest_path);
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

/// A `packages:` entry whose tarball resolution records no
/// `integrity` is refused for a plain remote tarball: the bytes come
/// off the network and nothing in the lockfile pins them. pnpm fails
/// the same entry closed
/// ([#13308](https://github.com/pnpm/pnpm/issues/13308)).
///
/// The refusal comes from the lockfile-verification gate, which batches
/// every offending entry into one error before anything is fetched —
/// not from the per-entry fetch-path backstop
/// (`tarball_url_and_integrity`), which only speaks up for entries that
/// reach a fetch without having passed the gate
/// ([#13364](https://github.com/pnpm/pnpm/issues/13364)). The
/// zero-request expectation on the re-armed tarball mocks is what pins
/// "before anything is fetched": a refusal that only came from the fetch
/// path would have downloaded the bytes first.
///
/// Git-host archive URLs are the exemption — they are what older pnpm
/// versions wrote without an `integrity` — and that shape is covered
/// by `unverified_fetch_is_allowed`'s unit tests, since no local
/// server can answer for a git host.
#[test]
fn frozen_install_refuses_a_remote_tarball_without_integrity() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let tarball_path = "/pkg-from-tarball-1.0.0.tgz";
    let mut tarball_server = mockito::Server::new();
    let head_mock = tarball_server.mock("HEAD", tarball_path).with_status(200).create();
    let get_mock = tarball_server
        .mock("GET", tarball_path)
        .with_status(200)
        .with_body(minimal_tarball("pkg-from-tarball", "1.0.0"))
        .create();
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
    let integrity = package_integrity(&lockfile, &package_key).unwrap_or_else(|| {
        panic!("the fresh install must record an integrity for the tarball dep:\n{lockfile}")
    });
    let stripped = lockfile.replace(&format!("integrity: {integrity}, "), "");
    assert!(
        package_integrity(&stripped, &package_key).is_none(),
        "the fixture must end up without an integrity:\n{stripped}",
    );
    fs::write(&lockfile_path, &stripped).expect("write pnpm-lock.yaml");
    fs::remove_dir_all(workspace.join("node_modules")).expect("remove node_modules");

    // Re-arm the same endpoints with a zero-request expectation, so the
    // frozen install below has to fail *before* fetching: the mocks still
    // answer, and answering is what the assertion catches.
    drop((head_mock, get_mock));
    let head_mock = tarball_server.mock("HEAD", tarball_path).with_status(200).expect(0).create();
    let get_mock = tarball_server
        .mock("GET", tarball_path)
        .with_status(200)
        .with_body(minimal_tarball("pkg-from-tarball", "1.0.0"))
        .expect(0)
        .create();

    let output = pacquet_at(&workspace)
        .with_args(["install", "--frozen-lockfile"])
        .assert()
        .failure()
        .get_output()
        .clone();
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    // miette wraps the rendered message to the terminal width, so the
    // sentences are matched against a single-spaced rendering.
    let unwrapped = stderr.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        unwrapped.contains("ERR_PNPM_MISSING_TARBALL_INTEGRITY")
            && unwrapped.contains("1 lockfile entries failed verification")
            && unwrapped.contains(
                r#"has no "integrity" field, so its downloaded tarball cannot be verified"#
            )
            && unwrapped.contains(r#"run "pnpm clean --lockfile""#),
        "the verification gate must name the error code, the entry, and the way out; got:\n{stderr}",
    );
    head_mock.assert();
    get_mock.assert();

    drop((root, mock_instance, tarball_server));
}

/// A remote tarball lands in the store index under exactly one row, keyed
/// by the bare URL — the `pkgId` the TypeScript CLI writes, so a store
/// warmed by one stack stays warm for the other
/// ([#13365](https://github.com/pnpm/pnpm/issues/13365)).
///
/// One row, not two: the resolve-time fetch and the install pass address
/// the same key, so a tarball costs one index entry and one extraction.
#[test]
fn a_remote_tarball_is_indexed_once_under_the_bare_url() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let store_dir = pnpm_store_dir::StoreDir::from(npmrc_info.store_dir.clone());
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let tarball_path = "/pkg-from-tarball-1.0.0.tgz";
    let mut tarball_server = mockito::Server::new();
    let head_mock = tarball_server.mock("HEAD", tarball_path).with_status(200).create();
    let get_mock = tarball_server
        .mock("GET", tarball_path)
        .with_status(200)
        .with_body(minimal_tarball("pkg-from-tarball", "1.0.0"))
        .create();
    let tarball_url = format!("{}{tarball_path}", tarball_server.url());

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "pkg-from-tarball": &tarball_url } }).to_string(),
    )
    .expect("write package.json");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let keys = pnpm_store_dir::StoreIndex::open_readonly_in(&store_dir)
        .expect("open the store index")
        .keys()
        .expect("read the store index keys");
    let rows: Vec<&String> =
        keys.iter().filter(|key| key.contains("pkg-from-tarball")).collect::<Vec<_>>();
    assert_eq!(rows.len(), 1, "one tarball dependency must occupy one store-index row: {keys:?}");
    assert!(
        rows[0].ends_with(&format!("\t{tarball_url}")),
        "the store-index row must be keyed by the bare tarball URL: {:?}",
        rows[0],
    );

    drop((head_mock, get_mock, root, mock_instance, tarball_server));
}

/// A tarball whose host answers the preflight with an immutable redirect
/// is still reused from a warm store on re-resolution.
///
/// The lockfile records the *post-redirect* URL in `resolution.tarball`
/// while the entry's `pkg_id` — its key, and the store-index row it
/// lands at — stays the bare specifier the manifest asked for. A
/// re-resolve looks the warm entry up by that specifier, before any
/// preflight has revealed the redirect, so both sides have to agree on
/// it. The proof is the same as
/// [`remote_tarball_reresolves_from_warm_store_without_refetch`]: tear
/// the host down, then re-resolve.
#[test]
fn remote_tarball_behind_an_immutable_redirect_reuses_the_warm_store() {
    let CommandTempCwd { workspace, root, npmrc_info, .. } =
        CommandTempCwd::init().add_mocked_registry();
    let AddMockedRegistry { mock_instance, .. } = npmrc_info;

    let requested_path = "/pkg-from-tarball.tgz";
    let canonical_path = "/cdn/pkg-from-tarball-1.0.0.tgz";
    let mut tarball_server = mockito::Server::new();
    let canonical_url = format!("{}{canonical_path}", tarball_server.url());
    let redirect_mock = tarball_server
        .mock("HEAD", requested_path)
        .with_status(302)
        .with_header("location", &canonical_url)
        .create();
    let head_mock = tarball_server
        .mock("HEAD", canonical_path)
        .with_status(200)
        .with_header("cache-control", "immutable")
        .create();
    let get_mock = tarball_server
        .mock("GET", canonical_path)
        .with_status(200)
        .with_body(minimal_tarball("pkg-from-tarball", "1.0.0"))
        .create();
    let requested_url = format!("{}{requested_path}", tarball_server.url());

    fs::write(
        workspace.join("package.json"),
        serde_json::json!({ "dependencies": { "pkg-from-tarball": &requested_url } }).to_string(),
    )
    .expect("write package.json");
    pacquet_at(&workspace).with_arg("install").assert().success();

    let lockfile = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read the lockfile");
    assert!(
        lockfile.contains(&canonical_url),
        "the lockfile must record the post-redirect URL:\n{lockfile}",
    );
    let package_key = format!("pkg-from-tarball@{requested_url}");
    package_integrity(&lockfile, &package_key).unwrap_or_else(|| {
        panic!("the entry must be keyed by the requested URL and carry an integrity:\n{lockfile}")
    });

    drop((redirect_mock, head_mock, get_mock, tarball_server));

    pacquet_at(&workspace).with_arg("update").assert().success();

    let after = fs::read_to_string(workspace.join("pnpm-lock.yaml")).expect("read the lockfile");
    assert_eq!(after, lockfile, "a warm re-resolve must not rewrite the entry");

    drop((root, mock_instance));
}
