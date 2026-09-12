use super::{DEFAULT_REGISTRY_SCOPE, LoadWorkspaceYamlError, NpmrcAuth, split_inline_identity_key};

/// Reject a `tokenHelper` that a workspace or project `.npmrc` contributed.
///
/// `full` is the merged config including the project `.npmrc`; `trusted`
/// is the same merge minus the project source (mirroring pnpm's
/// `userConfig` vs `trustedConfig` split). A `tokenHelper` present in
/// `full` but not reproducible verbatim from `trusted` — because the
/// project source added it, or overrode a trusted one with a different
/// command — is repository-controlled and rejected. A project value that
/// exactly equals the trusted one is harmless and allowed, matching pnpm.
pub(crate) fn enforce_token_helper_trust(
    full: &NpmrcAuth,
    trusted: &NpmrcAuth,
) -> Result<(), LoadWorkspaceYamlError> {
    for (uri, by_scope) in &full.creds_by_scope_by_uri {
        for (scope, raw) in by_scope {
            let Some(value) = &raw.token_helper else { continue };
            let trusted_value = trusted
                .creds_by_scope_by_uri
                .get(uri)
                .and_then(|by_scope| by_scope.get(scope))
                .and_then(|raw| raw.token_helper.as_deref());
            if trusted_value != Some(value.as_str()) {
                return Err(LoadWorkspaceYamlError::TokenHelperInProjectConfig {
                    key: token_helper_key(uri, scope),
                });
            }
        }
    }
    // `default_creds` is normally emptied by `rescope_unscoped` before the
    // merge; guard it too for the paths that skip rescoping.
    if let Some(value) = &full.default_creds.token_helper
        && trusted.default_creds.token_helper.as_deref() != Some(value.as_str())
    {
        return Err(LoadWorkspaceYamlError::TokenHelperInProjectConfig {
            key: "tokenHelper".to_owned(),
        });
    }
    Ok(())
}

/// Reconstruct a representative `.npmrc` key from a nerf-darted registry
/// URI and scope, for the [`LoadWorkspaceYamlError::TokenHelperInProjectConfig`]
/// message.
fn token_helper_key(uri: &str, scope: &str) -> String {
    if scope == DEFAULT_REGISTRY_SCOPE {
        format!("{uri}:tokenHelper")
    } else {
        format!("{uri}:{scope}:tokenHelper")
    }
}

/// Characters pnpm reserves in a `tokenHelper` value for future quoting /
/// interpolation support; their presence is an error.
const TOKEN_HELPER_RESERVED_CHARACTERS: [char; 5] = ['$', '%', '`', '"', '\''];

/// Parse an optional raw `tokenHelper` value into `[program, ...args]`,
/// returning `Ok(None)` when unset or empty (no command to run). Rejects
/// a reserved character. Mirrors pnpm's `parseTokenHelper`.
pub(super) fn parse_token_helper_field(
    raw: Option<&str>,
) -> Result<Option<Vec<String>>, LoadWorkspaceYamlError> {
    let Some(raw) = raw else { return Ok(None) };
    let source = raw.trim();
    if let Some(character) =
        source.chars().find(|character| TOKEN_HELPER_RESERVED_CHARACTERS.contains(character))
    {
        return Err(LoadWorkspaceYamlError::TokenHelperUnsupportedCharacter { character });
    }
    let command: Vec<String> = source.split_whitespace().map(str::to_owned).collect();
    Ok((!command.is_empty()).then_some(command))
}

/// Auth-suffix keys recognised on a `//host[:port]/path/:` prefix in
/// **any** source, including URL-scoped environment variables.
///
/// `tokenHelper` is deliberately absent: it names an executable, so it
/// is only ever honored from a trusted `.npmrc` / `auth.ini` file (see
/// [`INI_CREDS_SUFFIXES`]), never from the environment — mirroring
/// pnpm, which drops a `//host/:tokenHelper` env var outright.
const CREDS_SUFFIXES: &[&str] = &["_authToken", "_auth", "_password", "username"];

