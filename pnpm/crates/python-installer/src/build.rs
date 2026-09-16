use super::{
    environment::PythonPrepare,
    host::{self, Wheel},
    manifest::Manifest,
    resolver,
    workspace::LocalProject,
};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use pep508_rs::PackageName;
use pnpm_python_resolver::parse_requirement;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

/// What PEP 517 says a project means when it declares no build system.
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

    async fn build<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        root: &Path,
        manifest: &Manifest,
        editable: bool,
    ) -> Result<Build> {
        let requires = Self::build_requirements(manifest)?;
        let unapproved = unapproved(self.context.config, &requires);
        if !unapproved.is_empty() {
            return Ok(Build::NotApproved(unapproved));
        }
        let request = serde_json::json!({
            "root": root,
            "backend": backend(manifest).module,
            "backend_path": backend(manifest).path,
            "editable": editable,
        });
        let environment = match self.build_environment::<Reporter>(requires, &request).await? {
            BuildEnvironment::Ready(environment) => environment,
            BuildEnvironment::NotApproved(names) => return Ok(Build::NotApproved(names)),
        };
        let output = tempfile::tempdir().into_diagnostic()?;
        let built = self.run_backend(root, environment, &output, request).await?;
        identify(&built.metadata, manifest, root)?;
        let directory = root.display();
        let url = url::Url::from_directory_path(root)
            .map_err(|()| miette::miette!("the Python project at {directory} has no URL"))?;
        Ok(Build::Made(Box::new(Built {
            wheel: Wheel {
                filename: built.wheel.filename,
                files: built.wheel.files,
                metadata: built.metadata,
                direct_url: Some(host::DirectUrl { url: url.to_string(), editable }),
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
        environment: tempfile::TempDir,
        output: &tempfile::TempDir,
        request: serde_json::Value,
    ) -> Result<Backend517> {
        let mut request = request;
        request["output"] = serde_json::json!(output.path());
        let wheel: BuiltWheel = host::run(&interpreter(environment.path()), "build", request)
            .await
            .wrap_err_with(|| format!("build the Python project at {}", root.display()))?;
        let metadata = host::run(
            &self.interpreter.executable,
            "inspect",
            serde_json::json!({ "files": wheel.files, "filename": wheel.filename }),
        )
        .await
        .wrap_err_with(|| format!("read the wheel built from {}", root.display()))?;
        Ok(Backend517 { wheel, metadata })
    }

    /// What the project's backend needs installed to run at all.
    fn build_requirements(manifest: &Manifest) -> Result<Vec<pep508_rs::Requirement>> {
        let mut requires = Vec::new();
        if let Some(system) = manifest.build_system.as_ref() {
            for requirement in &system.requires {
                requires.push(parse_requirement(requirement)?);
            }
        }
        // PEP 517 has a project that names no backend built by setuptools'
        // legacy one, whether or not it thought to require it.
        if manifest.build_system
            .as_ref()
            .and_then(|system| system.build_backend.as_ref())
            .is_none()
        {
            for requirement in DEFAULT_REQUIRES {
                requires.push(parse_requirement(requirement)?);
            }
        }
        Ok(requires)
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
        mut requires: Vec<pep508_rs::Requirement>,
        request: &serde_json::Value,
    ) -> Result<BuildEnvironment> {
        let declared = self.install_requirements::<Reporter>(&requires).await?;
        let extra: Vec<String> =
            host::run(&interpreter(declared.path()), "build_requires", request.clone()).await?;
        if extra.is_empty() {
            return Ok(BuildEnvironment::Ready(declared));
        }
        let mut asked = Vec::new();
        for requirement in &extra {
            asked.push(parse_requirement(requirement)?);
        }
        // What a backend asks for once it can see the project runs in the
        // build too, so it is approved like what the manifest declared.
        let unapproved = unapproved(self.context.config, &asked);
        if !unapproved.is_empty() {
            return Ok(BuildEnvironment::NotApproved(unapproved));
        }
        requires.extend(asked);
        self.install_requirements::<Reporter>(&requires).await.map(BuildEnvironment::Ready)
    }

    /// A fresh environment holding exactly these requirements.
    async fn install_requirements<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        requires: &[pep508_rs::Requirement],
    ) -> Result<tempfile::TempDir> {
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
        Ok(root)
    }
}

/// A wheel built from a project's source, and the directory it was
/// unpacked into. The environment installs the unpacked files, so the
/// directory outlives the build.
/// An environment a backend can run in, or the requirements nothing has
/// approved to run in one.
enum BuildEnvironment {
    Ready(tempfile::TempDir),
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
    pub(super) output: tempfile::TempDir,
}

/// The wheel a backend produced, and what the target interpreter reads
/// in it.
struct Backend517 {
    wheel: BuiltWheel,
    metadata: host::WheelMetadata,
}

#[derive(serde::Deserialize)]
struct BuiltWheel {
    files: BTreeMap<String, std::path::PathBuf>,
    filename: String,
}

/// Refuse a wheel that is not the project it was built from. Resolution
/// answered with the manifest's identity and requirements, so installing
/// another distribution under it would install something the lockfile
/// does not describe.
fn identify(metadata: &host::WheelMetadata, manifest: &Manifest, root: &Path) -> Result<()> {
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
    requires_what_it_declares(metadata, manifest, root)?;
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
fn unapproved(config: &pnpm_config::Config, requires: &[pep508_rs::Requirement]) -> Vec<String> {
    if config.dangerously_allow_all_builds {
        return Vec::new();
    }
    let mut names = requires
        .iter()
        .map(|requirement| requirement.name.to_string())
        // A Python version is not a semver range, so only the name half of
        // an `allowBuilds` key decides a Python build.
        .filter(|name| config.allow_builds.get(name) != Some(&true))
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

/// Refuse a wheel that requires a distribution its project does not
/// declare. Resolution answered with what the manifest requires, so such
/// a wheel would be installed without it.
fn requires_what_it_declares(
    metadata: &host::WheelMetadata,
    manifest: &Manifest,
    root: &Path,
) -> Result<()> {
    let declared = manifest
        .distribution_requirements()?
        .iter()
        .map(|requirement| Ok(parse_requirement(requirement)?.name))
        .collect::<Result<BTreeSet<_>>>()?;
    for requirement in &metadata.requires_dist {
        let required = parse_requirement(requirement)?.name;
        if !declared.contains(&required) {
            let manifest_path = root.join("pyproject.toml");
            let manifest_path = manifest_path.display();
            bail!(
                "the wheel built from the Python project at {} requires `{required}`, which \
                 {manifest_path} does not declare",
                root.display(),
            );
        }
    }
    Ok(())
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
