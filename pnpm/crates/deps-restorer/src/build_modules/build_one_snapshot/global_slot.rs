//! Building a package into its global virtual store slot.
//!
//! A slot's package directory is final once its `package.json` exists:
//! nothing writes into it afterwards, so installs share slots without
//! locks. The link phase imports a package that this install builds into
//! a staging directory beside the slot's package directory
//! ([`crate::VirtualStoreLayout::staging_package_dir`]). The build runs
//! there, and the built package is renamed into place. A side-effects
//! cache hit is imported into the slot directly, like any package.

use super::{
    BuildCandidate, BuildModulesError, BuildOneSnapshot, LogEvent, LogLevel, PackageKey, Path,
    Reporter, SkippedOptionalDependencyLog, SkippedOptionalPackage, SkippedOptionalReason,
    apply_patch_to_dir, dependency_bins, lifecycle_scripts,
    patched_engines::skip_incompatible_optional_at,
    reject_frozen_store_build,
    side_effects::{self, FrozenStoreWrites, SideEffectsUpload},
    skip_incompatible_optional,
};
use std::{fs, io, path::PathBuf};

/// Whether `context` builds into global virtual store slots: the isolated
/// linker with the global virtual store on. The hoisted linker builds in
/// project-local copies.
pub(super) fn applies(context: &BuildOneSnapshot<'_>) -> bool {
    context.directories.layout.enable_global_virtual_store()
        && context.directories.pkg_roots_by_key.is_none()
}

pub(super) fn build<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    candidate: &BuildCandidate<'_>,
    cache_key: Option<&str>,
) -> Result<(), BuildModulesError> {
    let layout = context.directories.layout;
    let slot = PlacedPackage {
        target: crate::build_modules::slots::virtual_store_dir_for_key(layout, snapshot_key),
        staging: layout.staging_package_dir(snapshot_key),
    };
    let placed = slot.is_placed()?;
    let rerun = Rerun::of(context, candidate, (snapshot_key, cache_key));
    if placed && rerun == Rerun::No {
        slot.discard_staging();
        return already_placed::<Reporter>(context, snapshot_key, candidate);
    }
    if !placed
        && let Some(overlay) =
            side_effects::cached_overlay(context, snapshot_key, candidate, cache_key)
    {
        slot.place_cached::<Reporter>(context, overlay)?;
        return already_placed::<Reporter>(context, snapshot_key, candidate);
    }
    let optional = context.graph.snapshots.get(snapshot_key).is_some_and(|entry| entry.optional);
    if reject_frozen_store_build::<Reporter>(
        context,
        snapshot_key,
        (&candidate.name, &candidate.version),
        &FrozenStoreWrites {
            optional,
            has_patch: candidate.patch.is_some(),
            should_run_scripts: candidate.should_run_scripts,
        },
    )? {
        return Ok(());
    }
    if placed && side_effects::rebuild_found_empty_row(context, snapshot_key, cache_key) {
        return already_placed::<Reporter>(context, snapshot_key, candidate);
    }
    if !slot.staged(placed)? {
        return Ok(());
    }
    let built =
        build_staged::<Reporter>(context, snapshot_key, candidate, cache_key, &slot.staging)
            .inspect_err(|_| slot.discard_staging())?;
    if !built || (placed && rerun == Rerun::OutsideEffects) {
        slot.discard_staging();
        return Ok(());
    }
    slot.place_staged(placed)
}

/// Why a placed package is built again.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Rerun {
    No,
    /// The build changed nothing in its package, so its effect lies
    /// outside it, and every install runs it (pnpm/pnpm#14717). The
    /// placed package stays.
    OutsideEffects,
    /// A rebuild, or a package whose source is a local directory, which
    /// can change without the lockfile changing. The build replaces the
    /// placed package.
    Replace,
}

