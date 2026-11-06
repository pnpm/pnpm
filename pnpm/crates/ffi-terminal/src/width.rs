//! How wide the terminal behind a standard stream is.

use crate::Stream;

/// How many columns the terminal behind `stream` has, or `None` when the
/// stream is not a terminal or reports no width.
#[must_use]
pub fn terminal_columns(stream: Stream) -> Option<usize> {
    let descriptor = match stream {
        Stream::Input => libc::STDIN_FILENO,
        Stream::Output => libc::STDOUT_FILENO,
        Stream::Error => libc::STDERR_FILENO,
    };
    let mut size = libc::winsize { ws_row: 0, ws_col: 0, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: `TIOCGWINSZ` takes one `*mut winsize` as its variadic
    // argument, and the pointer is to a fully initialized stack local of
    // that type which outlives the call. A standard descriptor number is
    // always a valid argument: on a closed or non-terminal stream the call
    // fails rather than writing.
    let queried = unsafe { libc::ioctl(descriptor, libc::TIOCGWINSZ, &raw mut size) };
    (queried == 0 && size.ws_col > 0).then(|| usize::from(size.ws_col))
}
