use super::{
    Diagnostic, Display, Error, PackageManifestError, PathBuf, ReadProjectManifestOnlyError,
    ReadWorkspaceManifestError, io,
};

#[derive(Debug, Display, Error, Diagnostic)]
pub(super) enum UpdateChangesetError {
    #[display("Failed to read project manifest: {_0}")]
    #[diagnostic(transparent)]
    ReadProject(#[error(source)] ReadProjectManifestOnlyError),

    #[display("Failed to inspect project manifest: {_0}")]
    #[diagnostic(transparent)]
    InspectProject(#[error(source)] PackageManifestError),

    #[display("Failed to read pnpm-workspace.yaml: {_0}")]
    #[diagnostic(transparent)]
    ReadWorkspace(#[error(source)] ReadWorkspaceManifestError),

    #[display("Failed to read {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_INVALID_CHANGESET_CONFIG))]
    ReadConfig {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("Failed to parse {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_INVALID_CHANGESET_CONFIG))]
    ParseConfig {
        path: PathBuf,
        #[error(source)]
        source: serde_json::Error,
    },

    #[display("Failed to inspect changeset directory at {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_UNSAFE_CHANGESET_DIR))]
    InspectChangesetDir {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display(
        "Refusing to use changeset directory at {} because it is a symlink or not a directory",
        path.display()
    )]
    #[diagnostic(code(ERR_PNPM_UNSAFE_CHANGESET_DIR))]
    UnsafeChangesetDir { path: PathBuf },

    #[display("Failed to generate a changeset ID: {source}")]
    #[diagnostic(code(ERR_PNPM_CHANGESET_ID_FAILED))]
    GenerateId {
        #[error(source)]
        source: getrandom::Error,
    },

    #[display("Failed to write {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_CHANGESET_WRITE_FAILED))]
    WriteChangeset {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },
}
