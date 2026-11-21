//! `pnpr grants export` and `pnpr grants import`: the grants the database
//! stores, as YAML, read and written through the configured database without
//! a running server.

use crate::{
    Config,
    GrantsSubcommand,
    RegistryError,
    audit::Actor,
    server::validate_grant,
};
use indexmap::IndexMap;
use pnpm_diagnostics::{
    print,
    println,
};
use pnpr_storage::{
    Storage,
    database::{
        GrantContent,
        GrantWrite,
        StoredGrant,
    },
};
use serde::{
    Deserialize,
    Serialize,
};
use std::path::Path;

/// The file `export` writes and `import` reads.
#[derive(Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantsFile {
    /// Each grant by name.
    #[serde(default)]
    grants: IndexMap<String, GrantFile>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantFile {
    registries: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    include: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    exclude: Vec<String>,
    /// Each subject with the actions it is given.
    #[serde(default)]
    subjects: IndexMap<String, Vec<String>>,
}

impl GrantFile {
    fn of(content: &GrantContent) -> Self {
        let mut subjects: IndexMap<String, Vec<String>> = IndexMap::new();
        for (subject, action) in &content.subjects {
            subjects
                .entry(subject.clone())
                .or_default()
                .push(action.clone());
        }
        Self {
            registries: content.registries.clone(),
            include: content.include.clone(),
            exclude: content.exclude.clone(),
            subjects,
        }
    }

    /// The grant named `name`, its lists sorted the way the database returns
    /// them, so a file compares equal to what it stored.
    fn into_content(self, name: String) -> GrantContent {
        let sorted = |mut list: Vec<String>| {
            list.sort();
            list.dedup();
            list
        };
        let mut subjects: Vec<(String, String)> = self.subjects
            .into_iter()
            .flat_map(|(subject, actions)| {
                actions
                    .into_iter()
                    .map(move |action| (subject.clone(), action))
            })
            .collect();
        subjects.sort();
        subjects.dedup();
        GrantContent {
            name,
            registries: sorted(self.registries),
            include: sorted(self.include),
            exclude: sorted(self.exclude),
            subjects,
        }
    }
}

pub(crate) async fn run(config: &Config, action: GrantsSubcommand) -> miette::Result<()> {
    let result = match action {
        GrantsSubcommand::Export { out } => export(config, out.as_deref()).await.map(|()| false),
        GrantsSubcommand::Import { file, dry_run, check } => {
            let mode = match (dry_run, check) {
                (_, true) => ImportMode::Check,
                (true, false) => ImportMode::DryRun,
                (false, false) => ImportMode::Store,
            };
            import(config, &file, mode).await
        }
    };
    match result {
        Ok(false) => Ok(()),
        Ok(true) => Err(miette::miette!("the stored grants differ from the file")),
        Err(err) => {
            let message = err.log_message();
            Err(miette::miette!("{message}"))
        }
    }
}

/// What `pnpr grants import` does with a checked file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportMode {
    Store,
    /// Report what storing it would change.
    DryRun,
    /// Report what differs, and fail when anything does.
    Check,
}

async fn export(config: &Config, out: Option<&Path>) -> Result<(), RegistryError> {
    let storage = Storage::from_config(&config.storage)?;
    let stored = storage
        .read_grants(None)
        .await?
        .map(|stored| stored.grants)
        .unwrap_or_default();
    let file = GrantsFile {
        grants: stored
            .iter()
            .map(|grant| (grant.content.name.clone(), GrantFile::of(&grant.content)))
            .collect(),
    };
    let yaml = serde_saphyr::to_string(&file)
        .map_err(|err| RegistryError::Internal { reason: err.to_string() })?;
    if let Some(path) = out {
        tokio::fs::write(path, yaml).await?;
    } else {
        print!("{yaml}");
    }
    Ok(())
}

