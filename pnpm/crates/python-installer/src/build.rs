pub(super) use validation::{extra_set, requirement_set};

pub(super) use metadata::FallbackWheel;

mod metadata;
mod validation;
use validation::requires_what_it_declares;

use super::{
    environment::PythonPrepare,
    host::{self, Wheel},
    manifest::Manifest,
    resolver,
    workspace::LocalProject,
};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pep508_rs::PackageName;
use pnpm_python_resolver::{parse_requirement, wheel_identity};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

/// What PEP 517 says a project means when it declares no build system.
/// How an `allowBuilds` key names a Python distribution: the [Package
/// URL] type for `PyPI`, which every ecosystem pnpm installs has one of.
///
/// [Package URL]: https://github.com/package-url/purl-spec
const PYPI_PURL: &str = "pkg:pypi/";

const DEFAULT_BACKEND: &str = "setuptools.build_meta:__legacy__";
const DEFAULT_REQUIRES: &[&str] = &["setuptools>=40.8.0", "wheel"];

/// Build a wheel from each project's own source, so a requirement on a
/// project in this repository installs that project rather than whatever
/// the index publishes under its name.
///
/// Each build runs the project's declared backend in an environment
/// holding only what that backend asked for, which is what makes the
/// build reproduce away from the developer's machine.
impl PythonPrepare<'_> {
    pub(super) async fn build_projects<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        projects: &[&LocalProject],
    ) -> Result<BTreeMap<PackageName, Build>> {
        let mut built = BTreeMap::new();
        for project in projects {
            let wheel =
                self.build::<Reporter>(&project.root, &project.manifest, project.editable).await?;
            built.insert(project.name.clone(), wheel);
        }
        Ok(built)
    }

    /// Build the project whose environment this is, so that `pnpm install`
    /// leaves it able to import its own code.
    pub(super) async fn build_self<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        root: &Path,
        manifest: &Manifest,
    ) -> Result<Build> {
        self.build::<Reporter>(root, manifest, true).await
    }

    pub(super) async fn build<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        root: &Path,
        manifest: &Manifest,
        editable: bool,
    ) -> Result<Build> {
        let requires = self.build_requirements(root, manifest)?;
        let unapproved = unapproved(self.context.config, &requires);
        if !unapproved.is_empty() {
            return Ok(Build::NotApproved(unapproved));
        }
        let cached = manifest.metadata_wheel
            .as_ref()
            .filter(|wheel| !editable && wheel.interpreter == self.interpreter.executable);
        let (built, output) = if let Some(cached) = cached {
            let metadata = host::run(
                &self.interpreter.executable,
                "inspect",
                serde_json::json!({ "files": cached.wheel.files, "filename": cached.wheel.filename }),
            )
            .await
            .wrap_err_with(|| format!("read the wheel built from {}", root.display()))?;
            (Backend517 { wheel: cached.wheel.clone(), metadata }, Arc::clone(&cached.output))
        } else {
            let request = serde_json::json!({
                "root": root,
                "backend": backend(manifest).module,
                "backend_path": backend(manifest).path,
                "editable": editable,
                "metadata_directory": manifest.metadata_output.as_ref().zip(manifest.metadata.as_ref())
                    .map(|(output, metadata)| output.path().join(&metadata.dist_info)),
            });
            let environment =
                match self.build_environment::<Reporter>(root, requires, &request).await? {
                    BuildEnvironment::Ready(environment) => environment,
                    BuildEnvironment::NotApproved(names) => return Ok(Build::NotApproved(names)),
                };
            let output = tempfile::tempdir().into_diagnostic()?;
            let built = self.run_backend(root, environment, &output, request).await?;
            (built, Arc::new(output))
        };
        self.finish_build(root, manifest, editable, built, output)
    }

    fn finish_build(
        &self,
        root: &Path,
        manifest: &Manifest,
        editable: bool,
        built: Backend517,
        output: Arc<tempfile::TempDir>,
    ) -> Result<Build> {
        self.installable_here(&built, root)?;
        identify(&built.metadata, manifest, root)?;
        let directory = root.display();
        let url = url::Url::from_directory_path(root)
            .map_err(|()| miette::miette!("the Python project at {directory} has no URL"))?;
        Ok(Build::Made(Box::new(Built {
            wheel: Wheel {
                filename: built.wheel.filename,
                files: built.wheel.files,
                metadata: built.metadata,
                direct_url: Some(host::DirectUrl::directory(url.to_string(), editable)),
            },
            output,
        })))
    }

    /// Run the backend, and read the wheel it produced with the
    /// interpreter the wheel is installed for rather than the one the
    /// backend needed.
    async fn run_backend(
        &self,
        root: &Path,
        environment: Arc<tempfile::TempDir>,
        output: &tempfile::TempDir,
        request: serde_json::Value,
    ) -> Result<Backend517> {
        let mut request = request;
        request["output"] = serde_json::json!(output.path());
        let wheel: BuiltWheel = host::run(&interpreter(environment.path()), "build", request)
            .await
            .wrap_err_with(|| format!("build the Python project at {}", root.display()))?;
        let metadata = host::inspect(&self.interpreter.executable, &wheel.files, &wheel.filename)
            .await
            .wrap_err_with(|| format!("read the wheel built from {}", root.display()))?;
        Ok(Backend517 { wheel, metadata })
    }

    /// Refuse a wheel this interpreter would not install. A backend
    /// reads its own configuration, so one told to build for another
    /// interpreter or platform produces a wheel that is not for the
    /// environment pnpm is filling.
    fn installable_here(&self, built: &Backend517, root: &Path) -> Result<()> {
        let tags = &self.interpreter.target.tags;
        if wheel_identity(&built.wheel.filename, tags)?.is_none() {
            bail!(
                "the Python project at {} built {}, which this interpreter does not install",
                root.display(),
                built.wheel.filename,
            );
        }
        let Some(declared) = built.metadata.requires_python.as_deref() else { return Ok(()) };
        let specifiers: pep440_rs::VersionSpecifiers = declared.parse().into_diagnostic()?;
        if !specifiers.contains(self.interpreter.target.environment.python_full_version()) {
            bail!(
                "the Python project at {} built a wheel requiring Python {specifiers}, but {} was \
                 selected",
                root.display(),
                self.interpreter.target.environment.python_full_version(),
            );
        }
        Ok(())
    }

    /// What the project declares it needs to build, with PEP 517's
    /// defaults where it names no backend.
    fn build_requirements(
        &self,
        root: &Path,
        manifest: &Manifest,
    ) -> Result<Vec<pep508_rs::Requirement>> {
        let system = manifest.build_system.as_ref();
        let mut requires = system
            .map(|system| {
                system.requires
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        // PEP 517 has a project that names no backend built by setuptools'
        // legacy one, whether or not it thought to require it.
        if system.and_then(|system| system.build_backend.as_ref()).is_none() {
            requires.extend(DEFAULT_REQUIRES.iter().copied());
        }
        self.usable_build_requirements(root, requires)
    }

    /// A build requirement read the same way wherever it was written: one
    /// a marker excludes is not installed and does not run, and one
    /// naming a project in this project's workspace is refused rather
    /// than taken from the index.
    fn usable_build_requirements<'a>(
        &self,
        root: &Path,
        requires: impl IntoIterator<Item = &'a str>,
    ) -> Result<Vec<pep508_rs::Requirement>> {
        let environment = &self.interpreter.target.environment;
        let members = self.members.get(root);
        let mut usable = Vec::new();
        for requirement in requires {
            let requirement = parse_requirement(requirement)?;
            if !requirement.marker.evaluate(environment, &[]) {
                continue;
            }
            if members.is_some_and(|members| members.contains(&requirement.name)) {
                bail!(
                    "the Python project at {} needs `{}` to build, which is a project in its \
                     workspace. pnpm does not build with a workspace project's own backend yet, \
                     and will not run the index's package of that name instead.",
                    root.display(),
                    requirement.name,
                );
            }
            usable.push(requirement);
        }
        Ok(usable)
    }

    /// An environment holding the project's build requirements and nothing
    /// else. The interpreter is the one the project installs for, so a
    /// backend that compiles against it compiles against the right one.
    ///
    /// A backend may only be able to name some of its requirements once it
    /// can see the project, so the declared ones are installed first and
    /// asked what else this build needs.
    async fn build_environment<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        root: &Path,
        mut requires: Vec<pep508_rs::Requirement>,
        request: &serde_json::Value,
    ) -> Result<BuildEnvironment> {
        let declared = self.install_requirements::<Reporter>(&requires).await?;
        let extra: Vec<String> =
            host::run(&interpreter(declared.path()), "build_requires", request.clone()).await?;
        if extra.is_empty() {
            return Ok(BuildEnvironment::Ready(declared));
        }
        let asked = self.usable_build_requirements(root, extra.iter().map(String::as_str))?;
        if asked.is_empty() {
            return Ok(BuildEnvironment::Ready(declared));
        }
        let unapproved = unapproved(self.context.config, &asked);
        if !unapproved.is_empty() {
            return Ok(BuildEnvironment::NotApproved(unapproved));
        }
        requires.extend(asked);
        self.install_requirements::<Reporter>(&requires).await.map(BuildEnvironment::Ready)
    }

    /// An environment holding exactly these requirements, built once per
    /// run. Every project declaring one backend needs the same
    /// environment, and resolving and installing it again for each would
    /// be most of what a workspace's install does.
    ///
    /// One environment belongs to the interpreter that installed it: a
    /// backend runs in the interpreter it was installed for, and what it
    /// compiles is built for that one.
    async fn install_requirements<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        requires: &[pep508_rs::Requirement],
    ) -> Result<Arc<tempfile::TempDir>> {
        let key = self.build_environment_key(requires);
        {
            let mut environments = self.build_environments.lock().await;
            match environments.get(&key) {
                Some(Some(environment)) => return Ok(Arc::clone(environment)),
                Some(None) => bail!("cyclic Python build requirements: {}", key.1),
                None => {
                    environments.insert(key.clone(), None);
                }
            }
        }
        let result = self.install_build_requirements::<Reporter>(requires).await;
        let mut environments = self.build_environments.lock().await;
        match &result {
            Ok(root) => {
                environments.insert(key, Some(Arc::clone(root)));
            }
            Err(_) => {
                environments.remove(&key);
            }
        }
        result
    }

    fn build_environment_key(
        &self,
        requires: &[pep508_rs::Requirement],
    ) -> super::environment::BuildEnvironmentKey {
        let mut requirements = requires
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        requirements.sort();
        requirements.dedup();
        (
            format!(
                "{} {}",
                self.interpreter.executable,
                self.interpreter.target.environment.python_full_version()
            ),
            requirements.join(" "),
        )
    }

    async fn install_build_requirements<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        requires: &[pep508_rs::Requirement],
    ) -> Result<Arc<tempfile::TempDir>> {
        let root = tempfile::tempdir().into_diagnostic()?;
        let mut registry = self.registry();
        let solution = resolver::resolve::<Reporter>(&mut registry, requires).await?;
        let mut wheels = Vec::new();
        for package in solution {
            wheels.push(&registry.wheels[&package]);
        }
        host::run::<serde_json::Value>(
            &self.interpreter.executable,
            "install",
            serde_json::json!({ "root": root.path(), "packages": wheels }),
        )
        .await?;
        Ok(Arc::new(root))
    }
}

