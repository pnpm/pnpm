use super::{AuthEntry, AuthKind, is_url_secure_for_credentials, nerf_dart, normalize_auth_key};
use std::collections::{HashMap, HashSet};

pub(super) fn split_scoped_auth_key(uri: &str) -> Option<(String, String)> {
    let trimmed = uri.strip_suffix('/').unwrap_or(uri);
    if let Some(scope_separator_index) = trimmed.rfind(":@") {
        let scope = &trimmed[scope_separator_index + 1..];
        if is_package_scope(scope) {
            return Some((
                normalize_auth_key(trimmed[..scope_separator_index].to_owned()),
                scope.to_owned(),
            ));
        }
    }
    let last_slash_index = trimmed.rfind('/')?;
    let scope = &trimmed[last_slash_index + 1..];
    if !is_package_scope(scope) {
        return None;
    }
    Some((trimmed[..=last_slash_index].to_owned(), scope.to_owned()))
}

pub(super) fn is_package_scope(scope: &str) -> bool {
    scope.starts_with('@') && scope.len() > 1 && !scope.contains('/') && !scope.contains(':')
}

pub(crate) fn package_scope(pkg_name: Option<&str>) -> Option<&str> {
    let pkg_name = pkg_name?;
    if !pkg_name.starts_with('@') {
        return None;
    }
    let (scope, name) = pkg_name.split_once('/')?;
    if scope.len() <= 1 || name.is_empty() {
        return None;
    }
    Some(scope)
}

fn insert_or_upgrade(map: &mut HashMap<String, AuthEntry>, key: String, entry: AuthEntry) {
    if let Some(existing) = map.get_mut(&key) {
        if entry.allow_insecure && !existing.allow_insecure {
            *existing = entry;
        }
    } else {
        map.insert(key, entry);
    }
}

fn nerf_dart_if_schemed(uri: &str) -> String {
    if uri.starts_with("http://") || uri.starts_with("https://") {
        nerf_dart(uri)
    } else {
        String::new()
    }
}

pub(super) fn record_uri_entry<Value: Clone>(
    uri: &str,
    value: Value,
    to_kind: impl Fn(Value) -> AuthKind,
    by_uri: &mut HashMap<String, AuthEntry>,
    insecure_uris: &mut HashSet<String>,
) {
    let allow_insecure = uri.starts_with("http://") && !is_url_secure_for_credentials(uri);
    let normalized = normalize_auth_key(uri.to_string());
    let nerfed = nerf_dart_if_schemed(uri);
    if allow_insecure {
        if !nerfed.is_empty() {
            insecure_uris.insert(nerfed.clone());
        }
        insecure_uris.insert(normalized.clone());
    }
    if !nerfed.is_empty() {
        insert_or_upgrade(
            by_uri,
            nerfed,
            AuthEntry { kind: to_kind(value.clone()), allow_insecure },
        );
    }
    insert_or_upgrade(by_uri, normalized, AuthEntry { kind: to_kind(value), allow_insecure });
}

pub(super) fn record_scoped_entries<Value: Clone>(
    uri: &str,
    scoped: HashMap<String, Value>,
    to_kind: impl Fn(Value) -> AuthKind,
    scoped_entries: &mut HashMap<String, HashMap<String, AuthEntry>>,
) {
    let allow_insecure = uri.starts_with("http://") && !is_url_secure_for_credentials(uri);
    let normalized = normalize_auth_key(uri.to_string());
    let nerfed = nerf_dart_if_schemed(uri);
    for (scope, val) in scoped {
        if !nerfed.is_empty() {
            let map = scoped_entries.entry(nerfed.clone()).or_default();
            insert_or_upgrade(
                map,
                scope.clone(),
                AuthEntry { kind: to_kind(val.clone()), allow_insecure },
            );
        }
        let map = scoped_entries.entry(normalized.clone()).or_default();
        insert_or_upgrade(map, scope, AuthEntry { kind: to_kind(val), allow_insecure });
    }
}
