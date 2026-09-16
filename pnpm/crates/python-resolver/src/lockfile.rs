pub use environments::{Metadata, Solved, Target, environment_marker};
pub use inputs::Inputs;

mod environments;
mod inputs;

use crate::{
    candidates::{WheelFilename, wheel_identity},
    packages::{Candidate, IndexCandidate, Packages},
};
use environments::{check_environment_decides, referenced_marker_keys};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pep440_rs::Version;
use pep508_rs::{MarkerEnvironment, MarkerTree, PackageName, Requirement};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Lockfile {
    pub lock_version: String,
    pub created_by: String,
    pub environments: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires_python: Option<String>,
    pub packages: Vec<LockedPackage>,
    pub tool: ToolMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolMetadata {
    pub pnpm: Inputs,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedPackage {
    pub name: PackageName,
    pub version: Version,
    /// Which of the lockfile's environments install the package, left out
    /// when all of them do.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wheels: Vec<LockedWheel>,
    /// The project in this repository the package is built from, for one
    /// that is not downloaded. PEP 751 calls this a directory package.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<LockedDirectory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vcs: Option<LockedVcs>,
}

/// A project built from a directory in this workspace, as PEP 751 records
/// it: a path relative to the lockfile, and whether it is installed so
/// that edits to its source take effect without reinstalling it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedDirectory {
    pub path: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub editable: bool,
}

/// A PEP 751 git source pinned to an immutable commit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct LockedVcs {
    #[serde(rename = "type")]
    pub kind: String,
    pub url: String,
    pub requested_revision: String,
    pub commit_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdirectory: Option<String>,
}

impl LockedVcs {
    pub fn validate(&self) -> Result<()> {
        if self.kind != "git"
            || self.commit_id.len() != 40
            || !self.commit_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            bail!("Python git lockfile source requires a full commit hash");
        }
        let crate::Source::Git(source) =
            crate::Source::parse(&format!("git+{}@{}", self.url, self.commit_id))?
        else {
            bail!("Python git lockfile source requires a git URL");
        };
        if source.url != self.url {
            bail!("invalid Python git lockfile URL");
        }
        crate::source::validate_subdirectory(self.subdirectory.as_deref())?;
        Ok(())
    }
}

impl LockedPackage {
    /// Whether an environment installs this package at all.
    fn selected_by(&self, environment: &MarkerEnvironment) -> Result<bool> {
        let Some(marker) = &self.marker else { return Ok(true) };
        let marker: MarkerTree = marker
            .parse()
            .into_diagnostic()
            .wrap_err_with(|| format!("read the marker of Python package {}", self.name))?;
        Ok(marker.evaluate(environment, &[]))
    }

    /// The wheel a target installs, of the ones the lockfile pins for the
    /// package. Every pinned wheel has to be a file of this package; the
    /// target then takes whichever of them it prefers.
    fn installable_wheel(&self, target: &Target) -> Result<&LockedWheel> {
        let mut installable: Option<(usize, &LockedWheel)> = None;
        for wheel in &self.wheels {
            wheel.integrity()?;
            let filename = wheel.filename(&self.name, &self.version)?;
            let Some(rank) = filename.rank(&target.tags) else { continue };
            if installable.is_none_or(|(preferred, _)| rank < preferred) {
                installable = Some((rank, wheel));
            }
        }
        installable
            .map(|(_, wheel)| wheel)
            .ok_or_else(|| {
                miette::miette!(
                    "the Python lockfile pins no wheel of {}=={} this interpreter can install",
                    self.name,
                    self.version,
                )
            })
    }
}

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
    fn filename(&self, name: &PackageName, version: &Version) -> Result<WheelFilename> {
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

    /// The wheel's SHA-256 digest as an integrity string. A wheel with no
    /// SHA-256 is refused: it is the digest every index publishes and the
    /// only one a download is checked against.
    pub fn integrity(&self) -> Result<ssri::Integrity> {
        let digest = self.hashes
            .get("sha256")
            .ok_or_else(|| miette::miette!("Python wheel {} has no SHA-256 digest", self.name))?;
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("invalid Python wheel SHA-256 digest for {}", self.name);
        }
        ssri::Integrity::from_hex(digest, ssri::Algorithm::Sha256).into_diagnostic()
    }
}

