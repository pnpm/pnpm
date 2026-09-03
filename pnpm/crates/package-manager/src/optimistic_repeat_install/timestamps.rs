//! Modification times, and the baseline a recorded validation is compared against.

use super::{Config, Lockfile, PackageManifest, Path, PathBuf, SystemTime, fs};

pub(crate) const NANOS_PER_MILLI: i64 = 1_000_000;

pub(crate) const NANOS_PER_SEC: i64 = 1_000_000_000;

/// A file's mtime, kept at full nanosecond precision. The recorded
/// `lastValidatedTimestamp` is only millisecond-precise (it is a
/// filesystem mtime truncated to ms — see
/// [`crate::install::build_workspace_state`]), but the comparison against
/// it must not be: pacquet installs are fast enough that a lockfile
/// written by one install and a manifest edited moments later can share
/// the same millisecond, so a millisecond-granular comparison would miss
/// the edit and wrongly keep the fast path. Comparing the file's
/// nanosecond mtime against the truncated (rounded-down) reference
/// distinguishes them.
#[derive(Clone, Copy)]
pub(crate) struct FileMtime {
    /// Milliseconds since the epoch. Used where the value is *recorded* as
    /// the reference (the state stores `lastValidatedTimestamp` in ms, and
    /// pnpm's `checkDepsStatus` compares in ms).
    pub(crate) ms: i64,
    /// Nanoseconds since the epoch. Used as the *subject* of a comparison,
    /// where sub-millisecond precision matters.
    pub(crate) ns: i64,
    /// The filesystem stored no sub-second component, so the real
    /// modification time lies anywhere in `[ns, ns + 1s)`. True on
    /// second-granularity filesystems (ext4 with 128-byte inodes, HFS+,
    /// some CI runner disks); false wherever mtimes keep sub-second
    /// precision (ext4 256-byte inodes, APFS, NTFS, xfs, and so on).
    pub(crate) whole_second: bool,
}

/// [`FileMtime`] of `path`, `None` when it can't be stat'd.
pub(crate) fn file_mtime(path: &Path) -> Option<FileMtime> {
    let metadata = fs::metadata(path).ok()?;
    file_mtime_from_metadata(&metadata)
}

pub(crate) fn file_mtime_from_metadata(metadata: &fs::Metadata) -> Option<FileMtime> {
    let modified = metadata.modified().ok()?;
    let elapsed = modified.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    Some(FileMtime {
        ms: i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX),
        ns: i64::try_from(elapsed.as_nanos()).unwrap_or(i64::MAX),
        whole_second: elapsed.subsec_nanos() == 0,
    })
}

/// Millisecond mtime of `path`, `None` when it can't be stat'd.
pub(crate) fn mtime_ms(path: &Path) -> Option<i64> {
    file_mtime(path).map(|mtime| mtime.ms)
}

/// Whether a file with mtime `subject` may have been modified at or after
/// `reference_ms`. The reference is millisecond-precise (a filesystem
/// mtime truncated toward zero); the subject is compared at nanosecond
/// precision, so a file whose whole-millisecond mtime equals the
/// reference's but which was really written later in that millisecond is
/// still seen as modified. On a filesystem that rounds mtimes down to
/// whole seconds the file could have been touched anywhere within its
/// second, so the whole second counts as possibly-after. Erring toward
/// "modified" only runs the authoritative content check — it never skips
/// a needed install.
pub(crate) fn modified_at_or_after(subject: FileMtime, reference_ms: i64) -> bool {
    let reference_ns = reference_ms.saturating_mul(NANOS_PER_MILLI);
    if subject.whole_second {
        subject.ns.saturating_add(NANOS_PER_SEC) > reference_ns
    } else {
        subject.ns > reference_ns
    }
}

/// The freshness baseline recorded in the workspace state: the latest
/// mtime among the lockfile this install wrote (the wanted
/// `pnpm-lock.yaml`, or the current `<virtual_store_dir>/lock.yaml` when
/// the wanted one is absent) and the project manifests it validated.
///
/// A filesystem mtime, not the wall clock, so the baseline shares a clock
/// with every file the repeat-install check later compares against it: a
/// wall-vs-mtime clock skew (observed ~2 ms on some CI microVMs, where the
/// wall clock ran ahead of the filesystem's mtime clock) would otherwise
/// let a `now()` baseline sit above the mtime of a manifest/pnpmfile
/// edited moments after the install, hiding the edit and wrongly keeping
/// the fast path. Taking the max over the manifests too means a content
/// check that passed on an already-edited manifest still blesses it, so
/// the next run can take the pure-mtime fast path. `None` when nothing can
/// be stat'd.
pub(crate) fn validation_baseline_ms(
    workspace_root: &Path,
    config: &Config,
    project_manifests: &[(PathBuf, &PackageManifest)],
) -> Option<i64> {
    let lockfile = mtime_ms(&workspace_root.join(config.wanted_lockfile_name()))
        .or_else(|| mtime_ms(&config.virtual_store_dir.join(Lockfile::CURRENT_FILE_NAME)));
    project_manifests
        .iter()
        .filter_map(|(_, manifest)| mtime_ms(manifest.path()))
        .chain(lockfile)
        .max()
}

