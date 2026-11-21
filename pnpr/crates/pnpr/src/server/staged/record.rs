use super::{
    Identity,
    STAGED_RECORD_FORMAT_VERSION,
    StagedRecord,
    ValidatedPublish,
    Value,
};
use crate::server::publishing::AttachmentKind;
use pnpr_storage::publish::now_iso;

pub(super) fn staged_record(
    validated: &ValidatedPublish,
    identity: &Identity,
    registry: Option<&str>,
    stage_id: String,
) -> StagedRecord {
    let (version, dist) = validated.prepared
        .iter()
        .find(|attachment| attachment.kind == AttachmentKind::Tarball)
        .map(|attachment| (Some(attachment.version.clone()), attachment.dist.clone()))
        .unwrap_or((None, Value::Null));
    let (actor, actor_type) = actor_of(identity);
    StagedRecord {
        format_version: STAGED_RECORD_FORMAT_VERSION,
        id: stage_id,
        package_name: validated.name.as_str().to_string(),
        tag: staged_tag(&validated.incoming, version.as_deref()),
        version,
        created_at: now_iso(),
        actor,
        actor_id: identity.user_id().map(str::to_owned),
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
        Identity::Anonymous | _ => ("anonymous".to_string(), "user".to_string()),
    }
}

impl StagedRecord {
    pub(super) fn record_approval(&self, validated: &mut ValidatedPublish) {
        for tarball in validated.prepared
            .iter()
            .filter(|entry| entry.kind == AttachmentKind::Tarball)
        {
            let Some(manifest) = validated.incoming["versions"]
                .get_mut(&tarball.version)
                .and_then(Value::as_object_mut)
            else {
                continue;
            };
            let approver = manifest.remove("_npmUser");
            let Some(id) = &self.actor_id else { continue };
            let mut user = serde_json::json!({ "name": self.actor, "userId": id });
            if let Some(approver) = approver
                && validated.publisher
                    .user_id()
                    .is_some_and(|approver_id| approver_id != id)
            {
                user["approver"] = approver;
            }
            manifest.insert("_npmUser".to_string(), user);
        }
    }
}
