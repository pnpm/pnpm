//! Packing a tarball obtained ready-made: a registry download, a tarball
//! URL, or a local `.tgz` passed to `pack` as a package spec. The archive
//! bytes are written through unchanged; what packing adds on top is the
//! output naming, the integrity check against the registry's declared
//! integrity, and the report.

use super::{
    ArchiveDigest,
    DigestingWriter,
    FsAtomicWrite,
    FsCreateDirAll,
    PackError,
    PackResult,
    Path,
    Value,
    sort_paths_en_locale,
};
use flate2::read::GzDecoder;
use pnpm_package_name::is_valid_old_npm_package_name;
use std::io::{
    Read,
    Write,
};

/// Inputs for [`pack_fetched_tarball`].
pub struct FetchedPackOptions<'a> {
    /// The archive's bytes, gzipped or a plain tar.
    pub bytes: &'a [u8],
    /// How errors name the archive: its URL, spec, or file path.
    pub source: &'a str,
    /// The integrity the registry declares for the bytes, when known.
    pub expected_integrity: Option<&'a ssri::Integrity>,
    /// Directory a relative `out` / `destination` (and the default output
    /// directory) resolves against: the CLI's working directory.
    pub base_dir: &'a Path,
    /// Report without writing the tarball.
    pub dry_run: bool,
    /// `--out` template (`%s` = name, `%v` = version).
    pub out: Option<&'a str>,
    /// `--pack-destination`.
    pub destination: Option<&'a str>,
}

const GZIP_MAGIC: [u8; 2] = [0x1F, 0x8B];

/// Write `opts.bytes` to the pack destination under the name the archive's
/// own `package.json` implies, and report what the archive carries.
pub fn pack_fetched_tarball<Sys: FsCreateDirAll + FsAtomicWrite>(
    opts: &FetchedPackOptions<'_>,
) -> Result<PackResult, PackError> {
    verify_declared_integrity(opts)?;
    let archive = read_fetched_archive(
        opts.bytes,
        opts.source,
        pnpm_tarball::DEFAULT_MAX_TARBALL_UNPACKED_SIZE,
    )?;
    let manifest = archive_manifest(&archive, opts.source)?;
    let (name, version) = manifest_identity(&manifest)?;
    let (tarball_name, pack_destination) = super::output::resolve_output_values(
        opts.out,
        opts.destination,
        &super::output::normalize_tarball_name(name),
        super::output::strip_build_metadata(version),
    )?;
    let dest_dir =
        super::output::resolve_dest_dir_from_base(opts.base_dir, pack_destination.as_deref());
    let digest = write_fetched_tarball::<Sys>(opts, &dest_dir.join(&tarball_name))?;
    let mut contents: Vec<String> = archive.files.into_iter().collect();
    sort_paths_en_locale(&mut contents);
    Ok(PackResult {
        published_manifest: manifest.clone(),
        packed_manifest: manifest,
        contents,
        tarball_path: super::output::packed_tarball_path(
            opts.base_dir,
            opts.base_dir,
            &dest_dir,
            &tarball_name,
        ),
        unpacked_size: archive.unpacked_size,
        archive: digest,
    })
}

/// Check the bytes against the registry-declared integrity, when there is
/// one. The digest is computed with the integrity's own strongest
/// algorithm: an old registry entry that declares only SHA-1 still
/// verifies.
fn verify_declared_integrity(opts: &FetchedPackOptions<'_>) -> Result<(), PackError> {
    let Some(expected) = opts.expected_integrity else { return Ok(()) };
    let actual = ssri::IntegrityOpts::new()
        .algorithm(expected.pick_algorithm())
        .chain(opts.bytes)
        .result();
    if expected.matches(&actual).is_none() {
        return Err(PackError::FetchedIntegrityMismatch {
            source: opts.source.to_string(),
            expected: expected.to_string(),
            actual: actual.to_string(),
        });
    }
    Ok(())
}

/// Scan the archive, reading at most `limit` decoded bytes: a small hostile
/// gzip must not expand without bound. The written file keeps the original
/// bytes; the bound is on the scan only.
pub(crate) fn read_fetched_archive(
    bytes: &[u8],
    source: &str,
    limit: u64,
) -> Result<pnpm_tarball::PublicationArchive, PackError> {
    let decoded: Box<dyn Read> = if bytes.starts_with(&GZIP_MAGIC) {
        Box::new(GzDecoder::new(bytes))
    } else {
        Box::new(bytes)
    };
    let mut bounded = decoded.take(limit + 1);
    let scanned = pnpm_tarball::read_publication_archive(&mut bounded);
    // Checked before the scan's own result: hitting the bound truncates the
    // stream, which the reader reports as a malformed archive.
    if bounded.limit() == 0 {
        return Err(PackError::FetchedTarballTooLarge { source: source.to_string(), limit });
    }
    scanned.map_err(|error| PackError::FetchedTarballUnreadable {
        source: source.to_string(),
        message: error.to_string(),
    })
}

fn archive_manifest(
    archive: &pnpm_tarball::PublicationArchive,
    source: &str,
) -> Result<Value, PackError> {
    pnpm_package_manifest::parse_manifest_bytes(&archive.manifest)
        .map_err(|error| PackError::FetchedManifestInvalid {
            source: source.to_string(),
            message: error.to_string(),
        })
}

/// The name and version the archive's manifest declares, checked the way
/// directory packing checks them: both land in the tarball's output path,
/// so a separator or a `.` / `..` segment would write outside the
/// destination.
fn manifest_identity(manifest: &Value) -> Result<(&str, &str), PackError> {
    let name = manifest
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or(PackError::PackageNameNotFound)?;
    if !is_valid_old_npm_package_name(name) {
        return Err(PackError::InvalidPackageName { name: name.to_string() });
    }
    let version = manifest
        .get("version")
        .and_then(Value::as_str)
        .filter(|version| !version.is_empty())
        .ok_or(PackError::PackageVersionNotFound)?;
    if super::source::is_invalid_tarball_version(super::output::strip_build_metadata(version)) {
        return Err(PackError::InvalidPackageVersion { version: version.to_string() });
    }
    Ok((name, version))
}

/// Write the bytes to `dest_file`, digested as they are written. A dry run
/// writes nothing and reports no digest.
fn write_fetched_tarball<Sys: FsCreateDirAll + FsAtomicWrite>(
    opts: &FetchedPackOptions<'_>,
    dest_file: &Path,
) -> Result<Option<ArchiveDigest>, PackError> {
    if opts.dry_run {
        return Ok(None);
    }
    let dest_dir = dest_file.parent().expect("a dest file has a parent");
    Sys::create_dir_all(dest_dir)
        .map_err(|source| PackError::CreateDir { path: dest_dir.display().to_string(), source })?;
    let mut digest = None;
    Sys::atomic_write(dest_file, &mut |writer| {
        let mut digesting = DigestingWriter::new(writer);
        digesting.write_all(opts.bytes)?;
        digest = Some(digesting.finish());
        Ok(())
    })
    .map_err(|error| PackError::WriteTarball {
        path: dest_file.display().to_string(),
        source: error,
    })?;
    Ok(digest)
}
