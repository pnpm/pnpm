//! The stderr channel for config-load warnings.
//!
//! pnpm collects the warnings raised while reading config and prints them
//! with `console.warn` — to stderr, outside the reporter, on every command
//! and under every reporter — so they never mix into the stdout a script
//! captures. Warnings emitted through the reporter stay on stdout; only
//! config-load warnings belong here.

use pacquet_default_reporter::colors::Colors;
use std::{
    collections::HashSet,
    io::{IsTerminal, Write},
    sync::{Mutex, OnceLock, PoisonError},
};

/// Config-load warnings already written this process.
///
/// One command can load `Config` several times — the install fast path falls
/// through to `run`, and a handler may call its `config` / `state` closure more
/// than once — and every load re-reads the same files and re-collects the same
/// warnings. pnpm reads config once per command and prints each warning once,
/// so the second and later copies are suppressed here.
static EMITTED_CONFIG_WARNINGS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

/// Emit every warning [`pacquet_config::Config`] collected while loading,
/// skipping any already written this process, and clear them off the config.
pub(crate) fn drain_config_warnings(config: &mut pacquet_config::Config) {
    let unemitted = {
        // A poisoned lock means another thread panicked mid-insert; showing a
        // warning twice beats aborting the command over it. The guard is
        // dropped before emitting so a slow stderr holds it no longer than the
        // set update itself.
        let mut emitted = EMITTED_CONFIG_WARNINGS
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        take_unemitted(&mut emitted, config)
    };
    for warning in unemitted {
        emit_config_warning(&warning);
    }
}

/// Take the warnings off `config` and return the ones `emitted` has not seen,
/// recording them there. Takes the set rather than reaching for the process
/// global so the emit-once rule can be asserted against a local one.
fn take_unemitted(
    emitted: &mut HashSet<String>,
    config: &mut pacquet_config::Config,
) -> Vec<String> {
    std::mem::take(&mut config.config_warnings)
        .into_iter()
        .filter(|warning| emitted.insert(warning.clone()))
        .collect()
}

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

#[cfg(test)]
mod tests;
