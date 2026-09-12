use crate::packages::{Candidate, Packages};
use miette::{IntoDiagnostic, Result, bail};
use pep440_rs::Version;
use pep508_rs::{MarkerEnvironment, PackageName, Requirement};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What a resolution is for: the interpreter's marker environment and the
/// wheel tags it accepts, in the order it prefers them. Both come from the
/// interpreter that will run the environment, so a lockfile records them
/// and is only reused for the same pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub environment: MarkerEnvironment,
    pub tags: Vec<String>,
}

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

/// Everything a resolution depended on, so a lockfile can be reused only
/// for the inputs that produced it.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Inputs {
    requirements: Vec<String>,
    environment: MarkerEnvironment,
    tags: Vec<String>,
    index: String,
}

impl Inputs {
    pub fn set_requirements(&mut self, requirements: &[Requirement]) {
        self.requirements = requirements.iter().map(ToString::to_string).collect();
        self.requirements.sort();
        self.requirements.dedup();
    }

    #[must_use]
    pub fn new(requirements: &[Requirement], target: &Target, index: &str) -> Self {
        let mut requirements = requirements.iter().map(ToString::to_string).collect::<Vec<_>>();
        requirements.sort();
        requirements.dedup();
        Self {
            requirements,
            environment: target.environment.clone(),
            tags: target.tags.clone(),
            index: index.to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedPackage {
    pub name: PackageName,
    pub version: Version,
    pub wheels: Vec<LockedWheel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedWheel {
    pub name: String,
    pub url: String,
    pub hashes: BTreeMap<String, String>,
}

impl LockedWheel {
    /// The wheel's SHA-256 digest as an integrity string. A wheel with no
    /// SHA-256 is refused: it is the digest every index publishes and the
    /// only one a download is checked against.
    pub fn integrity(&self) -> Result<ssri::Integrity> {
        let digest = self
            .hashes
            .get("sha256")
            .ok_or_else(|| miette::miette!("Python wheel {} has no SHA-256 digest", self.name))?;
        if digest.len() != 64 || !digest.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("invalid Python wheel SHA-256 digest for {}", self.name);
        }
        ssri::Integrity::from_hex(digest, ssri::Algorithm::Sha256).into_diagnostic()
    }
}

impl Lockfile {
    /// The lockfile a solved project produces: one wheel per package, the
    /// marker environment it was solved for, and the inputs that chose it.
    pub fn new(
        packages: &Packages,
        target: &Target,
        solution: BTreeMap<PackageName, Version>,
        inputs: Inputs,
        requires_python: Option<String>,
    ) -> Result<Self> {
        let environment = serde_json::to_value(&target.environment).into_diagnostic()?;
        let marker = environment
            .as_object()
            .expect("marker environment serializes to an object")
            .iter()
            .map(|(key, value)| {
                let value = value.as_str().expect("marker environment values are strings");
                if value.contains(['\'', '"', '\n', '\r']) {
                    bail!(
                        "Python environment value cannot be represented as a lockfile marker: {key}",
                    );
                }
                Ok(format!("{key} == '{value}'"))
            })
            .collect::<Result<Vec<_>>>()?
            .join(" and ");
        Ok(Self {
            lock_version: "1.0".to_string(),
            created_by: "pnpm".to_string(),
            environments: vec![marker],
            requires_python,
            packages: solution
                .into_iter()
                .map(|(name, version)| {
                    let wheel = packages.candidates[&name][&version].wheel.clone();
                    LockedPackage { name, version, wheels: vec![wheel] }
                })
                .collect(),
            tool: ToolMetadata { pnpm: inputs },
        })
    }

    /// Load this lockfile's packages as the only candidates a resolution
    /// may pick, so a locked install solves to exactly what was locked.
    pub fn seed(&self, packages: &mut Packages) -> Result<()> {
        if self.lock_version != "1.0" {
            bail!("unsupported Python lock-version: {}", self.lock_version);
        }
        for package in &self.packages {
            let [wheel] = package.wheels.as_slice() else {
                bail!("pnpm requires one target-compatible wheel per locked Python package")
            };
            wheel.integrity()?;
            if packages
                .candidates
                .insert(
                    package.name.clone(),
                    BTreeMap::from([(
                        package.version.clone(),
                        Candidate { wheel: wheel.clone(), core_metadata: None },
                    )]),
                )
                .is_some()
            {
                bail!("duplicate Python lockfile package: {}", package.name);
            }
        }
        Ok(())
    }
}
