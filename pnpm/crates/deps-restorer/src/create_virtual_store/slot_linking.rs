use super::{
    ColdCapture, CreateVirtualStoreError, cache_keys::dir_clone_cacheable, removed_aliases_for,
    snapshot_needs_build_marker,
};
use crate::InstallPackageBySnapshotError;
use pnpm_lockfile::{PackageKey, PackageMetadata, PkgName, SnapshotEntry};
use pnpm_reporter::{LogEvent, LogLevel, ProgressLog, ProgressMessage, Reporter};
use pnpm_tarball::SharedReportedProgressKeys;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

pub(super) struct SlotLink<'a> {
    pub source: crate::SlotImportSource<'a>,
    pub(super) snapshot_key: &'a PackageKey,
    pub(super) snapshot: &'a SnapshotEntry,
    pub(super) cas_paths: &'a HashMap<String, PathBuf>,
    pub(super) warm_cache_key: Option<&'a str>,
    /// Whether the directory-clone cache may serve this slot — see
    /// [`dir_clone_cacheable`].
    pub(super) dir_clone_cacheable: bool,
    /// Child aliases dropped since the previous install, threaded into
    /// [`crate::SnapshotDependencyLinks::removed_aliases`] so their
    /// stale symlinks are unlinked during the link pass.
    pub(super) removed_aliases: &'a [PkgName],
}
/// One unique slot directory and every [`SlotLink`] that resolved to
/// it. Under the global virtual store, hash-equal peer variants share
/// a slot path, and `stage_and_swap` in
/// [`fn@crate::import_indexed_dir`] assumes an exclusive owner per
/// directory — so the link pass runs one task per group, with the
/// `removed_aliases` of every member unioned for cleanup.
pub(super) struct SlotDirGroup<'a> {
    representative: &'a SlotLink<'a>,
    /// Kept so each warm variant still emits its own progress line.
    pub(super) duplicates: Vec<&'a SlotLink<'a>>,
    /// `None` until a duplicate contributes an alias the
    /// representative lacks.
    pub(super) merged_removed_aliases: Option<Vec<PkgName>>,
}
impl SlotDirGroup<'_> {
    pub(super) fn removed_aliases(&self) -> &[PkgName] {
        self.merged_removed_aliases.as_deref().unwrap_or(self.representative.removed_aliases)
    }
}
/// Group `slots` by [`crate::VirtualStoreLayout::slot_dir`], preserving
/// first-occurrence order.
pub(super) fn group_slots_by_dir<'a>(
    slots: &'a [SlotLink<'a>],
    layout: &crate::VirtualStoreLayout,
) -> Vec<SlotDirGroup<'a>> {
    if !layout.enable_global_virtual_store() {
        // Project-local slot names embed the peer-suffixed key: every
        // group is a singleton, so skip the path construction.
        return slots
            .iter()
            .map(|slot| SlotDirGroup {
                representative: slot,
                duplicates: Vec::new(),
                merged_removed_aliases: None,
            })
            .collect();
    }
    let mut index_by_dir: HashMap<PathBuf, usize> = HashMap::with_capacity(slots.len());
    let mut groups: Vec<SlotDirGroup<'a>> = Vec::with_capacity(slots.len());
    for slot in slots {
        match index_by_dir.entry(layout.slot_dir(slot.snapshot_key)) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(groups.len());
                groups.push(SlotDirGroup {
                    representative: slot,
                    duplicates: Vec::new(),
                    merged_removed_aliases: None,
                });
            }
            std::collections::hash_map::Entry::Occupied(entry) => {
                merge_into_slot_group(&mut groups[*entry.get()], slot);
            }
        }
    }
    groups
}
/// Every slot sharing a directory has to have its removed aliases unlinked,
/// so the group carries their union.
pub(super) fn merge_into_slot_group<'a>(group: &mut SlotDirGroup<'a>, slot: &'a SlotLink<'a>) {
    group.duplicates.push(slot);
    if slot.removed_aliases.is_empty() {
        return;
    }
    let merged = group.merged_removed_aliases.get_or_insert_with(|| {
        group.representative.removed_aliases.to_vec()
    });
    for alias in slot.removed_aliases {
        if !merged.contains(alias) {
            merged.push(alias.clone());
        }
    }
}
/// How many completed cold snapshots accumulate before a link chunk
/// runs. Small enough that each chunk's `block_in_place` pause stays in
/// the low milliseconds; large enough that the rayon pass has real
/// parallelism to spend it on.
pub(super) const COLD_LINK_CHUNK: usize = 32;
/// Build [`SlotLink`]s for one chunk of cold captures and run the
/// parallel link pass over them. `template` carries the pass-invariant
/// fields; its `slots` are ignored.
pub(super) fn link_cold_chunk<Reporter: self::Reporter>(
    chunk: &[ColdCapture<'_>],
    packages: &HashMap<PackageKey, PackageMetadata>,
    marker_path: Option<&Path>,
    removed_aliases_by_key: &HashMap<PackageKey, Vec<PkgName>>,
    template: &LinkSlotsParallel<'_>,
) -> Result<(), CreateVirtualStoreError> {
    if chunk.is_empty() {
        return Ok(());
    }
    let cold_slots: Vec<SlotLink<'_>> = chunk
        .iter()
        .map(|capture| {
            let needs_build =
                snapshot_needs_build_marker(capture.snapshot_key, capture.requires_build);
            SlotLink {
                source: crate::SlotImportSource {
                    is_mutable: capture.source_is_mutable,
                    force: capture.force_import,
                    build_marker: needs_build.then_some(marker_path).flatten(),
                },
                snapshot_key: capture.snapshot_key,
                snapshot: capture.snapshot,
                cas_paths: &capture.cas_paths,
                warm_cache_key: None,

                dir_clone_cacheable: dir_clone_cacheable(
                    packages,
                    capture.snapshot_key,
                    needs_build,
                    capture.source_is_mutable,
                    capture.force_import,
                ),
                removed_aliases: removed_aliases_for(removed_aliases_by_key, capture.snapshot_key),
            }
        })
        .collect();
    link_slots_parallel::<Reporter>(LinkSlotsParallel { slots: &cold_slots, ..*template })
}
#[derive(Clone, Copy)]
pub(super) struct LinkSlotsParallel<'a> {
    pub import: crate::PackageImportOptions<'a>,
    pub link: crate::VirtualStoreLinkOptions<'a>,
    pub(super) batch: &'static str,
    pub(super) slots: &'a [SlotLink<'a>],
    pub(super) progress_reported: &'a SharedReportedProgressKeys,
    #[cfg(test)]
    pub(super) link_concurrency_probe:
        Option<&'a crate::create_virtual_dir_by_snapshot::tests::LinkConcurrencyProbe>,
}
pub(super) fn link_slots_parallel<Reporter: self::Reporter>(
    opts: LinkSlotsParallel<'_>,
) -> Result<(), CreateVirtualStoreError> {
    use rayon::prelude::*;

    let phase_start = std::time::Instant::now();
    let groups = group_slots_by_dir(opts.slots, opts.link.layout);
    let link_work =
        || groups.par_iter().try_for_each(|group| link_slot_group::<Reporter>(group, &opts));
    // Driving the link pass from inside an `async fn` means the
    // `par_iter` blocks the calling tokio worker for the duration. On
    // the production multi-thread runtime, `block_in_place` migrates
    // other futures off this worker so async progress continues; it
    // panics on the `current_thread` runtime that `#[tokio::test]`
    // defaults to, so fall back to a plain call there.
    let on_multi_thread = tokio::runtime::Handle::try_current()
        .is_ok_and(|handle| handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread);
    if on_multi_thread {
        tokio::task::block_in_place(link_work)?;
    } else {
        link_work()?;
    }
    tracing::info!(
        target: "pacquet::install::phase",
        phase = "link_slots",
        batch = opts.batch,
        slots = opts.slots.len(),
        unique_dirs = groups.len(),
        elapsed_ms = phase_start.elapsed().as_millis() as u64,
        "phase complete",
    );

    Ok(())
}
pub(super) fn link_slot_group<Reporter: self::Reporter>(
    group: &SlotDirGroup<'_>,
    opts: &LinkSlotsParallel<'_>,
) -> Result<(), CreateVirtualStoreError> {
    let slot = group.representative;
    let package_id = slot.snapshot_key.pkg_id();
    emit_group_warm_progress::<Reporter>(group, opts.import.requester, opts.progress_reported);

    crate::CreateVirtualDirBySnapshot {
        dependencies: crate::SnapshotDependencyLinks {
            package_key: slot.snapshot_key,
            snapshot: slot.snapshot,
            skipped: opts.link.skipped,
            include_optional: opts.link.include_optional,
            removed_aliases: group.removed_aliases(),
            symlink: opts.link.symlink,
        },
        import: opts.import,
        source: slot.source,
        layout: opts.link.layout,
        cas_paths: slot.cas_paths,

        package_id: &package_id,

        dir_clone_cache: if slot.dir_clone_cacheable { opts.link.dir_clone_cache } else { None },
        #[cfg(test)]
        link_concurrency_probe: opts.link_concurrency_probe,
    }
    .run::<Reporter>()
    .map_err(|error| {
        CreateVirtualStoreError::InstallPackageBySnapshot(
            InstallPackageBySnapshotError::CreateVirtualDir(error),
        )
    })
}
pub(super) fn emit_group_warm_progress<Reporter: self::Reporter>(
    group: &SlotDirGroup<'_>,
    requester: &str,
    progress_reported: &SharedReportedProgressKeys,
) {
    let reported = std::iter::once(group.representative).chain(group.duplicates.iter().copied());
    for slot in reported {
        if let Some(cache_key) = slot.warm_cache_key {
            emit_warm_snapshot_progress::<Reporter>(
                &slot.snapshot_key.pkg_id(),
                requester,
                progress_reported.contains(cache_key),
            );
        }
    }
}
pub(super) fn emit_warm_snapshot_progress<Reporter: self::Reporter>(
    package_id: &str,
    requester: &str,
    progress_reported: bool,
) {
    Reporter::emit(&LogEvent::Progress(ProgressLog {
        level: LogLevel::Debug,
        message: ProgressMessage::Resolved {
            package_id: package_id.to_owned(),
            requester: requester.to_owned(),
        },
    }));
    if !progress_reported {
        Reporter::emit(&LogEvent::Progress(ProgressLog {
            level: LogLevel::Debug,
            message: ProgressMessage::FoundInStore {
                package_id: package_id.to_owned(),
                requester: requester.to_owned(),
            },
        }));
    }
}
