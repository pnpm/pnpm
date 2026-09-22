use super::{
    BuildEnvironment,
    BuiltWheel,
    PythonPrepare,
    backend,
    host,
    interpreter,
    unapproved,
    validation::identify_identity,
};
use crate::{
    environment::Shared,
    interpreter::Interpreters,
    manifest::Manifest,
    targets::Environments,
    workspace::members::Membership,
};
use miette::{
    IntoDiagnostic,
    Result,
    WrapErr,
    bail,
};
use std::{
    collections::{
        BTreeMap,
        BTreeSet,
    },
    path::{
        Path,
        PathBuf,
    },
    sync::Arc,
};

impl PythonPrepare<'_> {
    pub(in super::super) async fn metadata<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        root: &Path,
        manifest: &Manifest,
    ) -> Result<(host::WheelMetadata, Option<Arc<tempfile::TempDir>>, Option<Arc<FallbackWheel>>)>
    {
        let requires = self.build_requirements(root, manifest)?;
        let unapproved = unapproved(self.context.config, &requires);
        if !unapproved.is_empty() {
            bail!(
                "dynamic Python metadata at {} needs approved build requirements: {}. Configure allowBuilds before running the backend",
                root.display(),
                unapproved.join(", "),
            );
        }
        let request = serde_json::json!({
            "root": root, "backend": backend(manifest).module,
            "backend_path": backend(manifest).path, "editable": false,
        });
        let environment = match self.build_environment::<Reporter>(root, requires, &request).await?
        {
            BuildEnvironment::Ready(environment) => environment,
            BuildEnvironment::NotApproved(names) => {
                bail!(
                    "dynamic Python metadata at {} needs approved build requirements: {}",
                    root.display(),
                    names.join(", "),
                );
            }
        };
        let output = tempfile::tempdir().into_diagnostic()?;
        let mut request = request;
        request["output"] = serde_json::json!(output.path());
        let response: BackendMetadata =
            host::run(&interpreter(environment.path()), "metadata", request)
                .await
                .wrap_err_with(|| format!("prepare Python metadata at {}", root.display()))?;
        validate_metadata(&response.metadata, manifest, root)
            .wrap_err_with(|| format!("validate Python metadata at {}", root.display()))?;
        Ok(response.retain(output, &self.interpreter.executable))
    }
}

#[derive(serde::Deserialize)]
struct BackendMetadata {
    metadata: host::WheelMetadata,
    prepared: bool,
    wheel: Option<BuiltWheel>,
}

impl BackendMetadata {
    fn retain(
        self,
        output: tempfile::TempDir,
        interpreter: &str,
    ) -> (host::WheelMetadata, Option<Arc<tempfile::TempDir>>, Option<Arc<FallbackWheel>>) {
        let output = Arc::new(output);
        let fallback = self.wheel.map(|wheel| {
            Arc::new(FallbackWheel {
                wheel,
                output: Arc::clone(&output),
                interpreter: interpreter.to_owned(),
            })
        });
        (self.metadata, self.prepared.then_some(output), fallback)
    }
}

fn validate_metadata(
    metadata: &host::WheelMetadata,
    manifest: &Manifest,
    root: &Path,
) -> Result<()> {
    identify_identity(metadata, manifest, root)?;
    manifest.validate_static_metadata(metadata)?;
    Ok(())
}

/// Which projects an install reads, and which of them install together.
pub(in super::super) struct MetadataScope<'a> {
    pub(in super::super) needed: &'a BTreeSet<PathBuf>,
    pub(in super::super) memberships: &'a [Membership],
}

/// One project whose dynamic metadata is prepared: where it is, and its
/// manifest as written, which is what the backend reads.
struct Preparing<'a> {
    root: &'a Path,
    original: &'a Manifest,
}

