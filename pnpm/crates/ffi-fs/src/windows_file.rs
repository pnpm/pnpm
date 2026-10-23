//! Windows file queries and the one rename that has no `std` equivalent.

use std::{
    ffi::OsStr,
    fs::File,
    io,
    mem::MaybeUninit,
    os::windows::{ffi::OsStrExt as _, fs::OpenOptionsExt as _, io::AsRawHandle as _},
    path::{Path, PathBuf},
};
use windows_sys::Win32::{
    Foundation::S_OK,
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_BASIC_INFO, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FileBasicInfo, GetFileAttributesW, GetFileInformationByHandle,
        GetFileInformationByHandleEx, GetShortPathNameW, INVALID_FILE_ATTRIBUTES,
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    },
    System::{Com::CoTaskMemFree, SystemInformation::GetSystemDirectoryW},
    UI::Shell::{FOLDERID_LocalAppData, KF_FLAG_DONT_VERIFY, SHGetKnownFolderPath},
};

/// What one `GetFileInformationByHandle` reports about an open file.
///
/// The three fields are returned together because they come from one call:
/// a caller that wants the identity and the link count pays for one query,
/// not two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileInformation {
    /// Serial number of the volume holding the file. Unique only while the
    /// volume is mounted, so it identifies a file alongside `index` and
    /// never on its own.
    pub volume_serial: u32,
    /// The file's index on its volume, the Windows counterpart of an inode
    /// number.
    pub index: u64,
    /// How many directory entries name this file.
    pub hard_links: u32,
}

/// Query the identity and link count of the file at `path`.
///
/// `path` is opened for reading, so a file another process holds without
/// sharing reads is reported as inaccessible rather than queried.
pub fn file_information(path: &Path) -> io::Result<FileInformation> {
    let file = File::open(path)?;
    let mut info = MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::uninit();
    // SAFETY: the `&File` keeps the handle open for the whole call, and the
    // pointer is to a stack local of exactly the structure the call writes,
    // which outlives it.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), info.as_mut_ptr()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call reported success, so it initialized every field.
    let info = unsafe { info.assume_init() };
    Ok(FileInformation {
        volume_serial: info.dwVolumeSerialNumber,
        index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
        hard_links: info.nNumberOfLinks,
    })
}

/// The change time of the file at `path`, in 100-nanosecond ticks since
/// 1601-01-01 UTC.
///
/// This is the closest Windows has to a Unix ctime: the moment any of the
/// file's metadata last changed. `std` exposes creation, access, and
/// modification times but not this one.
///
/// `path` is opened for its attributes alone, which is the least access
/// the query needs and the only one a file open for writing elsewhere
/// still grants.
pub fn change_time_ticks(path: &Path) -> io::Result<i64> {
    let file = File::options()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(path)?;
    let mut info = MaybeUninit::<FILE_BASIC_INFO>::uninit();
    let size = u32::try_from(size_of::<FILE_BASIC_INFO>())
        .map_err(|_| io::Error::other("FILE_BASIC_INFO does not fit a DWORD"))?;
    // SAFETY: the `&File` keeps the handle open for the whole call, and the
    // pointer is to a stack local of exactly the structure `FileBasicInfo`
    // selects, whose size is the one passed, and which outlives the call.
    let queried = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileBasicInfo,
            info.as_mut_ptr().cast(),
            size,
        )
    };
    if queried == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call reported success, so it initialized the structure.
    Ok(unsafe { info.assume_init() }.ChangeTime)
}

/// The attributes of `path` that distinguish what kind of entry it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileAttributes {
    /// Whether the entry is a directory.
    pub is_directory: bool,
    /// Whether the entry is a reparse point: a symlink, a junction, or
    /// another filter's redirect.
    pub is_reparse_point: bool,
}

/// Query `path`'s attributes without following a reparse point at `path`
/// itself, so a symlink reports as one rather than as its target.
pub fn file_attributes(path: &Path) -> io::Result<FileAttributes> {
    let path = wide(path.as_os_str())?;
    // SAFETY: `path` is a NUL-terminated UTF-16 buffer that `wide` built,
    // alive for the whole call, which reads it and retains no pointer.
    let attributes = unsafe { GetFileAttributesW(path.as_ptr()) };
    if attributes == INVALID_FILE_ATTRIBUTES {
        return Err(io::Error::last_os_error());
    }
    Ok(FileAttributes {
        is_directory: attributes & FILE_ATTRIBUTE_DIRECTORY != 0,
        is_reparse_point: attributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
    })
}

