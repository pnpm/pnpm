//! Packing the archives `publishConfig.variants` declares.
//!
//! The package's files are listed as `pack` lists them today. A file that a
//! pattern of an entry's `files` matches goes only into the archives of the
//! entries whose patterns match it, even when the package's own `files`
//! leaves it out; every other file goes into every archive. Each archive
//! also holds the files its own manifest's `main`, `browser`, and `bin`
//! name, and the dependencies it bundles, after its entry's `manifest` is
//! applied, rather than the base manifest's. `prepack` and `postpack` run once around all the archives.

use super::{VariantDeclaration, VariantEntry, archive_file_name, check_archive};
use crate::{
    FsAtomicWrite, FsCreateDirAll, FsFileLen, FsIsExecutable, FsReadFile, PackError, PackOptions,
    PackResult, PackResultJson, PackedTarball,
    contents::executable_sources,
    create_dest_dir,
    dotenv::warn_about_unlisted_dotenv_files,
    node_linker::prevent_bundled_dependencies_with_pnp,
    pack_source, packed_contents_with_injected, packed_files_map, packed_tarball_path,
    resolve_dest_dir, resolve_output,
    source::{PackSource, check_packed_bins_for_crlf, prepare_source, with_registry_readme},
    unpacked_size, write_tarball,
};
use indexmap::IndexMap;
use pnpm_fs_packlist::{PacklistOptions, packlist_with_sources, required_field_paths};
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

/// The archives [`pack_archives`] packed, and the manifest of the version
/// they share: the base manifest without the declaration, which a version
/// without an `any` archive is published with.
#[derive(Debug)]
pub struct PackedArchives {
    pub base_manifest: Value,
    pub archives: Vec<PackedArchive>,
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
) -> Result<PackedArchives, PackError>
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
        let base_manifest = result.published_manifest.clone();
        return Ok(PackedArchives {
            base_manifest,
            archives: vec![PackedArchive { entry: None, result }],
        });
    };
    super::warn_about_declaration::<Reporter>(&source.dir, &source.publish_manifest, &declaration);
    let selected = selected_entries(&declaration, targets)?;
    let claims = Claims::of(opts, &source, &declaration, &selected)?;
    let mut packed = Vec::with_capacity(selected.len());
    for index in selected {
        let entry = &declaration.entries[index];
        let result = pack_entry::<Reporter, Sys>(opts, &source, &claims, index, entry).await?;
        packed.push(PackedArchive { entry: Some(entry.clone()), result });
    }
    if !opts.output.dry_run && !opts.scripts.ignore {
        opts.scripts.run_if_present::<Reporter>(&opts.dir, &["postpack"], &source.entry_manifest)?;
    }
    let base_manifest =
        with_registry_readme(super::base_manifest(&source.publish_manifest), &source.dir)?;
    Ok(PackedArchives { base_manifest, archives: packed })
}

