use super::{
    AuthEntry, AuthKind, CredentialOrigin, ScopedAuthMap, is_url_secure_for_credentials, nerf_dart,
    normalize_auth_key,
};
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

pub(super) fn origin_of(uri: &str) -> CredentialOrigin {
    if uri.starts_with("http://") {
        CredentialOrigin::Http
    } else if uri.starts_with("https://") {
        CredentialOrigin::Https
    } else {
        CredentialOrigin::SchemeLess
    }
}

pub(super) fn index_scoped_entries(
    scoped_by_uri: ScopedAuthMap,
) -> (ScopedAuthMap, HashMap<String, usize>, bool) {
    let mut scoped_by_scope: ScopedAuthMap = HashMap::new();
    let mut max_scoped_parts_by_scope: HashMap<String, usize> = HashMap::new();
    let mut has_token_helpers = false;
    for (uri, scoped) in scoped_by_uri {
        let parts = uri.split('/').count();
        for (scope, value) in scoped {
            has_token_helpers |= matches!(value.kind, AuthKind::TokenHelper(_));
            max_scoped_parts_by_scope
                .entry(scope.clone())
                .and_modify(|max| *max = (*max).max(parts))
                .or_insert(parts);
            scoped_by_scope
                .entry(scope)
                .or_default()
                .insert(uri.clone(), value);
        }
    }
    (scoped_by_scope, max_scoped_parts_by_scope, has_token_helpers)
}

pub(super) fn max_parts_of(map: &HashMap<String, AuthEntry>) -> usize {
    map.keys()
        .map(|key| key.split('/').count())
        .max()
        .unwrap_or(0)
}

fn insert_or_upgrade(map: &mut HashMap<String, AuthEntry>, key: String, entry: AuthEntry) {
    if let Some(existing) = map.get_mut(&key) {
        let allow_insecure = existing.allow_insecure || entry.allow_insecure;
        let origin = match (existing.origin, entry.origin) {
            (CredentialOrigin::Https, _) | (_, CredentialOrigin::Https) => CredentialOrigin::Https,
            (CredentialOrigin::Http, _) | (_, CredentialOrigin::Http) => CredentialOrigin::Http,
            _ => CredentialOrigin::SchemeLess,
        };
        if matches!(entry.kind, AuthKind::TokenHelper(_))
            || !matches!(existing.kind, AuthKind::TokenHelper(_))
        {
            *existing = AuthEntry { kind: entry.kind, allow_insecure, origin };
        } else {
            existing.allow_insecure = allow_insecure;
            existing.origin = origin;
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
    http_by_uri: &mut HashMap<String, AuthEntry>,
    insecure_uris: &mut HashSet<String>,
) {
    let origin = origin_of(uri);
    let allow_insecure = origin == CredentialOrigin::Http && !is_url_secure_for_credentials(uri);
    let normalized = normalize_auth_key(uri.to_string());
    let nerfed = nerf_dart_if_schemed(uri);
    if allow_insecure {
        if !nerfed.is_empty() {
            insecure_uris.insert(nerfed.clone());
        }
        insecure_uris.insert(normalized.clone());
    }
    let target_map = if origin == CredentialOrigin::Http { http_by_uri } else { by_uri };
    if !nerfed.is_empty() {
        insert_or_upgrade(
            target_map,
            nerfed,
            AuthEntry { kind: to_kind(value.clone()), allow_insecure, origin },
        );
    }
    insert_or_upgrade(
        target_map,
        normalized,
        AuthEntry { kind: to_kind(value), allow_insecure, origin },
    );
}

pub(super) fn record_scoped_entries<Value: Clone>(
    uri: &str,
    scoped: HashMap<String, Value>,
    to_kind: impl Fn(Value) -> AuthKind,
    scoped_entries: &mut HashMap<String, HashMap<String, AuthEntry>>,
    http_scoped_entries: &mut HashMap<String, HashMap<String, AuthEntry>>,
) {
    let origin = origin_of(uri);
    let allow_insecure = origin == CredentialOrigin::Http && !is_url_secure_for_credentials(uri);
    let normalized = normalize_auth_key(uri.to_string());
    let nerfed = nerf_dart_if_schemed(uri);
    let target_map =
        if origin == CredentialOrigin::Http { http_scoped_entries } else { scoped_entries };
    for (scope, val) in scoped {
        if !nerfed.is_empty() {
            let map = target_map.entry(nerfed.clone()).or_default();
            insert_or_upgrade(
                map,
                scope.clone(),
                AuthEntry { kind: to_kind(val.clone()), allow_insecure, origin },
            );
        }
        let map = target_map.entry(normalized.clone()).or_default();
        insert_or_upgrade(map, scope, AuthEntry { kind: to_kind(val), allow_insecure, origin });
    }
}

pub(super) fn collect_uri_entries(
    by_uri: HashMap<String, String>,
    token_helper_by_uri: HashMap<String, Vec<String>>,
) -> (HashMap<String, AuthEntry>, HashMap<String, AuthEntry>, HashSet<String>) {
    let mut insecure_uris = HashSet::new();
    let mut by_uri_entries = HashMap::new();
    let mut http_by_uri_entries = HashMap::new();
    for (uri, value) in by_uri {
        record_uri_entry(
            &uri,
            value,
            AuthKind::Header,
            &mut by_uri_entries,
            &mut http_by_uri_entries,
            &mut insecure_uris,
        );
    }
    for (uri, command) in token_helper_by_uri {
        record_uri_entry(
            &uri,
            command,
            AuthKind::TokenHelper,
            &mut by_uri_entries,
            &mut http_by_uri_entries,
            &mut insecure_uris,
        );
    }
    (by_uri_entries, http_by_uri_entries, insecure_uris)
}

pub(super) fn collect_scoped_entries(
    scoped_by_uri: HashMap<String, HashMap<String, String>>,
    token_helper_scoped_by_uri: HashMap<String, HashMap<String, Vec<String>>>,
) -> (ScopedAuthMap, ScopedAuthMap) {
    let mut scoped_entries = HashMap::new();
    let mut http_scoped_entries = HashMap::new();
    for (uri, scoped) in scoped_by_uri {
        record_scoped_entries(
            &uri,
            scoped,
            AuthKind::Header,
            &mut scoped_entries,
            &mut http_scoped_entries,
        );
    }
    for (uri, scoped) in token_helper_scoped_by_uri {
        record_scoped_entries(
            &uri,
            scoped,
            AuthKind::TokenHelper,
            &mut scoped_entries,
            &mut http_scoped_entries,
        );
    }
    (scoped_entries, http_scoped_entries)
}
