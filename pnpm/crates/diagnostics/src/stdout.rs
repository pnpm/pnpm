//! Writes to stdout that never panic.
//!
//! [`std::println!`] panics when the write fails, and the release profile
//! turns that panic into an abort (pnpm/tasks#132). A reader that closed
//! the pipe, as `head` does, has all the output it wants, so the write is
//! dropped and the command ends with its own exit status. A command that
//! would otherwise run forever stops on [`reader_closed`]. Any other failed
//! write is kept for [`take_write_error`].

use std::{
    io,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};

static READER_CLOSED: AtomicBool = AtomicBool::new(false);
static WRITE_ERROR: Mutex<Option<io::Error>> = Mutex::new(None);

/// [`std::println!`] that hands a failed write to [`check`] instead of
/// panicking.
#[macro_export]
macro_rules! println {
    ($($arg:tt)*) => {{
        use ::std::io::Write as _;
        $crate::stdout::check(::std::writeln!(::std::io::stdout(), $($arg)*));
    }};
}

/// [`std::print!`] that hands a failed write to [`check`] instead of
/// panicking.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        use ::std::io::Write as _;
        $crate::stdout::check(::std::write!(::std::io::stdout(), $($arg)*));
    }};
}

/// Drop the error of a write to stdout whose reader closed the pipe, and
/// record it for [`reader_closed`]. Keep the first other error for
/// [`take_write_error`].
pub fn check<Value>(result: io::Result<Value>) {
    let Err(error) = result else { return };
    if error.kind() == io::ErrorKind::BrokenPipe {
        READER_CLOSED.store(true, Ordering::Relaxed);
        return;
    }
    WRITE_ERROR
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert(error);
}

/// Whether a write to stdout found that its reader closed the pipe.
pub fn reader_closed() -> bool {
    READER_CLOSED.load(Ordering::Relaxed)
}

/// The first failed write to stdout other than a closed pipe, if any: the
/// output the command produced is incomplete.
pub fn take_write_error() -> Option<io::Error> {
    WRITE_ERROR
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
}
