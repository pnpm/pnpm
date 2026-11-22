// Copyright 2018-2025 the Deno authors. MIT license.

use std::path::{
    Path,
    PathBuf,
};

/// Similar to `std::fs::canonicalize()` but strips UNC prefixes on Windows.
pub fn canonicalize_path(path: &Path) -> std::io::Result<PathBuf> {
    Ok(deno_path_util::strip_unc_prefix(path.canonicalize()?))
}
