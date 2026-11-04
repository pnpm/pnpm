//! Packing the archives `publishConfig.variants` declares.
//!
//! The package's files are listed as `pack` lists them today. A file that a
//! pattern of an entry's `files` matches goes only into the archives of the
//! entries whose patterns match it, even when the package's own `files`
//! leaves it out; every other file goes into every archive. `prepack` and
//! `postpack` run once around all the archives.

use super::{VariantDeclaration, VariantEntry, archive_file_name, check_archive};
use crate::{
    FsAtomicWrite, FsCreateDirAll, FsFileLen, FsIsExecutable, FsReadFile, PackError, PackOptions,
    PackResult, PackResultJson, PackedTarball,
    contents::executable_sources,
    create_dest_dir, pack_source, packed_contents_with_injected, packed_files_map,
    packed_tarball_path, resolve_dest_dir, resolve_output,
    source::{PackSource, prepare_source, with_registry_readme},
    unpacked_size, write_tarball,
};
use indexmap::IndexMap;
use pnpm_fs_packlist::{PacklistOptions, packlist_with_sources};
use pnpm_reporter::Reporter;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

/// One packed archive of a package, and the entry of `publishConfig.variants`
/// it was packed for; `None` for a package that declares no variants.
#[derive(Debug)]
pub struct PackedArchive {
    pub entry: Option<VariantEntry>,
    pub result: PackResult,
}

/// Pack the package at `opts.dir`: every archive its `publishConfig.variants`
/// declares, or the archives of the entries `targets` names (any one target
/// of an entry names it, `any` the primary's). A package without variants
/// packs its one archive, and names none.
///
/// A pattern of a packed entry that matches no file fails the pack. The
/// patterns of entries that are not packed do not have to match anything,
/// so a build job packs its own archive without the other platforms' files.
pub async fn pack_archives<Reporter, Sys>(
    opts: &PackOptions,
    targets: &[String],
) -> Result<Vec<PackedArchive>, PackError>
where
    Reporter: self::Reporter,
    Sys: FsReadFile + FsFileLen + FsCreateDirAll + FsAtomicWrite + FsIsExecutable,
{
    let source = prepare_source::<Reporter>(opts).await?;
    let Some(declaration) =
        VariantDeclaration::read(&source.publish_manifest).map_err(PackError::Variants)?
    else {
        if let Some(target) = targets.first() {
            return Err(PackError::NoVariants { target: target.clone() });
        }
        let result = pack_source::<Reporter, Sys>(opts, source).await?;
        return Ok(vec![PackedArchive { entry: None, result }]);
    };
    let selected = selected_entries(&declaration, targets)?;
    let claims = Claims::of(opts, &source, &declaration, &selected)?;
    let mut packed = Vec::with_capacity(selected.len());
    for index in selected {
        let entry = &declaration.entries[index];
        let result = pack_entry::<Sys>(opts, &source, entry, &claims.files_of(index)).await?;
        packed.push(PackedArchive { entry: Some(entry.clone()), result });
    }
    if !opts.output.dry_run && !opts.scripts.ignore {
        opts.scripts.run_if_present::<Reporter>(&opts.dir, &["postpack"], &source.entry_manifest)?;
    }
    Ok(packed)
}

/// The indexes of the entries `targets` names, in declaration order, or of
/// every entry when it names none.
fn selected_entries(
    declaration: &VariantDeclaration,
    targets: &[String],
) -> Result<Vec<usize>, PackError> {
    if targets.is_empty() {
        return Ok((0..declaration.entries.len()).collect());
    }
    let mut selected = BTreeSet::new();
    for target in targets {
        let entry = declaration.find(target).map_err(PackError::Variants)?;
        let index = declaration.entries
            .iter()
            .position(|candidate| candidate == entry)
            .expect("found among the entries");
        selected.insert(index);
    }
    Ok(selected.into_iter().collect())
}

/// The package's files, and the files each entry's patterns claim.
struct Claims {
    /// `package/<path>` to its source, as `pack` lists them.
    shared: IndexMap<String, PathBuf>,
    /// The files each entry claims, by entry index.
    claimed: Vec<BTreeMap<String, PathBuf>>,
}

impl Claims {
    fn of(
        opts: &PackOptions,
        source: &PackSource,
        declaration: &VariantDeclaration,
        selected: &[usize],
    ) -> Result<Self, PackError> {
        let shared = packed_files_map(opts, source)?;
        let always = listed(opts, source, &[])?;
        let mut claimed = Vec::with_capacity(declaration.entries.len());
        for (index, entry) in declaration.entries.iter().enumerate() {
            let mut files = BTreeMap::new();
            for pattern in &entry.files {
                let matched: BTreeMap<String, PathBuf> =
                    listed(opts, source, std::slice::from_ref(pattern))?
                        .into_iter()
                        .filter(|(path, _)| !always.contains_key(path))
                        .collect();
                if matched.is_empty() && selected.contains(&index) {
                    return Err(PackError::VariantPatternMatchesNothing {
                        targets: entry.label(),
                        pattern: pattern.clone(),
                    });
                }
                files.extend(matched);
            }
            claimed.push(files);
        }
        Ok(Claims { shared, claimed })
    }

