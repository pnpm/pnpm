use crate::api::EnvVar;
use serde::de::DeserializeOwned;

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
/// one, which is why [`read_env`] drops it. Two settings are exceptions,
/// where `""` is observably different from unset:
///
/// - `savePrefix`: `""` is the value that selects an exact version pin.
/// - `scope`: `""` must override a scope from the global `config.yaml` so
///   that `PNPM_CONFIG_SCOPE=` yields an unscoped `pnpm login`. Dropping it
///   would let the lower layer's scope leak through, diverging from the
///   TypeScript CLI.
pub(super) fn read_env_allow_empty<Sys: EnvVar>(suffix: &str) -> Option<String> {
    let upper = format!("PNPM_CONFIG_{suffix}");
    let lower = format!("pnpm_config_{}", suffix.to_lowercase());
    Sys::var(&upper).or_else(|| Sys::var(&lower))
}

/// Parse `value` as JSON. Returns `None` on parse failure so the
/// caller falls through to its default (skip the field).
pub(super) fn parse_json<Target: DeserializeOwned>(value: &str) -> Option<Target> {
    serde_json::from_str(value).ok()
}

/// Parse `value` as JSON; if that fails, retry with `value` wrapped as
/// a JSON string. Used for enum fields whose serde representation is a
/// bare identifier (`hoisted`, `warn-only`, `no-downgrade`, ...) — the
/// raw env var value isn't valid JSON on its own but becomes valid
/// once quoted.
pub(super) fn parse_json_or_string<Target: DeserializeOwned>(value: &str) -> Option<Target> {
    parse_json(value)
        .or_else(|| {
            let quoted = serde_json::to_string(value).ok()?;
            parse_json(&quoted)
        })
}

/// Parse a `hoist_pattern` / `public_hoist_pattern` env var into the
/// tri-state `Option<Option<Vec<String>>>` shape used by
/// [`crate::WorkspaceSettings`].
///
/// Env vars cannot express the "explicit null disable" state that yaml
/// supports: an array-schema env var is JSON-parsed and then required to
/// be an array, so `PNPM_CONFIG_HOIST_PATTERN=null` fails the array check
/// and is silently dropped. The tri-state's `Some(None)` branch stays
/// reachable through yaml only; from env we either return `None` (parse
/// failed, leave config default) or `Some(Some(vec))` (explicit list).
pub(super) fn parse_tri_array(value: &str) -> Option<Option<Vec<String>>> {
    parse_json::<Vec<String>>(value).map(Some)
}