/// Auth-suffix keys recognised when parsing an `.npmrc` / `auth.ini`
/// file. Adds `tokenHelper` to [`CREDS_SUFFIXES`]; the file-vs-env
/// split is the boundary the trust guard relies on.
const INI_CREDS_SUFFIXES: &[&str] =
    &["_authToken", "_auth", "_password", "username", "tokenHelper"];

pub(super) fn is_auth_value_key(key: &str) -> bool {
    matches!(
        key,
        "_authToken" | "_auth" | "_password" | "username" | "tokenHelper" | "cert" | "key",
    ) || split_ini_creds_key(key).is_some()
        || split_inline_identity_key(key).is_some()
}

/// Match a `npm_config_//…` / `pnpm_config_//…` environment variable name.
/// Returns `(is_pnpm, key)` where `key` is the URL-scoped remainder
/// (e.g. `//registry.npmjs.org/:_authToken`) with its case preserved.
/// The prefix is matched case-insensitively, mirroring npm. `None` for any
/// name that isn't one of these prefixes followed by `//`.
pub(super) fn parse_url_scoped_env_name(name: &str) -> Option<(bool, &str)> {
    for (prefix, is_pnpm) in [("pnpm_config_", true), ("npm_config_", false)] {
        // Slice with `get` rather than `name[..prefix.len()]`: a byte index
        // landing inside a multi-byte char (a non-ASCII env var name) would
        // otherwise panic before the prefix check can reject it.
        let (Some(head), Some(rest)) = (name.get(..prefix.len()), name.get(prefix.len()..)) else {
            continue;
        };
        if head.eq_ignore_ascii_case(prefix) && rest.starts_with("//") {
            return Some((is_pnpm, rest));
        }
    }
    None
}

pub(super) fn split_creds_key(key: &str) -> Option<(&str, &'static str)> {
    split_creds_key_in(key, CREDS_SUFFIXES)
}

/// Like [`split_creds_key`] but also recognises `:tokenHelper`. Used only
/// on `.npmrc` / `auth.ini` file keys, never on environment variables.
pub(super) fn split_ini_creds_key(key: &str) -> Option<(&str, &'static str)> {
    split_creds_key_in(key, INI_CREDS_SUFFIXES)
}

fn split_creds_key_in<'a>(
    key: &'a str,
    suffixes: &[&'static str],
) -> Option<(&'a str, &'static str)> {
    if !key.starts_with("//") {
        return None;
    }
    for suffix in suffixes {
        let needle = format!(":{suffix}");
        if let Some(stripped) = key.strip_suffix(needle.as_str()) {
            return Some((stripped, suffix));
        }
    }
    None
}

pub(super) fn split_scope_from_uri(uri: &str) -> (String, Option<String>) {
    if let Some((registry_uri, scope)) = split_scope_from_uri_by_colon(uri) {
        return (normalize_registry_key(registry_uri), Some(scope.to_owned()));
    }
    split_scope_from_uri_by_path(uri)
}

fn split_scope_from_uri_by_colon(uri: &str) -> Option<(&str, &str)> {
    if !uri.starts_with("//") {
        return None;
    }
    let scope_separator_index = uri.rfind(":@")?;
    let scope = &uri[scope_separator_index + 1..];
    if !is_package_scope(scope) {
        return None;
    }
    Some((&uri[..scope_separator_index], scope))
}

fn split_scope_from_uri_by_path(uri: &str) -> (String, Option<String>) {
    let trimmed = uri.strip_suffix('/').unwrap_or(uri);
    let Some(last_slash_index) = trimmed.rfind('/') else {
        return (uri.to_owned(), None);
    };
    let scope = &trimmed[last_slash_index + 1..];
    if !is_package_scope(scope) {
        return (uri.to_owned(), None);
    }
    (trimmed[..=last_slash_index].to_owned(), Some(scope.to_owned()))
}

pub(super) fn is_package_scope(scope: &str) -> bool {
    scope.starts_with('@') && scope.len() > 1 && !scope.contains('/') && !scope.contains(':')
}

fn normalize_registry_key(registry: &str) -> String {
    if registry.ends_with('/') { registry.to_owned() } else { format!("{registry}/") }
}
