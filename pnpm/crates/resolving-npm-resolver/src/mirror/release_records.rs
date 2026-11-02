//! The on-disk cache of release records
//! (`<registry><name>/<version>+r<revision>`), which never change, so they are
//! kept with no expiry.

use std::path::{Path, PathBuf};

use super::{EncodeRegistryError, encode_pkg_name, get_registry_name};

/// Mirror directory for release records, read from registries declared
/// `serverType: pnpr`.
pub const RELEASE_RECORDS_DIR: &str = "v12/metadata-pnpr-records";

/// Where the record of `revision` of `pkg_name@version` from `registry` is
/// kept under the `meta_dir` root of `cache_dir`.
pub fn release_record_path(
    cache_dir: &Path,
    meta_dir: &str,
    registry: &str,
    pkg_name: &str,
    (version, revision): (&str, u64),
) -> Result<PathBuf, EncodeRegistryError> {
    Ok(cache_dir
        .join(meta_dir)
        .join(get_registry_name(registry)?)
        .join(encode_pkg_name(pkg_name))
        .join(format!("{version}+r{revision}.json")))
}
