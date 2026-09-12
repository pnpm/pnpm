use super::{
    Arc, ArchiveStoreProjection, AuthHeaders, CafsFileInfo, ErrorKind, FetchTarballForResolution,
    HashMap, IngestTarballToStore, MAX_UNTRUSTED_PREALLOC_BYTES, MemCache, PackageFilesIndex,
    STREAM_ENTRY_BUFFER_MAX, SharedVerifiedFilesCache, SilentReporter, StoreIndex,
    StoreIndexWriter, TarballError, ThrottledClient, apply_append_manifest,
    apply_placeholder_manifest, assert_eq, extract_tarball_entries, fast_fail_client,
    fast_retry_opts, gzip_bomb_tarball, gzip_bytes, gzipped_archive, gzipped_tar,
    read_local_tarball_metadata, stream_extract_gzipped_tarball, tar_with_entries,
    tar_with_raw_entry_name, tempdir, tempdir_with_leaked_path, test_retry_opts,
};

#[test]
fn extract_tarball_records_requires_build_from_manifest() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let body = br#"{"scripts":{"install":"node-gyp rebuild"}}"#;
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder
            .append_data(&mut header, "package/package.json", &body[..])
            .expect("append manifest");
        builder.finish().expect("finalize tar");
    }

    let (_cas_paths, pkg_files_idx) =
        extract_tarball_entries(&tar_bytes, store_path, None).expect("tarball extraction");

    assert_eq!(pkg_files_idx.requires_build, Some(true));
    drop(tempdir);
}

/// Published packages ship `package.json` files carrying a UTF-8 BOM.
/// The bundled manifest and the install-script detection derived from it
/// must survive one, or the package silently loses its build pass.
#[test]
fn extract_tarball_reads_a_manifest_that_starts_with_a_utf8_bom() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let body = b"\xEF\xBB\xBF{\"name\":\"bom\",\"scripts\":{\"install\":\"node-gyp rebuild\"}}";
        let mut header = tar::Header::new_gnu();
        header.set_size(body.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder
            .append_data(&mut header, "package/package.json", &body[..])
            .expect("append manifest");
        builder.finish().expect("finalize tar");
    }

    let (_cas_paths, pkg_files_idx) =
        extract_tarball_entries(&tar_bytes, store_path, None).expect("tarball extraction");

    assert_eq!(pkg_files_idx.requires_build, Some(true));
    assert!(pkg_files_idx.manifest.is_some(), "the bundled manifest must be recorded");
    drop(tempdir);
}

/// A `package.json` above [`STREAM_ENTRY_BUFFER_MAX`] must still be
/// buffered and parsed on the streaming path — routing it through the
/// direct-to-store branch would silently record
/// `requires_build: Some(false)` and drop the bundled manifest.
#[test]
fn streaming_extract_parses_manifest_larger_than_entry_buffer() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let padding = "p".repeat(usize::try_from(STREAM_ENTRY_BUFFER_MAX).unwrap() + 1);
    let manifest =
        format!(r#"{{"name":"pad","scripts":{{"install":"node-gyp rebuild"}},"x":"{padding}"}}"#);

    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let mut header = tar::Header::new_gnu();
        header.set_size(manifest.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder
            .append_data(&mut header, "package/package.json", manifest.as_bytes())
            .expect("append manifest");
        builder.finish().expect("finalize tar");
    }

    let (_cas_paths, pkg_files_idx) =
        stream_extract_gzipped_tarball(&gzip_bytes(&tar_bytes), store_path, None)
            .expect("streaming extraction");

    assert_eq!(pkg_files_idx.requires_build, Some(true));
    assert!(pkg_files_idx.manifest.is_some(), "the bundled manifest must be recorded");
    drop(tempdir);
}

