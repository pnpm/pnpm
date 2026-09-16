use super::environment::{LockfileInputs, PythonPrepare};
use miette::{IntoDiagnostic, Result};
use pnpm_python_resolver::Lockfile;
use pnpm_reporter::Reporter;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::Mutex;

type Entry = Arc<Mutex<Option<String>>>;

/// Fresh registry resolutions shared within one install. Each project still
/// checks its own lockfile before consulting this cache.
#[derive(Default)]
pub(super) struct Resolutions(Mutex<BTreeMap<String, Entry>>);

impl PythonPrepare<'_> {
    pub(super) async fn shared_lockfile<Reporter: self::Reporter + 'static>(
        &self,
        registry: &mut super::Registry<'_>,
        inputs: LockfileInputs<'_>,
    ) -> Result<Lockfile> {
        let Some(key) = self.resolution_key(&inputs)? else {
            return self.lockfile::<Reporter>(registry, inputs).await;
        };
        let entry = Arc::clone(
            self.state.caches.resolutions.0
                .lock()
                .await
                .entry(key)
                .or_default(),
        );
        let mut cached = entry.lock().await;
        if let Some(contents) = cached.as_deref() {
            let lock = toml::from_str(contents).into_diagnostic()?;
            if self.context.lockfile_only {
                return Ok(lock);
            }
            drop(cached);
            return self.accept_lockfile::<Reporter>(
                registry,
                lock,
                inputs.requirements,
                &inputs.local,
            )
            .await;
        }
        let lock = self.lockfile::<Reporter>(registry, inputs).await?;
        *cached = Some(toml::to_string(&lock).into_diagnostic()?);
        Ok(lock)
    }

    fn resolution_key(&self, inputs: &LockfileInputs<'_>) -> Result<Option<String>> {
        if inputs.existing.is_some()
            || !inputs.local.is_empty()
            || inputs.requirements
                .iter()
                .any(|requirement| {
                    matches!(requirement.version_or_url, Some(pep508_rs::VersionOrUrl::Url(_)))
                })
        {
            return Ok(None);
        }
        serde_json::to_string(&(&inputs.inputs, &inputs.requires_python, self.interpreter))
            .into_diagnostic()
            .map(Some)
    }
}
