pub(crate) use add::add;

mod add;
mod metadata;
mod selection;
use selection::Pnpm;

use miette::{
    IntoDiagnostic,
    Result,
    bail,
};
use pep440_rs::Version;
use pep508_rs::PackageName;
use pnpm_python_resolver::parse_requirement;
use serde::Deserialize;
use std::collections::{
    BTreeMap,
    BTreeSet,
};

#[derive(Clone, Copy)]
pub struct DependencySelection {
    pub production: bool,
    pub development: bool,
}

impl DependencySelection {
    pub const ALL: Self = Self { production: true, development: true };
}

/// Which of a project's requirement lists a reader counts. A dependency
/// group is a development input, so a production reader leaves it out.
#[derive(Clone, Copy)]
pub(super) enum RequirementScope {
    All,
    Production,
}

#[derive(Clone, Deserialize)]
pub(super) struct Manifest {
    pub(super) project: Option<Project>,
    #[serde(skip)]
    pub(super) metadata: Option<super::host::WheelMetadata>,
    #[serde(skip)]
    pub(super) metadata_wheel: Option<std::sync::Arc<super::build::FallbackWheel>>,
    #[serde(skip)]
    pub(super) metadata_output: Option<std::sync::Arc<tempfile::TempDir>>,
    /// Whether the `requirements.txt` beside the manifest was left unread
    /// because the directory's `.venv` is unmanaged.
    #[serde(skip)]
    pub(super) has_unmanaged_requirements: bool,
    #[serde(
        default,
        rename = "dependency-groups",
        deserialize_with = "selection::deserialize_groups"
    )]
    groups: BTreeMap<PackageName, Vec<toml::Value>>,
    #[serde(default, rename = "build-system")]
    pub(super) build_system: Option<BuildSystem>,
    #[serde(default)]
    pub(super) tool: Tool,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) struct Project {
    /// Absent only in a manifest that is not a distribution: PEP 621
    /// requires it of one that is.
    pub(super) name: Option<PackageName>,
    pub(super) version: Option<Version>,
    #[serde(default)]
    pub(super) dependencies: Vec<String>,
    #[serde(default)]
    pub(super) dynamic: Vec<String>,
    pub(super) requires_python: Option<String>,
    #[serde(default)]
    optional_dependencies: BTreeMap<String, Vec<String>>,
}

/// The PEP 517 backend that turns a project directory into a wheel.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) struct BuildSystem {
    #[serde(default)]
    pub(super) requires: Vec<String>,
    pub(super) build_backend: Option<String>,
    #[serde(default)]
    pub(super) backend_path: Vec<String>,
}

#[derive(Default, Clone, Deserialize)]
pub(super) struct Tool {
    #[serde(default)]
    pub(super) uv: Uv,
    #[serde(default)]
    pnpm: Pnpm,
}

/// The parts of uv's table pnpm reads. A project that declares a
/// dependency on another project in the repository says so here, which is
/// where every Python workspace in the wild already writes it.
#[derive(Default, Clone, Deserialize)]
pub(super) struct Uv {
    #[serde(default, rename = "override-dependencies")]
    pub(super) overrides: Vec<String>,
    #[serde(default, rename = "constraint-dependencies")]
    pub(super) constraints: Vec<String>,
    #[serde(default)]
    pub(super) sources: BTreeMap<PackageName, SourceDeclaration>,
    /// Whether this project is built and installed at all. A project that
    /// sets this to `false` contributes its dependencies and nothing else.
    pub(super) package: Option<bool>,
    /// Present on the manifest that is a workspace root, naming which
    /// projects under it the workspace contains.
    pub(super) workspace: Option<UvWorkspace>,
}

#[derive(Clone, Deserialize)]
pub(super) struct UvWorkspace {
    #[serde(default)]
    pub(super) members: Vec<String>,
    #[serde(default)]
    pub(super) exclude: Vec<String>,
}

