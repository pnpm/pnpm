//! Where [`WorkspaceSettings`](crate::WorkspaceSettings)'s per-field readers
//! take each setting's string value from.

use crate::{
    api::EnvVar,
    naming_cases::{to_camel_case, to_kebab_case},
};
use std::collections::BTreeMap;

/// Read an env var by suffix, accepting both `PNPM_CONFIG_<UPPER>` and
/// `pnpm_config_<lower>`. Empty values are treated as unset.
pub(super) fn read_env<Sys: EnvVar>(suffix: &str) -> Option<String> {
    let upper = format!("PNPM_CONFIG_{suffix}");
    let lower = format!("pnpm_config_{}", suffix.to_lowercase());
    Sys::var(&upper)
        .or_else(|| Sys::var(&lower))
        .filter(|value| !value.is_empty())
}

/// Read an env var by suffix, keeping an empty value as `Some("")`.
///
/// pnpm's own env pass only skips a variable that is absent, never one
/// that is empty, so an empty value clobbers lower-priority layers. For
/// nearly every setting an empty value is indistinguishable from an unset
/// one, which is why [`read_env`] drops it. The exceptions,
/// where `""` is observably different from unset:
///
/// - `savePrefix`: `""` is the value that selects an exact version pin.
/// - `scope`: `""` must override a scope from the global `config.yaml` so
///   that `PNPM_CONFIG_SCOPE=` yields an unscoped `pnpm login`. Dropping it
///   would let the lower layer's scope leak through, diverging from the
///   TypeScript CLI.
/// - `tagVersionPrefix`: `""` removes the default `"v"` prefix from version
///   tags.
/// - `nodeOptions`: `""` disables options from lower-priority settings while
///   leaving the child process's inherited `NODE_OPTIONS` intact.
pub(super) fn read_env_allow_empty<Sys: EnvVar>(suffix: &str) -> Option<String> {
    let upper = format!("PNPM_CONFIG_{suffix}");
    let lower = format!("pnpm_config_{}", suffix.to_lowercase());
    Sys::var(&upper).or_else(|| Sys::var(&lower))
}

/// Where a section reader gets one setting's string value.
///
/// The environment is the production source (`WorkspaceSettings::from_pnpm_config_env`,
/// where an empty variable reads as unset); the command line's
/// `--config.<setting>=<value>` tokens are the other
/// (`WorkspaceSettings::from_string_values`). `value_allow_empty` is kept
/// separate because some settings read `""` as a value of their own — see
/// [`read_env_allow_empty`].
#[derive(Clone, Copy)]
pub(super) struct StringReader<'a> {
    pub(super) read: &'a dyn Fn(&str) -> Option<String>,
    pub(super) read_allow_empty: &'a dyn Fn(&str) -> Option<String>,
}

impl StringReader<'_> {
    pub(super) fn value(&self, suffix: &str) -> Option<String> {
        (self.read)(suffix)
    }

    pub(super) fn value_allow_empty(&self, suffix: &str) -> Option<String> {
        (self.read_allow_empty)(suffix)
    }
}

/// Answer a field's env-var suffix (`UPDATE_NOTIFIER`) from a dotted
/// `--config.<key>=<value>` map, which names settings by their own spelling.
///
/// The suffix list and the setting names differ only in case and separator, so
/// both spellings a caller can write are tried before the value is given up
/// on; a suffix naming a nested key (`MACOS_BACKUP_EXCLUDE_MODULES_DIR`)
/// matches no dotted setting and reads as unset.
pub(super) fn dotted_value(values: &BTreeMap<String, String>, suffix: &str) -> Option<String> {
    let lower = suffix.to_lowercase();
    values
        .get(&to_kebab_case(&lower))
        .or_else(|| values.get(&to_camel_case(&lower)))
        .cloned()
}
