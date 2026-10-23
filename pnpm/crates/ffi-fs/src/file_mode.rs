//! A file's permission bits, told apart from the file type a mode carries.

use std::{fs::Permissions, os::unix::fs::PermissionsExt as _};

/// The bits a mode holds below its file type: a permission triple for each
/// class, and the setuid, setgid and sticky bits above them.
const PERMISSION_BITS: u32 = 0o7777;

/// The permission bits of a file mode.
///
/// A `stat` mode carries the file's type in its high bits, so a directory
/// reads back as `0o40755` rather than `0o755`. Every call that takes a
/// mode here wants the low bits alone, and every way of building this type
/// drops the rest, so a mode read off one file can be handed straight to a
/// call that sets another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMode(u32);

impl FileMode {
    /// The permission bits of `mode`, dropping its file type.
    #[must_use]
    pub const fn from_mode(mode: u32) -> Self {
        Self(mode & PERMISSION_BITS)
    }

    /// The bits, as the number a foreign call takes.
    #[must_use]
    pub const fn mode(self) -> u32 {
        self.0
    }

    /// The bits as the platform's `mode_t`, a `u16` on macOS and a `u32` on
    /// Linux.
    ///
    /// Infallible because the value is at most [`PERMISSION_BITS`], which
    /// every platform's `mode_t` represents.
    pub(crate) const fn narrow(self) -> libc::mode_t {
        // Narrowing cannot lose a bit: the mask above bounds the value well
        // inside the `u16` that macOS uses.
        self.0 as libc::mode_t
    }
}

impl From<Permissions> for FileMode {
    fn from(permissions: Permissions) -> Self {
        Self::from_mode(permissions.mode())
    }
}

impl From<FileMode> for Permissions {
    fn from(mode: FileMode) -> Self {
        Permissions::from_mode(mode.mode())
    }
}

#[cfg(test)]
mod tests;
