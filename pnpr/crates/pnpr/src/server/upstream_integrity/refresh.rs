use super::{AppState, CanonicalPackageName, RegistryError, packument_tarball_filename};
use serde_json::Value;
use std::collections::HashSet;

enum BlobEviction {
    Package,
    Files(HashSet<String>),
}

pub(in crate::server) async fn refresh_cached_packument(
    state: &AppState,
    namespace: &str,
    name: &CanonicalPackageName,
    bytes: Vec<u8>,
) -> Result<Vec<u8>, RegistryError> {
    let _guard = super::lock_upstream_package(state, namespace, name).await;
    let previous = state.inner.storage.read_upstream_document_any(namespace, name).await?;
    let package = name.clone();
    let (bytes, removed) =
        state.inner.packuments.workers.run(move || reconcile(previous.as_deref(), bytes, &package))
            .await??;
    // Evict before publishing the new document so a failed deletion cannot be forgotten.
    match removed {
        BlobEviction::Package => {
            state.inner.storage.remove_upstream_package(namespace, name).await?;
        }
        BlobEviction::Files(filenames) => {
            for filename in filenames {
                state.inner.storage.remove_upstream_blob(namespace, name, &filename).await?;
            }
        }
    }
    state.inner.storage.write_upstream_document(namespace, name, &bytes).await?;
    Ok(bytes)
}

fn reconcile(
    previous: Option<&[u8]>,
    bytes: Vec<u8>,
    name: &CanonicalPackageName,
) -> Result<(Vec<u8>, BlobEviction), RegistryError> {
    let mut current: Value = serde_json::from_slice(&bytes)?;
    let Some(previous) = previous else {
        return Ok((bytes, BlobEviction::Files(HashSet::new())));
    };
    let Ok(previous) = serde_json::from_slice::<Value>(previous) else {
        return Ok((bytes, BlobEviction::Package));
    };
    let mut removed = HashSet::new();
    let Some(versions) = previous.get("versions").and_then(Value::as_object) else {
        return Ok((bytes, BlobEviction::Files(removed)));
    };
    for (version, old) in versions {
        let new = current
            .get_mut("versions")
            .and_then(|versions| versions.get_mut(version));
        if let Some(new) = new.filter(|new| same_tarball(old, new)) {
            carry_integrity(old, new);
        } else if let Some(url) = old.pointer("/dist/tarball").and_then(Value::as_str)
            && let Ok(filename) = packument_tarball_filename(url, name, version)
        {
            removed.insert(filename);
        }
    }
    Ok((serde_json::to_vec(&current)?, BlobEviction::Files(removed)))
}

fn same_tarball(old: &Value, new: &Value) -> bool {
    old.pointer("/dist/tarball").is_some()
        && old.pointer("/dist/tarball") == new.pointer("/dist/tarball")
        && old.pointer("/dist/shasum") == new.pointer("/dist/shasum")
        && (new.pointer("/dist/integrity").is_none_or(missing_integrity)
            || old.pointer("/dist/integrity") == new.pointer("/dist/integrity"))
}

fn missing_integrity(value: &Value) -> bool {
    value.is_null()
        || value
            .as_str()
            .is_some_and(|value| {
                value
                    .parse::<ssri::Integrity>()
                    .is_ok_and(|integrity| integrity.hashes.is_empty())
            })
}

fn carry_integrity(old: &Value, new: &mut Value) {
    let Some(integrity) = old.pointer("/dist/integrity").and_then(Value::as_str) else {
        return;
    };
    if integrity
        .parse::<ssri::Integrity>()
        .is_ok_and(|integrity| !integrity.hashes.is_empty())
    {
        new["dist"]["integrity"] = Value::String(integrity.to_string());
    }
}

#[cfg(test)]
mod tests;
