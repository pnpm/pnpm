use std::time::{Duration, SystemTime};

const RACY_WINDOW: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FileStamp {
    pub(super) mtime: SystemTime,
    pub(super) len: u64,
    #[cfg(unix)]
    pub(super) inode: u64,
    #[cfg(unix)]
    pub(super) ctime: (i64, i64),
    #[cfg(windows)]
    pub(super) file_index: Option<u64>,
}

impl FileStamp {
    pub(super) fn from_metadata(meta: &std::fs::Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        #[cfg(windows)]
        use std::os::windows::fs::MetadataExt;

        Self {
            mtime: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            len: meta.len(),
            #[cfg(unix)]
            inode: meta.ino(),
            #[cfg(unix)]
            ctime: (meta.ctime(), meta.ctime_nsec()),
            #[cfg(windows)]
            file_index: meta.file_index(),
        }
    }

    pub(super) fn is_racy(self, synced_at: Option<SystemTime>) -> bool {
        synced_at.is_none_or(|at| {
            at.duration_since(self.mtime)
                .is_ok_and(|duration| duration < RACY_WINDOW)
                || self.mtime >= at
        })
    }
}