/// Reports whether [`ImportMode::Check`] found the stored grants differ.
async fn import(config: &Config, file: &Path, mode: ImportMode) -> Result<bool, RegistryError> {
    let declared = read_file(file).await?;
    let storage = Storage::from_config(&config.storage)?;
    let stored = storage.read_grants(None).await?;
    let generation = stored.as_ref().map(|stored| stored.generation);
    let stored = stored.map(|stored| stored.grants).unwrap_or_default();
    let next = declared_grants(declared, &stored);
    let changes = describe(&stored, &next);
    if changes.is_empty() {
        println!("The stored grants already match the file.");
        return Ok(false);
    }
    if mode == ImportMode::Check {
        println!("Would change: {changes}");
        return Ok(true);
    }
    for grant in &next {
        validate_grant(&config.routing, &grant.content)
            .map_err(|err| RegistryError::BadRequest {
                reason: format!("grant {:?}: {}", grant.content.name, err.log_message()),
            })?;
    }
    if mode == ImportMode::DryRun {
        println!("Would change: {changes}");
        return Ok(false);
    }
    store(&storage, next, generation).await?;
    println!("Stored the grants: {changes}");
    Ok(false)
}

async fn read_file(file: &Path) -> Result<GrantsFile, RegistryError> {
    let raw = tokio::fs::read_to_string(file).await
        .map_err(|err| RegistryError::BadRequest {
            reason: format!("read {}: {err}", file.display()),
        })?;
    Ok(serde_saphyr::from_str::<Option<GrantsFile>>(&raw)
        .map_err(|err| RegistryError::BadRequest { reason: err.to_string() })?
        .unwrap_or_default())
}

/// The grants `declared` declares, each under the ID the stored grant of its
/// name has, or a new one.
fn declared_grants(declared: GrantsFile, stored: &[StoredGrant]) -> Vec<StoredGrant> {
    declared.grants
        .into_iter()
        .map(|(name, grant)| {
            let id = stored
                .iter()
                .find(|existing| existing.content.name == name)
                .map_or_else(pnpr_auth::new_principal_id, |existing| existing.id.clone());
            StoredGrant { id, content: grant.into_content(name) }
        })
        .collect()
}

async fn store(
    storage: &Storage,
    grants: Vec<StoredGrant>,
    generation: Option<u64>,
) -> Result<(), RegistryError> {
    match storage.replace_grants(grants, generation).await? {
        GrantWrite::Written(version) => {
            crate::audit!(Actor::NONE, "grants.import", version = %version, "grants imported");
            Ok(())
        }
        GrantWrite::Conflict { .. } => Err(RegistryError::AdminConflict {
            reason: "the grants changed during the import; run it again".to_string(),
        }),
        GrantWrite::UnknownTeam(id) => Err(RegistryError::BadRequest {
            reason: format!("a grant names `team:{id}`, and no team has that ID"),
        }),
        GrantWrite::UnknownRegistry(registry) => Err(RegistryError::BadRequest {
            reason: format!("a grant names the registry {registry:?}, which does not exist"),
        }),
        GrantWrite::NameTaken | GrantWrite::NotFound => {
            Err(RegistryError::Internal { reason: "the grants could not be replaced".to_string() })
        }
    }
}

/// The names of the grants that `next` adds to, removes from, and changes in
/// `stored`, or nothing when they hold the same.
fn describe(stored: &[StoredGrant], next: &[StoredGrant]) -> String {
    let find = |grants: &[StoredGrant], name: &str| {
        grants
            .iter()
            .find(|grant| grant.content.name == name)
            .map(|grant| grant.content.clone())
    };
    let names = |grants: Vec<&StoredGrant>| {
        grants
            .iter()
            .map(|grant| grant.content.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    let added: Vec<_> = next
        .iter()
        .filter(|grant| find(stored, &grant.content.name).is_none())
        .collect();
    let removed: Vec<_> = stored
        .iter()
        .filter(|grant| find(next, &grant.content.name).is_none())
        .collect();
    let changed: Vec<_> = next
        .iter()
        .filter(|grant| {
            find(stored, &grant.content.name).is_some_and(|existing| existing != grant.content)
        })
        .collect();
    [("added", added), ("removed", removed), ("changed", changed)]
        .into_iter()
        .filter(|(_, grants)| !grants.is_empty())
        .map(|(label, grants)| format!("{label} {}", names(grants)))
        .collect::<Vec<_>>()
        .join("; ")
}
