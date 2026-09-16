use super::{
    BuildEnvironment, PythonPrepare, backend, host, identify_identity, interpreter, unapproved,
};
use crate::{
    environment::Shared, interpreter::Interpreters, manifest::Manifest, targets::Environments,
};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

impl PythonPrepare<'_> {
    pub(in super::super) async fn metadata<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        root: &Path,
        manifest: &Manifest,
    ) -> Result<(host::WheelMetadata, Option<Arc<tempfile::TempDir>>)> {
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
        let output = response.prepared.then(|| Arc::new(output));
        Ok((response.metadata, output))
    }
}

#[derive(serde::Deserialize)]
struct BackendMetadata {
    metadata: host::WheelMetadata,
    prepared: bool,
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

impl Shared<'_> {
    pub(in super::super) async fn prepare_metadata<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        projects: &mut [(PathBuf, Arc<Manifest>)],
        interpreters: &mut Interpreters<'_>,
    ) -> Result<BTreeMap<PathBuf, Arc<host::Interpreter>>> {
        let mut selected = BTreeMap::new();
        for (root, manifest) in projects {
            if manifest.needs_metadata() {
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
            let requires_python = manifest.project
                .as_ref()
                .and_then(|project| project.requires_python.clone());
            let environments = Environments::of(self.context.config, &interpreter)?;
            let (metadata, output) = PythonPrepare::for_project(self, &interpreter, &environments)
                .metadata::<Reporter>(root, &original)
                .await?;
            Arc::make_mut(manifest)
                .set_metadata(metadata, output)
                .wrap_err_with(|| format!("metadata for {}", root.display()))?;
            if manifest.project
                .as_ref()
                .and_then(|project| project.requires_python.clone())
                == requires_python
            {
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
}