/// A wheel built from a project's source, and the directory it was
/// unpacked into. The environment installs the unpacked files, so the
/// directory outlives the build.
/// An environment a backend can run in, or the requirements nothing has
/// approved to run in one.
enum BuildEnvironment {
    Ready(Arc<tempfile::TempDir>),
    NotApproved(Vec<String>),
}

/// What building a project produced, or why pnpm did not run its backend.
pub(super) enum Build {
    Made(Box<Built>),
    /// The build requirements nothing has approved to run. A backend is
    /// code from the index that a build executes, so it is approved the
    /// way a dependency's build scripts are.
    NotApproved(Vec<String>),
}

pub(super) struct Built {
    pub(super) wheel: Wheel,
    pub(super) output: Arc<tempfile::TempDir>,
}

/// The wheel a backend produced, and what the target interpreter reads
/// in it.
struct Backend517 {
    wheel: BuiltWheel,
    metadata: host::WheelMetadata,
}

#[derive(Clone, serde::Deserialize)]
struct BuiltWheel {
    files: BTreeMap<String, std::path::PathBuf>,
    filename: String,
}

/// Refuse a wheel that is not the project it was built from. Resolution
/// answered with the manifest's identity and requirements, so installing
/// another distribution under it would install something the lockfile
/// does not describe.
fn identify(metadata: &host::WheelMetadata, manifest: &Manifest, root: &Path) -> Result<()> {
    identify_identity(metadata, manifest, root)?;
    requires_what_it_declares(metadata, manifest, root)
}

