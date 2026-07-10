//! Registry URL normalization shared by the `login` and `logout` commands.

/// Append a trailing slash if the registry URL lacks one. Mirrors npm's
/// `normalize-registry-url`.
pub(crate) fn normalize_registry_url(registry: &str) -> String {
    if registry.ends_with('/') { registry.to_string() } else { format!("{registry}/") }
}
