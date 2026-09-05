use super::{host::Interpreter, registry::Registry};
use miette::{IntoDiagnostic, Result, bail};
use pep440_rs::Version;
use pep508_rs::{PackageName, Requirement};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(super) struct Lockfile {
    pub(super) lock_version: String,
    pub(super) created_by: String,
    pub(super) environments: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) requires_python: Option<String>,
    pub(super) packages: Vec<LockedPackage>,
    pub(super) tool: ToolMetadata,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ToolMetadata {
    pub(super) pnpm: Inputs,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(super) struct Inputs {
    requirements: Vec<String>,
    environment: pep508_rs::MarkerEnvironment,
    tags: Vec<String>,
    index: String,
}

impl Inputs {
    pub(super) fn set_requirements(&mut self, requirements: &[Requirement]) {
        self.requirements = requirements.iter().map(ToString::to_string).collect();
        self.requirements.sort();
        self.requirements.dedup();
    }

    pub(super) fn new(
        requirements: &[Requirement],
        interpreter: &Interpreter,
        index: &str,
    ) -> Self {
        let mut requirements = requirements.iter().map(ToString::to_string).collect::<Vec<_>>();
        requirements.sort();
        requirements.dedup();
        Self {
            requirements,
            environment: interpreter.environment.clone(),
            tags: interpreter.tags.clone(),
            index: index.to_string(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LockedPackage {
    pub(super) name: PackageName,
    pub(super) version: Version,
    pub(super) wheels: Vec<LockedWheel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LockedWheel {
    pub(super) name: String,
    pub(super) url: String,
    pub(super) hashes: BTreeMap<String, String>,
}

impl LockedWheel {
    pub(super) fn integrity(&self) -> Result<ssri::Integrity> {
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
    pub(super) fn new(
        registry: &Registry<'_>,
        solution: BTreeMap<PackageName, Version>,
        inputs: Inputs,
        requires_python: Option<String>,
    ) -> Result<Self> {
        let environment =
            serde_json::to_value(&registry.interpreter.environment).into_diagnostic()?;
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
                    let wheel = registry.candidates[&name][&version].clone();
                    LockedPackage { name, version, wheels: vec![wheel] }
                })
                .collect(),
            tool: ToolMetadata { pnpm: inputs },
        })
    }

    pub(super) fn seed(&self, registry: &mut Registry<'_>) -> Result<()> {
        if self.lock_version != "1.0" {
            bail!("unsupported Python lock-version: {}", self.lock_version);
        }
        for package in &self.packages {
            let [wheel] = package.wheels.as_slice() else {
                bail!("pnpm requires one target-compatible wheel per locked Python package")
            };
            wheel.integrity()?;
            if registry
                .candidates
                .insert(
                    package.name.clone(),
                    BTreeMap::from([(package.version.clone(), wheel.clone())]),
                )
                .is_some()
            {
                bail!("duplicate Python lockfile package: {}", package.name);
            }
        }
        Ok(())
    }
}