impl Rerun {
    fn of(
        context: &BuildOneSnapshot<'_>,
        candidate: &BuildCandidate<'_>,
        (snapshot_key, cache_key): (&PackageKey, Option<&str>),
    ) -> Self {
        let from_a_local_directory = context.graph.packages
            .and_then(|packages| packages.get(&candidate.metadata_key))
            .is_none_or(|metadata| {
                matches!(metadata.resolution, pnpm_lockfile::LockfileResolution::Directory(_))
            });
        if candidate.force_rebuild || from_a_local_directory {
            return Self::Replace;
        }
        if cache_key.is_some_and(|key| context.cache.side_effects.has_empty_row(snapshot_key, key))
        {
            return Self::OutsideEffects;
        }
        Self::No
    }
}

/// The slot already holds the package, from this install or another.
fn already_placed<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    candidate: &BuildCandidate<'_>,
) -> Result<(), BuildModulesError> {
    skip_incompatible_optional::<Reporter>(context, snapshot_key, candidate)?;
    dependency_bins::refresh_private_hoisted_for(context, snapshot_key)
}

/// Patch and build the staged package. `false` when the package is
/// skipped instead: an optional dependency whose build failed or whose
/// engines the patch made incompatible.
fn build_staged<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    candidate: &BuildCandidate<'_>,
    cache_key: Option<&str>,
    staging: &Path,
) -> Result<bool, BuildModulesError> {
    let is_patched = candidate.patch.is_some();
    if let Some(patch) = candidate.patch {
        let patch_file_path = patch.patch_file_path
            .as_deref()
            .ok_or_else(|| BuildModulesError::PatchFilePathMissing {
                dep_path: snapshot_key.to_string(),
            })?;
        dependency_bins::record_mutation(context, snapshot_key)?;
        apply_patch_to_dir(staging, patch_file_path).map_err(BuildModulesError::PatchApply)?;
        if skip_incompatible_optional_at::<Reporter>(context, snapshot_key, candidate, staging)? {
            return Ok(false);
        }
    }
    let has_side_effects = if candidate.should_run_scripts {
        dependency_bins::refresh(context, snapshot_key)?;
        dependency_bins::record_mutation(context, snapshot_key)?;
        match run_scripts::<Reporter>(context, snapshot_key, candidate, staging)? {
            Some(ran) => ran,
            None => return Ok(false),
        }
    } else {
        false
    };
    if is_patched || candidate.should_run_scripts {
        dependency_bins::refresh_private_hoisted_for(context, snapshot_key)?;
    }
    side_effects::upload_side_effects_cache(
        context,
        snapshot_key,
        &SideEffectsUpload {
            metadata_key: &candidate.metadata_key,
            pkg_dir: staging,
            cache_key,
            patch: candidate.patch,
            is_patched,
            has_side_effects,
        },
    );
    Ok(true)
}

/// Whether the scripts left side effects behind. `None` when an optional
/// dependency's build failed, which skips it.
fn run_scripts<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    candidate: &BuildCandidate<'_>,
    staging: &Path,
) -> Result<Option<bool>, BuildModulesError> {
    let optional = context.graph.snapshots.get(snapshot_key).is_some_and(|entry| entry.optional);
    match lifecycle_scripts::run_candidate_hooks::<Reporter>(
        context,
        snapshot_key,
        staging,
        &lifecycle_scripts::snapshot_extra_bin_paths(context, staging),
        optional,
    ) {
        Ok(ran) => Ok(Some(ran)),
        Err(error) if optional => {
            Reporter::emit(&LogEvent::SkippedOptionalDependency(SkippedOptionalDependencyLog {
                level: LogLevel::Debug,
                details: Some(error.to_string()),
                package: SkippedOptionalPackage::Installed {
                    id: snapshot_key.to_string(),
                    name: candidate.name.clone(),
                    version: candidate.version.clone(),
                },
                parents: None,
                prefix: context.directories.lockfile_dir.to_string_lossy().into_owned(),
                reason: SkippedOptionalReason::BuildFailure,
            }));
            Ok(None)
        }
        Err(error) => Err(BuildModulesError::LifecycleScript(error)),
    }
}

/// A package's directory in its slot, and this install's staging
/// directory beside it.
struct PlacedPackage {
    target: PathBuf,
    staging: PathBuf,
}

