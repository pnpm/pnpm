use crate::{
    candidates::{WheelFilename, parse_requirement, wheel_identity},
    metadata::WheelMetadata,
    packages::{Candidate, Packages},
    requires_python::declared_range,
};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pep440_rs::{Operator, Version, VersionSpecifiers};
use pep508_rs::{
    MarkerEnvironment, MarkerExpression, MarkerTree, MarkerTreeKind, MarkerValueVersion,
    PackageName, Requirement,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// What one resolution pass is for: the interpreter's marker environment
/// and the wheel tags it accepts, in the order it prefers them. Both come
/// from the interpreter that will run the environment, or from the
/// interpreter's report on an environment the project declares.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Target {
    pub environment: MarkerEnvironment,
    pub tags: Vec<String>,
}

/// The metadata of every wheel a resolution read, which is what the
/// lockfile writer needs to know which marker variables the solved graph
/// depends on.
pub type Metadata = BTreeMap<(PackageName, Version), WheelMetadata>;

/// One environment's answer: the versions it solved the project to, the
/// wheel it installs for each of them, and the marker keys that name the
/// environment when the project declared it rather than taking the
/// running interpreter.
#[derive(Debug)]
pub struct Solved {
    pub target: Target,
    pub solution: BTreeMap<PackageName, Version>,
    pub wheels: BTreeMap<PackageName, LockedWheel>,
    /// The marker variables the environment pins, empty for the running
    /// interpreter. A declared environment names a platform and a Python
    /// version, and its lockfile marker has to say so even when nothing
    /// in the solved graph reads them: it is what tells the environment
    /// apart from the others the same lockfile covers.
    pub declared: Vec<String>,
}

impl Solved {
    /// The answer one resolution pass produced, with the wheel each
    /// solved version installs on its own target.
    pub fn new(
        target: Target,
        solution: BTreeMap<PackageName, Version>,
        packages: &Packages,
        declared: Vec<String>,
    ) -> Result<Self> {
        let wheels = solution
            .iter()
            .map(|(name, version)| {
                let candidate = packages.candidates
                    .get(name)
                    .and_then(|versions| versions.get(version))
                    .ok_or_else(|| {
                        miette::miette!("solved Python package {name} {version} was never offered")
                    })?;
                Ok((name.clone(), candidate.wheel.clone()))
            })
            .collect::<Result<_>>()?;
        Ok(Self { target, solution, wheels, declared })
    }

    /// The marker naming the environment this was solved for: every
    /// marker variable the solved graph reads, plus the ones a declared
    /// environment pins.
    fn marker(&self, referenced: &BTreeSet<String>, metadata: &Metadata) -> Result<String> {
        let mut keys = referenced.clone();
        if self.declared.is_empty() {
            if splits_a_minor_release(metadata, &self.solution, &self.target.environment) {
                keys.insert("python_full_version".to_string());
            }
        } else {
            for key in UNANSWERED_BY_A_DECLARATION {
                keys.remove(key);
            }
            // A declared environment names a Python minor and is resolved
            // as that minor's first release, so the patch release it ran
            // as is not something the lockfile can claim either.
            if self.declared
                .iter()
                .any(|key| key == "python_version")
            {
                keys.remove("python_full_version");
            }
            keys.extend(self.declared.iter().cloned());
        }
        environment_marker(&self.target.environment, &keys)
    }
}

/// The marker variables a declared environment cannot answer. Nothing
/// says which kernel a machine pnpm never runs reports, so the
/// environment pnpm resolves against reports none and its marker says
/// nothing about it.
const UNANSWERED_BY_A_DECLARATION: [&str; 2] = ["platform_release", "platform_version"];

