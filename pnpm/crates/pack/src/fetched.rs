//! Packing a tarball obtained ready-made: a registry download, a tarball
//! URL, or a local `.tgz` passed to `pack` as a package spec. The archive
//! bytes are written through unchanged; what packing adds on top is the
//! output naming, the integrity check against the registry's declared
//! integrity, and the report.

use super::{
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
    if let Some(expected) = opts.expected_integrity {
        let actual = ssri::IntegrityOpts::new()
            .algorithm(ssri::Algorithm::Sha512)
            .chain(opts.bytes)
            .result();
        if expected.matches(&actual).is_none() {
            return Err(PackError::FetchedIntegrityMismatch {
                source: opts.source.to_string(),
                expected: expected.to_string(),
                actual: actual.to_string(),
            });
        }
    }
    let reader: Box<dyn Read> = if opts.bytes.starts_with(&GZIP_MAGIC) {
        Box::new(GzDecoder::new(opts.bytes))
    } else {
        Box::new(opts.bytes)
    };
    let archive = pnpm_tarball::read_publication_archive(reader)
        .map_err(|error| PackError::FetchedTarballUnreadable {
            source: opts.source.to_string(),
            message: error.to_string(),
        })?;
    let manifest: Value = pnpm_package_manifest::parse_manifest_bytes(&archive.manifest)
        .map_err(|error| invalid_fetched_manifest(opts.source, error.to_string()))?;
    let name = manifest
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.is_empty())
        .ok_or(PackError::PackageNameNotFound)?;
    let version = manifest
        .get("version")
        .and_then(Value::as_str)
        .filter(|version| !version.is_empty())
        .ok_or(PackError::PackageVersionNotFound)?;

    let (tarball_name, pack_destination) = super::output::resolve_output_values(
        opts.out,
        opts.destination,
        &super::output::normalize_tarball_name(name),
        super::output::strip_build_metadata(version),
    )?;
    let dest_dir =
        super::output::resolve_dest_dir_from_base(opts.base_dir, pack_destination.as_deref());
    let dest_file = dest_dir.join(&tarball_name);

    let mut digest = None;
    if !opts.dry_run {
        Sys::create_dir_all(&dest_dir)
            .map_err(|source| PackError::CreateDir {
                path: dest_dir.display().to_string(),
                source,
            })?;
        Sys::atomic_write(&dest_file, &mut |writer| {
            let mut digesting = DigestingWriter::new(writer);
            digesting.write_all(opts.bytes)?;
            digest = Some(digesting.finish());
            Ok(())
        })
        .map_err(|error| PackError::WriteTarball {
            path: dest_file.display().to_string(),
            source: error,
        })?;
    }

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

fn invalid_fetched_manifest(source: &str, message: String) -> PackError {
    PackError::FetchedManifestInvalid { source: source.to_string(), message }
}
