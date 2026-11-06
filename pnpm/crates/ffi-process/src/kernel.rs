//! The running kernel's release, for the version gates that decide whether
//! a syscall flag exists.

/// The major component of the Darwin kernel release, which is what dates a
/// macOS version: Darwin 20 is macOS 11. `None` when `uname` fails or the
/// release does not start with a number.
///
/// A flag gate needs this because an older XNU ignores an open flag it does
/// not know instead of reporting `EINVAL`, so a caller cannot learn from
/// the failure that the flag went unapplied.
#[cfg(target_os = "macos")]
#[must_use]
pub fn darwin_major_version() -> Option<u32> {
    release()?
        .split('.')
        .next()?
        .parse()
        .ok()
}

#[cfg(target_os = "macos")]
fn release() -> Option<String> {
    let mut info = std::mem::MaybeUninit::<libc::utsname>::uninit();
    // SAFETY: the pointer is to a stack local of exactly the type `uname`
    // writes, and it outlives the call. `uname` writes the whole structure
    // or none of it, and the `!= 0` below returns before anything reads an
    // uninitialized field.
    if unsafe { libc::uname(info.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: `uname` returned success, so it initialized every field.
    let info = unsafe { info.assume_init() };
    // SAFETY: a successful `uname` leaves `release` holding a
    // NUL-terminated string inside the array it is borrowed from, which
    // outlives the borrow.
    let release = unsafe { std::ffi::CStr::from_ptr(info.release.as_ptr()) };
    release.to_str().ok().map(str::to_owned)
}

#[cfg(test)]
mod tests;
