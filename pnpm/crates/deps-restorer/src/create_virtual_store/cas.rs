use super::{CreateVirtualStore, CreateVirtualStoreError, CreateVirtualStoreOutput};
use pnpm_config::NodeLinker;
use pnpm_lockfile::{PackageKey, SnapshotEntry};
use pnpm_reporter::Reporter;
use std::collections::{HashMap, HashSet};

impl CreateVirtualStore<'_> {
    pub async fn run<Report: Reporter>(
        mut self,
    ) -> Result<CreateVirtualStoreOutput, CreateVirtualStoreError> {
        if self.ctx.linker.kind == NodeLinker::Cas {
            return Box::pin(self.run_cas::<Report>()).await;
        }
        self.run_inner::<Report>().await
    }

    pub(super) async fn run_cas<Report: Reporter>(
        self,
    ) -> Result<CreateVirtualStoreOutput, CreateVirtualStoreError> {
        validate_config(self.ctx.config)?;
        let selected = materialized_snapshots(self.ctx.config, self.entries.snapshots);
        let fetch_context = crate::InstallContext {
            linker: crate::ModuleLinkerContext { kind: NodeLinker::Hoisted, ..self.ctx.linker },
            ..self.ctx.clone()
        };
        let materialize_context = crate::InstallContext {
            linker: crate::ModuleLinkerContext { kind: NodeLinker::Isolated, ..self.ctx.linker },
            ..self.ctx.clone()
        };
        let mut installer = CreateVirtualStore { ctx: &fetch_context, ..self };
        let fetched = installer.run_inner::<Report>().await?;
        validate_cas_builds(&fetched, &selected, installer.ctx.config)?;
        installer.ctx = &materialize_context;
        installer.entries.snapshots = (!selected.is_empty()).then_some(&selected);
        let mut materialized = installer.run_inner::<Report>().await?;
        materialized.cas_paths_by_pkg_id = fetched.cas_paths_by_pkg_id;
        materialized.package_manifests.extend(fetched.package_manifests);
        materialized.fetch_failed.extend(fetched.fetch_failed);
        Ok(materialized)
    }
}

pub(crate) fn materialized_snapshots(
    config: &pnpm_config::Config,
    snapshots: Option<&HashMap<PackageKey, SnapshotEntry>>,
) -> HashMap<PackageKey, SnapshotEntry> {
    let Some(snapshots) = snapshots else { return HashMap::new() };
    let mut pending: Vec<_> = snapshots
        .keys()
        .filter(|key| {
            config.cas_materialize.contains(&key.name.to_string()) || crate::snapshot_has_patch(key)
        })
        .cloned()
        .collect();
    let mut selected = HashSet::new();
    while let Some(key) = pending.pop() {
        if !selected.insert(key.clone()) {
            continue;
        }
        let Some(snapshot) = snapshots.get(&key) else { continue };
        pending.extend(
            snapshot.dependencies
                .iter()
                .flatten()
                .chain(snapshot.optional_dependencies.iter().flatten())
                .filter_map(|(alias, reference)| reference.resolve(alias)),
        );
    }
    snapshots
        .iter()
        .filter(|(key, _)| selected.contains(*key))
        .map(|(key, snapshot)| (key.clone(), snapshot.clone()))
        .collect()
}

fn validate_cas_builds(
    fetched: &CreateVirtualStoreOutput,
    selected: &HashMap<PackageKey, SnapshotEntry>,
    config: &pnpm_config::Config,
) -> Result<(), CreateVirtualStoreError> {
    if config.ignore_scripts {
        return Ok(());
    }
    for (key, requires_build) in &fetched.requires_build_by_snapshot {
        if *requires_build && !selected.contains_key(key) {
            return Err(CreateVirtualStoreError::CasRequiresMaterialization {
                package: key.name.to_string(),
            });
        }
    }
    Ok(())
}

fn validate_config(config: &pnpm_config::Config) -> Result<(), CreateVirtualStoreError> {
    let message = if !config.enable_global_virtual_store {
        Some("CAS opt-outs require the global virtual store")
    } else if !config.symlink {
        Some("CAS opt-outs require symlink=true for their dependency trees")
    } else if config.node_experimental_package_map {
        Some("nodeExperimentalPackageMap cannot be combined with the CAS loader")
    } else {
        None
    };
    match message {
        Some(message) => Err(CreateVirtualStoreError::CasConfiguration { message }),
        None => Ok(()),
    }
}
