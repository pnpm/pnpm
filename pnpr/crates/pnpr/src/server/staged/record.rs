use super::{Identity, StagedRecord, ValidatedPublish, Value};
use pnpr_storage::publish::now_iso;

pub(super) fn staged_record(
    validated: &ValidatedPublish,
    identity: &Identity,
    registry: Option<&str>,
    stage_id: &str,
) -> StagedRecord {
    let (version, dist) = validated.prepared
        .first()
        .map_or((None, Value::Null), |attachment| {
            (Some(attachment.version.clone()), attachment.dist.clone())
        });
    let (actor, actor_type) = actor_of(identity);
    StagedRecord {
        id: stage_id.to_string(),
        package_name: validated.name.as_str().to_string(),
        tag: staged_tag(&validated.incoming, version.as_deref()),
        version,
        created_at: now_iso(),
        actor,
        actor_type,
        shasum: dist
            .get("shasum")
            .and_then(Value::as_str)
            .map(str::to_string),
        registry: registry.map(str::to_string),
        approving_since: None,
    }
}

/// The dist-tag naming the staged version, else the first tag declared.
fn staged_tag(incoming: &Value, version: Option<&str>) -> Option<String> {
    let tags = incoming.get("dist-tags").and_then(Value::as_object)?;
    match version {
        Some(version) => tags
            .iter()
            .find(|(_, tagged)| tagged.as_str() == Some(version))
            .or_else(|| tags.iter().next())
            .map(|(tag, _)| tag.clone()),
        None => tags.keys().next().cloned(),
    }
}

fn actor_of(identity: &Identity) -> (String, String) {
    match identity {
        Identity::User { username, .. } => (username.clone(), "user".to_string()),
        // Reachable only when the registry's publish rule allows anonymous
        // writes; the record still needs an actor to display.
        Identity::Anonymous => ("anonymous".to_string(), "user".to_string()),
    }
}
