//! The environments a lockfile is resolved for, and the markers that
//! name them.

use super::LockedWheel;
use crate::{
    candidates::parse_requirement, metadata::WheelMetadata, packages::Packages,
    requires_python::declared_range,
};
use miette::{IntoDiagnostic, Result, bail};
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    pub(super) fn marker(
        &self,
        referenced: &BTreeSet<String>,
        metadata: &Metadata,
    ) -> Result<String> {
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

/// else about a machine has to be locked for the interpreter that runs
/// the install.
pub(super) fn check_environment_decides(
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

/// The marker variables the solved graphs read, plus the interpreter
/// version: the variables in the markers of the root requirements and of
/// every solved package's `Requires-Dist`, which are all a target
/// contributes to a solution besides the wheels it accepts.
pub(super) fn referenced_marker_keys(
    metadata: &Metadata,
    requirements: &[Requirement],
    solved: &[Solved],
) -> Result<BTreeSet<String>> {
    let mut keys = BTreeSet::from(["python_version".to_string()]);
    for requirement in requirements {
        collect_marker_keys(&requirement.marker, &mut keys);
    }
    let solved_versions = solved
        .iter()
        .flat_map(|solved| &solved.solution)
        .collect::<BTreeSet<_>>();
    for (name, version) in solved_versions {
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
            if significant_release_segments(specifier) > 2 || falls_between_releases(specifier) {
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

/// Whether a specifier's version falls between two releases. A post or
/// local component puts it above the release it names, so a bound there
/// tells that release apart from the next one; a pre-release or
/// development component puts it below, which under the operators that
/// reach here cannot.
fn falls_between_releases(specifier: &pep440_rs::VersionSpecifier) -> bool {
    let version = specifier.version();
    *version > Version::new(version.release().iter().copied())
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
