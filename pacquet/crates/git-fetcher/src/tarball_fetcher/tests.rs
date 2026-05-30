use super::GitHostedTarballFetcher;
use crate::error::{GitFetcherError, PreparePackageError};
use pacquet_executor::ScriptsPrependNodePath;
use pacquet_reporter::SilentReporter;
use pacquet_store_dir::{StoreDir, StoreIndex, StoreIndexWriter};
use std::{collections::HashMap, fs, path::PathBuf, sync::Arc};
use tempfile::tempdir;

fn deny_all_builds<'a>() -> &'a (dyn Fn(&str, &str) -> bool + Send + Sync) {
    &|_, _| false
}

/// Build the `cas_paths` map the dispatcher would hand the fetcher
/// after `DownloadTarballToStore` finishes: a fresh `StoreDir`, a few
/// files written via `write_cas_file`, and a `path → cas_path` map.
fn write_to_cas(store_dir: &StoreDir, files: &[(&str, &[u8], bool)]) -> HashMap<String, PathBuf> {
    let mut out = HashMap::new();
    for &(rel, bytes, executable) in files {
        let (cas_path, _hash) = store_dir.write_cas_file(bytes, executable).unwrap();
        out.insert(rel.to_string(), cas_path);
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn passes_through_package_without_scripts() {
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());

    let cas_paths = write_to_cas(
        &store_dir,
        &[
            ("package.json", br#"{"name":"x","version":"1.0.0","main":"index.js"}"#, false),
            ("index.js", b"module.exports = 42;\n", false),
            // A README that the packlist's always-included rule
            // should preserve regardless of the (absent) `files`
            // field.
            ("README.md", b"# x\n", false),
        ],
    );

    let received = GitHostedTarballFetcher {
        cas_paths: cas_paths.clone(),
        path: None,
        allow_build: deny_all_builds(),
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        store_dir: &store_dir,
        package_id: "x@1.0.0",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "x@1.0.0\tbuilt",
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    assert!(!received.built, "no `prepare` script → not built");
    assert!(received.cas_paths.contains_key("package.json"));
    assert!(received.cas_paths.contains_key("index.js"));
    assert!(received.cas_paths.contains_key("README.md"));

    // Hash-dedup: re-importing the same bytes lands at the same CAS
    // path, so the new map's CAS entries point at the same files we
    // wrote up front.
    for (rel, original) in &cas_paths {
        assert_eq!(received.cas_paths.get(rel), Some(original), "deterministic CAS path for {rel}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn filters_files_outside_files_field() {
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());

    let cas_paths = write_to_cas(
        &store_dir,
        &[
            ("package.json", br#"{"name":"x","version":"1.0.0","files":["dist/**"]}"#, false),
            ("dist/index.js", b"// built\n", false),
            ("src/index.ts", b"// source\n", false),
            ("test/foo.test.js", b"// test\n", false),
        ],
    );

    let received = GitHostedTarballFetcher {
        cas_paths,
        path: None,
        allow_build: deny_all_builds(),
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        store_dir: &store_dir,
        package_id: "x@1.0.0",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "x@1.0.0\tbuilt",
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    let keys: Vec<&str> = received.cas_paths.keys().map(String::as_str).collect();
    assert!(keys.contains(&"dist/index.js"));
    assert!(keys.contains(&"package.json"), "package.json always included");
    assert!(!keys.contains(&"src/index.ts"), "src excluded by files field");
    assert!(!keys.contains(&"test/foo.test.js"), "test excluded by files field");
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_build_when_not_allowed() {
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());

    let cas_paths = write_to_cas(
        &store_dir,
        &[
            (
                "package.json",
                br#"{"name":"naughty","version":"2.0.0","main":"index.js","scripts":{"prepare":"tsc"}}"#,
                false,
            ),
            ("index.js", b"module.exports = 1;\n", false),
        ],
    );

    let err = GitHostedTarballFetcher {
        cas_paths,
        path: None,
        allow_build: deny_all_builds(),
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        store_dir: &store_dir,
        package_id: "naughty@2.0.0",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "naughty@2.0.0\tbuilt",
    }
    .run::<SilentReporter>()
    .await
    .unwrap_err();

    match err {
        GitFetcherError::Prepare(PreparePackageError::NotAllowed { name, version }) => {
            assert_eq!(name, "naughty");
            assert_eq!(version, "2.0.0");
        }
        other => panic!("expected Prepare::NotAllowed, got {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn path_field_packs_only_subdirectory() {
    // Git-hosted tarballs from monorepos pin a `path` to point at the
    // sub-package they actually publish. The fetcher must run
    // `preparePackage` + `packlist` inside that sub-dir so the
    // resulting `cas_paths` only contain that package's files.
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());

    let cas_paths = write_to_cas(
        &store_dir,
        &[
            // Monorepo root manifest — not the published package.
            ("package.json", br#"{"name":"monorepo","version":"0.0.0","private":true}"#, false),
            // The sub-package we're packing.
            (
                "packages/sub/package.json",
                br#"{"name":"sub","version":"1.0.0","main":"index.js"}"#,
                false,
            ),
            ("packages/sub/index.js", b"module.exports = 1;\n", false),
            ("packages/sub/README.md", b"# sub\n", false),
            // A sibling package that must NOT end up in the result.
            ("packages/other/package.json", br#"{"name":"other","version":"1.0.0"}"#, false),
            ("packages/other/index.js", b"// other\n", false),
        ],
    );

    let received = GitHostedTarballFetcher {
        cas_paths,
        path: Some("packages/sub"),
        allow_build: deny_all_builds(),
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        store_dir: &store_dir,
        package_id: "sub@1.0.0",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "sub@1.0.0\tbuilt",
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    let keys: Vec<&str> = received.cas_paths.keys().map(String::as_str).collect();
    // The fetcher packlists relative to `pkg_dir` (which is
    // `<tmp>/packages/sub`), so the returned keys are *also* relative
    // to that sub-dir — never carrying the `packages/sub/` prefix.
    assert!(keys.contains(&"package.json"), "sub-dir manifest must be included");
    assert!(keys.contains(&"index.js"), "sub-dir main must be included");
    assert!(keys.contains(&"README.md"), "always-included file must be included");
    assert!(
        !keys.iter().any(|k| k.contains("other")),
        "sibling-package files must not appear in {keys:?}",
    );
    assert!(
        !keys.iter().any(|k| k.contains("packages/")),
        "keys are relative to the sub-dir, not the monorepo root: {keys:?}",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn materialized_temp_dir_does_not_corrupt_cas() {
    // Regression: when the prepare phase modifies a working-tree
    // file, the CAS entry it was sourced from must remain unchanged.
    // We exercise the materialization path explicitly: a fresh
    // working tree (made via `fs::copy`) should have a different
    // inode than the CAS entry on POSIX.
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());

    let cas_paths =
        write_to_cas(&store_dir, &[("package.json", br#"{"name":"x","version":"1.0.0"}"#, false)]);
    let original_cas_path = cas_paths["package.json"].clone();
    let cas_bytes_before = fs::read(&original_cas_path).unwrap();

    let _ = GitHostedTarballFetcher {
        cas_paths,
        path: None,
        allow_build: deny_all_builds(),
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        store_dir: &store_dir,
        package_id: "x@1.0.0",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "x@1.0.0\tbuilt",
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    let cas_bytes_after = fs::read(&original_cas_path).unwrap();
    assert_eq!(
        cas_bytes_before, cas_bytes_after,
        "fetcher must not mutate CAS entries it sourced from",
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn writes_index_row_when_writer_provided() {
    // Round-trip check: when the fetcher receives a `StoreIndexWriter`,
    // it queues a `PackageFilesIndex` row at the given key. A future
    // installs warm prefetch reads the same key and rebuilds the
    // `cas_paths` map without re-running the fetcher.
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);

    let cas_paths = write_to_cas(
        &store_dir,
        &[
            ("package.json", br#"{"name":"x","version":"1.0.0","main":"index.js"}"#, false),
            ("index.js", b"module.exports = 7;\n", false),
        ],
    );

    let key = "x@1.0.0\tbuilt";
    let received = GitHostedTarballFetcher {
        cas_paths,
        path: None,
        allow_build: deny_all_builds(),
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        store_dir: &store_dir,
        package_id: "x@1.0.0",
        requester: "/test",
        store_index_writer: Some(&Arc::clone(&writer)),
        files_index_file: key,
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    // Drop the producer handle so the writer task drains the channel
    // and exits; await its join so the row is committed before we
    // open a reader.
    drop(writer);
    writer_task.await.unwrap().unwrap();

    let index = StoreIndex::open_in(&store_dir).unwrap();
    let row = index.get(key).unwrap().expect("row must exist at the git-hosted key");
    assert_eq!(row.algo, "sha512");
    assert_eq!(row.requires_build, Some(received.built));
    let keys: Vec<&str> = row.files.keys().map(String::as_str).collect();
    assert!(keys.contains(&"package.json"), "package.json missing from row.files: {keys:?}");
    assert!(keys.contains(&"index.js"), "index.js missing from row.files: {keys:?}");

    // Per-file metadata must round-trip cleanly — the warm prefetch
    // reconstructs the CAS file path from `digest` + `mode`, and the
    // verify pass compares `size` against the on-disk file. If any
    // of these drift, a follow-up install would miss the cache and
    // silently fall through to the cold path.
    let pj = row.files.get("package.json").expect("package.json entry");
    assert!(!pj.digest.is_empty(), "digest must be populated");
    assert!(
        pj.digest.bytes().all(|b| b.is_ascii_hexdigit()),
        "digest must be hex: {:?}",
        pj.digest,
    );
    // The exec bit is captured via the POSIX mode. `package.json` is
    // a regular file, so on POSIX the mode lands as `0o644`; on
    // Windows pacquet writes a fixed `0o644` (matching
    // `add_files_from_dir`).
    assert_eq!(pj.mode & 0o777, 0o644, "package.json must be a non-executable regular-mode file");
    assert_eq!(pj.size as usize, br#"{"name":"x","version":"1.0.0","main":"index.js"}"#.len());
    assert_eq!(
        pj.checked_at, None,
        "freshly imported entries have no integrity-check timestamp yet",
    );
}

/// Fast path: when there's no sub-path, no build is needed, and the
/// packlist returns every input file, the fetcher must skip
/// `import_into_cas` and hand the input `cas_paths` straight back
/// to the dispatcher. Mirrors upstream's
/// [`gitHostedTarballFetcher.ts:88-100`](https://github.com/pnpm/pnpm/blob/94240bc046/fetching/tarball-fetcher/src/gitHostedTarballFetcher.ts#L88-L100)
/// "raw → prepared" promotion.
#[tokio::test(flavor = "multi_thread")]
async fn fast_path_returns_input_cas_paths_when_no_build_needed() {
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());

    // No `files` field, no `prepare` script — packlist returns
    // everything, `should_be_built` is false. All three input
    // entries (package.json + index.js + README.md) survive
    // packlist's always-included rules.
    let cas_paths = write_to_cas(
        &store_dir,
        &[
            ("package.json", br#"{"name":"x","version":"1.0.0","main":"index.js"}"#, false),
            ("index.js", b"module.exports = 42;\n", false),
            ("README.md", b"# x\n", false),
        ],
    );
    let input_snapshot = cas_paths.clone();

    let received = GitHostedTarballFetcher {
        cas_paths,
        path: None,
        allow_build: deny_all_builds(),
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        store_dir: &store_dir,
        package_id: "x@1.0.0",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "x@1.0.0\tbuilt",
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    // The returned map is the *exact* input — same keys, same CAS
    // paths. The slow path would re-import and produce a fresh map
    // that just happens to point at the same hashes; the fast path
    // skips that work entirely.
    assert!(!received.built);
    assert_eq!(received.cas_paths, input_snapshot, "fast path returns input cas_paths verbatim");
}

/// When the fast path triggers and a writer is provided, the
/// synthesized row must be queued at the final key with the same
/// shape the slow path would have produced — same digests, same
/// `requires_build: false`, same file set. A warm prefetch reading
/// the row from `index.db` can't tell which path produced it.
#[tokio::test(flavor = "multi_thread")]
async fn fast_path_queues_synthesized_index_row() {
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);

    let cas_paths = write_to_cas(
        &store_dir,
        &[
            ("package.json", br#"{"name":"x","version":"1.0.0","main":"index.js"}"#, false),
            // Mark this one executable to confirm the synthesized
            // row's `mode` bit round-trips through the `-exec` suffix.
            ("bin/cli.js", b"#!/usr/bin/env node\nconsole.log('hi');\n", true),
            ("index.js", b"module.exports = 42;\n", false),
        ],
    );
    let bin_cas_path = cas_paths["bin/cli.js"].clone();

    let key = "x@1.0.0\tbuilt";
    let _received = GitHostedTarballFetcher {
        cas_paths,
        path: None,
        allow_build: deny_all_builds(),
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        store_dir: &store_dir,
        package_id: "x@1.0.0",
        requester: "/test",
        store_index_writer: Some(&Arc::clone(&writer)),
        files_index_file: key,
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    drop(writer);
    writer_task.await.unwrap().unwrap();

    let index = StoreIndex::open_in(&store_dir).unwrap();
    let row = index.get(key).unwrap().expect("fast path must still queue a row at the final key");
    assert_eq!(row.requires_build, Some(false), "fast path implies no build needed");
    assert_eq!(row.algo, "sha512");
    let row_keys: Vec<&str> = row.files.keys().map(String::as_str).collect();
    assert!(row_keys.contains(&"package.json"));
    assert!(row_keys.contains(&"index.js"));
    assert!(row_keys.contains(&"bin/cli.js"));

    // The synthesized `bin/cli.js` entry must round-trip through
    // `cas_file_path_by_mode` to the same path the input map points
    // at — that's the property a warm prefetch relies on.
    let bin_entry = row.files.get("bin/cli.js").expect("bin entry must exist");
    assert_eq!(bin_entry.mode & 0o111, 0o111, "executable bit must survive the synthesis");
    let resolved =
        store_dir.cas_file_path_by_mode(&bin_entry.digest, bin_entry.mode).expect("valid digest");
    assert_eq!(resolved, bin_cas_path, "synthesized digest must round-trip to the input CAS path");
}

/// Sub-path resolutions never qualify for the fast path: even when
/// `cas_paths.len() == packlist.len()` (e.g. a tarball that only
/// contains the sub-package's files), the keys themselves live
/// under `packages/sub/...` while packlist runs *inside* `pkg_dir`
/// and produces keys relative to that sub-dir. Returning the input
/// `cas_paths` verbatim would surface monorepo-prefixed paths to
/// the dispatcher and corrupt the virtual store. Pin the slow-path
/// behavior to guard against a future refactor that loosens the
/// `path.is_none()` guard.
#[tokio::test(flavor = "multi_thread")]
async fn sub_path_never_takes_fast_path() {
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);

    // Tarball contains *only* the sub-package's files, so the
    // count check (`packlist.len() == cas_paths.len()`) would
    // otherwise pass: both sides see exactly two files. The only
    // thing keeping the fast path out is `path.is_none()`. If that
    // guard ever weakens, the assertion below would flag the
    // misshaped `packages/sub/...` keys in the row.
    let cas_paths = write_to_cas(
        &store_dir,
        &[
            (
                "packages/sub/package.json",
                br#"{"name":"sub","version":"1.0.0","main":"index.js"}"#,
                false,
            ),
            ("packages/sub/index.js", b"module.exports = 1;\n", false),
        ],
    );

    let key = "sub@1.0.0\tbuilt";
    let received = GitHostedTarballFetcher {
        cas_paths,
        path: Some("packages/sub"),
        allow_build: deny_all_builds(),
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        store_dir: &store_dir,
        package_id: "sub@1.0.0",
        requester: "/test",
        store_index_writer: Some(&Arc::clone(&writer)),
        files_index_file: key,
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    drop(writer);
    writer_task.await.unwrap().unwrap();

    // Output keys are relative to the sub-dir — never carrying the
    // `packages/sub/` prefix. If the fast path had triggered
    // (returning input `cas_paths` verbatim), the keys would still
    // be the monorepo-prefixed paths.
    let out_keys: Vec<&str> = received.cas_paths.keys().map(String::as_str).collect();
    assert!(
        !out_keys.iter().any(|k| k.contains("packages/")),
        "slow path strips the sub-dir prefix: {out_keys:?}",
    );
    assert!(out_keys.contains(&"package.json"));
    assert!(out_keys.contains(&"index.js"));

    let index = StoreIndex::open_in(&store_dir).unwrap();
    let row = index.get(key).unwrap().expect("sub-path takes slow path and writes a row");
    let row_keys: Vec<&str> = row.files.keys().map(String::as_str).collect();
    assert!(row_keys.contains(&"package.json"), "sub-dir manifest");
    assert!(row_keys.contains(&"index.js"), "sub-dir main");
    assert!(
        !row_keys.iter().any(|k| k.contains("packages/")),
        "no monorepo prefixes in {row_keys:?}",
    );
}

/// `should_be_built && ignore_scripts` is the second fast-path
/// branch: scripts were suppressed (a warning fired earlier), the
/// materialized tree is still untouched, so re-import is wasted
/// work. Upstream specifically does *not* queue a final-key row
/// here — subsequent installs must re-check the build gate. This
/// test pins both halves: the input `cas_paths` is returned
/// verbatim, and no row lands at the final key.
#[tokio::test(flavor = "multi_thread")]
async fn fast_path_ignore_scripts_returns_input_without_queueing_row() {
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());
    let (writer, writer_task) = StoreIndexWriter::spawn(&store_dir);

    // `prepare` script triggers `should_be_built = true`, but
    // `ignore_scripts: true` skips the actual execution. With no
    // `files` field, packlist returns every input file — fast-path
    // eligible.
    let cas_paths = write_to_cas(
        &store_dir,
        &[
            (
                "package.json",
                br#"{"name":"x","version":"1.0.0","main":"index.js","scripts":{"prepare":"tsc"}}"#,
                false,
            ),
            ("index.js", b"module.exports = 1;\n", false),
        ],
    );
    let input_snapshot = cas_paths.clone();

    let key = "x@1.0.0\tbuilt";
    let received = GitHostedTarballFetcher {
        cas_paths,
        path: None,
        allow_build: deny_all_builds(),
        ignore_scripts: true,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        store_dir: &store_dir,
        package_id: "x@1.0.0",
        requester: "/test",
        store_index_writer: Some(&Arc::clone(&writer)),
        files_index_file: key,
    }
    .run::<SilentReporter>()
    .await
    .unwrap();

    drop(writer);
    writer_task.await.unwrap().unwrap();

    assert!(received.built, "should_be_built stays true even when scripts were ignored");
    assert_eq!(received.cas_paths, input_snapshot, "ignored-build fast path returns input as-is");

    let index = StoreIndex::open_in(&store_dir).unwrap();
    assert!(
        index.get(key).unwrap().is_none(),
        "ignored-build fast path must NOT queue a final-key row",
    );
}

/// Ports pnpm's `prevent directory traversal attack when path is
/// present` at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/fetching/tarball-fetcher/test/fetch.ts#L610>.
/// A `..`-laden `resolution.path` must be rejected by
/// `prepare_package`'s `safe_join_path` with `INVALID_PATH` before
/// any extraction happens.
#[tokio::test(flavor = "multi_thread")]
async fn tarball_path_traversal_attack_is_rejected() {
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());

    let cas_paths =
        write_to_cas(&store_dir, &[("package.json", br#"{"name":"x","version":"1.0.0"}"#, false)]);

    let err = GitHostedTarballFetcher {
        cas_paths,
        path: Some("../escape"),
        allow_build: deny_all_builds(),
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        store_dir: &store_dir,
        package_id: "x@1.0.0",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "x@1.0.0\tbuilt",
    }
    .run::<SilentReporter>()
    .await
    .unwrap_err();

    match err {
        GitFetcherError::Prepare(PreparePackageError::InvalidPath { path }) => {
            assert_eq!(path, "../escape");
        }
        other => panic!("expected Prepare::InvalidPath, got {other:?}"),
    }
}

/// Ports pnpm's `fail when path is not exists` at
/// <https://github.com/pnpm/pnpm/blob/94240bc046/fetching/tarball-fetcher/test/fetch.ts#L637>.
/// A `path` pointing at a sub-directory the tarball doesn't contain
/// must surface as `INVALID_PATH` — silently packing the root would
/// produce a working install for the wrong package.
#[tokio::test(flavor = "multi_thread")]
async fn tarball_path_to_missing_subdir_is_rejected() {
    let store_root = tempdir().unwrap();
    let store_dir = StoreDir::from(store_root.path().to_path_buf());

    let cas_paths =
        write_to_cas(&store_dir, &[("package.json", br#"{"name":"x","version":"1.0.0"}"#, false)]);

    let err = GitHostedTarballFetcher {
        cas_paths,
        path: Some("does/not/exist"),
        allow_build: deny_all_builds(),
        ignore_scripts: false,
        unsafe_perm: true,
        user_agent: None,
        scripts_prepend_node_path: ScriptsPrependNodePath::Never,
        script_shell: None,
        node_execpath: None,
        npm_execpath: None,
        store_dir: &store_dir,
        package_id: "x@1.0.0",
        requester: "/test",
        store_index_writer: None,
        files_index_file: "x@1.0.0\tbuilt",
    }
    .run::<SilentReporter>()
    .await
    .unwrap_err();

    match err {
        GitFetcherError::Prepare(PreparePackageError::InvalidPath { path }) => {
            assert_eq!(path, "does/not/exist");
        }
        other => panic!("expected Prepare::InvalidPath, got {other:?}"),
    }
}
