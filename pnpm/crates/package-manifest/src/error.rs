use derive_more::{Display, Error, From};
use miette::Diagnostic;
use std::{io, path::PathBuf};

#[derive(Debug, Display, Error, Diagnostic, From)]
#[non_exhaustive]
pub enum PackageManifestError {
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANIFEST_SERIALIZATION_ERROR))]
    Serialization(serde_json::Error), // TODO: remove derive(From), split this variant

    #[from(ignore)] // TODO: remove this after derive(From) has been removed
    #[display("Failed to parse {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANIFEST_SERIALIZATION_ERROR))]
    Parse {
        path: PathBuf,
        #[error(source)]
        source: serde_json::Error,
    },

    #[from(ignore)]
    #[display("Failed to parse {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_YAML_PARSE))]
    ParseYaml {
        path: PathBuf,
        #[error(source)]
        source: Box<pnpm_yaml_document_sync::Error>,
    },

    #[from(ignore)]
    #[display("Failed to update {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANIFEST_SERIALIZATION_ERROR))]
    EditYaml {
        path: PathBuf,
        #[error(source)]
        source: Box<pnpm_yaml_document_sync::Error>,
    },

    #[diagnostic(code(ERR_PNPM_PACKAGE_MANIFEST_IO_ERROR))]
    Io(std::io::Error), // TODO: remove derive(From), split this variant

    #[from(ignore)] // TODO: remove this after derive(From) has been removed
    #[display("Failed to read {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANIFEST_IO_ERROR))]
    Read {
        path: PathBuf,
        #[error(source)]
        source: io::Error,
    },

    #[display("package.json file already exists")]
    #[diagnostic(
        code(ERR_PNPM_PACKAGE_JSON_EXISTS),
        help("Your current working directory already has a package.json file.")
    )]
    AlreadyExist,

    #[from(ignore)] // TODO: remove this after derive(From) has been removed
    #[display("invalid attribute: {_0}")]
    #[diagnostic(code(ERR_PNPM_PACKAGE_MANIFEST_INVALID_ATTRIBUTE))]
    InvalidAttribute(#[error(not(source))] String),

    #[from(ignore)] // TODO: remove this after derive(From) has been removed
    #[display("No package.json was found in {_0}")]
    #[diagnostic(code(ERR_PNPM_NO_IMPORTER_MANIFEST_FOUND))]
    NoImporterManifestFound(#[error(not(source))] String),

    #[from(ignore)] // TODO: remove this after derive(From) has been removed
    #[display("Missing script: {_0:?}")]
    #[diagnostic(code(ERR_PNPM_NO_SCRIPT))]
    NoScript(#[error(not(source))] String),
}