/// A `package.json` header claiming more than
/// [`MAX_UNTRUSTED_PREALLOC_BYTES`] must be rejected before its payload
/// is read — buffering it would defeat the streaming path's
/// bounded-memory guarantee, and skipping the parse would record wrong
/// build metadata. No payload follows the forged header below: the
/// guard has to fire on the header alone.
#[test]
fn streaming_extract_rejects_manifest_beyond_prealloc_cap() {
    let (tempdir, store_path) = tempdir_with_leaked_path();

    let mut header = tar::Header::new_gnu();
    header.set_path("package/package.json").expect("set tar entry path");
    header.set_size(MAX_UNTRUSTED_PREALLOC_BYTES as u64 + 1);
    header.set_mode(0o644);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();

    let err = stream_extract_gzipped_tarball(&gzip_bytes(header.as_bytes()), store_path, None)
        .expect_err("oversized manifest must be rejected, not buffered");

    match err {
        TarballError::ReadTarballEntries(io_err) => {
            assert_eq!(io_err.kind(), ErrorKind::InvalidData);
        }
        other => panic!("expected ReadTarballEntries(InvalidData), got: {other:?}"),
    }

    drop(tempdir);
}

/// A `file:` tarball is read to learn the name and version its
/// specifier does not carry, so the eager decode ceiling must not turn
/// a large local archive into an unresolvable dependency: past the
/// ceiling the manifest is found by streaming instead.
#[tokio::test]
async fn read_local_tarball_metadata_reads_a_manifest_past_the_eager_ceiling() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("pkg.tgz");

    let archive = gzip_bomb_tarball(
        &[("package/payload.bin", MAX_UNTRUSTED_PREALLOC_BYTES as u64 + 1)],
        &[("package/package.json", br#"{"name":"huge","version":"1.0.0"}"#)],
    );
    std::fs::write(&tarball_path, &archive).unwrap();

    let metadata = read_local_tarball_metadata(&tarball_path)
        .await
        .expect("an archive past the eager ceiling must still resolve");

    let manifest = metadata.manifest.expect("bundled manifest");
    assert_eq!(manifest.get("name").and_then(serde_json::Value::as_str), Some("huge"));
    assert_eq!(manifest.get("version").and_then(serde_json::Value::as_str), Some("1.0.0"));
    assert!(metadata.has_manifest_entry);
}

/// The manifest is the package's only source of identity here, so an
/// unparsable one fails the resolve rather than degrading to `None` —
/// matching how pnpm rejects the same tarball.
#[tokio::test]
async fn read_local_tarball_metadata_rejects_an_unparsable_manifest() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("pkg.tgz");
    std::fs::write(&tarball_path, gzipped_tar(&[("package/package.json", b"{ BROKEN")])).unwrap();

    let err = read_local_tarball_metadata(&tarball_path)
        .await
        .expect_err("an unparsable bundled manifest must fail the read");
    match err {
        TarballError::ParseBundledManifest { tarball, .. } => {
            assert_eq!(tarball, tarball_path.display().to_string());
        }
        other => panic!("expected ParseBundledManifest, got {other:?}"),
    }
}

/// Duplicate `package.json` entries are last-entry-wins, matching
/// `extract_tarball_entries`, so only the surviving one is parsed — the
/// two reads must agree on which manifest describes the package.
#[tokio::test]
async fn read_local_tarball_metadata_lets_a_later_manifest_supersede_a_malformed_one() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("pkg.tgz");
    std::fs::write(
        &tarball_path,
        gzipped_tar(&[
            ("package/package.json", b"{ BROKEN"),
            ("package/package.json", br#"{"name":"dup-pkg","version":"2.0.0"}"#),
        ]),
    )
    .unwrap();

    let metadata = read_local_tarball_metadata(&tarball_path)
        .await
        .expect("the surviving manifest parses, so the read succeeds");
    let manifest = metadata.manifest.expect("bundled manifest");
    assert_eq!(manifest.get("name").and_then(serde_json::Value::as_str), Some("dup-pkg"));
}

/// An archive with no `package.json` at all is a different shape from a
/// corrupt one: pnpm installs it, so the read degrades to `None` instead
/// of failing.
#[tokio::test]
async fn read_local_tarball_metadata_tolerates_an_archive_with_no_manifest() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("pkg.tgz");
    std::fs::write(&tarball_path, gzipped_tar(&[("package/README.md", b"hi")])).unwrap();

    let metadata = read_local_tarball_metadata(&tarball_path)
        .await
        .expect("an archive without a manifest still reads");
    assert!(metadata.manifest.is_none(), "got {:?}", metadata.manifest);
}

