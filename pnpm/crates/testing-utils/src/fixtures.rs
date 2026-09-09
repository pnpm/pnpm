pub const BIG_MANIFEST: &str = include_str!("fixtures/big/package.json");
pub const BIG_LOCKFILE: &str = include_str!("fixtures/big/pnpm-lock.yaml");

/// Returns a gzipped package tarball containing a manifest with the given identity.
#[must_use]
pub fn minimal_tarball(name: &str, version: &str) -> Vec<u8> {
    tarball_with_manifest(&serde_json::json!({ "name": name, "version": version }))
}

/// Returns a gzipped package tarball carrying a `README.md` and no
/// `package.json` at all.
#[must_use]
pub fn tarball_without_manifest() -> Vec<u8> {
    tarball_entries(&[("package/README.md", b"placeholder")])
}

/// Returns a gzipped package tarball whose only entry is `manifest`,
/// written to `package/package.json`.
#[must_use]
pub fn tarball_with_manifest(manifest: &serde_json::Value) -> Vec<u8> {
    tarball_entries(&[("package/package.json", manifest.to_string().as_bytes())])
}

/// Returns a gzipped tarball carrying `entries` in order, each as a
/// regular file at the path given.
///
/// Entry paths are written verbatim, so a caller can build an archive
/// that omits the top-level directory a published tarball wraps its
/// payload in, or that carries an entry beside it at the archive root.
#[must_use]
pub fn tarball_entries(entries: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write;

    let mut builder = tar::Builder::new(Vec::new());
    for (path, contents) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_path(path).expect("set tar entry path");
        header.set_size(contents.len() as u64);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.set_cksum();
        builder.append(&header, *contents).expect("append entry to tar");
    }
    let tar_bytes = builder.into_inner().expect("finish tar");

    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar_bytes).expect("gzip tar");
    encoder.finish().expect("finish gzip")
}

/// Returns the SHA-512 SSRI string for `bytes`.
#[must_use]
pub fn sha512_integrity(bytes: &[u8]) -> String {
    ssri::IntegrityOpts::new().algorithm(ssri::Algorithm::Sha512).chain(bytes).result().to_string()
}