/// The base manifest of the package at `opts.dir` and its declaration, if
/// it declares variants, without packing anything: what a publish of
/// archives that other machines packed and staged needs. `prepack` runs
/// only when `opts.scripts` allows.
pub async fn declared_manifests<Reporter: self::Reporter>(
    opts: &PackOptions,
) -> Result<(Value, Option<VariantDeclaration>), PackError> {
    let source = prepare_source::<Reporter>(opts).await?;
    let declaration =
        VariantDeclaration::read(&source.publish_manifest).map_err(PackError::Variants)?;
    let base_manifest =
        with_registry_readme(super::base_manifest(&source.publish_manifest), &source.dir)?;
    Ok((base_manifest, declaration))
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

/// The files the package shares, and the files each entry's patterns claim.
/// Both are listed from the base manifest without its `main`, `browser`,
/// `bin`, and bundled dependencies: an archive holds the files its own
/// manifest's entry points name and the dependencies it bundles, not the
/// base manifest's.
struct Claims {
    /// `package/<path>` to its source: what the package's own `files`
    /// selects, without injected files.
    shared: IndexMap<String, PathBuf>,
    /// The files every archive holds whatever its manifest: `package.json`,
    /// the README, and the license.
    always: BTreeMap<String, PathBuf>,
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
        let base = without_archive_fields(&source.publish_manifest);
        let shared = packed_files_map(opts, source, &base)?;
        let always = listed(opts, source, &base, &[])?;
        let claimed = declaration.entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                claimed_by(opts, source, (&base, &always), entry, selected.contains(&index))
            })
            .collect::<Result<_, _>>()?;
        Ok(Claims { shared, always, claimed })
    }

    /// The files of entry `index`'s archive, whose `package.json` is
    /// `manifest`: every shared file no entry claims, the files it claims,
    /// and the files its manifest's entry points name and bundles.
    fn files_of(
        &self,
        (opts, source): (&PackOptions, &PackSource),
        index: usize,
        manifest: &Value,
    ) -> Result<IndexMap<String, PathBuf>, PackError> {
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
        let entry_points = listed(opts, source, manifest, &[])?
            .into_iter()
            .filter(|(path, _)| !self.always.contains_key(path));
        for (path, source) in self.claimed[index]
            .clone()
            .into_iter()
            .chain(entry_points)
        {
            files.insert(format!("package/{path}"), source);
        }
        // An injected file, such as a composed changelog, supersedes the
        // same path on disk, as in a package without variants.
        for (name, _) in &opts.output.injected_files {
            files.shift_remove(name);
        }
        Ok(files)
    }
}

/// The files `entry`'s patterns claim from what `base` lists, leaving out
/// the files every archive holds. The patterns are one `files` list, so an
/// exclusion such as `!native/debug/**` applies to the others. When the
/// entry is `packed`, a pattern that includes nothing on its own fails.
fn claimed_by(
    opts: &PackOptions,
    source: &PackSource,
    (base, always): (&Value, &BTreeMap<String, PathBuf>),
    entry: &VariantEntry,
    packed: bool,
) -> Result<BTreeMap<String, PathBuf>, PackError> {
    let claimed = |patterns: &[String]| -> Result<BTreeMap<String, PathBuf>, PackError> {
        Ok(listed(opts, source, base, patterns)?
            .into_iter()
            .filter(|(path, _)| !always.contains_key(path))
            .collect())
    };
    if packed {
        for pattern in entry.files
            .iter()
            .filter(|pattern| !pattern.starts_with('!'))
        {
            if claimed(std::slice::from_ref(pattern))?.is_empty() {
                return Err(PackError::VariantPatternMatchesNothing {
                    targets: entry.label(),
                    pattern: pattern.clone(),
                });
            }
        }
    }
    if entry.files.is_empty() { Ok(BTreeMap::new()) } else { claimed(&entry.files) }
}

/// `manifest` without the fields whose files the packlist adds whatever
/// `files` says: `main`, `browser`, `bin`, and the bundled dependencies.
/// Each archive gets them from its own manifest.
fn without_archive_fields(manifest: &Value) -> Value {
    let mut manifest = manifest.clone();
    if let Value::Object(fields) = &mut manifest {
        for field in ["main", "browser", "bin", "bundleDependencies", "bundledDependencies"] {
            fields.shift_remove(field);
        }
    }
    manifest
}

