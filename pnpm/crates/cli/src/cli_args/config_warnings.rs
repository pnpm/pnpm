//! The stderr channel for config-load warnings.
//!
//! pnpm collects the warnings raised while reading config and prints them
//! with `console.warn` — to stderr, outside the reporter, on every command
//! and under every reporter — so they never mix into the stdout a script
//! captures. Warnings emitted through the reporter stay on stdout; only
//! config-load warnings belong here.

use pacquet_default_reporter::colors::Colors;
use std::io::{IsTerminal, Write};

/// Emit every warning [`pacquet_config::Config`] collected while loading, and
/// clear them so a second drain cannot repeat them.
pub(crate) fn drain_config_warnings(config: &mut pacquet_config::Config) {
    for warning in std::mem::take(&mut config.config_warnings) {
        emit_config_warning(&warning);
    }
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