impl Shared<'_> {
    /// Prepare the dynamic metadata of the projects this install reads,
    /// and report the interpreter each one was prepared with, keyed by
    /// the directory that installs it. `scope.needed` names those
    /// projects: a project the selection left out, and that no selected
    /// project reaches, is not built to find out what it declares. The
    /// members of a shared environment are prepared with the interpreter
    /// they share, keyed by their workspace root.
    pub(in super::super) async fn prepare_metadata<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        projects: &mut [(PathBuf, Arc<Manifest>)],
        interpreters: &mut Interpreters<'_>,
        scope: MetadataScope<'_>,
    ) -> Result<BTreeMap<PathBuf, Arc<host::Interpreter>>> {
        let mut selected = BTreeMap::new();
        let mut together = BTreeSet::new();
        for membership in scope.memberships.iter().filter(|membership| membership.shared) {
            together.extend(membership.members.iter().cloned());
            // Boxed to bound the nesting of the metadata futures, which
            // the compiler otherwise lays out past its recursion limit.
            let shared =
                Box::pin(self.shared_metadata::<Reporter>(projects, interpreters, membership));
            if let Some(interpreter) = shared.await? {
                selected.insert(membership.root.clone(), interpreter);
            }
        }
        for (root, manifest) in projects {
            if manifest.needs_metadata() && scope.needed.contains(root) && !together.contains(root)
            {
                let interpreter =
                    self.project_metadata::<Reporter>(root, manifest, interpreters).await?;
                selected.insert(root.clone(), interpreter);
            }
        }
        Ok(selected)
    }

    async fn project_metadata<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        root: &Path,
        manifest: &mut Arc<Manifest>,
        interpreters: &mut Interpreters<'_>,
    ) -> Result<Arc<host::Interpreter>> {
        let original = Arc::clone(manifest);
        let mut interpreter = interpreters.select::<Reporter>(root, &original).await?;
        for _ in 0..3 {
            let declared = requires_python(manifest);
            self.prepare_with::<Reporter>(
                &interpreter,
                Preparing { root, original: &original },
                manifest,
            )
            .await?;
            if requires_python(manifest) == declared {
                return Ok(interpreter);
            }
            let selected = interpreters.select::<Reporter>(root, manifest).await?;
            if Arc::ptr_eq(&interpreter, &selected) {
                return Ok(interpreter);
            }
            interpreter = selected;
        }
        bail!("dynamic Python metadata at {} did not select a stable interpreter", root.display())
    }

    /// Prepare the metadata of a shared membership's members with the
    /// interpreter they share, or `None` when none of them has any to
    /// prepare. Metadata may declare another interpreter range, so the
    /// selection is repeated until the range every member accepts is
    /// stable, as it is for a project on its own.
    async fn shared_metadata<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        projects: &mut [(PathBuf, Arc<Manifest>)],
        interpreters: &mut Interpreters<'_>,
        membership: &Membership,
    ) -> Result<Option<Arc<host::Interpreter>>> {
        let Some(members) = Members::needing_metadata(projects, membership) else {
            return Ok(None);
        };
        let root = membership.root.as_path();
        let mut interpreter = members.select::<Reporter>(interpreters, root, projects).await?;
        for _ in 0..3 {
            let declared = members.ranges(projects);
            members.prepare::<Reporter>(self, &interpreter, projects).await?;
            if members.ranges(projects) == declared {
                return Ok(Some(interpreter));
            }
            let selected = members.select::<Reporter>(interpreters, root, projects).await?;
            if Arc::ptr_eq(&interpreter, &selected) {
                return Ok(Some(interpreter));
            }
            interpreter = selected;
        }
        bail!(
            "dynamic Python metadata in the workspace at {} did not select a stable interpreter",
            root.display(),
        )
    }

    /// Prepare one project's metadata with `interpreter` and record it on
    /// the manifest, with the wheel the backend may have built on the way.
    async fn prepare_with<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        interpreter: &Arc<host::Interpreter>,
        preparing: Preparing<'_>,
        manifest: &mut Arc<Manifest>,
    ) -> Result<()> {
        let Preparing { root, original } = preparing;
        let environments = Environments::of(self.context.config, interpreter)?;
        let (metadata, output, fallback) =
            PythonPrepare::for_project(self, interpreter, &environments)
                .metadata::<Reporter>(root, original)
                .await?;
        Arc::make_mut(manifest)
            .set_metadata(metadata, output)
            .wrap_err_with(|| format!("metadata for {}", root.display()))?;
        Arc::make_mut(manifest).metadata_wheel = fallback;
        Ok(())
    }
}

/// The members of one shared membership among the discovered projects,
/// by position, with each one's manifest as written.
struct Members {
    positions: Vec<usize>,
    originals: Vec<Arc<Manifest>>,
}

impl Members {
    /// The members of `membership`, when any of them has metadata to
    /// prepare.
    fn needing_metadata(
        projects: &[(PathBuf, Arc<Manifest>)],
        membership: &Membership,
    ) -> Option<Self> {
        let positions = projects
            .iter()
            .enumerate()
            .filter(|(_, (root, _))| membership.members.contains(root))
            .map(|(position, _)| position)
            .collect::<Vec<_>>();
        let originals = positions
            .iter()
            .map(|&position| Arc::clone(&projects[position].1))
            .collect::<Vec<_>>();
        originals
            .iter()
            .any(|manifest| manifest.needs_metadata())
            .then_some(Self { positions, originals })
    }

    /// The interpreter range each member declares now.
    fn ranges(&self, projects: &[(PathBuf, Arc<Manifest>)]) -> Vec<Option<String>> {
        self.positions
            .iter()
            .map(|&position| requires_python(&projects[position].1))
            .collect()
    }

    /// The interpreter the members share: one every member's
    /// `requires-python` accepts, preferring what a `.python-version` at
    /// or above `root` asks for.
    async fn select<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        interpreters: &mut Interpreters<'_>,
        root: &Path,
        projects: &[(PathBuf, Arc<Manifest>)],
    ) -> Result<Arc<host::Interpreter>> {
        let range = crate::interpreter::requires_python_of(
            self.positions
                .iter()
                .map(|&position| (projects[position].0.as_path(), &*projects[position].1)),
        )?;
        interpreters.select_accepting::<Reporter>(root, range.as_ref()).await
    }

    /// Prepare the metadata of every member that has any, with
    /// `interpreter`.
    async fn prepare<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        shared: &Shared<'_>,
        interpreter: &Arc<host::Interpreter>,
        projects: &mut [(PathBuf, Arc<Manifest>)],
    ) -> Result<()> {
        for (&position, original) in self.positions.iter().zip(&self.originals) {
            if original.needs_metadata() {
                let (root, manifest) = &mut projects[position];
                shared.prepare_with::<Reporter>(
                    interpreter,
                    Preparing { root, original },
                    manifest,
                )
                .await?;
            }
        }
        Ok(())
    }
}

fn requires_python(manifest: &Manifest) -> Option<String> {
    manifest.project.as_ref().and_then(|project| project.requires_python.clone())
}

pub(in super::super) struct FallbackWheel {
    pub(super) wheel: BuiltWheel,
    pub(super) output: Arc<tempfile::TempDir>,
    pub(super) interpreter: String,
}
