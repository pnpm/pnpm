//! What the registry graph means for the grants: a graph write seeds and
//! forgets grants with the registries it adds and removes, and a grant names
//! only registries the stored graph has.

use super::{
    GrantContent,
    StoredGrant,
    advance_directory,
    delete_grant,
    insert_grant,
    name_owner,
};
use crate::database::sql::{
    adapter::Connection,
    registry_graph,
};
use pnpr_error::{
    RegistryError,
    Result,
};

/// What a graph write changes in the grants: the registries it removes leave
/// every grant, and the registries it adds get their default grants.
pub(in crate::database::sql) fn follow_graph(
    connection: &dyn Connection,
    removed: &[String],
    seeded: &[StoredGrant],
) -> Result<()> {
    forget_registries(connection, removed)?;
    seed_grants(connection, seeded)
}

/// Create each of `grants` unless a grant of its name already covers its
/// registries. A graph write seeds a registry it adds this way, in its
/// transaction. A name another grant has is not a reason to leave the
/// registry without its grant, so the grant then takes the next free name.
fn seed_grants(connection: &dyn Connection, grants: &[StoredGrant]) -> Result<()> {
    let mut seeded = false;
    for grant in grants {
        let Some(name) = seeded_name(connection, &grant.content)? else {
            continue;
        };
        insert_grant(connection, &grant.id, &GrantContent { name, ..grant.content.clone() })?;
        seeded = true;
    }
    if seeded {
        advance_directory(connection)?;
    }
    Ok(())
}

/// The name to seed `content` under, or `None` when the grant of its name
/// already covers one of its registries.
fn seeded_name(connection: &dyn Connection, content: &GrantContent) -> Result<Option<String>> {
    let Some(owner) = name_owner(connection, &content.name)? else {
        return Ok(Some(content.name.clone()));
    };
    for registry in &content.registries {
        let covered = connection.query_optional(
            "SELECT 1 FROM grant_registries
             WHERE grant_id = ?1 AND (registry = ?2 OR registry = '*')",
            params![&owner, registry],
        )?;
        if covered.is_some() {
            return Ok(None);
        }
    }
    let mut suffix = 2_u32;
    loop {
        let name = format!("{}-{suffix}", content.name);
        if name_owner(connection, &name)?.is_none() {
            return Ok(Some(name));
        }
        suffix += 1;
    }
}

/// Take each of `registries` out of every grant, and remove the grants left
/// without a registry. A graph write does this for the registries it removes,
/// so a registry added later under the same key starts without their grants.
fn forget_registries(connection: &dyn Connection, registries: &[String]) -> Result<()> {
    let mut changed = false;
    for registry in registries {
        let named = connection.query_optional(
            "SELECT 1 FROM grant_registries WHERE registry = ?1 LIMIT 1",
            params![registry],
        )?;
        if named.is_none() {
            continue;
        }
        connection.execute("DELETE FROM grant_registries WHERE registry = ?1", params![registry])?;
        changed = true;
    }
    if !changed {
        return Ok(());
    }
    let orphans = connection.query(
        "SELECT grant_id FROM grants
         WHERE grant_id NOT IN (SELECT grant_id FROM grant_registries)",
        params![],
    )?;
    for mut orphan in orphans {
        delete_grant(connection, &orphan.get::<String>(0)?)?;
    }
    advance_directory(connection).map(drop)
}

/// The keys of the registries the stored graph declares, read in the
/// transaction of a grant write: a graph write takes the grants of the
/// registries it removes, so a grant written after it must not name one.
/// `None` when no graph is stored and the configuration declares the registries.
pub(super) fn stored_registries(connection: &dyn Connection) -> Result<Option<Vec<String>>> {
    let Some(document) = registry_graph::current_document(connection)? else {
        return Ok(None);
    };
    let document: serde_json::Value = serde_json::from_str(&document)
        .map_err(|err| RegistryError::Internal {
            reason: format!("the stored registry graph is unreadable: {err}"),
        })?;
    let registries = document.get("registries").and_then(serde_json::Value::as_object);
    Ok(Some(
        registries
            .into_iter()
            .flat_map(|registries| registries.keys().cloned())
            .collect(),
    ))
}

/// The first of `named` that is neither `*` nor one of `stored`.
pub(super) fn unknown_registry(stored: Option<&Vec<String>>, named: &[String]) -> Option<String> {
    let stored = stored?;
    named
        .iter()
        .find(|registry| *registry != "*" && !stored.contains(registry))
        .cloned()
}