/// A source as it is written: one table, or several selected by markers.
#[derive(Clone, Deserialize)]
#[serde(untagged)]
pub(super) enum SourceDeclaration {
    One(Box<Source>),
    Many(Vec<Source>),
}

impl SourceDeclaration {
    /// Every source this declaration names. Resolving a requirement
    /// through a declaration that names several is refused, while reading
    /// where they all point is not: the workspace graph covers a manifest
    /// an install would go on to reject.
    pub(super) fn sources(&self) -> &[Source] {
        match self {
            Self::One(source) => std::slice::from_ref(&**source),
            Self::Many(sources) => sources,
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) struct Source {
    #[serde(default)]
    pub(super) workspace: bool,
    pub(super) path: Option<String>,
    pub(super) editable: Option<bool>,
    pub(super) git: Option<String>,
    #[serde(flatten)]
    pub(super) revision: GitRevision,
    pub(super) url: Option<String>,
    pub(super) index: Option<String>,
    #[serde(flatten)]
    pub(super) narrowing: Narrowing,
}

#[derive(Clone, Deserialize)]
pub(super) struct GitRevision {
    pub(super) rev: Option<String>,
    pub(super) tag: Option<String>,
    pub(super) branch: Option<String>,
    pub(super) subdirectory: Option<String>,
}

/// What narrows a source to some targets, or to one extra or group. pnpm
/// reads these to refuse them: a source it applied everywhere would
/// install a project the manifest asked for somewhere else.
#[derive(Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(super) struct Narrowing {
    marker: Option<String>,
    extra: Option<String>,
    group: Option<String>,
}

impl Narrowing {
    /// What this source is narrowed by, if anything.
    pub(super) fn kind(&self) -> Option<&'static str> {
        if self.marker.is_some() {
            return Some("conditional");
        }
        if self.extra.is_some() {
            return Some("extra-qualified");
        }
        self.group.is_some().then_some("group-qualified")
    }
}

impl Project {
    fn distribution_requirements(&self, static_only: bool) -> Result<Vec<String>> {
        let mut requirements = if static_only
            && self.dynamic
                .iter()
                .any(|field| field == "dependencies")
        {
            Vec::new()
        } else {
            self.dependencies.clone()
        };
        if static_only
            && self.dynamic
                .iter()
                .any(|field| field == "optional-dependencies")
        {
            return Ok(requirements);
        }
        for (extra, dependencies) in &self.optional_dependencies {
            for dependency in dependencies {
                requirements.push(super::workspace::requirement_for_extra(dependency, extra)?);
            }
        }
        Ok(requirements)
    }

    fn ensure_static_dependencies(&self) -> Result<()> {
        if self.dynamic
            .iter()
            .any(|field| {
                matches!(
                    field.as_str(),
                    "dependencies" | "optional-dependencies" | "requires-python",
                )
            })
        {
            bail!("pnpm Python integration requires static dependency metadata in pyproject.toml");
        }
        Ok(())
    }
}

impl Manifest {
    pub(super) fn set_requirements_file(&mut self, requirements: Vec<String>) {
        self.project = Some(Project {
            name: None,
            version: None,
            dependencies: requirements,
            dynamic: Vec::new(),
            requires_python: None,
            optional_dependencies: BTreeMap::new(),
        });
        self.tool.uv.package = Some(false);
    }

    pub(super) fn parse(contents: &str) -> Result<Self> {
        toml::from_str(contents).into_diagnostic()
    }

    /// The distributions this project requires for development and nothing
    /// else, which a production reader leaves out.
    ///
    /// A dependency group is a development input, so a name only a group
    /// requires is one. Saying that needs the requirements the group is
    /// compared against: a project whose own a build backend generates
    /// names none here, because that backend may require the same
    /// distribution to run.
    pub(super) fn development_only_names(&self) -> BTreeSet<PackageName> {
        let Some(names) = self.declared_requirement_names(RequirementScope::Production) else {
            return BTreeSet::new();
        };
        self.groups
            .values()
            .flatten()
            .filter_map(toml::Value::as_str)
            .map(parse_requirement)
            .filter_map(Result::ok)
            .map(|requirement| requirement.name)
            .filter(|name| !names.contains(name))
            .collect()
    }

