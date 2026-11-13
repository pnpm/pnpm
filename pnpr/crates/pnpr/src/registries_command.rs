//! `pnpr registries export` and `pnpr registries import`: the registry graph
//! the metadata database stores, as YAML, read and written through the
//! configured database without a running server.

use crate::{
    Config, RegistriesSubcommand, RegistryError,
    audit::Actor,
    server::{
        Credentials, HostedChange, check_revision, graph_version, occupied_namespace_error,
        stored_document,
    },
};
use pnpm_diagnostics::{print, println};
use pnpr_config::{RegistryGraphChanges, RegistryGraphDocument};
use pnpr_storage::{
    Storage,
    metadata::{NewRegistryGraph, RegistryGraphWrite},
};
use std::path::Path;

pub(crate) async fn run(config: &Config, action: RegistriesSubcommand) -> miette::Result<()> {
    let result = match action {
        RegistriesSubcommand::Export { out } => {
            export(config, out.as_deref()).await.map(|()| Imported::Done)
        }
        RegistriesSubcommand::Import { file, dry_run, check } => {
            let mode = match (dry_run, check) {
                (_, true) => ImportMode::Check,
                (true, false) => ImportMode::DryRun,
                (false, false) => ImportMode::Store,
            };
            import(config, &file, mode).await
        }
    };
    match result {
        Ok(Imported::Done) => Ok(()),
        Ok(Imported::Differs) => {
            Err(miette::miette!("the stored registries differ from the file",))
        }
        Err(err) => {
            let message = err.log_message();
            Err(miette::miette!("{message}"))
        }
    }
}

/// What `pnpr registries import` does with a checked file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportMode {
    /// Store it as the next revision.
    Store,
    /// Report what storing it would change.
    DryRun,
    /// Report what differs, and fail when anything does.
    Check,
}

/// The outcome of an import that did not fail.
enum Imported {
    Done,
    /// [`ImportMode::Check`] found the stored registries differ.
    Differs,
}

async fn export(config: &Config, out: Option<&Path>) -> Result<(), RegistryError> {
    let storage = Storage::from_config(&config.storage)?;
    let stored = storage.read_registry_graph().await?;
    let yaml = stored_document(stored.as_ref())?.to_yaml()?;
    if let Some(path) = out {
        tokio::fs::write(path, yaml).await?;
    } else {
        print!("{yaml}");
    }
    Ok(())
}

async fn import(config: &Config, file: &Path, mode: ImportMode) -> Result<Imported, RegistryError> {
    let next = read_document(file).await?;
    let storage = Storage::from_config(&config.storage)?;
    let stored = storage.read_registry_graph().await?;
    let current = stored.as_ref().map(|stored| stored.generation);
    let previous = stored_document(stored.as_ref())?;
    let checked = check_revision(config, &storage, &previous, &next, Credentials::Skip).await?;
    let changes = next.changes_since(&previous);
    if changes.is_empty() && mode != ImportMode::DryRun {
        println!("Version {} already holds these registries.", graph_version(current));
        return Ok(Imported::Done);
    }
    match mode {
        ImportMode::DryRun => println!("Would change: {}", describe(&changes)),
        ImportMode::Check => {
            println!("Would change: {}", describe(&changes));
            return Ok(Imported::Differs);
        }
        ImportMode::Store => {
            let import = Import { current, document: &next, hosted: &checked.hosted };
            let version = store_import(&storage, import).await?;
            println!("Stored version {version}: {}", describe(&changes));
        }
    }
    Ok(Imported::Done)
}

async fn read_document(file: &Path) -> Result<RegistryGraphDocument, RegistryError> {
    let raw = tokio::fs::read_to_string(file).await
        .map_err(|err| RegistryError::BadRequest {
            reason: format!("read {}: {err}", file.display()),
        })?;
    RegistryGraphDocument::from_yaml(&raw)
}

/// A checked document to store after the stored generation `current`.
struct Import<'a> {
    current: Option<u64>,
    document: &'a RegistryGraphDocument,
    hosted: &'a HostedChange,
}

/// Store `import` as the next revision, returning its version.
async fn store_import(storage: &Storage, import: Import<'_>) -> Result<String, RegistryError> {
    let document = serde_json::to_string(import.document)?;
    let revision = NewRegistryGraph {
        document: &document,
        actor_id: Actor::NONE.id,
        restored_from: None,
        vacated: &import.hosted.vacated,
        bound: &import.hosted.bound,
    };
    match storage.write_registry_graph(import.current, revision).await? {
        RegistryGraphWrite::Written(generation) => {
            let version = graph_version(Some(generation));
            let previous_version = graph_version(import.current);
            crate::audit!(
                Actor::NONE,
                "registries.import",
                version = %version,
                previous_version = %previous_version,
                "registries imported",
            );
            Ok(version)
        }
        RegistryGraphWrite::Conflict { .. } => Err(RegistryError::AdminConflict {
            reason: "the registries changed during the import; run it again".to_string(),
        }),
        RegistryGraphWrite::HostedNotEmpty(occupied) => Err(occupied_namespace_error(&occupied)),
    }
}

fn describe(changes: &RegistryGraphChanges) -> String {
    if changes.is_empty() {
        return "nothing".to_string();
    }
    let list = |label: &str, names: &[String]| {
        (!names.is_empty()).then(|| format!("{label} {}", names.join(", ")))
    };
    [
        list("added", &changes.added),
        list("removed", &changes.removed),
        list("changed", &changes.changed),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("; ")
}
