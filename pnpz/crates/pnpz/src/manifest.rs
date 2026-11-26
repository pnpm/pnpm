use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use serde::{
    Deserialize,
    Serialize,
};
use std::{
    collections::BTreeMap,
    fs,
    io,
    path::{
        Path,
        PathBuf,
    },
};

pub const MANIFEST_FILE_NAME: &str = "pnpz.yaml";

/// The version a package gets when its entry names none.
pub const LATEST: &str = "latest";

/// A project's declaration of the system packages it needs.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// `<name>` or `<name>@<version>` entries.
    ///
    /// The entries are strings, not a map from name to version, because YAML
    /// reads an unquoted `3.10` as the number `3.1`.
    #[serde(default)]
    packages: Vec<String>,
    /// The platforms to lock, in nixpkgs' naming. Empty means the host's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub systems: Vec<String>,
}

#[derive(Debug, Display, Error, Diagnostic)]
pub enum ManifestError {
    #[display("failed to read {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPZ_MANIFEST_READ))]
    Read { path: PathBuf, source: io::Error },
    #[display("failed to write {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPZ_MANIFEST_WRITE))]
    Write { path: PathBuf, source: io::Error },
    #[display("{} is not a valid manifest: {source}", path.display())]
    #[diagnostic(code(ERR_PNPZ_MANIFEST_INVALID))]
    Invalid { path: PathBuf, source: yaml_serde::Error },
    #[display("{spec:?} is not a package in the form <name> or <name>@<version>")]
    #[diagnostic(code(ERR_PNPZ_INVALID_PACKAGE_SPEC))]
    InvalidSpec { spec: String },
}

impl Manifest {
    /// Reads the manifest in `directory`. A missing file is an empty manifest.
    pub fn load(directory: &Path) -> Result<Self, ManifestError> {
        let path = directory.join(MANIFEST_FILE_NAME);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(source) => return Err(ManifestError::Read { path, source }),
        };
        let manifest: Self =
            yaml_serde::from_str(&text).map_err(|source| ManifestError::Invalid { path, source })?;
        for spec in &manifest.packages {
            parse_spec(spec)?;
        }
        Ok(manifest)
    }

    pub fn save(&self, directory: &Path) -> Result<(), ManifestError> {
        let path = directory.join(MANIFEST_FILE_NAME);
        let text = yaml_serde::to_string(self).expect("a manifest serializes to YAML");
        fs::write(&path, text).map_err(|source| ManifestError::Write { path, source })
    }

    /// The declared packages, as a map from name to version specifier.
    #[must_use]
    pub fn packages(&self) -> BTreeMap<String, String> {
        self.packages
            .iter()
            .map(|spec| parse_spec(spec))
            .map(|spec| spec.expect("specs are validated when they enter the manifest"))
            .collect()
    }

    /// Adds `spec`, replacing an entry for the same package.
    pub fn add(&mut self, spec: &str) -> Result<(), ManifestError> {
        let (name, _) = parse_spec(spec)?;
        self.packages.retain(|existing| parse_spec(existing).is_ok_and(|(other, _)| other != name));
        self.packages.push(spec.to_string());
        self.packages.sort();
        Ok(())
    }
}

fn parse_spec(spec: &str) -> Result<(String, String), ManifestError> {
    let (name, version) = spec
        .split_once('@')
        .unwrap_or((spec, LATEST));
    let is_valid = !name.is_empty()
        && !version.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.+".contains(&byte));
    if !is_valid {
        return Err(ManifestError::InvalidSpec { spec: spec.to_string() });
    }
    Ok((name.to_string(), version.to_string()))
}

#[cfg(test)]
mod tests;
