use std::io;

/// Whether `err` means the path does not exist, the way Node.js reports
/// `ENOENT`.
///
/// Windows rejects a name that cannot exist, such as one holding `:` or
/// `#`, with `ERROR_INVALID_NAME` (`os error 123`) rather than
/// `ERROR_FILE_NOT_FOUND`. Node.js maps both to `ENOENT`, so pnpm treats
/// them alike.
#[must_use]
pub fn is_not_found(err: &io::Error) -> bool {
    const ERROR_INVALID_NAME: i32 = 123;
    err.kind() == io::ErrorKind::NotFound
        || (cfg!(windows) && err.raw_os_error() == Some(ERROR_INVALID_NAME))
}

#[cfg(test)]
mod tests;