/// The files `pack` lists for the package whose manifest is `manifest`,
/// with `patterns` as its `files` field.
fn listed(
    opts: &PackOptions,
    source: &PackSource,
    manifest: &Value,
    patterns: &[String],
) -> Result<BTreeMap<String, PathBuf>, PackError> {
    let mut manifest = manifest.clone();
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

/// Check and write the archive of `entry`, the entry at `index`, with the
/// checks a package without variants gets: its bins' shebangs, and the
/// dotenv files no `files` pattern names.
async fn pack_entry<Reporter, Sys>(
    opts: &PackOptions,
    source: &PackSource,
    claims: &Claims,
    index: usize,
    entry: &VariantEntry,
) -> Result<PackResult, PackError>
where
    Reporter: self::Reporter,
    Sys: FsReadFile + FsFileLen + FsCreateDirAll + FsAtomicWrite + FsIsExecutable,
{
    let (manifest, files_map) = checked_archive(opts, source, (claims, index), entry)?;
    let files_map = &files_map;
    let listing = listing_manifest(&source.publish_manifest, entry);
    warn_about_unlisted_dotenv_files::<Reporter>(&source.dir, &listing, files_map);
    let bins = executable_sources(&manifest, &manifest, &source.dir);
    check_packed_bins_for_crlf(files_map, &bins)?;
    let (tarball_name, dest_dir) = archive_destination(opts, source, entry, &manifest)?;
    let manifest_json = serde_json::to_string_pretty(&manifest)
        .expect("an archive manifest serializes to JSON")
        .into_bytes();
    let unpacked_size = unpacked_size::<Sys>(files_map, manifest_json.len() as u64)?
        + opts.output.injected_files
            .iter()
            .map(|(_, bytes)| bytes.len() as u64)
            .sum::<u64>();
    let mut archive = None;
    if !opts.output.dry_run {
        create_dest_dir::<Sys>(&dest_dir)?;
        let packed = PackedTarball {
            dest_file: dest_dir.join(&tarball_name),
            files_map,
            manifest_json: &manifest_json,
        };
        archive = Some(write_tarball::<Sys>(&opts.output, &packed, &bins).await?);
    }
    Ok(PackResult {
        contents: packed_contents_with_injected(files_map, &opts.output.injected_files),
        tarball_path: packed_tarball_path(&opts.dir, &source.dir, &dest_dir, &tarball_name),
        published_manifest: with_registry_readme(manifest, &source.dir)?,
        unpacked_size,
        archive,
    })
}

/// The `package.json` and the files of `entry`'s archive, the entry at
/// `index`, once they pass the checks: the bundling guard of the node
/// linker, the entry points, and the checks pnpr runs. The checks see the
/// archive as it is written, injected files included.
fn checked_archive(
    opts: &PackOptions,
    source: &PackSource,
    (claims, index): (&Claims, usize),
    entry: &VariantEntry,
) -> Result<(Value, IndexMap<String, PathBuf>), PackError> {
    let manifest = VariantDeclaration::archive_manifest(&source.publish_manifest, entry);
    prevent_bundled_dependencies_with_pnp(opts.manifest.node_linker, &manifest)?;
    let files_map = claims.files_of((opts, source), index, &manifest)?;
    let visible: BTreeSet<String> = files_map
        .keys()
        .cloned()
        .chain(opts.output.injected_files.iter().map(|(name, _)| name.clone()))
        .collect();
    check_entry_points(entry, &manifest, &visible)?;
    let manifest = checked_manifest(source, entry, manifest, &visible)?;
    Ok((manifest, files_map))
}

/// The manifest whose `files` lists what the package names on purpose for
/// `entry`'s archive: its own `files` and the entry's patterns.
fn listing_manifest(base: &Value, entry: &VariantEntry) -> Value {
    let mut manifest = base.clone();
    let listed: Vec<Value> = base
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .cloned()
        .chain(
            entry.files
                .iter()
                .cloned()
                .map(Value::String),
        )
        .collect();
    if let Value::Object(fields) = &mut manifest {
        fields.insert("files".into(), Value::Array(listed));
    }
    manifest
}

/// `manifest`, the `package.json` of `entry`'s archive, once the archive
/// whose files are `visible` passes the checks pnpr runs.
fn checked_manifest(
    source: &PackSource,
    entry: &VariantEntry,
    manifest: Value,
    visible: &BTreeSet<String>,
) -> Result<Value, PackError> {
    let paths: Vec<&str> = visible
        .iter()
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

mod entry_points;
use entry_points::check_entry_points;