fn identify_identity(
    metadata: &host::WheelMetadata,
    manifest: &Manifest,
    root: &Path,
) -> Result<()> {
    let Some(name) = manifest.distribution() else { return Ok(()) };
    if metadata.name
        .parse::<PackageName>()
        .ok()
        .as_ref()
        != Some(name)
    {
        bail!(
            "the Python project at {} declares `{name}`, but its backend built `{}`",
            root.display(),
            metadata.name,
        );
    }
    // A project may leave its version to the backend, and then what the
    // backend says it is is the only answer there is.
    let Some(version) =
        manifest.project.as_ref().and_then(|project| project.version.as_ref())
    else {
        return Ok(());
    };
    if metadata.version
        .parse::<pep440_rs::Version>()
        .ok()
        .as_ref()
        != Some(version)
    {
        bail!(
            "the Python project at {} declares `{name}` {version}, but its backend built {}",
            root.display(),
            metadata.version,
        );
    }
    Ok(())
}

/// The build requirements the configuration has not approved to run.
pub(super) fn unapproved(
    config: &pnpm_config::Config,
    requires: &[pep508_rs::Requirement],
) -> Vec<String> {
    if config.dangerously_allow_all_builds {
        return Vec::new();
    }
    let approvals = Approvals::of(config);
    let mut names = requires
        .iter()
        .filter(|requirement| !approvals.any_version.contains(&requirement.name))
        .map(|requirement| approvals.describe(&requirement.name))
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

/// The distributions `allowBuilds` approves to run in a build.
///
/// A key says which ecosystem's package it names, as a Package URL.
/// Approving is a statement about one piece of code, and a bare name is
/// not one: npm and `PyPI` both publish `esbuild`, `ruff` and `black`, so a
/// key naming no ecosystem would let approving a build script approve a
/// build backend nobody looked at.
///
/// A key naming no version approves the distribution however it
/// resolves, which is the only form a check made before resolving can
/// answer. A version-qualified key names a release, and a build approved
/// by one says so rather than silently doing nothing.
struct Approvals {
    any_version: BTreeSet<pep508_rs::PackageName>,
    only_a_version: BTreeSet<pep508_rs::PackageName>,
}

impl Approvals {
    fn of(config: &pnpm_config::Config) -> Self {
        let mut approvals = Self { any_version: BTreeSet::new(), only_a_version: BTreeSet::new() };
        for (spec, allowed) in &config.allow_builds {
            if !allowed {
                continue;
            }
            let Some(spec) = spec.strip_prefix(PYPI_PURL) else { continue };
            let (name, version) = spec
                .rsplit_once('@')
                .map_or((spec, None), |(name, version)| (name, Some(version)));
            // The key is read as a distribution name, so it names the
            // same one however it is spelled.
            if let Ok(name) = name.parse::<pep508_rs::PackageName>() {
                if version.is_some() {
                    approvals.only_a_version.insert(name);
                } else {
                    approvals.any_version.insert(name);
                }
            }
        }
        approvals
    }

    fn describe(&self, name: &pep508_rs::PackageName) -> String {
        if self.only_a_version.contains(name) {
            return format!(
                "{PYPI_PURL}{name} (approved only for a version, which a Python build is not \
                 checked against)",
            );
        }
        format!("{PYPI_PURL}{name}")
    }
}

struct Backend {
    module: String,
    path: Vec<String>,
}

fn backend(manifest: &Manifest) -> Backend {
    let declared = manifest.build_system.as_ref();
    Backend {
        module: declared
            .and_then(|system| system.build_backend.clone())
            .unwrap_or_else(|| DEFAULT_BACKEND.to_string()),
        path: declared.map(|system| system.backend_path.clone()).unwrap_or_default(),
    }
}

fn interpreter(environment: &std::path::Path) -> String {
    environment
        .join(if cfg!(windows) { "Scripts/python.exe" } else { "bin/python" })
        .display()
        .to_string()
}