    /// Every distribution this project names in a requirement its own
    /// manifest declares, or `None` when a build backend generates them and
    /// the manifest does not say which they are.
    ///
    /// A requirement pnpm cannot parse is left out: the resolution that
    /// reads it reports it, and this is read where a name that is not there
    /// only means one source fewer to consider.
    pub(super) fn declared_requirement_names(
        &self,
        scope: RequirementScope,
    ) -> Option<BTreeSet<PackageName>> {
        let project = self.project.as_ref()?;
        if project.dynamic
            .iter()
            .any(|field| matches!(field.as_str(), "dependencies" | "optional-dependencies"))
        {
            return None;
        }
        // Every group's own requirements are read, so a `{ include-group }`
        // entry names nothing this does not already have: PEP 735 includes
        // a group of this same table.
        let groups = matches!(scope, RequirementScope::All)
            .then(|| {
                self.groups
                    .values()
                    .flatten()
                    .filter_map(toml::Value::as_str)
            })
            .into_iter()
            .flatten();
        Some(
            project.dependencies
                .iter()
                .chain(project.optional_dependencies.values().flatten())
                .map(String::as_str)
                .chain(groups)
                .map(parse_requirement)
                .filter_map(Result::ok)
                .map(|requirement| requirement.name)
                .collect(),
        )
    }

    /// Every distribution this project declares a requirement on,
    /// wherever it declares it. Which source satisfies a requirement is
    /// decided per distribution, so a name is reported once.
    pub(super) fn declared_distributions(&self) -> Result<BTreeSet<PackageName>> {
        let Some(project) = &self.project else { return Ok(BTreeSet::new()) };
        if self.metadata.is_none() {
            project.ensure_static_dependencies()?;
        }
        let metadata_requirements = self.metadata
            .as_ref()
            .map(|metadata| metadata.requires_dist.as_slice())
            .unwrap_or_default();
        let groups = self.groups
            .values()
            .flatten()
            .filter_map(toml::Value::as_str)
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        project.dependencies
            .iter()
            .chain(project.optional_dependencies.values().flatten())
            .chain(&groups)
            .chain(metadata_requirements)
            .map(|requirement| Ok(parse_requirement(requirement)?.name))
            .collect()
    }

    /// What a wheel built from this project would state in its
    /// `METADATA`: its requirements, and its extras' requirements under
    /// the marker that selects each extra. A dependency group is a
    /// development input, so a wheel does not carry it.
    pub(super) fn distribution_requirements(&self) -> Result<Vec<String>> {
        let Some(project) = &self.project else { return Ok(Vec::new()) };
        if self.metadata.is_none() {
            project.ensure_static_dependencies()?;
        }
        if let Some(metadata) = &self.metadata {
            return Ok(metadata.requires_dist.clone());
        }
        project.distribution_requirements(false)
    }

    /// The extras this project offers.
    pub(super) fn extras(&self) -> Vec<String> {
        if let Some(metadata) = &self.metadata {
            return metadata.provides_extra.clone();
        }
        self.project
            .iter()
            .flat_map(|project| project.optional_dependencies.keys())
            .cloned()
            .collect()
    }

    /// The distribution this manifest declares, if it declares one.
    pub(super) fn distribution(&self) -> Option<&PackageName> {
        self.project.as_ref()?.name.as_ref()
    }

    /// Whether a wheel is built from this project and installed into its
    /// own environment. A project that declares no build backend and does
    /// not ask to be packaged is a place to collect dependencies, not a
    /// distribution.
    pub(super) fn is_packaged(&self) -> bool {
        self.tool.uv.package.unwrap_or_else(|| self.build_system.is_some())
    }
}
