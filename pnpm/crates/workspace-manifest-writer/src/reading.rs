use super::{Manifest, Path, UpdateWorkspaceManifestError, fs, io};

pub(super) fn read_manifest_text(
    path: &Path,
) -> Result<Option<String>, UpdateWorkspaceManifestError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(UpdateWorkspaceManifestError::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

pub(super) fn read_manifest(path: &Path) -> Result<Manifest, UpdateWorkspaceManifestError> {
    let original = read_manifest_text(path)?;
    Manifest::parse(original.as_deref())
        .map_err(|source| UpdateWorkspaceManifestError::Parse {
            path: path.to_path_buf(),
            source,
        })
}
