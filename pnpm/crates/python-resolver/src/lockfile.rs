use crate::{
    candidates::{parse_requirement, wheel_identity},
    packages::{Candidate, Packages},
};
use miette::{IntoDiagnostic, Result, bail};
use pep440_rs::{Operator, Version, VersionSpecifiers};
use pep508_rs::{MarkerEnvironment, MarkerTree, MarkerTreeKind, PackageName, Requirement};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// What a resolution is for: the interpreter's marker environment and the
/// wheel tags it accepts, in the order it prefers them. Both come from the
/// interpreter that will run the environment. A lockfile records the pair
/// it was resolved for and replays on any target that still installs it.
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

/// Everything a resolution depended on: what the project asked for, and
/// the target it was answered for. A server's answer is accepted only
/// when it was for exactly these; a lockfile on disk is replayed on
/// whatever target still installs it — see [`Lockfile::applies_to`].
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
    /// markers it was solved under, and the inputs that chose it.
    ///
    /// The `environments` marker names the interpreter version and every
    /// marker variable a requirement in the solved graph reads. Those are
    /// the parts of the target the solution can depend on, so a PEP 751
    /// installer refuses the lockfile where they differ and nowhere else.
    /// The version is the minor one, unless a locked package's
    /// `Requires-Python` tells patch releases of it apart.
    pub fn new(
        packages: &Packages,
        target: &Target,
        requirements: &[Requirement],
        solution: BTreeMap<PackageName, Version>,
        inputs: Inputs,
        requires_python: Option<String>,
    ) -> Result<Self> {
        let referenced =
            referenced_marker_keys(packages, requirements, &solution, &target.environment)?;
        let environment = serde_json::to_value(&target.environment).into_diagnostic()?;
        let marker = environment
            .as_object()
            .expect("marker environment serializes to an object")
            .iter()
            .filter(|(key, _)| referenced.contains(key.as_str()))
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

    /// Refuse to replay this lockfile for a project or on a target it
    /// does not cover: the requirements or index it was resolved for
    /// changed, the interpreter range did, or a wheel it pins is one this
    /// target cannot install.
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
        if self.tool.pnpm.requirements != inputs.requirements {
            bail!("the project's Python requirements changed");
        }
        if self.tool.pnpm.index != inputs.index {
            bail!("the Python index changed");
        }
        if self.requires_python.as_deref() != requires_python {
            bail!("the project's requires-python changed");
        }
        for package in &self.packages {
            for wheel in &package.wheels {
                wheel.check_installable(&target.tags, &package.name, &package.version)?;
            }
        }
        Ok(())
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

/// The marker variables the solved graph reads, plus the interpreter
/// version: the variables in the markers of the root requirements and of
/// every solved package's `Requires-Dist`, which are all a target
/// contributes to the solution besides the wheels it accepts.
fn referenced_marker_keys(
    packages: &Packages,
    requirements: &[Requirement],
    solution: &BTreeMap<PackageName, Version>,
    environment: &MarkerEnvironment,
) -> Result<BTreeSet<String>> {
    let mut keys = BTreeSet::from(["python_version".to_string()]);
    for requirement in requirements {
        collect_marker_keys(&requirement.marker, &mut keys);
    }
    for (name, version) in solution {
        let metadata =
            packages.metadata.get(&(name.clone(), version.clone())).ok_or_else(|| {
                miette::miette!("solved Python package {name} {version} was never read")
            })?;
        for requirement in &metadata.requires_dist {
            collect_marker_keys(&parse_requirement(requirement)?.marker, &mut keys);
        }
        if let Some(requires_python) = &metadata.requires_python {
            let specifiers: VersionSpecifiers = requires_python.parse().into_diagnostic()?;
            if !admits_every_patch_release(&specifiers, &environment.python_full_version().version)
            {
                keys.insert("python_full_version".to_string());
            }
        }
    }
    Ok(keys)
}

/// Whether `specifiers`, which `running` satisfies, admit every patch
/// release of the minor version `running` belongs to. When they do, the
/// minor version says everything they can about a target; when they do
/// not, only the full version does.
fn admits_every_patch_release(specifiers: &VersionSpecifiers, running: &Version) -> bool {
    let minor = |version: &Version| {
        let release = version.release();
        [release.first().copied().unwrap_or(0), release.get(1).copied().unwrap_or(0)]
    };
    let running_minor = minor(running);
    specifiers.iter().all(|specifier| {
        if specifier.version().release().len() > 2 {
            return false;
        }
        let at_running_minor = minor(specifier.version()) == running_minor;
        match specifier.operator() {
            Operator::GreaterThanEqual
            | Operator::TildeEqual
            | Operator::EqualStar
            | Operator::NotEqualStar
            | Operator::LessThan => true,
            Operator::GreaterThan
            | Operator::LessThanEqual
            | Operator::Equal
            | Operator::NotEqual => !at_running_minor,
            Operator::ExactEqual => false,
        }
    })
}

fn collect_marker_keys(marker: &MarkerTree, keys: &mut BTreeSet<String>) {
    let (key, children) = marker_node(marker);
    keys.extend(key);
    for child in &children {
        collect_marker_keys(child, keys);
    }
}

/// The environment variable a marker node reads, when it reads one, and
/// the nodes below it.
fn marker_node(marker: &MarkerTree) -> (Option<String>, Vec<MarkerTree>) {
    match marker.kind() {
        MarkerTreeKind::True | MarkerTreeKind::False => (None, Vec::new()),
        MarkerTreeKind::Version(node) => {
            (Some(node.key().to_string()), node.edges().map(|(_, child)| child).collect())
        }
        MarkerTreeKind::String(node) => {
            (Some(node.key().to_string()), node.children().map(|(_, child)| child).collect())
        }
        MarkerTreeKind::In(node) => {
            (Some(node.key().to_string()), node.children().map(|(_, child)| child).collect())
        }
        MarkerTreeKind::Contains(node) => {
            (Some(node.key().to_string()), node.children().map(|(_, child)| child).collect())
        }
        MarkerTreeKind::Extra(node) => (None, node.children().map(|(_, child)| child).collect()),
    }
}