#[tokio::test]
async fn mem_cache_partitions_synthesized_package_manifests_by_content() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("artifact.tgz");
    std::fs::write(&tarball_path, gzipped_tar(&[("artifact/README.md", b"archive")])).unwrap();
    let package_url = format!("file:{}", tarball_path.display());
    let (store_dir, store_path) = tempdir_with_leaked_path();
    let client = fast_fail_client();
    let auth_headers = AuthHeaders::default();
    let mem_cache = MemCache::default();
    let ingest = |append_manifest| IngestTarballToStore {
        http_client: &client,
        store_dir: store_path,
        store_index: None,
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        package_integrity: None,
        package_unpacked_size: None,
        package_file_count: None,
        package_url: &package_url,
        package_id: "artifact@1.0.0",
        requester: "",
        prefetched_cas_paths: None,
        retry_opts: test_retry_opts(),
        auth_headers: &auth_headers,
        ignore_file_pattern: None,
        offline: true,
        progress_reported: None,
        store_projection: ArchiveStoreProjection::Package {
            append_manifest: Some(append_manifest),
        },
    };
    let first_manifest = br#"{"name":"first"}"#;
    let second_manifest = br#"{"name":"second"}"#;

    let first =
        ingest(first_manifest).run_with_mem_cache::<SilentReporter>(&mem_cache).await.unwrap();
    let second =
        ingest(second_manifest).run_with_mem_cache::<SilentReporter>(&mem_cache).await.unwrap();

    assert_eq!(std::fs::read(&first["package.json"]).unwrap(), first_manifest);
    assert_eq!(std::fs::read(&second["package.json"]).unwrap(), second_manifest);
    assert_eq!(mem_cache.len(), 2);
    drop((store_dir, local_dir));
}

#[tokio::test]
async fn store_index_partitions_synthesized_package_manifests_by_content() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("artifact.tgz");
    let archive = gzipped_tar(&[("artifact/README.md", b"archive")]);
    std::fs::write(&tarball_path, &archive).unwrap();
    let package_url = format!("file:{}", tarball_path.display());
    let mut integrity_opts = ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512);
    integrity_opts.input(&archive);
    let package_integrity = integrity_opts.result();
    let package_id = "runtime:artifact@1.0.0";
    let (store_dir, store_path) = tempdir_with_leaked_path();
    let client = fast_fail_client();
    let auth_headers = AuthHeaders::default();
    let first_manifest = br#"{"name":"first"}"#;
    let second_manifest = br#"{"name":"second"}"#;

    for manifest in [first_manifest.as_slice(), second_manifest.as_slice()] {
        let (writer, writer_task) = StoreIndexWriter::spawn(store_path);
        IngestTarballToStore {
            http_client: &client,
            store_dir: store_path,
            store_index: StoreIndex::shared_readonly_in(store_path),
            store_index_writer: Some(Arc::clone(&writer)),
            verify_store_integrity: true,
            strict_store_pkg_content_check: true,
            verified_files_cache: SharedVerifiedFilesCache::default(),
            package_integrity: Some(&package_integrity),
            package_unpacked_size: None,
            package_file_count: None,
            package_url: &package_url,
            package_id,
            requester: "",
            prefetched_cas_paths: None,
            retry_opts: test_retry_opts(),
            auth_headers: &auth_headers,
            ignore_file_pattern: None,
            offline: true,
            progress_reported: None,
            store_projection: ArchiveStoreProjection::Package { append_manifest: Some(manifest) },
        }
        .run_without_mem_cache::<SilentReporter>()
        .await
        .expect("each synthesized manifest should be ingested independently");
        drop(writer);
        writer_task.await.expect("writer task").expect("writer flushed");
    }

    std::fs::remove_file(&tarball_path).unwrap();
    let store_index = StoreIndex::shared_readonly_in(store_path);
    for manifest in [first_manifest.as_slice(), second_manifest.as_slice()] {
        let files = IngestTarballToStore {
            http_client: &client,
            store_dir: store_path,
            store_index: store_index.clone(),
            store_index_writer: None,
            verify_store_integrity: true,
            strict_store_pkg_content_check: true,
            verified_files_cache: SharedVerifiedFilesCache::default(),
            package_integrity: Some(&package_integrity),
            package_unpacked_size: None,
            package_file_count: None,
            package_url: &package_url,
            package_id,
            requester: "",
            prefetched_cas_paths: None,
            retry_opts: test_retry_opts(),
            auth_headers: &auth_headers,
            ignore_file_pattern: None,
            offline: true,
            progress_reported: None,
            store_projection: ArchiveStoreProjection::Package { append_manifest: Some(manifest) },
        }
        .run_without_mem_cache::<SilentReporter>()
        .await
        .expect("the matching synthesized manifest should be read from the store");
        assert_eq!(std::fs::read(&files["package.json"]).unwrap(), manifest);
    }

    let index = StoreIndex::open_in(store_path).expect("open store index");
    assert_eq!(index.keys().expect("read index keys").len(), 2);
    drop((index, store_dir, local_dir));
}