    /// The files of entry `index`'s archive: every file no entry claims,
    /// and the files it claims.
    fn files_of(&self, index: usize) -> IndexMap<String, PathBuf> {
        let claimed_by_any: BTreeSet<&String> = self.claimed
            .iter()
            .flat_map(BTreeMap::keys)
            .collect();
        let mut files: IndexMap<String, PathBuf> = self.shared
            .iter()
            .filter(|(path, _)| {
                path.strip_prefix("package/")
                    .is_none_or(|path| !claimed_by_any.contains(&path.to_string()))
            })
            .map(|(path, source)| (path.clone(), source.clone()))
            .collect();
        for (path, source) in &self.claimed[index] {
            files.insert(format!("package/{path}"), source.clone());
        }
        files
    }
}

/// The files `pack` lists for the package when its `files` field is
/// `patterns`.
fn listed(
    opts: &PackOptions,
    source: &PackSource,
    patterns: &[String],
) -> Result<BTreeMap<String, PathBuf>, PackError> {
    let mut manifest = source.publish_manifest.clone();
    if let Value::Object(fields) = &mut manifest {
        fields.insert(
            "files".into(),
            patterns
                .iter()
                .cloned()
                .map(Value::String)
                .collect(),
        );
    }
    packlist_with_sources(
        &source.dir,
        &manifest,
        PacklistOptions {
            workspace_dir: opts.workspace_dir.as_deref(),
            bundled_dependencies_dir: Some(&opts.dir),
        },
    )
    .map_err(PackError::Packlist)
}

/// Check and write the archive of `entry`, which holds `files_map`.
async fn pack_entry<Sys>(
    opts: &PackOptions,
    source: &PackSource,
    entry: &VariantEntry,
    files_map: &IndexMap<String, PathBuf>,
) -> Result<PackResult, PackError>
where
    Sys: FsReadFile + FsFileLen + FsCreateDirAll + FsAtomicWrite + FsIsExecutable,
{
    let manifest = checked_manifest(source, entry, files_map)?;
    let (tarball_name, dest_dir) = archive_destination(opts, source, entry, &manifest)?;
    let manifest_json = serde_json::to_string_pretty(&manifest)
        .expect("an archive manifest serializes to JSON")
        .into_bytes();
    let unpacked_size = unpacked_size::<Sys>(files_map, manifest_json.len() as u64)?
        + opts.output.injected_files
            .iter()
            .map(|(_, bytes)| bytes.len() as u64)
            .sum::<u64>();
    if !opts.output.dry_run {
        create_dest_dir::<Sys>(&dest_dir)?;
        let packed = PackedTarball {
            dest_file: dest_dir.join(&tarball_name),
            files_map,
            manifest_json: &manifest_json,
        };
        let bins = executable_sources(&manifest, &manifest, &source.dir);
        write_tarball::<Sys>(&opts.output, &packed, &bins).await?;
    }
    Ok(PackResult {
        contents: packed_contents_with_injected(files_map, &opts.output.injected_files),
        tarball_path: packed_tarball_path(&opts.dir, &source.dir, &dest_dir, &tarball_name),
        published_manifest: with_registry_readme(manifest, &source.dir)?,
        unpacked_size,
    })
}

/// The `package.json` of `entry`'s archive, once the archive that holds
/// `files_map` passes the checks pnpr runs.
fn checked_manifest(
    source: &PackSource,
    entry: &VariantEntry,
    files_map: &IndexMap<String, PathBuf>,
) -> Result<Value, PackError> {
    let manifest = VariantDeclaration::archive_manifest(&source.publish_manifest, entry);
    let paths: Vec<&str> = files_map
        .keys()
        .filter_map(|path| path.strip_prefix("package/"))
        .collect();
    check_archive(&source.publish_manifest, entry, &manifest, &paths)
        .map_err(PackError::VariantArchive)?;
    Ok(manifest)
}

/// The file name of `entry`'s archive and the directory it is written to.
fn archive_destination(
    opts: &PackOptions,
    source: &PackSource,
    entry: &VariantEntry,
    manifest: &Value,
) -> Result<(String, PathBuf), PackError> {
    let (any_name, pack_destination) =
        resolve_output(&opts.output, &source.normalized_name, &source.published_version)?;
    let package = manifest
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let tarball_name =
        archive_file_name(&any_name, package, entry).map_err(PackError::VariantArchive)?;
    Ok((tarball_name, resolve_dest_dir(&source.dir, pack_destination.as_deref())))
}

/// Project a [`PackedArchive`] into its JSON shape, with the tag and
/// targets of the entry it was packed for.
#[must_use]
pub fn archive_result_json(archive: &PackedArchive) -> PackResultJson {
    let mut json = crate::to_pack_result_json(&archive.result);
    if let Some(entry) = &archive.entry {
        json.tag.clone_from(&entry.tag);
        json.targets = Some(entry.targets.clone());
    }
    json
}
