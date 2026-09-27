use pnpm_reporter::{LogEvent, LogLevel, PnpmLog};
use pnpm_text_sanitize::sanitize_inline;

/// The warning for a package that is about to be depended on through
/// `link:`, or `None` when its manifest declares no peer dependencies.
///
/// A `link:` dependency is a symlink, so Node resolves the package's imports
/// from its own directory and never sees the peers installed in the project
/// that links it.
pub fn linked_peer_dependencies_warning(
    package_name: &str,
    package_manifest: &serde_json::Value,
    prefix: &str,
) -> Option<LogEvent> {
    let peer_deps_map = package_manifest
        .get("peerDependencies")
        .and_then(serde_json::Value::as_object)
        .filter(|peer_deps_map| !peer_deps_map.is_empty())?;
    let sanitized_pkg_name = sanitize_warning_text(package_name);
    let peer_deps = peer_deps_map
        .iter()
        .map(|(key, value)| {
            let sanitized_key = sanitize_warning_text(key);
            let val_str = value.as_str().map_or_else(|| value.to_string(), ToString::to_string);
            let sanitized_val = sanitize_warning_text(&val_str);
            format!("  - {sanitized_key}@{sanitized_val}")
        })
        .collect::<Vec<_>>()
        .join(", ");
    Some(LogEvent::Pnpm(PnpmLog {
        level: LogLevel::Warn,
        message: format!(
            "The package {sanitized_pkg_name}, which you have just pnpm linked, has the following peerDependencies specified in its package.json:\n\n{peer_deps}\n\nThe linked in dependency will not resolve the peer dependencies from the target node_modules.\nThis might cause issues in your project. To resolve this, you may use the \"file:\" protocol to reference the local dependency.",
        ),
        prefix: prefix.to_string(),
    }))
}

fn sanitize_warning_text(text: &str) -> String {
    let stripped = console::strip_ansi_codes(text);
    sanitize_inline(&stripped).into_owned()
}
