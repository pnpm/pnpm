//! A project packed for publishing: its archive that targets `any`, its
//! platform variants, and the manifests the publish scripts and notes read.

use super::PackedPkg;
use miette::{Context, IntoDiagnostic};
use pnpm_pack::PackedArchives;
use pnpm_publish::PackedVariant;
use serde_json::Value;
use std::path::{Path, PathBuf};

pub(super) struct PackedDirectory {
    pub(super) project_dir: PathBuf,
    pub(super) source_manifest: Value,
    /// The version's manifest: its `any` archive's, or the base manifest of
    /// a version published with variants only.
    pub(super) published_manifest: Value,
    /// The version's archive that targets `any`; `None` for a version
    /// published with variants only.
    pub(super) any: Option<PackedTarball>,
    pub(super) notes: Option<String>,
    /// The platform variants published with the version.
    pub(super) variants: Vec<PackedVariant>,
}

/// A packed archive's bytes, where pack wrote it, and what it holds.
pub(super) struct PackedTarball {
    pub(super) data: Vec<u8>,
    pub(super) path: String,
    pub(super) contents: Vec<String>,
    pub(super) unpacked_size: u64,
}

impl PackedDirectory {
    pub(super) fn packed_pkg(&self) -> PackedPkg<'_> {
        let any = self.any.as_ref();
        PackedPkg {
            published_manifest: &self.published_manifest,
            tarball_data: any.map_or(&[], |any| &any.data),
            tarball_path: any.map_or("", |any| &any.path),
            contents: any.map_or(&[], |any| &any.contents),
            unpacked_size: any.map_or(0, |any| any.unpacked_size),
            notes: self.notes.as_deref(),
            variants: &self.variants,
            any_archive: any.is_some(),
        }
    }
}

/// Whether the project's manifest declares `publishConfig.variants`.
pub(super) fn declares_variants(manifest: &Value) -> bool {
    manifest.pointer("/publishConfig/variants").is_some()
}

/// The packed archives of the project at `project_dir`, read for publishing:
/// the archive that targets `any` as the version's, and every other one as a
/// platform variant. A version without an `any` archive is published with
/// the base manifest.
pub(super) fn packed_directory(
    project_dir: &Path,
    source_manifest: Value,
    packed: PackedArchives,
) -> miette::Result<PackedDirectory> {
    let mut any = None;
    let mut variants = Vec::new();
    for archive in packed.archives {
        match archive.entry.and_then(|entry| entry.tag) {
            Some(tag) => {
                let tarball_data = read_tarball(&archive.result.tarball_path)?;
                variants.push(PackedVariant { tag, tarball_data });
            }
            None => any = Some(archive.result),
        }
    }
    let published_manifest = match &any {
        Some(result) => result.published_manifest.clone(),
        None => packed.base_manifest,
    };
    let any = any
        .map(|result| -> miette::Result<PackedTarball> {
            Ok(PackedTarball {
                data: read_tarball(&result.tarball_path)?,
                path: result.tarball_path,
                contents: result.contents,
                unpacked_size: result.unpacked_size,
            })
        })
        .transpose()?;
    Ok(PackedDirectory {
        project_dir: project_dir.to_path_buf(),
        source_manifest,
        published_manifest,
        any,
        notes: None,
        variants,
    })
}

fn read_tarball(path: &str) -> miette::Result<Vec<u8>> {
    std::fs::read(path).into_diagnostic().wrap_err("read packed tarball")
}
