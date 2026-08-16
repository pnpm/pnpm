//! The stderr channel for config-load warnings.
//!
//! pnpm collects the warnings raised while reading config and prints them
//! with `console.warn` — to stderr, outside the reporter, on every command
//! and under every reporter — so they never mix into the stdout a script
//! captures. Warnings emitted through the reporter stay on stdout; only
//! config-load warnings belong here.

use pnpm_config::Config;
use pnpm_default_reporter::colors::Colors;
use pnpm_resolving_npm_resolver::BUILTIN_NAMED_REGISTRIES;
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

/// Warn about `registryOptions` entries that name no configured registry.
///
/// Such an entry is inert — the wrong URL, a stale entry, a scope that moved —
/// and silently doing nothing is the failure mode users cannot debug. A warning
/// rather than an error: a shared config dependency can legitimately describe
/// registries a given project does not use.
pub(crate) fn warn_unmatched_registry_options(config: &Config) {
    if config.registry_options.is_empty() {
        return;
    }
    let configured: BTreeSet<&str> = config
        .registries
        .values()
        .chain(config.named_registries.values())
        .map(String::as_str)
        .chain(BUILTIN_NAMED_REGISTRIES.iter().map(|(_, url)| *url))
        .collect();
    let unmatched = config
        .registry_options
        .keys()
        .filter(|registry| !configured.contains(registry.as_str()))
        .map(|registry| format!(r#""{registry}""#))
        .collect::<Vec<_>>();
    if unmatched.is_empty() {
        return;
    }
    let configured =
        configured.iter().map(|registry| format!(r#""{registry}""#)).collect::<Vec<_>>().join(", ");
    emit_config_warning(&format!(
        r#"The following "registryOptions" entries do not match any configured registry and were ignored: {}. The configured registries are: {configured}."#,
        unmatched.join(", "),
    ));
}
