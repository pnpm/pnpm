//! The calling process's file-creation mask.

#[cfg(target_os = "linux")]
use pipe_trait::Pipe;

/// The bits a mask can take away: a permission triple for each class.
/// `umask(2)` uses only these bits of its argument, so a mask drops the
/// rest rather than carrying a bit that would never be applied.
const MASKABLE_BITS: u32 = 0o777;

/// The permissions a file creation takes away.
///
/// Its own type rather than a file mode, because the bits mean the opposite
/// of each other: a mode says which permissions a file has, a mask which
/// ones its creation removes. Handing one to the other's caller inverts the
/// result, which no test of a single mode would notice, so the two do not
/// convert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Umask(u32);

impl Umask {
    /// The mask `mask` names, dropping the bits a mask cannot take away.
    #[must_use]
    pub const fn from_mask(mask: u32) -> Self {
        Self(mask & MASKABLE_BITS)
    }

    /// The bits, as the number a caller masks a mode with.
    #[must_use]
    pub const fn mask(self) -> u32 {
        self.0
    }

    /// The bits as the platform's `mode_t`, for a caller that makes the
    /// `umask` call itself.
    ///
    /// Infallible because every way of building this type drops the bits
    /// above [`MASKABLE_BITS`], which every platform's `mode_t` represents.
    #[cfg(unix)]
    #[must_use]
    pub const fn narrow(self) -> libc::mode_t {
        self.0 as libc::mode_t
    }
}

/// The process's file-creation mask.
///
/// POSIX offers no way to read the mask without writing it: `umask(2)`
/// returns the previous value, so a reader has to set one and put the old
/// one back. The window between the two calls is process-wide, and a file
/// another thread creates inside it is created unmasked. Linux publishes
/// the mask, which is read first so that the window never opens there.
#[cfg(unix)]
#[must_use]
pub fn current_umask() -> Umask {
    #[cfg(target_os = "linux")]
    if let Some(mask) = published_umask() {
        return mask;
    }
    read_umask_by_writing_it()
}

/// The mask as Linux publishes it, in the `Umask:` line of
/// `/proc/thread-self/status`.
///
/// `thread-self` rather than `self` so that the line read is the calling
/// thread's, which is the mask [`read_umask_by_writing_it`] would have
/// reported. The two name the same mask unless a thread has unshared its
/// filesystem information, which pnpm never does.
///
/// `None` where `procfs` is not mounted, on a kernel before 3.17, which
/// has no `thread-self`, or before 4.7, which does not print the line.
#[cfg(target_os = "linux")]
fn published_umask() -> Option<Umask> {
    "/proc/thread-self/status"
        .pipe(std::fs::read_to_string)
        .ok()?
        .pipe_as_ref(parse_published_umask)
}

/// Separate from [`published_umask`] so a test can drive it without a
/// `procfs` whose contents it cannot choose.
#[cfg(target_os = "linux")]
fn parse_published_umask(status: &str) -> Option<Umask> {
    let field = status
        .lines()
        .find_map(|line| line.strip_prefix("Umask:"))?;
    u32::from_str_radix(field.trim(), 8).ok().map(Umask::from_mask)
}

/// Set the mask to `0` to learn what it was, then put it back.
///
/// The fallback for a target that does not publish the mask, and the whole
/// of the implementation on one that has no `procfs`. Both calls are made
/// with nothing between them, so the unmasked window is as short as two
/// syscalls.
#[cfg(unix)]
fn read_umask_by_writing_it() -> Umask {
    // SAFETY: `umask` takes its argument by value, writes no memory, and
    // cannot fail. The pair leaves the process's mask as it found it: the
    // first call reports the mask it replaced, and the second restores
    // exactly that value.
    let mask = unsafe {
        let previous = libc::umask(0);
        libc::umask(previous);
        previous
    };
    Umask::from_mask(widen_mode(mask))
}

/// `libc::umask` speaks `mode_t`, a `u16` on macOS and a `u32` on Linux.
/// `Into` is the one widening both platforms' clippy accepts: a cast is
/// `cast_lossless` on macOS and `u32::from` is `useless_conversion` on
/// Linux.
#[cfg(unix)]
fn widen_mode(mode: impl Into<u32>) -> u32 {
    mode.into()
}

#[cfg(all(test, unix))]
mod tests;
