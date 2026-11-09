//! Building a package into its global virtual store slot.
//!
//! A slot's package directory is final once its `package.json` exists:
//! its files never change afterwards, so installs share slots without
//! locks. The link phase imports a package that this install builds into
//! a staging directory beside the slot's package directory
//! ([`crate::VirtualStoreLayout::staging_package_dir`]). The build runs
//! there, and the built package is renamed into place. A side-effects
//! cache hit is imported into the slot directly, like any package. Only the
//! shims in the package's `node_modules/.bin` are linked again later, to
//! the same targets.

use super::{
    BuildCandidate, BuildModulesError, BuildOneSnapshot, LogEvent, LogLevel, PackageKey, Path,
    Reporter, SkippedOptionalDependencyLog, SkippedOptionalPackage, SkippedOptionalReason,
    apply_patch_to_dir, dependency_bins, lifecycle_scripts,
    patched_engines::{remove_linked_copies, skip_incompatible_optional_at},
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
        placed: false,
    };
    let slot = PlacedPackage { placed: slot.is_placed()?, ..slot };
    let rerun = Rerun::of(context, candidate, (snapshot_key, cache_key));
    if slot.placed && !rerun.builds_placed() {
        slot.discard_staging();
        return already_placed::<Reporter>(context, snapshot_key, candidate);
    }
    if !slot.placed
        && let Some(overlay) =
            side_effects::cached_overlay(context, snapshot_key, candidate, cache_key)
    {
        slot.place_cached::<Reporter>(context, overlay)?;
        return already_placed::<Reporter>(context, snapshot_key, candidate);
    }
    build_and_place::<Reporter>(context, (snapshot_key, cache_key), candidate, (&slot, rerun))
}

/// Build the package in this install's staging directory and place it.
fn build_and_place<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    (snapshot_key, cache_key): (&PackageKey, Option<&str>),
    candidate: &BuildCandidate<'_>,
    (slot, rerun): (&PlacedPackage, Rerun),
) -> Result<(), BuildModulesError> {
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
    if slot.placed && side_effects::rebuild_found_empty_row(context, snapshot_key, cache_key) {
        return already_placed::<Reporter>(context, snapshot_key, candidate);
    }
    if !slot.staged()? {
        return Ok(());
    }
    let built = build_staged::<Reporter>(context, snapshot_key, candidate, &slot.staging)
        .inspect_err(|_| slot.discard_staging())?;
    settle_built(context, (snapshot_key, cache_key), candidate, (slot, rerun), built)
}

/// Place what the build left in staging, then cache it. `built` is `None`
/// when the package was skipped instead.
fn settle_built(
    context: &BuildOneSnapshot<'_>,
    (snapshot_key, cache_key): (&PackageKey, Option<&str>),
    candidate: &BuildCandidate<'_>,
    (slot, rerun): (&PlacedPackage, Rerun),
    built: Option<bool>,
) -> Result<(), BuildModulesError> {
    let Some(has_side_effects) = built else {
        slot.discard_staging();
        // A package that never reaches its slot must not stay linked. A
        // placed package keeps its links, as a failed rebuild leaves it.
        if !slot.placed {
            remove_linked_copies(context, snapshot_key);
        }
        return Ok(());
    };
    if slot.placed && !rerun.replaces_placed() {
        slot.discard_staging();
        return Ok(());
    }
    slot.place_staged(if slot.placed { WhenPlaced::Replace } else { WhenPlaced::Keep })?;
    // Only once the package is placed, so a cache row always names a placed
    // package, and no install imports it into a slot a build still places.
    side_effects::upload_side_effects_cache(
        context,
        snapshot_key,
        &SideEffectsUpload {
            metadata_key: &candidate.metadata_key,
            pkg_dir: &slot.target,
            cache_key,
            patch: candidate.patch,
            is_patched: candidate.patch.is_some(),
            has_side_effects,
        },
    );
    Ok(())
}

