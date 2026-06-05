use pacquet_config::Config;
use std::ffi::{OsStr, OsString};

/// CLI overrides parsed from pnpm's `--config.<key>=<value>` dotted-key
/// syntax. Upstream pnpm uses [`npm-conf`](https://github.com/npm/npm-conf)
/// to translate each `--config.<key>=<value>` token into a runtime config
/// assignment that wins over `.npmrc` and `pnpm-workspace.yaml`; pacquet
/// mirrors that by stripping the same tokens out of argv before clap sees
/// them and re-applying them onto [`Config`] after the file-based layers
/// have run.
///
/// Unknown keys are accepted silently: pnpm exposes a long tail of config
/// keys, and erroring on an unrecognized one would break the moment pnpm
/// adds a new key that pacquet hasn't ported yet. The token has already
/// been honored by pnpm itself before delegation, so dropping it on
/// pacquet's side just means the pacquet leg falls back to the yaml/npmrc
/// value — never an incorrect override.
#[derive(Debug, Default)]
pub struct ConfigOverrides {
    registry: Option<String>,
}

impl ConfigOverrides {
    /// Pull `--config.<key>=<value>` tokens out of `argv` and collect
    /// them. Returns the parsed overrides together with the remaining
    /// argv tokens (in their original order) for clap to parse.
    ///
    /// Malformed tokens — `--config.foo` with no `=`, or `--config.=value`
    /// with an empty key — are dropped: clap would reject `--config.*` as
    /// unknown anyway, and the dropped tokens carry no usable signal.
    pub fn extract<Argv>(argv: Argv) -> (Self, Vec<OsString>)
    where
        Argv: IntoIterator<Item = OsString>,
    {
        let mut overrides = Self::default();
        let mut remaining = Vec::new();
        for arg in argv {
            match classify(&arg) {
                ConfigToken::WellFormed { key, value } => overrides.set(key, value),
                ConfigToken::Malformed => {}
                ConfigToken::NotOurs => remaining.push(arg),
            }
        }
        (overrides, remaining)
    }

    fn set(&mut self, key: &str, value: &str) {
        if key == "registry" {
            self.registry = Some(value.to_owned());
        }
    }

    /// Layer the CLI overrides on top of a [`Config`] that has already
    /// been built from defaults, `.npmrc`, and `pnpm-workspace.yaml`.
    /// Mirrors pnpm 11's "CLI > yaml > .npmrc > defaults" precedence.
    pub fn apply(&self, config: &mut Config) {
        if let Some(registry) = &self.registry {
            config.registry.clone_from(registry);
        }
    }
}

enum ConfigToken<'a> {
    WellFormed { key: &'a str, value: &'a str },
    Malformed,
    NotOurs,
}

/// Decide whether an argv token belongs to the `--config.<key>=<value>`
/// family. Everything with a `--config.` prefix is claimed, so a typo
/// like `--config.foo` never escapes into clap's "unexpected argument"
/// path; non-prefixed tokens are returned untouched.
fn classify(arg: &OsStr) -> ConfigToken<'_> {
    let Some(rest) = arg.to_str().and_then(|arg| arg.strip_prefix("--config.")) else {
        return ConfigToken::NotOurs;
    };
    let Some((key, value)) = rest.split_once('=') else {
        return ConfigToken::Malformed;
    };
    if key.is_empty() {
        return ConfigToken::Malformed;
    }
    ConfigToken::WellFormed { key, value }
}

#[cfg(test)]
mod tests;