#[tokio::test]
async fn raw_archive_projection_does_not_inject_an_npm_manifest() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("artifact.tgz");
    std::fs::write(&tarball_path, gzipped_tar(&[("artifact/README.md", b"raw")])).unwrap();
    let package_url = format!("file:{}", tarball_path.display());
    let (store_dir, store_path) = tempdir_with_leaked_path();

    let cas_paths = IngestTarballToStore {
        http_client: &fast_fail_client(),
        store_dir: store_path,
        store_index: None,
        store_index_writer: None,
        verify_store_integrity: true,
        strict_store_pkg_content_check: true,
        verified_files_cache: SharedVerifiedFilesCache::default(),
        package_integrity: None,
        package_unpacked_size: None,
        package_file_count: None,
        package_url: &package_url,
        package_id: "artifact@1.0.0",
        requester: "",
        prefetched_cas_paths: None,
        retry_opts: test_retry_opts(),
        auth_headers: &AuthHeaders::default(),
        ignore_file_pattern: None,
        offline: true,
        progress_reported: None,
        store_projection: ArchiveStoreProjection::RawArchive,
    }
    .run_without_mem_cache::<SilentReporter>()
    .await
    .expect("a local raw archive should be ingested");

    assert_eq!(cas_paths.keys().collect::<Vec<_>>(), ["README.md"]);
    drop((store_dir, local_dir));
}

