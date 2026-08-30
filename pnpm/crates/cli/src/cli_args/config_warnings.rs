//! The stderr channel for config-load warnings.
//!
//! pnpm collects the warnings raised while reading config and prints them
//! with `console.warn` — to stderr, outside the reporter, on every command
//! and under every reporter — so they never mix into the stdout a script
//! captures. Warnings emitted through the reporter stay on stdout; only
//! config-load warnings belong here.

use derive_more::{Display, Error};
use miette::Diagnostic;
use pnpm_config::{
    Config, WorkspaceKeyIssues, known_settings::annotate_unknown_setting,
    naming_cases::to_camel_case, refused_keys::where_refused_key_belongs,
};
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
    let colors =
        Colors { enabled: pnpm_default_reporter::colors_enabled(std::io::stdout().is_terminal()) };
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

/// Settings in a project's `pnpm-workspace.yaml` that this version of pnpm
/// does not recognize, raised instead of a warning when the project pins a
/// pnpm the running pnpm satisfies: with the pin honored, the keys cannot be
/// meant for a different pnpm version, so they are a typo or a removed
/// setting the project must fix.
#[derive(Debug, Display, Error, Diagnostic)]
#[display(
    "The following settings in pnpm-workspace.yaml are not recognized by this version of pnpm: {keys}."
)]
#[diagnostic(
    code(ERR_PNPM_UNRECOGNIZED_WORKSPACE_SETTINGS),
    help(
        "The project pins pnpm to a version the running pnpm satisfies, so these settings cannot be meant for a different pnpm version. Remove them from pnpm-workspace.yaml or fix their spelling."
    )
)]
pub(crate) struct UnrecognizedWorkspaceSettingsError {
    keys: String,
}

/// Report the problem keys of the project's `pnpm-workspace.yaml`, in pnpm's
/// order (refused, unrecognized, kebab-case). Unrecognized keys are a
/// warning, or — when `strict` (the running pnpm is the version the project
/// pins) — the error above, raised after the other warnings are out.
pub(crate) fn report_workspace_key_issues(
    issues: &WorkspaceKeyIssues,
    strict: bool,
) -> Result<(), UnrecognizedWorkspaceSettingsError> {
    if !issues.refused.is_empty() {
        emit_config_warning(&refused_workspace_keys_warning(&issues.refused));
    }
    let unrecognized = annotate_unknown_settings(&issues.unrecognized);
    if let Some(unrecognized) = unrecognized.as_deref()
        && !strict
    {
        emit_config_warning(&format!(
            "The following settings in pnpm-workspace.yaml are not recognized by this version of pnpm and were ignored: {unrecognized}.",
        ));
    }
    if !issues.non_camel_case.is_empty() {
        emit_config_warning(&non_camel_case_workspace_keys_warning(&issues.non_camel_case));
    }
    match unrecognized {
        Some(keys) if strict => Err(UnrecognizedWorkspaceSettingsError { keys }),
        _ => Ok(()),
    }
}

fn refused_workspace_keys_warning(keys: &[String]) -> String {
    let keys = keys
        .iter()
        .map(|key| redact_and_sanitize(key))
        .map(|key| format!(r#""{key}" ({})"#, where_refused_key_belongs(&to_camel_case(&key))))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "The following settings cannot be set in a project's pnpm-workspace.yaml and were ignored: {keys}.",
    )
}

fn annotate_unknown_settings(keys: &[String]) -> Option<String> {
    if keys.is_empty() {
        return None;
    }
    Some(
        keys.iter()
            .map(|key| annotate_unknown_setting(&redact_and_sanitize(key)))
            .collect::<Vec<_>>()
            .join(", "),
    )
}

fn non_camel_case_workspace_keys_warning(keys: &[String]) -> String {
    let keys = keys
        .iter()
        .map(|key| redact_and_sanitize(key))
        .map(|key| format!(r#""{key}" (use "{}")"#, to_camel_case(&key)))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "The following settings in pnpm-workspace.yaml were ignored because they are not written in camelCase: {keys}.",
    )
}

#[cfg(test)]
mod tests;
