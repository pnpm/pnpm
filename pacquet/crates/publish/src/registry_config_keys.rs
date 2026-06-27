//! Port of `registryConfigKeys.ts`: parse a registry URL into the `.npmrc`
//! config-key form (`//host/path/`) and enumerate every config key of the
//! same host from the longest path to the shortest.

/// A registry URL normalized to match its [`RegistryConfigKey`]: an HTTP or
/// HTTPS URL with a guaranteed trailing slash. Ports the branded TS type
/// `NormalizedRegistryUrl` (`` `${'http'|'https'}://${string}/` ``);
/// constructed only by [`parse_supported_registry_url`], which validates the
/// scheme.
#[derive(Debug, derive_more::Display, Clone, PartialEq, Eq)]
pub struct NormalizedRegistryUrl(String);

impl NormalizedRegistryUrl {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A registry config key as it appears in `.npmrc`: a `//`-prefixed host and
/// path that ends with `/` (e.g. `//registry.npmjs.org/`). Ports the branded
/// TS type `RegistryConfigKey` (`` `//${string}/` ``).
#[derive(Debug, derive_more::Display, Clone, PartialEq, Eq)]
pub struct RegistryConfigKey(String);

impl RegistryConfigKey {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The longest config key for a registry URL plus the URL normalized to match
/// it. Ports TS `SupportedRegistryUrlInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupportedRegistryUrlInfo {
    pub normalized_url: NormalizedRegistryUrl,
    pub longest_config_key: RegistryConfigKey,
}

/// If `registry_url` is an HTTP or HTTPS registry URL, return the longest
/// [`RegistryConfigKey`] that corresponds to it and the matching
/// [`NormalizedRegistryUrl`]. Returns `None` for any other protocol.
///
/// Ports TS `parseSupportedRegistryUrl`.
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
/// to the shortest (`//host/`), including `longest` itself. Ports TS
/// `allRegistryConfigKeys`.
///
/// The shortest key produced still carries a host (`//host/`); the bare
/// hostless `//` is never yielded, matching the TS termination guard against
/// `'///'`.
#[must_use]
pub fn all_registry_config_keys(longest: &RegistryConfigKey) -> Vec<RegistryConfigKey> {
    // `'///'` is the TS termination sentinel: a `//`-prefixed key that is no
    // longer than it carries no host segment, so we stop before yielding it.
    const EMPTY_LEN: usize = "///".len();
    let mut keys = Vec::new();
    let mut current = longest.0.clone();
    while current.starts_with("//") && current.len() > EMPTY_LEN {
        keys.push(RegistryConfigKey(current.clone()));
        current = strip_last_segment(&current);
    }
    keys
}

/// Drop the final `<segment>/` of a config key, mirroring the TS
/// `replace(/[^/]*\/$/, '')`. `//host/a/` → `//host/`, `//host/` → `//`.
fn strip_last_segment(key: &str) -> String {
    let without_trailing = &key[..key.len().saturating_sub(1)];
    match without_trailing.rfind('/') {
        Some(index) => key[..=index].to_owned(),
        None => String::new(),
    }
}

/// If `text` starts with `prefix`, replace that prefix with `//`. Mirrors the
/// TS `replacePrefix(text, oldPrefix, '//')`.
fn replace_prefix(text: &str, prefix: &str) -> Option<String> {
    text.strip_prefix(prefix).map(|rest| format!("//{rest}"))
}

/// Ensure `text` ends with a single trailing slash. Ports `normalizeRegistryUrl`
/// (for the trailing-slash concern) and the TS `ensureSuffix(text, '/')`.
fn ensure_trailing_slash(text: &str) -> String {
    if text.ends_with('/') { text.to_owned() } else { format!("{text}/") }
}

#[cfg(test)]
mod tests {
    use super::{all_registry_config_keys, parse_supported_registry_url};
    use pretty_assertions::assert_eq;

    #[test]
    fn rejects_unsupported_protocol() {
        assert_eq!(parse_supported_registry_url("ftp://example.com"), None);
    }

    #[test]
    fn normalizes_url_and_derives_config_key() {
        let info = parse_supported_registry_url("https://registry.npmjs.org").unwrap();
        assert_eq!(info.normalized_url.as_str(), "https://registry.npmjs.org/");
        assert_eq!(info.longest_config_key.as_str(), "//registry.npmjs.org/");
    }

    #[test]
    fn keeps_existing_trailing_slash() {
        let info = parse_supported_registry_url("http://localhost:4873/path/").unwrap();
        assert_eq!(info.normalized_url.as_str(), "http://localhost:4873/path/");
        assert_eq!(info.longest_config_key.as_str(), "//localhost:4873/path/");
    }

    #[test]
    fn enumerates_keys_longest_to_shortest() {
        let info = parse_supported_registry_url("https://host/a/b/").unwrap();
        let keys: Vec<_> = all_registry_config_keys(&info.longest_config_key)
            .iter()
            .map(|key| key.as_str().to_owned())
            .collect();
        assert_eq!(keys, vec!["//host/a/b/", "//host/a/", "//host/"]);
    }

    #[test]
    fn single_key_for_bare_host() {
        let info = parse_supported_registry_url("https://host").unwrap();
        let keys: Vec<_> = all_registry_config_keys(&info.longest_config_key)
            .iter()
            .map(|key| key.as_str().to_owned())
            .collect();
        assert_eq!(keys, vec!["//host/"]);
    }
}
