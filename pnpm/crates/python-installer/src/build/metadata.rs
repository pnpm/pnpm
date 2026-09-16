use super::{
    BuildEnvironment, PythonPrepare, backend, host, identify_identity, interpreter,
    requires_what_it_declares, unapproved,
};
use crate::manifest::Manifest;
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use std::{path::Path, sync::Arc};

impl PythonPrepare<'_> {
    pub(in super::super) async fn metadata<Reporter: pnpm_reporter::Reporter + 'static>(
        &self,
        root: &Path,
        manifest: &Manifest,
    ) -> Result<(host::WheelMetadata, Arc<tempfile::TempDir>)> {
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
        let metadata: host::WheelMetadata =
            host::run(&interpreter(environment.path()), "metadata", request)
                .await
                .wrap_err_with(|| format!("prepare Python metadata at {}", root.display()))?;
        validate_metadata(&metadata, manifest, root)?;
        Ok((metadata, Arc::new(output)))
    }
}

fn validate_metadata(
    metadata: &host::WheelMetadata,
    manifest: &Manifest,
    root: &Path,
) -> Result<()> {
    identify_identity(metadata, manifest, root)?;
    if !manifest.project
        .as_ref()
        .is_some_and(|project| {
            project.dynamic
                .iter()
                .any(|field| {
                    matches!(
                        field.as_str(),
                        "dependencies" | "optional-dependencies" | "requires-python",
                    )
                })
        })
    {
        requires_what_it_declares(metadata, manifest, root)?;
    }
    Ok(())
}