impl PlacedPackage {
    /// Whether the slot holds the package. Its `package.json` is written
    /// last by an import and is part of a renamed build, so its presence
    /// means the directory is final.
    fn is_placed(&self) -> Result<bool, BuildModulesError> {
        crate::build_modules::slots::package_is_placed(&self.target)
            .map_err(|source| BuildModulesError::PlaceBuiltPackage {
                path: self.target.clone(),
                source,
            })
    }

    /// Whether a package is staged for the build. A placed package that
    /// the link phase left alone is staged as a copy of it.
    fn staged(&self, placed: bool) -> Result<bool, BuildModulesError> {
        if self.staging.is_dir() {
            return Ok(true);
        }
        if !placed {
            return Ok(false);
        }
        pnpm_fs::copy_dirent(&self.target, &self.staging)
            .map_err(|source| BuildModulesError::PlaceBuiltPackage {
                path: self.staging.clone(),
                source,
            })?;
        Ok(true)
    }

    /// Import a cached build into the slot, as the link phase imports any
    /// package. A cached build that restores symlinks is staged and
    /// renamed instead, so its `package.json` never appears before them.
    fn place_cached<Reporter: self::Reporter>(
        &self,
        context: &BuildOneSnapshot<'_>,
        overlay: &pnpm_store_dir::SideEffectsOverlay,
    ) -> Result<(), BuildModulesError> {
        if overlay.symlinks.is_empty() {
            self.discard_staging();
            return crate::build_modules::slots::import_side_effects_into_slot::<Reporter>(
                &context.directories,
                &self.target,
                overlay,
            );
        }
        crate::build_modules::slots::materialize_side_effects::<Reporter>(
            &context.directories,
            &self.staging,
            overlay,
        )
        .inspect_err(|_| self.discard_staging())?;
        self.place_staged(false)
    }

    /// Rename the staged package into the slot. Another install that
    /// placed the package first wins, and this one discards its copy,
    /// unless `replace_placed` asks to replace it, as a rebuild does. A
    /// directory without a `package.json` is an import that did not
    /// finish, and the staged package takes its place.
    fn place_staged(&self, replace_placed: bool) -> Result<(), BuildModulesError> {
        const ATTEMPTS: usize = 3;
        for attempt in 1..=ATTEMPTS {
            let error = match pnpm_fs::rename_with_retry(&self.staging, &self.target) {
                Ok(()) => return Ok(()),
                Err(error) => error,
            };
            if !self.target.exists() || attempt == ATTEMPTS {
                self.discard_staging();
                return Err(BuildModulesError::PlaceBuiltPackage {
                    path: self.target.clone(),
                    source: error,
                });
            }
            if !replace_placed && self.is_placed()? {
                self.discard_staging();
                return Ok(());
            }
            self.move_target_aside().inspect_err(|_| self.discard_staging())?;
        }
        unreachable!("the last attempt returns")
    }

    /// Move the slot's package directory out of the way and remove it. A
    /// directory another install moved first is already gone.
    fn move_target_aside(&self) -> Result<(), BuildModulesError> {
        let mut replaced = self.staging.clone().into_os_string();
        replaced.push("-replaced");
        let replaced = PathBuf::from(replaced);
        match pnpm_fs::rename_with_retry(&self.target, &replaced) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(source) => {
                return Err(BuildModulesError::PlaceBuiltPackage {
                    path: self.target.clone(),
                    source,
                });
            }
        }
        if let Err(error) = pnpm_fs::remove_dir_all_with_retry(&replaced) {
            tracing::warn!(
                target: "pacquet::build",
                ?error,
                path = %replaced.display(),
                "failed to remove a replaced package directory",
            );
        }
        Ok(())
    }

    fn discard_staging(&self) {
        match fs::remove_dir_all(&self.staging) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => tracing::warn!(
                target: "pacquet::build",
                ?error,
                path = %self.staging.display(),
                "failed to remove a staged package",
            ),
        }
    }
}
