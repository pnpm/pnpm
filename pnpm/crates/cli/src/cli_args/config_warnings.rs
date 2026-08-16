//! The stderr channel for config-load warnings.
//!
//! pnpm collects the warnings raised while reading config and prints them
//! with `console.warn` — to stderr, outside the reporter, on every command
//! and under every reporter — so they never mix into the stdout a script
//! captures. Warnings emitted through the reporter stay on stdout; only
//! config-load warnings belong here.

use pnpm_config::Config;
use pnpm_default_reporter::colors::Colors;
use pnpm_network::redact_and_sanitize;
use pnpm_resolving_npm_resolver::BUILTIN_REGISTRIES_BY_PREFIX;
use std::{
    collections::BTreeSet,
    io::{IsTerminal, Write},
};

/// Write a `[WARN]`-labelled config-load warning to stderr. Best-effort:
/// a warning must never abort the command, so a failed write (a closed
/// stderr, a consumer that exited) is discarded, as the reporter sinks
/// discard theirs.
pub(crate) fn emit_config_warning(message: &str) {
    // Styling is keyed off stdout, not stderr: pnpm's `formatWarn` colors
    // with chalk's default (stdout-probing) instance even though
    // `console.warn` writes to stderr.
    let colors = Colors {
        enabled: std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none(),
    };
    let _ = writeln!(std::io::stderr(), "{} {message}", colors.warn_label());
}

/// Warn about `registries` entries that name no configured registry.
///
/// Such an entry is inert — the wrong URL, a stale entry, a scope that moved —
/// and silently doing nothing is the failure mode users cannot debug. A warning
/// rather than an error: a shared config dependency can legitimately describe
/// registries a given project does not use.
pub(crate) fn warn_unmatched_registry_options(config: &Config) {
    if let Some(message) = unmatched_registry_options_warning(config) {
        emit_config_warning(&message);
    }
}

/// The message [`warn_unmatched_registry_options`] emits, or [`None`] when
/// every entry matches. Split out so the wording is testable without
/// capturing stderr.
fn unmatched_registry_options_warning(config: &Config) -> Option<String> {
    if config.registry_options_by_url.is_empty() {
        return None;
    }
    let configured: BTreeSet<&str> = config
        .registries_by_scope
        .values()
        .chain(config.registries_by_prefix.values())
        .map(String::as_str)
        .chain(BUILTIN_REGISTRIES_BY_PREFIX.iter().map(|(_, url)| *url))
        .collect();
    let unmatched = config
        .registry_options_by_url
        .keys()
        .filter(|registry| !configured.contains(registry.as_str()))
        .map(|registry| format!(r#""{}""#, redact_and_sanitize(registry)))
        .collect::<Vec<_>>();
    if unmatched.is_empty() {
        return None;
    }
    // A registry URL can carry `user:pass@` credentials, so neither list may be
    // echoed raw into a terminal or a CI log.
    let configured = configured
        .iter()
        .map(|registry| format!(r#""{}""#, redact_and_sanitize(registry)))
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        r#"The following "registries" entries do not match any configured registry and were ignored: {}. The configured registries are: {configured}."#,
        unmatched.join(", "),
    ))
}

#[cfg(test)]
mod tests;
