use std::{io, path::Path};

/// Swap the dirents at `first` and `second` in one step, so that neither
/// path is ever absent and each names one of the two trees throughout.
///
/// Linux (`renameat2` with `RENAME_EXCHANGE`) and macOS (`renamex_np` with
/// `RENAME_SWAP`) provide this on their main filesystems. Elsewhere, and on
/// a filesystem without the operation, the error has kind
/// [`io::ErrorKind::Unsupported`], and the caller swaps with two renames.
pub fn exchange_dirents(first: &Path, second: &Path) -> io::Result<()> {
    pnpm_ffi_fs::exchange_dirents(first, second)
}

#[cfg(test)]
mod tests;