/// The filesystem clock's current time in milliseconds, taken from an
/// mtime the filesystem stamps itself, so it shares a clock with every
/// file the repeat-install check compares — the reason
/// [`validation_baseline_ms`] cannot use the wall clock either.
///
/// A repeat-install check reads it *before* validating the contents it
/// will later bless: a file written after the probe carries a later
/// mtime and so still reads as modified, while one written before it is
/// covered by the check that just passed. An install reads it as it
/// writes the workspace state, where pnpm records `Date.now()`.
///
/// The probe is an unnamed temporary file in the directory holding the
/// workspace state — pnpm's own, on the volume the state write lands on
/// — so nothing else can observe it and it needs no cleanup. The
/// directory is created first because an install may be about to write
/// the state file for the first time. `None` when the probe cannot be
/// created or stat'd, leaving the caller with the mtime-derived
/// baseline.
pub(crate) fn filesystem_now_ms(workspace_root: &Path) -> Option<i64> {
    let state_path = pnpm_workspace_state::get_file_path(workspace_root);
    let parent = state_path.parent()?;
    fs::create_dir_all(parent).ok()?;
    let probe = tempfile::tempfile_in(parent).ok()?;
    file_mtime_from_metadata(&probe.metadata().ok()?).map(|mtime| mtime.ms)
}

/// The `lastValidatedTimestamp` to record once an install or a
/// repeat-install content check has validated the manifests:
/// `baseline_ms` — the mtimes of the files it validated, per
/// [`validation_baseline_ms`] — raised to the filesystem clock's
/// `now_ms`.
///
/// `baseline_ms` on its own never converges. It is a file mtime
/// truncated to milliseconds, and [`modified_at_or_after`] deliberately
/// reads a file whose mtime falls inside that same millisecond — or, on
/// a whole-second filesystem, inside that same second — as
/// possibly-modified. The very file that forced this content check keeps
/// forcing one on every later run, so the pure-mtime fast path becomes
/// unreachable
/// ([#13907](https://github.com/pnpm/pnpm/issues/13907)). An install
/// that leaves the lockfile alone records the newest manifest's mtime
/// the same way, so on a sub-millisecond filesystem that manifest reads
/// as modified against its own truncated mtime on every `pnpm run`
/// ([#14486](https://github.com/pnpm/pnpm/issues/14486)). Raising the
/// baseline to the filesystem's *now* closes that window without
/// post-dating it into the future, which would hide an edit made in the
/// interval it skipped over. Keeping `baseline_ms` as the floor
/// preserves the blessing of a validated file whose mtime already lies
/// ahead of the filesystem clock.
///
/// pnpm's `checkDepsStatus` records `Date.now()` at this point. Reading
/// the same *now* off the filesystem keeps the wall clock — which can
/// run ahead of the mtime clock — out of the comparison.
///
/// A check that finishes inside the millisecond it is blessing leaves
/// the baseline where it was, on purpose: `now_ms` is the present, not a
/// point past it, and there is nothing later to record yet. The next run
/// lands in a later millisecond and converges then, so the equality case
/// costs one more content check rather than repeating forever.
pub(crate) fn refreshed_validation_baseline_ms(baseline_ms: i64, now_ms: Option<i64>) -> i64 {
    now_ms.map_or(baseline_ms, |now| baseline_ms.max(now))
}

/// Whether `<workspace_root>/pnpm-lock.yaml` has an mtime newer than the
/// last validation. A lockfile-only change leaves every manifest
/// untouched but must still defeat the manifest-mtime fast path. A
/// missing lockfile reports `false` here — it is handled by the
/// existence and stand-in gates, not treated as a modification.
///
/// Compared at whole-millisecond precision, unlike the manifest / patch /
/// pnpmfile checks: `lastValidatedTimestamp` is itself a lockfile mtime
/// truncated to milliseconds (see
/// [`crate::install::build_workspace_state`]), so a nanosecond comparison
/// would flag the *unchanged* lockfile against its own truncated value on
/// every repeat install and force a content check each time. An external
/// lockfile edit (git checkout, manual rewrite) lands in a later
/// millisecond, so millisecond precision still catches it.
pub(crate) fn wanted_lockfile_modified(
    workspace_root: &Path,
    config: &Config,
    last_validated_timestamp: i64,
) -> bool {
    file_mtime(&workspace_root.join(config.wanted_lockfile_name()))
        .is_some_and(|mtime| lockfile_modified_since(mtime, last_validated_timestamp))
}

/// Whether the lockfile's `subject` mtime post-dates `reference_ms`.
///
/// On a sub-second filesystem this is a whole-*millisecond* comparison,
/// unlike the nanosecond [`modified_at_or_after`] used for
/// manifests/patches/pnpmfiles: the baseline is the lockfile's own mtime
/// truncated to milliseconds, so a nanosecond comparison would flag the
/// unchanged lockfile against its own truncated value on every repeat
/// install. An external lockfile edit lands in a later millisecond and is
/// still caught. On a whole-second-mtime filesystem the whole second is
/// treated as possibly-after (as [`modified_at_or_after`] does), because
/// there a same-second external edit is indistinguishable from the
/// install's own lockfile write by mtime alone, so it must fall through to
/// the authoritative content check. See [`wanted_lockfile_modified`].
pub(crate) fn lockfile_modified_since(subject: FileMtime, reference_ms: i64) -> bool {
    if subject.whole_second {
        subject.ms.saturating_add(1_000) > reference_ms
    } else {
        subject.ms > reference_ms
    }
}
