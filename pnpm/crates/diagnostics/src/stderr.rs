//! Writes to stderr that never panic.
//!
//! [`std::eprintln!`] panics when the write fails, for example when the
//! reader of a piped stderr has exited, and the release profile turns
//! that panic into an abort (pnpm/pnpm#16904). Nothing useful can be
//! done about a failed diagnostic write, so these macros drop the error.

/// [`std::eprintln!`] that ignores a failed write instead of panicking.
#[macro_export]
macro_rules! eprintln {
    ($($arg:tt)*) => {{
        use ::std::io::Write as _;
        let _ = ::std::writeln!(::std::io::stderr(), $($arg)*);
    }};
}

/// [`std::eprint!`] that ignores a failed write instead of panicking.
#[macro_export]
macro_rules! eprint {
    ($($arg:tt)*) => {{
        use ::std::io::Write as _;
        let _ = ::std::write!(::std::io::stderr(), $($arg)*);
    }};
}
