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
}

impl FileStamp {
    pub(super) fn from_metadata(meta: &std::fs::Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        Self {
            mtime: meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            len: meta.len(),
            #[cfg(unix)]
            inode: meta.ino(),
            #[cfg(unix)]
            ctime: (meta.ctime(), meta.ctime_nsec()),
        }
    }

    pub(super) fn is_racy(self, synced_at: Option<SystemTime>) -> bool {
        let Some(at) = synced_at else {
            return true;
        };
        if SystemTime::now()
            .duration_since(at)
            .is_ok_and(|elapsed| elapsed >= RACY_WINDOW)
        {
            return false;
        }
        at.duration_since(self.mtime)
            .or_else(|_| self.mtime.duration_since(at))
            .is_ok_and(|duration| duration < RACY_WINDOW)
    }
}

#[cfg(test)]
mod tests;