impl Lockfile {
    /// The lockfile one solved environment produces.
    pub fn new(
        packages: &Packages,
        target: &Target,
        requirements: &[Requirement],
        solution: BTreeMap<PackageName, Version>,
        inputs: Inputs,
        requires_python: Option<String>,
    ) -> Result<Self> {
        let solved = Solved::new(target.clone(), solution, packages, Vec::new())?;
        Self::merged(&packages.metadata, requirements, &[solved], inputs, requires_python)
    }

    /// The lockfile the solved environments produce together: every
    /// package any of them installs, each pinning the wheels those
    /// environments take and carrying the marker that says which of them
    /// install it.
    ///
    /// An environment's marker names the interpreter version, the marker
    /// variables the solved graph reads, and whatever a declared
    /// environment pins. Those are the parts of a target a solution can
    /// depend on, so a PEP 751 installer refuses the lockfile where they
    /// differ and nowhere else. For the running interpreter the version
    /// is the minor one, unless a locked package's `Requires-Python`
    /// tells patch releases apart.
    pub fn merged(
        metadata: &Metadata,
        requirements: &[Requirement],
        solved: &[Solved],
        inputs: Inputs,
        requires_python: Option<String>,
    ) -> Result<Self> {
        let referenced = referenced_marker_keys(metadata, requirements, solved)?;
        let environments = solved
            .iter()
            .map(|environment| environment.marker(&referenced, metadata))
            .collect::<Result<Vec<_>>>()?;
        let markers = environments
            .iter()
            .map(|marker| marker.parse::<MarkerTree>().into_diagnostic())
            .collect::<Result<Vec<_>>>()?;
        for ((environment, marker), written) in solved
            .iter()
            .zip(&markers)
            .zip(&environments)
        {
            if !environment.declared.is_empty() {
                check_environment_decides(environment, marker, written, metadata, requirements)?;
            }
        }
        Ok(Self {
            lock_version: "1.0".to_string(),
            created_by: "pnpm".to_string(),
            packages: merge_packages(solved, &markers)?,
            environments,
            requires_python,
            tool: ToolMetadata { pnpm: inputs },
        })
    }

    /// Refuse to replay this lockfile for a project or on a target it
    /// does not cover: the requirements, index or declared environments
    /// it was resolved for changed, the interpreter range did, or it pins
    /// no wheel this target can install for a package this target needs.
    ///
    /// The markers it was resolved under are not compared. A wheel that
    /// installs here is the same wheel wherever it was chosen, and
    /// whether the locked graph is still the one the markers select is
    /// settled by re-solving it against them ([`crate::validate_locked`]),
    /// which is exact where an equality check on the whole environment
    /// would refuse every kernel update.
    pub fn applies_to(
        &self,
        inputs: &Inputs,
        requires_python: Option<&str>,
        target: &Target,
    ) -> Result<()> {
        if let Some(reason) = self.tool.pnpm.differs_from(inputs) {
            bail!("{reason}");
        }
        if self.requires_python.as_deref() != requires_python {
            bail!("the project's requires-python changed");
        }
        for package in self.selected_packages(&target.environment)? {
            if let Some(vcs) = &package.vcs {
                vcs.validate()?;
            }
            if package.directory.is_none() && package.vcs.is_none() {
                package.installable_wheel(target)?;
            }
        }
        Ok(())
    }

    /// Load this lockfile's packages as the only candidates a resolution
    /// may pick, so a locked install solves to exactly what was locked.
    /// A lockfile covering several environments is narrowed to the one
    /// being installed: a package another environment installs is not a
    /// candidate here, and the wheel taken for each package is the one
    /// this target prefers.
    pub fn seed(&self, packages: &mut Packages, target: &Target) -> Result<()> {
        if self.lock_version != "1.0" {
            bail!("unsupported Python lock-version: {}", self.lock_version);
        }
        for package in self.selected_packages(&target.environment)? {
            let candidate = match (&package.vcs, &package.directory) {
                (Some(vcs), None) if package.wheels.is_empty() => {
                    vcs.validate()?;
                    Candidate::Vcs(vcs.clone())
                }
                (None, Some(directory)) if package.wheels.is_empty() => {
                    Candidate::Directory(directory.clone())
                }
                (None, None) => Candidate::Wheel(IndexCandidate {
                    wheel: package.installable_wheel(target)?.clone(),
                    core_metadata: None,
                }),
                _ => bail!("Python lockfile pins multiple sources for {}", package.name),
            };
            if packages.candidates
                .insert(
                    package.name.clone(),
                    BTreeMap::from([(package.version.clone(), candidate)]),
                )
                .is_some()
            {
                bail!("duplicate Python lockfile package: {}", package.name);
            }
        }
        Ok(())
    }