/// The marker naming an environment: each of `keys` fixed to the value
/// the environment gives it.
pub fn environment_marker(
    environment: &MarkerEnvironment,
    keys: &BTreeSet<String>,
) -> Result<String> {
    let environment = serde_json::to_value(environment).into_diagnostic()?;
    Ok(environment
        .as_object()
        .expect("marker environment serializes to an object")
        .iter()
        .filter(|(key, _)| keys.contains(key.as_str()))
        .map(|(key, value)| {
            let value = value.as_str().expect("marker environment values are strings");
            if value.contains(['\'', '"', '\n', '\r']) {
                bail!("Python environment value cannot be represented as a lockfile marker: {key}");
            }
            Ok(format!("{key} == '{value}'"))
        })
        .collect::<Result<Vec<_>>>()?
        .join(" and "))
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
/// the environments it was answered for. A server's answer is accepted
/// only when it was for exactly these; a lockfile on disk is replayed on
/// whatever target still installs it — see [`Lockfile::applies_to`].
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Inputs {
    requirements: Vec<String>,
    /// The interpreter a lockfile resolved for the running interpreter
    /// was answered for. A lockfile resolved for declared environments
    /// carries neither: no one interpreter stands for them.
    #[serde(skip_serializing_if = "Option::is_none")]
    environment: Option<MarkerEnvironment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tags: Option<Vec<String>>,
    /// The platforms the project declares, as configured.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    platforms: Vec<String>,
    /// The Python versions the project declares, as configured.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    python_versions: Vec<String>,
    index: String,
}

impl Inputs {
    pub fn set_requirements(&mut self, requirements: &[Requirement]) {
        self.requirements = normalized(requirements);
    }

    /// The inputs of a resolution answered for one interpreter.
    #[must_use]
    pub fn new(requirements: &[Requirement], target: &Target, index: &str) -> Self {
        Self {
            requirements: normalized(requirements),
            environment: Some(target.environment.clone()),
            tags: Some(target.tags.clone()),
            platforms: Vec::new(),
            python_versions: Vec::new(),
            index: index.to_string(),
        }
    }

    /// The inputs of a resolution answered for the environments the
    /// project declares, which no interpreter of its own stands for.
    #[must_use]
    pub fn declared(
        requirements: &[Requirement],
        platforms: &[String],
        python_versions: &[String],
        index: &str,
    ) -> Self {
        Self {
            requirements: normalized(requirements),
            environment: None,
            tags: None,
            platforms: platforms.to_vec(),
            python_versions: python_versions.to_vec(),
            index: index.to_string(),
        }
    }
}

fn normalized(requirements: &[Requirement]) -> Vec<String> {
    let mut requirements = requirements
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    requirements.sort();
    requirements.dedup();
    requirements
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
    pub wheels: Vec<LockedWheel>,
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
        if self.tool.pnpm.requirements != inputs.requirements {
            bail!("the project's Python requirements changed");
        }
        if self.tool.pnpm.index != inputs.index {
            bail!("the Python index changed");
        }
        if self.tool.pnpm.platforms != inputs.platforms
            || self.tool.pnpm.python_versions != inputs.python_versions
        {
            bail!("the environments the project locks for changed");
        }
        if self.requires_python.as_deref() != requires_python {
            bail!("the project's requires-python changed");
        }
        for package in self.selected_packages(&target.environment)? {
            package.installable_wheel(target)?;
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
            let wheel = package.installable_wheel(target)?.clone();
            if packages.candidates
                .insert(
                    package.name.clone(),
                    BTreeMap::from([(
                        package.version.clone(),
                        Candidate { wheel, core_metadata: None },
                    )]),
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

/// Refuse to lock for a declared environment that does not decide every
/// marker the solved graph reads.
///
/// A lockfile says a package is installed wherever an environment's
/// marker holds. An environment that leaves a requirement's own marker
/// open holds on machines the resolution never answered for, which is a
/// claim the lockfile cannot make. The environment pnpm resolves against
/// is a platform and a Python version; a requirement that reads anything
/// else about a machine has to be locked for the interpreter that runs
/// the install.
fn check_environment_decides(
    solved: &Solved,
    marker: &MarkerTree,
    written: &str,
    metadata: &Metadata,
    requirements: &[Requirement],
) -> Result<()> {
    for requirement in requirements {
        check_decided(marker, written, &requirement.marker)?;
    }
    for (name, version) in &solved.solution {
        let Some(metadata) = metadata.get(&(name.clone(), version.clone())) else { continue };
        for requirement in &metadata.requires_dist {
            check_decided(marker, written, &parse_requirement(requirement)?.marker)?;
        }
        if let Some(specifiers) = metadata.requires_python.as_deref().and_then(declared_range) {
            check_decided(marker, written, &interpreter_range(&specifiers))?;
        }
    }
    Ok(())
}

/// Whether an environment decides a marker: it either holds throughout
/// the environment or nowhere in it. Extras are not part of an
/// environment, so a requirement an extra brings in is read as brought
/// in.
fn check_decided(environment: &MarkerTree, written: &str, marker: &MarkerTree) -> Result<()> {
    let marker = marker.clone().simplify_extras_with(|_| true);
    if environment.is_disjoint(&marker) || environment.is_disjoint(&marker.negate()) {
        return Ok(());
    }
    bail!(
        "the environment {written} leaves the Python marker {} undecided: \
         name the Python version in full, or lock for the interpreter running the install",
        marker.try_to_string().unwrap_or_default(),
    );
}

/// A `Requires-Python` range as the marker it stands for, which is what
/// lets an environment be asked whether it decides the range.
fn interpreter_range(specifiers: &VersionSpecifiers) -> MarkerTree {
    let mut marker = MarkerTree::TRUE;
    for specifier in specifiers.iter() {
        marker.and(MarkerTree::expression(MarkerExpression::Version {
            key: MarkerValueVersion::PythonFullVersion,
            specifier: specifier.clone(),
        }));
    }
    marker
}

/// Every package the solved environments install, with the wheels they
/// take and the marker saying which of them install it. A package all of
/// them install carries no marker: the lockfile's `environments` already
/// says where it is installed.
fn merge_packages(solved: &[Solved], markers: &[MarkerTree]) -> Result<Vec<LockedPackage>> {
    let mut merged =
        BTreeMap::<(PackageName, Version), (MarkerTree, BTreeMap<String, LockedWheel>)>::new();
    let mut scope = MarkerTree::FALSE;
    for (environment, marker) in solved.iter().zip(markers) {
        scope.or(marker.clone());
        for (name, version) in &environment.solution {
            let wheel = environment.wheels
                .get(name)
                .ok_or_else(|| miette::miette!("solved Python package {name} pins no wheel"))?;
            let entry = merged
                .entry((name.clone(), version.clone()))
                .or_insert_with(|| (MarkerTree::FALSE, BTreeMap::new()));
            entry.0.or(marker.clone());
            entry.1.insert(wheel.name.clone(), wheel.clone());
        }
    }
    let packages = merged
        .into_iter()
        .map(|((name, version), (marker, wheels))| LockedPackage {
            name,
            version,
            marker: (marker != scope).then(|| marker.try_to_string()).flatten(),
            wheels: wheels.into_values().collect(),
        })
        .collect::<Vec<_>>();
    check_one_version_per_environment(&packages)?;
    Ok(packages)
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

/// The marker variables the solved graphs read, plus the interpreter
/// version: the variables in the markers of the root requirements and of
/// every solved package's `Requires-Dist`, which are all a target
/// contributes to a solution besides the wheels it accepts.
fn referenced_marker_keys(
    metadata: &Metadata,
    requirements: &[Requirement],
    solved: &[Solved],
) -> Result<BTreeSet<String>> {
    let mut keys = BTreeSet::from(["python_version".to_string()]);
    for requirement in requirements {
        collect_marker_keys(&requirement.marker, &mut keys);
    }
    for (name, version) in solved.iter().flat_map(|solved| &solved.solution) {
        let metadata = metadata
            .get(&(name.clone(), version.clone()))
            .ok_or_else(|| {
                miette::miette!("solved Python package {name} {version} was never read")
            })?;
        for requirement in &metadata.requires_dist {
            collect_marker_keys(&parse_requirement(requirement)?.marker, &mut keys);
        }
    }
    Ok(keys)
}

/// Whether a locked package's `Requires-Python` tells patch releases of
/// the interpreter's own minor version apart, which is the only thing
/// besides a marker that can make the full version matter to a solution.
fn splits_a_minor_release(
    metadata: &Metadata,
    solution: &BTreeMap<PackageName, Version>,
    environment: &MarkerEnvironment,
) -> bool {
    solution
        .iter()
        .any(|(name, version)| {
            metadata
                .get(&(name.clone(), version.clone()))
                .and_then(|metadata| metadata.requires_python.as_deref())
                .and_then(declared_range)
                .is_some_and(|specifiers| {
                    !admits_every_patch_release(
                        &specifiers,
                        &environment.python_full_version().version,
                    )
                })
        })
}

/// Whether `specifiers`, which `running` satisfies, admit every patch
/// release of the minor version `running` belongs to. When they do, the
/// minor version says everything they can about a target; when they do
/// not, only the full version does. A bound in another minor version
/// cannot split this one; one in this minor version does unless it is
/// the minor version itself, taken whole.
fn admits_every_patch_release(specifiers: &VersionSpecifiers, running: &Version) -> bool {
    let minor = |version: &Version| {
        let release = version.release();
        [release.first().copied().unwrap_or(0), release.get(1).copied().unwrap_or(0)]
    };
    let running_minor = minor(running);
    specifiers
        .iter()
        .all(|specifier| {
            if minor(specifier.version()) != running_minor {
                return true;
            }
            if significant_release_segments(specifier) > 2 {
                return false;
            }
            matches!(
                specifier.operator(),
                Operator::GreaterThanEqual
                    | Operator::TildeEqual
                    | Operator::EqualStar
                    | Operator::NotEqualStar
                    | Operator::LessThan,
            )
        })
}

/// How many release segments of a specifier's version can tell versions
/// apart. PEP 440 zero-pads the ordered comparisons, so `>=3.12.0` is
/// `>=3.12`; a wildcard keeps every segment, so `==3.12.0.*` is not
/// `==3.12.*`; a compatible release is its lower bound and the wildcard
/// on all but its last segment, so `~=3.12.0.0` is `==3.12.0.*`.
fn significant_release_segments(specifier: &pep440_rs::VersionSpecifier) -> usize {
    let release = specifier.version().release();
    let zero_padded = release
        .iter()
        .rposition(|&segment| segment != 0)
        .map_or(0, |last| last + 1);
    match specifier.operator() {
        Operator::EqualStar | Operator::NotEqualStar => release.len(),
        Operator::TildeEqual => zero_padded.max(release.len() - 1),
        _ => zero_padded,
    }
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
        MarkerTreeKind::Version(node) => (
            Some(node.key().to_string()),
            node.edges()
                .map(|(_, child)| child)
                .collect(),
        ),
        MarkerTreeKind::String(node) => (
            Some(node.key().to_string()),
            node.children()
                .map(|(_, child)| child)
                .collect(),
        ),
        MarkerTreeKind::In(node) => (
            Some(node.key().to_string()),
            node.children()
                .map(|(_, child)| child)
                .collect(),
        ),
        MarkerTreeKind::Contains(node) => (
            Some(node.key().to_string()),
            node.children()
                .map(|(_, child)| child)
                .collect(),
        ),
        MarkerTreeKind::Extra(node) => (
            None,
            node.children()
                .map(|(_, child)| child)
                .collect(),
        ),
    }
}
