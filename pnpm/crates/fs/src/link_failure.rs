//! One reading of a failed link for every importer that links a file into
//! place: the store import tiers, the macOS directory-clone cache, the
//! injected-dependency syncer, and `pnpm patch-commit`.

use crate::is_cross_device;
use std::io;

/// Why a hardlink, reflink, or directory clone failed, read from the error
/// the call returned.
///
/// [`LinkFailure::fallback`] turns the failure into what the importer does
/// next, so every importer answers one error the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinkFailure {
    /// A path the call named is missing or already occupied. No other way
    /// of placing the file changes that.
    Path,
    /// The source and the target are on different filesystems.
    CrossDevice,
    /// The filesystem or a security policy refuses the link. A copy may
    /// still be allowed: a FUSE filesystem without hardlinks and a
    /// restricted container answer `EPERM`, Android's `SELinux` policy
    /// `EACCES`, a store file's Windows ACL `ERROR_ACCESS_DENIED`
    /// (pnpm/pnpm#14722, pnpm/pnpm#14771, pnpm/pnpm#14784).
    Denied,
    /// The source already carries every name the filesystem gives one file:
    /// 1024 on NTFS, 65000 on ext4. Every other file can still be linked.
    TooManyLinks,
    /// The filesystem cannot make this kind of link, or the error is one
    /// this reading does not know. ext4 refuses `FICLONE` with `ENOTTY`,
    /// NTFS refuses `FSCTL_DUPLICATE_EXTENTS_TO_FILE` with
    /// `ERROR_INVALID_FUNCTION`, and the `reflink-copy` crate reports a
    /// filesystem it has no clone for as `Unsupported`. An unknown error is
    /// read the same way, because the codes a filesystem refuses a link
    /// with are too many to list.
    Unsupported,
}

/// How an import method answers a failed link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFallbackPolicy {
    /// The method is one of a ladder, as `auto` and `clone-or-copy` are: a
    /// filesystem that cannot link moves every later file to the next
    /// method, down to a copy.
    NextMethod,
    /// The user asked for hardlinks: a failure that costs one file is
    /// copied, and a filesystem that cannot link is an error. A silent
    /// copy of every package is the disk cost `hardlink` was chosen to
    /// avoid.
    CopyOneFile,
}

/// What an importer does after a failed link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkFallback {
    /// Surface the error.
    Fail,
    /// Retire the method for every later file and try the next one.
    NextMethod,
    /// Copy this file and keep the method for the next one.
    CopyFile,
}

impl LinkFailure {
    /// Read `error`, as a link call returned it.
    #[must_use]
    pub fn of(error: &io::Error) -> Self {
        if is_cross_device(error) {
            return Self::CrossDevice;
        }
        match error.kind() {
            io::ErrorKind::NotFound | io::ErrorKind::AlreadyExists => Self::Path,
            io::ErrorKind::PermissionDenied => Self::Denied,
            io::ErrorKind::TooManyLinks => Self::TooManyLinks,
            _ => Self::Unsupported,
        }
    }

    /// What an importer with `policy` does about this failure.
    ///
    /// A cross-device link copies under both policies: a store on another
    /// device than `node_modules` is a placement the user can change, so
    /// the copy is not hidden from a user who asked for hardlinks.
    #[must_use]
    pub fn fallback(self, policy: LinkFallbackPolicy) -> LinkFallback {
        match (self, policy) {
            (Self::Path, _) => LinkFallback::Fail,
            (Self::TooManyLinks, _) | (Self::CrossDevice, LinkFallbackPolicy::CopyOneFile) => {
                LinkFallback::CopyFile
            }
            (Self::Denied | Self::Unsupported, LinkFallbackPolicy::CopyOneFile) => {
                LinkFallback::Fail
            }
            (
                Self::CrossDevice | Self::Denied | Self::Unsupported,
                LinkFallbackPolicy::NextMethod,
            ) => LinkFallback::NextMethod,
        }
    }
}

#[cfg(feature = "test")]
impl LinkFailure {
    /// Every failure, for a test that must answer each.
    pub const ALL: [Self; 5] =
        [Self::Path, Self::CrossDevice, Self::Denied, Self::TooManyLinks, Self::Unsupported];

    /// An error a link call on this platform returns for this failure.
    #[must_use]
    pub fn sample_error(self) -> io::Error {
        match self {
            Self::Path => io::Error::from(io::ErrorKind::NotFound),
            Self::CrossDevice => cross_device_error(),
            Self::Denied => denied_error(),
            Self::TooManyLinks => io::Error::from(io::ErrorKind::TooManyLinks),
            Self::Unsupported => io::Error::from(io::ErrorKind::Unsupported),
        }
    }
}

/// `EXDEV`, or Windows's `ERROR_NOT_SAME_DEVICE`.
#[cfg(feature = "test")]
fn cross_device_error() -> io::Error {
    #[cfg(unix)]
    return io::Error::from_raw_os_error(libc::EXDEV);
    #[cfg(windows)]
    return io::Error::from_raw_os_error(17);
    #[cfg(not(any(unix, windows)))]
    return io::Error::from(io::ErrorKind::CrossesDevices);
}

/// `EPERM`, or Windows's `ERROR_ACCESS_DENIED`.
#[cfg(feature = "test")]
fn denied_error() -> io::Error {
    #[cfg(unix)]
    return io::Error::from_raw_os_error(libc::EPERM);
    #[cfg(windows)]
    return io::Error::from_raw_os_error(5);
    #[cfg(not(any(unix, windows)))]
    return io::Error::from(io::ErrorKind::PermissionDenied);
}

#[cfg(test)]
mod tests;
