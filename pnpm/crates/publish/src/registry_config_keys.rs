//! Parse a registry URL into the `.npmrc` config-key form (`//host/path/`) and
//! enumerate every config key of the same host from the longest path to the
//! shortest.

/// A registry URL normalized to match its [`RegistryConfigKey`]: an HTTP or
/// HTTPS URL with a guaranteed trailing slash. Constructed only by
/// [`parse_supported_registry_url`], which validates the scheme.
#[derive(Debug, derive_more::Display, Clone, PartialEq, Eq)]
pub struct NormalizedRegistryUrl(String);

impl NormalizedRegistryUrl {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A registry config key as it appears in `.npmrc`: a `//`-prefixed host and
/// path that ends with `/` (e.g. `//registry.npmjs.org/`).
#[derive(Debug, derive_more::Display, Clone, PartialEq, Eq)]
pub struct RegistryConfigKey(String);

impl RegistryConfigKey {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The longest config key for a registry URL plus the URL normalized to match
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportedRegistryUrlInfo {
    pub normalized_url: NormalizedRegistryUrl,
    pub longest_config_key: RegistryConfigKey,
}

/// If `registry_url` is an HTTP or HTTPS registry URL, return the longest
/// [`RegistryConfigKey`] that corresponds to it and the matching
/// [`NormalizedRegistryUrl`]. Returns `None` for any other protocol.
#[must_use]
pub fn parse_supported_registry_url(registry_url: &str) -> Option<SupportedRegistryUrlInfo> {
    let registry_url = ensure_trailing_slash(registry_url);
    let key_prefix = replace_prefix(&registry_url, "http://")
        .or_else(|| replace_prefix(&registry_url, "https://"))?;
    let normalized_url = NormalizedRegistryUrl(registry_url);
    let longest_config_key = RegistryConfigKey(ensure_trailing_slash(&key_prefix));
    Some(SupportedRegistryUrlInfo { normalized_url, longest_config_key })
}

/// Generate every [`RegistryConfigKey`] of the same host from `longest` down
/// to the shortest (`//host/`), including `longest` itself.
///
/// The shortest key produced still carries a host (`//host/`); the bare
/// hostless `//` is never yielded.
#[must_use]
pub fn all_registry_config_keys(longest: &RegistryConfigKey) -> Vec<RegistryConfigKey> {
    // A `//`-prefixed key no longer than `"///"` carries no host segment, so
    // stop before yielding it.
    const EMPTY_LEN: usize = "///".len();
    let mut keys = Vec::new();
    let mut current = longest.0.clone();
    while current.starts_with("//") && current.len() > EMPTY_LEN {
        keys.push(RegistryConfigKey(current.clone()));
        current = strip_last_segment(&current);
    }
    keys
}

/// Drop the final `<segment>/` of a config key: `//host/a/` → `//host/`,
/// `//host/` → `//`.
fn strip_last_segment(key: &str) -> String {
    let without_trailing = &key[..key.len().saturating_sub(1)];
    match without_trailing.rfind('/') {
        Some(index) => key[..=index].to_owned(),
        None => String::new(),
    }
}

/// If `text` starts with `prefix`, replace that prefix with `//`.
fn replace_prefix(text: &str, prefix: &str) -> Option<String> {
    text.strip_prefix(prefix).map(|rest| format!("//{rest}"))
}

/// Ensure `text` ends with a single trailing slash.
fn ensure_trailing_slash(text: &str) -> String {
    if text.ends_with('/') { text.to_owned() } else { format!("{text}/") }
}

#[cfg(test)]
mod tests;
