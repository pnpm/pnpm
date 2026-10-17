use super::FileStamp;
use std::time::{Duration, SystemTime};

#[test]
fn uninitialized_sync_is_always_racy() {
    let stamp = FileStamp {
        mtime: SystemTime::UNIX_EPOCH,
        len: 10,
        #[cfg(unix)]
        inode: 1,
        #[cfg(unix)]
        ctime: (0, 0),
    };
    assert!(stamp.is_racy(None));
}

#[test]
fn expired_sync_is_not_racy() {
    let now = SystemTime::now();
    let synced_at = now - Duration::from_secs(3);
    let stamp = FileStamp {
        mtime: now,
        len: 10,
        #[cfg(unix)]
        inode: 1,
        #[cfg(unix)]
        ctime: (0, 0),
    };
    assert!(!stamp.is_racy(Some(synced_at)));
}

#[test]
fn recent_mtime_within_window_is_racy() {
    let now = SystemTime::now();
    let synced_at = now;
    let stamp = FileStamp {
        mtime: now - Duration::from_millis(500),
        len: 10,
        #[cfg(unix)]
        inode: 1,
        #[cfg(unix)]
        ctime: (0, 0),
    };
    assert!(stamp.is_racy(Some(synced_at)));
}

#[test]
fn future_mtime_far_ahead_is_not_racy() {
    let now = SystemTime::now();
    let synced_at = now;
    let stamp = FileStamp {
        mtime: now + Duration::from_hours(1),
        len: 10,
        #[cfg(unix)]
        inode: 1,
        #[cfg(unix)]
        ctime: (0, 0),
    };
    assert!(!stamp.is_racy(Some(synced_at)));
}
