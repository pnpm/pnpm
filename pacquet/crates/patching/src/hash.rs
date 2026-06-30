use derive_more::{Display, Error};
use miette::Diagnostic;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

/// Error reading a patch file from disk during hashing.
#[derive(Debug, Display, Error, Diagnostic)]
pub enum CalcPatchHashError {
    #[display("Failed to read patch file {}: {source}", path.display())]
    ReadFile {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
}

/// SHA-256 hex digest of one patch file, with CRLF normalized to LF.
///
/// Ports upstream's
/// [`createHexHashFromFile`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/crypto/hash/src/index.ts#L31-L33)
/// composed with
/// [`readNormalizedFile`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/crypto/hash/src/index.ts#L36-L39).
pub fn create_hex_hash_from_file(path: &Path) -> Result<String, CalcPatchHashError> {
    let bytes = fs::read(path)
        .map_err(|source| CalcPatchHashError::ReadFile { path: path.to_path_buf(), source })?;
    let text = String::from_utf8_lossy(&bytes);
    let normalized = text.replace("\r\n", "\n");
    let mut hasher = Sha256::new();
    hasher.update(normalized.as_bytes());
    let digest = hasher.finalize();
    Ok(format!("{digest:x}"))
}

/// SHA-256 hex digests for every patch file in `patches`.
///
/// Ports upstream's
/// [`calcPatchHashes`](https://github.com/pnpm/pnpm/blob/b4f8f47ac2/lockfile/settings-checker/src/calcPatchHashes.ts).
/// The map keys are passed through verbatim — those are the
/// `patchedDependencies` keys (e.g. `lodash@4.17.21`); the values
/// are absolute patch file paths and are replaced with their
/// per-file hex digests.
pub fn calc_patch_hashes<Iter>(
    patches: Iter,
) -> Result<BTreeMap<String, String>, CalcPatchHashError>
where
    Iter: IntoIterator<Item = (String, PathBuf)>,
{
    let mut result = BTreeMap::new();
    for (key, path) in patches {
        let hash = create_hex_hash_from_file(&path)?;
        result.insert(key, hash);
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
