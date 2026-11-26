use derive_more::{
    Display,
    Error,
};
use miette::Diagnostic;
use pnpz_nix_store::{
    NarHash,
    StorePath,
};
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

pub const LOCKFILE_NAME: &str = "pnpz-lock.yaml";
pub const LOCKFILE_VERSION: u32 = 1;

/// Every store path a system needs, with what an install has to verify.
pub type Closure = BTreeMap<StorePath, LockedPath>;

/// The resolved form of a manifest: which store paths each package is on each
/// system, and the full closure of those paths.
///
/// It holds everything a frozen install trusts. A binary cache is then only
/// asked where an archive is, and the archive is checked against `nar_hash`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Lockfile {
    pub lockfile_version: u32,
    /// Keyed by package name.
    pub packages: BTreeMap<String, LockedPackage>,
    /// Keyed by system.
    pub closures: BTreeMap<String, Closure>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LockedPackage {
    /// The version the manifest asked for when this entry was resolved.
    pub specifier: String,
    pub version: String,
    /// Keyed by system.
    pub systems: BTreeMap<String, LockedSystem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LockedSystem {
    /// The nixpkgs revision the build comes from.
    pub nixpkgs: String,
    /// The package's attribute path in that revision.
    pub attr: String,
    /// The outputs an install puts in the environment, keyed by output name.
    pub outputs: BTreeMap<String, StorePath>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LockedPath {
    pub nar_hash: NarHash,
    pub nar_size: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<StorePath>,
}

#[derive(Debug, Display, Error, Diagnostic)]
pub enum LockfileError {
    #[display("failed to read {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPZ_LOCKFILE_READ))]
    Read { path: PathBuf, source: io::Error },
    #[display("failed to write {}: {source}", path.display())]
    #[diagnostic(code(ERR_PNPZ_LOCKFILE_WRITE))]
    Write { path: PathBuf, source: io::Error },
    #[display("{} is not a valid lockfile: {source}", path.display())]
    #[diagnostic(code(ERR_PNPZ_LOCKFILE_INVALID))]
    Invalid { path: PathBuf, source: yaml_serde::Error },
    #[display("{} has lockfile version {found}, and this pnpz reads version {LOCKFILE_VERSION}", path.display())]
    #[diagnostic(code(ERR_PNPZ_LOCKFILE_VERSION))]
    UnsupportedVersion { path: PathBuf, found: u32 },
}

impl Lockfile {
    pub fn load(directory: &Path) -> Result<Option<Self>, LockfileError> {
        let path = directory.join(LOCKFILE_NAME);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(source) => return Err(LockfileError::Read { path, source }),
        };
        let lockfile: Self = match yaml_serde::from_str(&text) {
            Ok(lockfile) => lockfile,
            Err(source) => return Err(LockfileError::Invalid { path, source }),
        };
        if lockfile.lockfile_version != LOCKFILE_VERSION {
            let found = lockfile.lockfile_version;
            return Err(LockfileError::UnsupportedVersion { path, found });
        }
        Ok(Some(lockfile))
    }

    pub fn save(&self, directory: &Path) -> Result<(), LockfileError> {
        let path = directory.join(LOCKFILE_NAME);
        let text = yaml_serde::to_string(self).expect("a lockfile serializes to YAML");
        fs::write(&path, text).map_err(|source| LockfileError::Write { path, source })
    }

    /// The store paths `system`'s environment is built from, in package order.
    #[must_use]
    pub fn environment_paths(&self, system: &str) -> Vec<&StorePath> {
        self.packages
            .values()
            .filter_map(|package| package.systems.get(system))
            .flat_map(|locked| locked.outputs.values())
            .collect()
    }
}

#[cfg(test)]
mod tests;
