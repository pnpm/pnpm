use std::{
    ffi::{
        CStr,
        CString,
    },
    io,
    os::unix::ffi::OsStrExt as _,
    path::Path,
    ptr,
};

/// Query the presence of an extended attribute, following symbolic links.
pub fn has_extended_attribute(path: &Path, name: &CStr) -> io::Result<bool> {
    let path = CString::new(path.as_os_str().as_bytes())?;
    // SAFETY: both strings are NUL-terminated and live through the call.
    // A null buffer with size zero queries the length without writing data.
    let size = unsafe { libc::getxattr(path.as_ptr(), name.as_ptr(), ptr::null_mut(), 0, 0, 0) };
    if size >= 0 {
        return Ok(true);
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ENOATTR) { Ok(false) } else { Err(error) }
}
