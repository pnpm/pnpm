//! chalk → owo-colors mapping used by the renderers.
//!
//! Every styling decision goes through a [`Colors`] whose `enabled` flag is
//! resolved once (stdout is a TTY and `NO_COLOR` is unset). Disabled, every
//! method returns the text unchanged, so the same render code produces the
//! plain output pnpm emits when piped. Tests construct `Colors { enabled }`
//! directly to assert both forms deterministically without touching a real
//! terminal.

use owo_colors::OwoColorize;

/// Palette wrapper. Mirrors the chalk colors `@pnpm/cli.default-reporter`
/// uses; method names match the chalk style they replace.
#[derive(Debug, Clone, Copy)]
pub struct Colors {
    pub enabled: bool,
}

macro_rules! paint {
    ($name:ident, $method:ident) => {
        pub fn $name(&self, text: &str) -> String {
            if self.enabled { text.$method().to_string() } else { text.to_string() }
        }
    };
}

impl Colors {
    paint!(cyan_bright, bright_cyan);
    paint!(cyan, cyan);
    paint!(green, green);
    paint!(red, red);
    paint!(yellow, yellow);
    // chalk's `grey`/`gray` — ANSI bright-black.
    paint!(grey, bright_black);
    paint!(magenta_bright, bright_magenta);

    /// The `[WARN]` label: yellow-on-yellow brackets around black-on-yellow
    /// `WARN`, matching `formatWarn`.
    #[must_use]
    pub fn warn_label(&self) -> String {
        if !self.enabled {
            return "[WARN]".to_string();
        }
        format!(
            "{}{}{}",
            "[".yellow().on_yellow(),
            "WARN".black().on_yellow(),
            "]".yellow().on_yellow(),
        )
    }
}
