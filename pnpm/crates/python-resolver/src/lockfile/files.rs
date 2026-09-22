//! The files a lockfile pins for a release: the wheels an environment
//! installs, and the source distribution it builds where none of them
//! installs here.

use crate::candidates::{
    WheelFilename,
    source_version,
    wheel_identity,
};
use miette::{
    IntoDiagnostic,
    Result,
    bail,
};
use pep440_rs::Version;
use pep508_rs::PackageName;
use serde::{
    Deserialize,
    Serialize,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedWheel {
    pub name: String,
    pub url: String,
    pub hashes: BTreeMap<String, String>,
}

impl LockedWheel {
    /// Read the wheel's filename, refusing a file that is not a wheel of
    /// `name==version`: what a lockfile pins under a package has to be
    /// that package.
    pub(super) fn filename(&self, name: &PackageName, version: &Version) -> Result<WheelFilename> {
        let filename = WheelFilename::parse(&self.name)?
            .ok_or_else(|| {
                miette::miette!("Python lockfile pins a file that is not a wheel: {}", self.name)
            })?;
        if filename.name != *name || filename.version != *version {
            bail!("Python lockfile wheel identity mismatch: {}", self.name);
        }
        Ok(filename)
    }

    /// Refuse a wheel that is not `name==version` in a build `tags`
    /// accept: what a lockfile pins under a package has to be that
    /// package, and installable where it is being replayed.
    pub fn check_installable(
        &self,
        tags: &[String],
        name: &PackageName,
        version: &Version,
    ) -> Result<()> {
        let Some((wheel_name, wheel_version, _)) = wheel_identity(&self.name, tags)? else {
            bail!("Python wheel is incompatible with this interpreter: {}", self.name)
        };
        if wheel_name != *name || wheel_version != *version {
            bail!("Python lockfile wheel identity mismatch: {}", self.name);
        }
        Ok(())
    }

    /// The wheel's SHA-256 digest as an integrity string.
    pub fn integrity(&self) -> Result<ssri::Integrity> {
        published_integrity(&self.hashes, &self.name)
    }
}

/// The source distribution a release is built from, as PEP 751 records
/// it. What it requires is whatever the wheel built from it declares, so
/// a lockfile pins the archive and its digest and nothing more.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedSdist {
    pub name: String,
    pub url: String,
    pub hashes: BTreeMap<String, String>,
}

impl LockedSdist {
    /// The archive's SHA-256 digest as an integrity string.
    pub fn integrity(&self) -> Result<ssri::Integrity> {
        published_integrity(&self.hashes, &self.name)
    }

    /// Refuse an archive that is not a source distribution of
    /// `name==version` served over HTTP(S): what a lockfile pins under a
    /// package has to be that package, fetched the way every other
    /// Python artifact is, and a build reads its identity from the
    /// archive's own name before running anything in it.
    pub fn check_published(&self, name: &PackageName, version: &Version) -> Result<()> {
        crate::validate_url(&self.url.parse().into_diagnostic()?)?;
        self.integrity()?;
        let carried = source_version(&self.name, name)?
            .ok_or_else(|| {
                miette::miette!(
                    "Python lockfile pins a file that is not a source distribution of {name}: {}",
                    self.name,
                )
            })?;
        if carried != *version {
            bail!("Python lockfile source distribution identity mismatch: {}", self.name);
        }
        Ok(())
    }

    /// The container the archive is published in, which is what
    /// unpacking it reads.
    #[must_use]
    pub fn is_zip(&self) -> bool {
        self.name.ends_with(".zip")
    }

    /// The directory a PEP 517 source distribution unpacks into, which
    /// is the only entry its archive may hold at the top level.
    #[must_use]
    pub fn root(&self) -> &str {
        self.name
            .strip_suffix(".zip")
            .or_else(|| self.name.strip_suffix(".tar.gz"))
            .unwrap_or(&self.name)
    }
}

/// The SHA-256 digest a published file is checked against. A file with no
/// SHA-256 is refused: it is the digest every index publishes and the only
/// one a download is checked against.
fn published_integrity(
    hashes: &BTreeMap<String, String>,
    filename: &str,
) -> Result<ssri::Integrity> {
    let digest = hashes
        .get("sha256")
        .ok_or_else(|| miette::miette!("Python file {filename} has no SHA-256 digest"))?;
    if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid Python SHA-256 digest for {filename}");
    }
    ssri::Integrity::from_hex(digest, ssri::Algorithm::Sha256).into_diagnostic()
}
