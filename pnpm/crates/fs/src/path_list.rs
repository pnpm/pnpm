#[cfg(unix)]
use std::os::unix::ffi::{OsStrExt, OsStringExt};
#[cfg(target_os = "wasi")]
use std::os::wasi::ffi::{OsStrExt, OsStringExt};
use std::{
    ffi::{OsStr, OsString},
    path::PathBuf,
};

/// Splits the `WebContainer`'s POSIX PATH without interpreting quotes or dropping empty entries.
pub fn split_paths<Value: AsRef<OsStr> + ?Sized>(
    unparsed: &Value,
) -> impl DoubleEndedIterator<Item = PathBuf> + '_ {
    unparsed
        .as_ref()
        .as_bytes()
        .split(|&byte| byte == b':')
        .map(|bytes| PathBuf::from(OsString::from_vec(bytes.to_vec())))
}

#[derive(Debug, derive_more::Display, derive_more::Error)]
#[display("path segment contains separator ':'")]
pub struct JoinPathsError;

/// Joins POSIX PATH entries, rejecting entries containing the separator.
pub fn join_paths<Paths, Entry>(paths: Paths) -> Result<OsString, JoinPathsError>
where
    Paths: IntoIterator<Item = Entry>,
    Entry: AsRef<OsStr>,
{
    let mut output = Vec::new();
    for (index, path) in paths.into_iter().enumerate() {
        let path = path.as_ref().as_bytes();
        if path.contains(&b':') {
            return Err(JoinPathsError);
        }
        if index != 0 {
            output.push(b':');
        }
        output.extend_from_slice(path);
    }
    Ok(OsString::from_vec(output))
}

#[cfg(test)]
mod tests;