/// A git dep pointing at one directory of a repo (`#path:/packages/foo`)
/// gets an archive spanning the whole repo, so the root `package.json`
/// is the repo's, not the package's. Reading the root would name the
/// lockfile key after the wrong package.
#[tokio::test]
async fn fetch_for_resolution_reads_manifest_from_subdirectory() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let archive = gzipped_archive(&[
        ("package.json", r#"{"name":"the-monorepo","version":"0.0.0"}"#),
        ("packages/foo/package.json", r#"{"name":"foo","version":"1.2.3"}"#),
    ]);
    let mock =
        server.mock("GET", "/repo.tgz").with_status(200).with_body(archive).create_async().await;

    let url = format!("{}/repo.tgz", server.url());
    let client = ThrottledClient::default();

    let resolved = FetchTarballForResolution {
        http_client: &client,
        store_dir: store_path,
        store_index_writer: None,
        package_url: &url,
        package_id: &url,
        auth_headers: &AuthHeaders::default(),
        retry_opts: fast_retry_opts(),
        // Leading slash, exactly as the resolution records it.
        manifest_subdir: Some("/packages/foo"),
    }
    .run::<SilentReporter>(None)
    .await
    .expect("subdirectory manifest should be readable from the extracted archive");

    let manifest = dbg!(resolved.manifest).expect("subdirectory manifest");
    assert_eq!(manifest.get("name").and_then(serde_json::Value::as_str), Some("foo"));
    assert_eq!(manifest.get("version").and_then(serde_json::Value::as_str), Some("1.2.3"));
    mock.assert_async().await;
    drop(store_dir_keep);
}

/// A subdirectory without its own `package.json` degrades to `None`,
/// the same best-effort contract the archive root has — never the
/// root's manifest, which would name the key after the wrong package.
#[tokio::test]
async fn fetch_for_resolution_returns_no_manifest_for_subdirectory_without_one() {
    let (store_dir_keep, store_path) = tempdir_with_leaked_path();
    let mut server = mockito::Server::new_async().await;
    let archive = gzipped_archive(&[
        ("package.json", r#"{"name":"the-monorepo","version":"0.0.0"}"#),
        ("packages/foo/index.js", "module.exports = 1"),
    ]);
    let mock =
        server.mock("GET", "/repo.tgz").with_status(200).with_body(archive).create_async().await;

    let url = format!("{}/repo.tgz", server.url());
    let client = ThrottledClient::default();

    let resolved = FetchTarballForResolution {
        http_client: &client,
        store_dir: store_path,
        store_index_writer: None,
        package_url: &url,
        package_id: &url,
        auth_headers: &AuthHeaders::default(),
        retry_opts: fast_retry_opts(),
        manifest_subdir: Some("/packages/foo"),
    }
    .run::<SilentReporter>(None)
    .await
    .expect("a subdirectory without a package.json is not a fetch failure");

    assert_eq!(dbg!(resolved.manifest), None);
    mock.assert_async().await;
    drop(store_dir_keep);
}

/// A runtime archive (Node.js / Bun / Deno) ships no `package.json`, so
/// `apply_append_manifest` must bake the synthesized manifest into the
/// persisted store-index row — both its `files` map and its bundled
/// `manifest` — and into this install's `cas_paths`. Without the row
/// entry, a later *warm* materialization reads a `package.json`-less row
/// and `pnpm dlx node@runtime:<v>` fails with `dlx_read_manifest`.
#[test]
fn apply_append_manifest_folds_the_synthesized_manifest_into_the_row() {
    let (_keep, store_path) = tempdir_with_leaked_path();
    let manifest_bytes =
        br#"{"name":"node","version":"26.4.0","bin":{"node":"bin/node"}}"#.to_vec();
    let mut cas_paths = HashMap::new();
    let mut idx = PackageFilesIndex { algo: "sha512".to_string(), ..Default::default() };

    apply_append_manifest(store_path, &manifest_bytes, &mut cas_paths, &mut idx)
        .expect("write the synthesized manifest into the CAS");

    // This install's slot materializes the manifest...
    assert!(cas_paths.contains_key("package.json"), "cas_paths gains package.json");
    // ...and so does the persisted row, so warm reinstalls get it too.
    let file = idx.files.get("package.json").expect("row records the package.json file");
    assert_eq!(file.size, manifest_bytes.len() as u64);
    assert!(!file.digest.is_empty(), "the synthesized file is content-addressed");
    // The bundled manifest carries the runtime's bin so the warm-batch
    // bin linker links it without stat-ing the slot's package.json.
    let manifest = idx.manifest.expect("row records the bundled manifest");
    assert_eq!(manifest.get("bin"), Some(&serde_json::json!({ "node": "bin/node" })));
}

/// An ordinary npm tarball already carries its own `package.json`;
/// `apply_append_manifest` must leave it untouched (pnpm's `manifest ==
/// null` guard) rather than displacing the real manifest with a
/// synthesized one.
#[test]
fn apply_append_manifest_is_a_noop_when_the_archive_ships_a_package_json() {
    let (_keep, store_path) = tempdir_with_leaked_path();
    let existing =
        CafsFileInfo { digest: "kept".to_string(), mode: 0o644, size: 3, checked_at: None };
    let mut idx = PackageFilesIndex { algo: "sha512".to_string(), ..Default::default() };
    idx.files.insert("package.json".to_string(), existing);
    let mut cas_paths = HashMap::new();

    apply_append_manifest(store_path, br#"{"name":"node"}"#, &mut cas_paths, &mut idx)
        .expect("a no-op still returns Ok");

    assert!(cas_paths.is_empty(), "the real package.json is not overwritten in cas_paths");
    assert_eq!(idx.files["package.json"].digest, "kept", "the row's real file entry is kept");
    assert!(idx.manifest.is_none(), "the archive's manifest handling is left alone");
}

/// The placeholder is a completion marker, not the package's identity,
/// so it must not become the row's bundled `manifest` the way
/// `apply_append_manifest`'s real one does — see
/// <https://github.com/pnpm/pnpm/issues/13410>.
#[test]
fn apply_placeholder_manifest_marks_an_archive_that_ships_no_package_json() {
    let (_keep, store_path) = tempdir_with_leaked_path();
    let mut cas_paths = HashMap::new();
    let mut idx = PackageFilesIndex { algo: "sha512".to_string(), ..Default::default() };

    apply_placeholder_manifest(store_path, &mut cas_paths, &mut idx)
        .expect("write the placeholder into the CAS");

    assert!(cas_paths.contains_key("package.json"), "cas_paths gains package.json");
    let file = idx.files.get("package.json").expect("row records the placeholder file");
    assert!(!file.digest.is_empty(), "the placeholder is content-addressed");
    let written =
        std::fs::read_to_string(&cas_paths["package.json"]).expect("read the placeholder");
    assert!(written.contains("_pnpmPlaceholder"), "got {written}");
    assert!(idx.manifest.is_none(), "a placeholder is not the package's bundled manifest");
}

#[test]
fn apply_placeholder_manifest_is_a_noop_when_a_package_json_is_already_recorded() {
    let (_keep, store_path) = tempdir_with_leaked_path();
    let existing =
        CafsFileInfo { digest: "kept".to_string(), mode: 0o644, size: 3, checked_at: None };
    let mut idx = PackageFilesIndex { algo: "sha512".to_string(), ..Default::default() };
    idx.files.insert("package.json".to_string(), existing);
    let mut cas_paths = HashMap::new();

    apply_placeholder_manifest(store_path, &mut cas_paths, &mut idx)
        .expect("a no-op still returns Ok");

    assert!(cas_paths.is_empty(), "the real package.json is not overwritten in cas_paths");
    assert_eq!(idx.files["package.json"].digest, "kept", "the row's real file entry is kept");
}

/// A flat archive keys its `package.json` at the package root, so the
/// resolve-time metadata read has to recognize it as the manifest. When
/// it and extraction disagree, a `file:` dependency is named after the
/// consumer's alias at version `0.0.0` while the `package.json` beside
/// it names something else.
#[tokio::test]
async fn read_local_tarball_metadata_reads_a_manifest_at_the_archive_root() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("flat.tgz");

    let tar_bytes = tar_with_entries(&[
        ("package.json", br#"{"name":"real-name","version":"9.9.9"}"#),
        ("index.js", b"module.exports = 1\n"),
    ]);
    std::fs::write(&tarball_path, gzip_bytes(&tar_bytes)).unwrap();

    let metadata = read_local_tarball_metadata(&tarball_path)
        .await
        .expect("read the local tarball's metadata");

    assert!(metadata.has_manifest_entry);
    let manifest = metadata.manifest.expect("bundled manifest");
    assert_eq!(manifest.get("name").and_then(serde_json::Value::as_str), Some("real-name"));
    assert_eq!(manifest.get("version").and_then(serde_json::Value::as_str), Some("9.9.9"));

    // The extraction the resolve-time read has to agree with.
    let (tempdir, store_path) = tempdir_with_leaked_path();
    let (_, pkg_files_idx) =
        extract_tarball_entries(&tar_bytes, store_path, None).expect("extract the tarball");
    assert_eq!(
        pkg_files_idx.manifest.as_ref().and_then(|manifest| manifest["name"].as_str()),
        Some("real-name"),
    );
    drop(tempdir);
}

/// A `./`-prefixed entry loses only its `./`, so `./package/package.json`
/// keys as `package/package.json` and is a manifest in a subdirectory,
/// not the package's own. The resolve-time read has to say so too, or it
/// names the package after a manifest that extraction files one level
/// down.
#[tokio::test]
async fn read_local_tarball_metadata_ignores_a_manifest_below_the_package_root() {
    let local_dir = tempdir().unwrap();
    let tarball_path = local_dir.path().join("dot-prefixed.tgz");

    let tar_bytes = tar_with_raw_entry_name(
        b"./package/package.json",
        br#"{"name":"real-name","version":"9.9.9"}"#,
    );
    std::fs::write(&tarball_path, gzip_bytes(&tar_bytes)).unwrap();

    let metadata = read_local_tarball_metadata(&tarball_path)
        .await
        .expect("read the local tarball's metadata");

    assert!(!metadata.has_manifest_entry);
    assert!(metadata.manifest.is_none(), "got {:?}", metadata.manifest);
}