/// The 8.3 short form of `path`, or `None` where the volume does not keep
/// one or the path does not exist.
///
/// Pass `path` in its extended-length (`\\?\`) form to have a path over
/// `MAX_PATH` accepted; the query does not add the prefix itself, because
/// which paths may carry it is the caller's to decide.
#[must_use]
pub fn short_path(path: &Path) -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt as _;

    let path = wide(path.as_os_str()).ok()?;
    // SAFETY: `path` is a NUL-terminated UTF-16 buffer that `wide` built,
    // alive for the whole call. A null output buffer with a zero length
    // asks only for the size the answer needs, which is the documented way
    // to size the buffer below.
    let required = unsafe { GetShortPathNameW(path.as_ptr(), std::ptr::null_mut(), 0) };
    if required == 0 {
        return None;
    }
    let mut buffer = vec![0_u16; required as usize];
    // SAFETY: as above for `path`, and `buffer` is a live `Vec` of exactly
    // `required` writable units, which is the capacity the call is told it
    // has.
    let written = unsafe { GetShortPathNameW(path.as_ptr(), buffer.as_mut_ptr(), required) };
    // On success the written length excludes the terminating NUL, so a
    // length that reaches `required` means the path changed under the two
    // calls and the answer was truncated.
    if written == 0 || written >= required {
        return None;
    }
    buffer.truncate(written as usize);
    Some(std::ffi::OsString::from_wide(&buffer).into())
}

/// The current user's local application-data directory, the per-user
/// location Windows offers for state that does not roam.
///
/// `None` carries the `HRESULT`, because a caller has nowhere else to look
/// and the code is what tells a configuration problem from a transient
/// one.
pub fn local_app_data_directory() -> Result<PathBuf, i32> {
    let folder = CoTaskMemPath::known(&FOLDERID_LocalAppData)?;
    Ok(folder.to_path_buf())
}

/// A path `SHGetKnownFolderPath` allocated, released when dropped.
struct CoTaskMemPath(*mut u16);

impl CoTaskMemPath {
    fn known(folder: &windows_sys::core::GUID) -> Result<Self, i32> {
        let mut path = std::ptr::null_mut();
        // SAFETY: the folder id is borrowed for the call, the flags and the
        // null token are taken by value, and the output is a fully
        // initialized stack local that outlives the call. A path the call
        // allocates is this call's to release.
        let result = unsafe {
            SHGetKnownFolderPath(
                std::ptr::from_ref(folder),
                KF_FLAG_DONT_VERIFY as u32,
                std::ptr::null_mut(),
                &raw mut path,
            )
        };
        if result == S_OK { Ok(Self(path)) } else { Err(result) }
    }

    fn to_path_buf(&self) -> PathBuf {
        use std::os::windows::ffi::OsStringExt as _;

        let mut length = 0;
        // SAFETY: a successful `SHGetKnownFolderPath` leaves a
        // NUL-terminated UTF-16 string at `self.0`, so every unit up to the
        // terminator is inside the allocation.
        while unsafe { *self.0.add(length) } != 0 {
            length += 1;
        }
        // SAFETY: `length` units precede the terminator, so the slice is
        // inside the allocation, which `self` owns and keeps alive.
        let units = unsafe { std::slice::from_raw_parts(self.0, length) };
        std::ffi::OsString::from_wide(units).into()
    }
}

impl Drop for CoTaskMemPath {
    fn drop(&mut self) {
        // SAFETY: the pointer came from `SHGetKnownFolderPath`, which
        // documents `CoTaskMemFree` as what releases it, and a guard is
        // dropped once, so it is freed once.
        unsafe { CoTaskMemFree(self.0.cast()) }
    }
}

/// The Windows system directory, which is where the bundled tools such as
/// `taskkill` live. `None` when it cannot be read.
///
/// Looking a tool up here rather than on `PATH` is what keeps a directory
/// a script prepended to `PATH` from supplying it instead.
#[must_use]
pub fn system_directory() -> Option<PathBuf> {
    use std::os::windows::ffi::OsStringExt as _;

    // SAFETY: a null buffer with a zero length asks only for the length the
    // answer needs, which is the documented way to size the buffer below.
    let required = unsafe { GetSystemDirectoryW(std::ptr::null_mut(), 0) };
    if required == 0 {
        return None;
    }
    let mut buffer = vec![0_u16; required as usize];
    // SAFETY: `buffer` is a live `Vec` of exactly `required` writable
    // units, which is the capacity the call is told it has.
    let written = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), required) };
    // On success the written length excludes the terminating NUL, so a
    // length that reaches `required` means the answer was truncated.
    if written == 0 || written >= required {
        return None;
    }
    buffer.truncate(written as usize);
    Some(std::ffi::OsString::from_wide(&buffer).into())
}

/// Rename `source` over `destination`, replacing it, and do not report
/// success until the change has reached the device.
///
/// [`std::fs::rename`] replaces the destination too but does not flush, so
/// a crash can leave neither name pointing at the content.
pub fn rename_over(source: &Path, destination: &Path) -> io::Result<()> {
    let source = wide(source.as_os_str())?;
    let destination = wide(destination.as_os_str())?;
    // SAFETY: both are NUL-terminated UTF-16 buffers that `wide` built,
    // alive for the whole call, which reads them and retains neither
    // pointer.
    let renamed = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if renamed == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
}

/// A path as the NUL-terminated UTF-16 the Win32 wide entry points take.
///
/// An interior NUL is rejected. Every entry point here stops at the first
/// one, so a path holding one would name a different file than the caller
/// asked for.
fn wide(path: &OsStr) -> io::Result<Vec<u16>> {
    let mut units: Vec<u16> = path.encode_wide().collect();
    if units.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a path cannot hold an interior NUL",
        ));
    }
    units.push(0);
    Ok(units)
}