    fn selected_packages(&self, environment: &MarkerEnvironment) -> Result<Vec<&LockedPackage>> {
        self.packages
            .iter()
            .filter_map(|package| {
                package
                    .selected_by(environment)
                    .map(|selected| selected.then_some(package))
                    .transpose()
            })
            .collect()
    }
}

/// Every package the solved environments install, with the wheels they
/// take and the marker saying which of them install it. A package all of
/// them install carries no marker: the lockfile's `environments` already
/// says where it is installed.
fn merge_packages(solved: &[Solved], markers: &[MarkerTree]) -> Result<Vec<LockedPackage>> {
    let mut merged = BTreeMap::<(PackageName, Version, String), Merged>::new();
    let mut scope = MarkerTree::FALSE;
    for (environment, marker) in solved.iter().zip(markers) {
        scope.or(marker.clone());
        for (name, version) in &environment.solution {
            let entry = merged
                .entry((name.clone(), version.clone(), environment.source_key(name)?))
                .or_insert_with(Merged::nowhere);
            entry.marker.or(marker.clone());
            // A project in the repository is the same directory on every
            // environment, where a release is the wheel each one takes.
            if let Some(vcs) = environment.vcs.get(name) {
                entry.vcs = Some(vcs.clone());
                continue;
            }
            if let Some(directory) = environment.directories.get(name) {
                entry.directory = Some(directory.clone());
                continue;
            }
            let wheel = environment.wheels
                .get(name)
                .ok_or_else(|| miette::miette!("solved Python package {name} pins no wheel"))?;
            entry.wheels.insert(wheel.name.clone(), wheel.clone());
        }
    }
    let packages = merged
        .into_iter()
        .map(|((name, version, _), entry)| LockedPackage {
            name,
            version,
            marker: (entry.marker != scope).then(|| entry.marker.try_to_string()).flatten(),
            wheels: entry.wheels.into_values().collect(),
            directory: entry.directory,
            vcs: entry.vcs,
        })
        .collect::<Vec<_>>();
    check_one_version_per_environment(&packages)?;
    Ok(packages)
}

/// What the environments merged so far say about one package: where it
/// is installed, and what installs it there.
struct Merged {
    marker: MarkerTree,
    wheels: BTreeMap<String, LockedWheel>,
    directory: Option<LockedDirectory>,
    vcs: Option<LockedVcs>,
}

impl Merged {
    /// A package no environment installs yet. The marker starts false
    /// because each environment adds itself to it.
    fn nowhere() -> Self {
        Self { marker: MarkerTree::FALSE, wheels: BTreeMap::new(), directory: None, vcs: None }
    }
}

/// Refuse a lockfile that offers an environment two versions of the same
/// distribution. A PEP 751 installer picks packages by name and marker,
/// so two entries an environment both selects leave it no answer.
fn check_one_version_per_environment(packages: &[LockedPackage]) -> Result<()> {
    for (position, package) in packages.iter().enumerate() {
        for other in &packages[position + 1..] {
            if other.name != package.name {
                continue;
            }
            if !marker_tree(package)?.is_disjoint(&marker_tree(other)?) {
                bail!(
                    "the environments this project locks for need {} at both {} and {}, \
                     and nothing in their markers tells them apart",
                    package.name,
                    package.version,
                    other.version,
                );
            }
        }
    }
    Ok(())
}

fn marker_tree(package: &LockedPackage) -> Result<MarkerTree> {
    package.marker
        .as_deref()
        .map_or(Ok(MarkerTree::TRUE), |marker| marker.parse().into_diagnostic())
}
