//! A file's stat, and whether a recorded stat still stands for the file's
//! content.

use pnpm_workspace_state::{FileSignature, Validation};
use std::{fs, path::Path};

/// A filesystem that keeps whole seconds may round an mtime down by up to
/// this much (FAT rounds to two seconds).
const WHOLE_SECOND_GRANULARITY_NS: i64 = 2_000_000_000;

/// Whether the next check trusts the stat of every file `validation` records.
pub(crate) fn trusts_every_file(validation: &Validation) -> bool {
    validation.files
        .values()
        .all(|signature| signature_trusted(signature, validation.recorded_at_ns))
}

pub(super) fn signature_trusted(signature: &FileSignature, recorded_at_ns: i64) -> bool {
    stat_trusted(signature.mtime_ns, signature.ctime_ns, recorded_at_ns)
}

/// Whether a stat with these times, recorded at `recorded_at_ns`, stands for
/// the file's content: the file was last written before the recording
/// stat'd it, outside the rounding of a whole-second filesystem.
pub(super) fn stat_trusted(mtime_ns: i64, ctime_ns: Option<i64>, recorded_at_ns: i64) -> bool {
    let changed_ns = last_change_ns(mtime_ns, ctime_ns);
    let window = if changed_ns % 1_000_000_000 == 0 { WHOLE_SECOND_GRANULARITY_NS } else { 0 };
    changed_ns.saturating_add(window) < recorded_at_ns
}

/// The parts of a file's stat a [`FileSignature`] records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileStat {
    mtime_ns: i64,
    ctime_ns: Option<i64>,
    size: u64,
}

impl FileStat {
    /// `None` when the file cannot be stat'd.
    pub(crate) fn at(path: &Path) -> Option<Self> {
        Self::of(path, &fs::metadata(path).ok()?)
    }

    pub(super) fn of(path: &Path, metadata: &fs::Metadata) -> Option<Self> {
        let elapsed = metadata
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        Some(Self {
            mtime_ns: i64::try_from(elapsed.as_nanos()).ok()?,
            ctime_ns: ctime_ns(path, metadata),
            size: metadata.len(),
        })
    }

    pub(crate) fn matches(&self, signature: &FileSignature) -> bool {
        self.matches_stat(signature.mtime_ns, signature.ctime_ns, signature.size)
    }

    pub(super) fn matches_stat(&self, mtime_ns: i64, ctime_ns: Option<i64>, size: u64) -> bool {
        self.mtime_ns == mtime_ns && self.ctime_ns == ctime_ns && self.size == size
    }

    /// The signature of a file read at this stat with content `digest`.
    pub(super) fn signature(self, digest: String) -> FileSignature {
        FileSignature { mtime_ns: self.mtime_ns, ctime_ns: self.ctime_ns, size: self.size, digest }
    }

    pub(super) fn parts(self) -> (i64, Option<i64>, u64) {
        (self.mtime_ns, self.ctime_ns, self.size)
    }
}

fn last_change_ns(mtime_ns: i64, ctime_ns: Option<i64>) -> i64 {
    ctime_ns.map_or(mtime_ns, |ctime_ns| ctime_ns.max(mtime_ns))
}

/// The inode change time, which also moves when an edit restores the mtime.
#[cfg(unix)]
fn ctime_ns(_path: &Path, metadata: &fs::Metadata) -> Option<i64> {
    use std::os::unix::fs::MetadataExt as _;
    metadata
        .ctime()
        .checked_mul(1_000_000_000)?
        .checked_add(metadata.ctime_nsec())
}

/// NTFS's change time, which also moves when an edit restores the mtime. It
/// is what Node.js reports as `ctime` on Windows.
#[cfg(windows)]
fn ctime_ns(path: &Path, _metadata: &fs::Metadata) -> Option<i64> {
    use std::os::windows::{fs::OpenOptionsExt as _, io::AsRawHandle as _};
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_BASIC_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FileBasicInfo, GetFileInformationByHandleEx,
    };
    /// 100-nanosecond intervals between 1601-01-01 and the Unix epoch.
    const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;

    let file = fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(path)
        .ok()?;
    let mut info = std::mem::MaybeUninit::<FILE_BASIC_INFO>::uninit();
    // SAFETY: the handle stays open for the call, and `info` is a writable
    // buffer of exactly the size passed.
    let succeeded = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileBasicInfo,
            info.as_mut_ptr().cast(),
            u32::try_from(size_of::<FILE_BASIC_INFO>()).ok()?,
        )
    };
    if succeeded == 0 {
        return None;
    }
    // SAFETY: the call succeeded, so it filled `info`.
    let change_ticks = unsafe { info.assume_init() }.ChangeTime;
    change_ticks.checked_sub(UNIX_EPOCH_TICKS)?.checked_mul(100)
}

#[cfg(not(any(unix, windows)))]
fn ctime_ns(_path: &Path, _metadata: &fs::Metadata) -> Option<i64> {
    None
}
