//! The share a mapped Windows network drive stands for, and the comparison
//! Windows makes between two path names.

use std::{ffi::OsString, os::windows::ffi::OsStringExt as _, path::PathBuf};
use windows_sys::Win32::{
    Foundation::ERROR_MORE_DATA,
    Globalization::{CSTR_EQUAL, CompareStringOrdinal},
    NetworkManagement::WNet::WNetGetConnectionW,
};

/// The UNC path the network drive `letter` is mapped to, or `None` when
/// `letter` names no mapped drive.
#[must_use]
pub fn mapped_drive_target(letter: char) -> Option<PathBuf> {
    let drive = [u16::try_from(u32::from(letter)).ok()?, u16::from(b':'), 0];
    let mut remote = vec![0u16; 512];
    let (mut result, wanted) = connection(&drive, &mut remote);
    if result == ERROR_MORE_DATA
        && let Ok(wanted) = usize::try_from(wanted)
        && wanted > remote.len()
    {
        remote.resize(wanted, 0);
        result = connection(&drive, &mut remote).0;
    }
    if result != 0 {
        return None;
    }
    let end = remote
        .iter()
        .position(|unit| *unit == 0)?;
    Some(PathBuf::from(OsString::from_wide(&remote[..end])))
}

/// Ask what `drive` is mapped to, writing the NUL-terminated answer into
/// `remote`. Reports the call's own status code and the buffer size, in
/// code units, that it wants.
fn connection(drive: &[u16], remote: &mut [u16]) -> (u32, u32) {
    let Ok(mut length) = u32::try_from(remote.len()) else { return (ERROR_MORE_DATA, 0) };
    // SAFETY: `drive` is a NUL-terminated local, `remote` is writable for
    // `length` code units because `length` is its own length, and `length`
    // is a stack local that outlives the call. The call retains no pointer.
    let result =
        unsafe { WNetGetConnectionW(drive.as_ptr(), remote.as_mut_ptr(), &raw mut length) };
    (result, length)
}

/// Whether the UTF-16 names `left` and `right` are one name, compared the
/// way Windows compares a path: by code unit, ignoring case.
///
/// The case folding is the system's own, so a name outside ASCII compares
/// as the filesystem compares it rather than as a Rust comparison would.
#[must_use]
pub fn wide_names_equal_ignoring_case(left: &[u16], right: &[u16]) -> bool {
    let (Ok(left_length), Ok(right_length)) =
        (i32::try_from(left.len()), i32::try_from(right.len()))
    else {
        return false;
    };
    // SAFETY: both pointers address initialized UTF-16 slices for the
    // lengths passed, which are the slices' own lengths. The call reads
    // through them, writes nothing, and retains neither.
    let compared = unsafe {
        CompareStringOrdinal(left.as_ptr(), left_length, right.as_ptr(), right_length, 1)
    };
    compared == CSTR_EQUAL
}