/// Why a placed package is built again.
#[derive(Clone, Copy)]
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
    /// Whether a placed package is built again.
    fn builds_placed(self) -> bool {
        match self {
            Rerun::No => false,
            Rerun::OutsideEffects | Rerun::Replace => true,
        }
    }

    /// Whether the build replaces the placed package.
    fn replaces_placed(self) -> bool {
        match self {
            Rerun::No | Rerun::OutsideEffects => false,
            Rerun::Replace => true,
        }
    }

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

/// Patch and build the staged package, reporting whether its scripts left
/// side effects. `None` when the package is skipped instead: an optional
/// dependency whose build failed or whose engines the patch made
/// incompatible.
fn build_staged<Reporter: self::Reporter>(
    context: &BuildOneSnapshot<'_>,
    snapshot_key: &PackageKey,
    candidate: &BuildCandidate<'_>,
    staging: &Path,
) -> Result<Option<bool>, BuildModulesError> {
    if let Some(patch) = candidate.patch {
        let patch_file_path = patch.patch_file_path
            .as_deref()
            .ok_or_else(|| BuildModulesError::PatchFilePathMissing {
                dep_path: snapshot_key.to_string(),
            })?;
        dependency_bins::record_mutation(context, snapshot_key)?;
        apply_patch_to_dir(staging, patch_file_path).map_err(BuildModulesError::PatchApply)?;
        if skip_incompatible_optional_at::<Reporter>(context, snapshot_key, candidate, staging)? {
            return Ok(None);
        }
    }
    let has_side_effects = if candidate.should_run_scripts {
        dependency_bins::link_staged(context, snapshot_key)?;
        dependency_bins::record_mutation(context, snapshot_key)?;
        match run_scripts::<Reporter>(context, snapshot_key, candidate, staging)? {
            Some(ran) => ran,
            None => return Ok(None),
        }
    } else {
        false
    };
    if candidate.patch.is_some() || candidate.should_run_scripts {
        dependency_bins::refresh_private_hoisted_for(context, snapshot_key)?;
    }
    Ok(Some(has_side_effects))
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

/// What placing a staged package does to a package already in the slot.
#[derive(Clone, Copy)]
enum WhenPlaced {
    /// Another install placed the package first, so this one discards its
    /// copy.
    Keep,
    /// A rebuild or a changed local directory replaces the placed package.
    Replace,
}

impl WhenPlaced {
    fn keeps_placed(self) -> bool {
        match self {
            WhenPlaced::Keep => true,
            WhenPlaced::Replace => false,
        }
    }
}

/// A package's directory in its slot, and this install's staging
/// directory beside it.
struct PlacedPackage {
    target: PathBuf,
    staging: PathBuf,
    /// Whether `target` held the package when the build began.
    placed: bool,
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
    fn staged(&self) -> Result<bool, BuildModulesError> {
        if self.staging.is_dir() {
            return Ok(true);
        }
        if !self.placed {
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
        self.place_staged(WhenPlaced::Keep)
    }

    /// Rename the staged package into the slot. A directory without a
    /// `package.json` is an import that did not finish, and the staged
    /// package takes its place.
    fn place_staged(&self, when_placed: WhenPlaced) -> Result<(), BuildModulesError> {
        const ATTEMPTS: usize = 3;
        for attempt in 1..=ATTEMPTS {
            let error = match self.rename_into_place() {
                Ok(()) => return Ok(()),
                Err(error) => error,
            };
            if when_placed.keeps_placed() && self.is_placed()? {
                self.discard_staging();
                return Ok(());
            }
            if !self.target.exists() || attempt == ATTEMPTS {
                self.discard_staging();
                return Err(BuildModulesError::PlaceBuiltPackage {
                    path: self.target.clone(),
                    source: error,
                });
            }
            self.move_target_aside().inspect_err(|_| self.discard_staging())?;
        }
        unreachable!("the last attempt returns")
    }

    /// A target that exists is a lost race or a rebuild, which a rename
    /// retried for transient Windows file locks would only delay.
    fn rename_into_place(&self) -> io::Result<()> {
        if self.target.exists() {
            fs::rename(&self.staging, &self.target)
        } else {
            pnpm_fs::rename_with_retry(&self.staging, &self.target)
        }
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
