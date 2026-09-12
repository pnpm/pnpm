use super::{
    HashSet, LinkWorkspacePackages, OsStr, PackageImportMethod, PmOnFail, RuntimeOnFail,
    SaveWorkspaceProtocol, TrustPolicy,
};

/// Whether the token at `index` belongs to the forwarded child command line.
pub(super) fn is_forwarded(passthrough_from: Option<usize>, index: usize) -> bool {
    passthrough_from.is_some_and(|from| index >= from)
}

/// Presence-only, like pnpm's `!= null` check: an empty value still
/// overrides (it disables the gate on the env-overlay side).
pub(super) fn verify_deps_env_is_set() -> bool {
    ["PNPM_CONFIG_VERIFY_DEPS_BEFORE_RUN", pnpm_executor::VERIFY_DEPS_BEFORE_RUN_ENV]
        .iter()
        .any(|name| std::env::var(name).is_ok())
}

pub(super) enum ConfigToken<'a> {
    WellFormed {
        key: &'a str,
        value: &'a str,
    },
    /// A bare `--<setting>` for a boolean setting: `true`, unless the next
    /// argv token spells a boolean and is claimed as its value.
    BooleanFollows(&'static str),
    /// A bare `--<setting>` for a value-taking setting: the next argv
    /// token is its value.
    ValueFollows(&'static str),
    Malformed,
    NotOurs,
}

/// Decide whether an argv token belongs to the `--config.<key>=<value>`
/// family or is one of the [`BARE_SETTING_FLAGS`]. Everything with a
/// `--config.` prefix is claimed, so a typo like `--config.foo` never
/// escapes into clap's "unexpected argument" path; every other token is
/// returned untouched.
///
/// `claimed_by_command` names the options the invoked command declares
/// itself, which win over the setting of the same name — see
/// [`subcommand_option_names`].
///
/// A setting given a value it does not take — a misspelled boolean, an
/// unknown `--trust-policy`, a non-numeric `--child-concurrency` — is
/// left for clap, which reports it as an unexpected argument. Dropping
/// it instead would leave the install running under a setting the user
/// believes they changed, which for a supply-chain setting like
/// `trustPolicy` means failing open.
///
/// [`subcommand_option_names`]: crate::parse_boundary::subcommand_option_names
pub(super) fn classify<'a>(arg: &'a OsStr, claimed_by_command: &HashSet<&str>) -> ConfigToken<'a> {
    let setting =
        |key: &str| named_bare_setting_flag(key).filter(|_| !claimed_by_command.contains(key));
    let Some(arg) = arg.to_str() else {
        return ConfigToken::NotOurs;
    };
    if let Some(rest) = arg.strip_prefix("--config.") {
        return classify_dotted(rest);
    }
    let Some(flag) = arg.strip_prefix("--") else {
        return ConfigToken::NotOurs;
    };
    if let Some(negated) = flag.strip_prefix("no-")
        && let Some((key, SettingArity::Boolean | SettingArity::BooleanOr { .. })) =
            setting(negated)
    {
        return ConfigToken::WellFormed { key, value: "false" };
    }
    let Some((key, value)) = flag.split_once('=') else {
        return match setting(flag) {
            Some((key, SettingArity::Boolean | SettingArity::BooleanOr { .. })) => {
                ConfigToken::BooleanFollows(key)
            }
            Some((key, _)) => ConfigToken::ValueFollows(key),
            None => ConfigToken::NotOurs,
        };
    };
    classify_valued_flag(key, value, setting(key))
}

/// A `--config.<key>=<value>` token, after the prefix. Everything the
/// prefix claims stays claimed, so a typo like `--config.foo` never
/// escapes into clap's "unexpected argument" path.
fn classify_dotted(rest: &str) -> ConfigToken<'_> {
    let Some((key, value)) = rest.split_once('=') else {
        return ConfigToken::Malformed;
    };
    if key.is_empty() {
        return ConfigToken::Malformed;
    }
    // The dotted spelling names a setting outright, so a command option of
    // the same name never shadows it.
    if !setting_takes(key, value) {
        return ConfigToken::NotOurs;
    }
    ConfigToken::WellFormed { key, value }
}

/// A `--<setting>=<value>` token, given what the setting table says about
/// `key`.
fn classify_valued_flag<'a>(
    key: &str,
    value: &'a str,
    setting: Option<(&'static str, SettingArity)>,
) -> ConfigToken<'a> {
    match setting {
        Some((_, SettingArity::BooleanOr { bare_keyword: false, .. }))
            if parse_bool(value).is_none() =>
        {
            ConfigToken::NotOurs
        }
        Some(_) if !setting_takes(key, value) => ConfigToken::NotOurs,
        Some((key, _)) => ConfigToken::WellFormed { key, value },
        None => ConfigToken::NotOurs,
    }
}

/// How much of argv a bare `--<setting>` flag claims, and which values it
/// takes there. A setting whose value is a list repeats the flag, one
/// value per occurrence.
#[derive(Debug, Clone, Copy)]
pub(super) enum SettingArity {
    /// `--<setting>`, `--no-<setting>`, `--<setting>=<bool>`, and
    /// `--<setting> <bool>`.
    Boolean,
    /// Every [`Boolean`] spelling, plus a keyword `takes` accepts, for a
    /// setting whose type is a boolean or a keyword. Only a boolean is
    /// claimed from the token after the flag.
    ///
    /// `bare_keyword` says whether the keyword is spellable as
    /// `--<setting>=<keyword>`, which follows the `nopt` type pnpm gives
    /// the setting: `linkWorkspacePackages` is `[Boolean, 'deep']`, so
    /// `--link-workspace-packages=deep` parses; `saveWorkspaceProtocol` is
    /// `Boolean` alone, so `rolling` reaches it only through the untyped
    /// `--config.` form.
    ///
    /// [`Boolean`]: Self::Boolean
    BooleanOr { takes: fn(&str) -> bool, bare_keyword: bool },
    /// A path, a glob pattern, or another free-form value: every spelling
    /// is one the setting takes, so the token after the flag is claimed
    /// only when it cannot be anything else — see [`claims_as_value`].
    Text,
    /// A value the carried predicate accepts, and only such a value.
    Parsed(fn(&str) -> bool),
}

/// Settings pnpm accepts as a bare `--<setting>` command-line flag, with
/// the values each takes.
///
/// pnpm declares a `nopt` type for every setting, which makes all of them
/// spellable on the command line; pacquet declares a clap flag for only a
/// subset, so the rest are recognized here and layered onto [`Config`](pnpm_config::Config)
/// exactly like a `--config.<setting>=<value>` token
/// ([pnpm/pnpm#14281](https://github.com/pnpm/pnpm/issues/14281)). A
/// setting the invoked command declares as its own option is left for
/// clap; a setting that collides with a *global* option would be claimed
/// on every command line and so must not appear here at all.
pub(super) const BARE_SETTING_FLAGS: [(&str, SettingArity); 39] = [
    ("allow-unused-patches", SettingArity::Boolean),
    ("child-concurrency", SettingArity::Parsed(is_i32)),
    ("dangerously-allow-all-builds", SettingArity::Boolean),
    ("engine-strict", SettingArity::Boolean),
    ("force-legacy-deploy", SettingArity::Boolean),
    ("frozen-store", SettingArity::Boolean),
    ("global-dir", SettingArity::Text),
    ("hoist", SettingArity::Boolean),
    ("hoist-pattern", SettingArity::Text),
    ("ignore-pnpmfile", SettingArity::Boolean),
    ("ignore-scripts", SettingArity::Boolean),
    (
        "link-workspace-packages",
        SettingArity::BooleanOr { takes: is_enum::<LinkWorkspacePackages>, bare_keyword: true },
    ),
    ("lockfile", SettingArity::Boolean),
    ("lockfile-include-tarball-url", SettingArity::Boolean),
    ("merge-git-branch-lockfiles", SettingArity::Boolean),
    ("modules-dir", SettingArity::Text),
    ("node-experimental-package-map", SettingArity::Boolean),
    ("offline", SettingArity::Boolean),
    ("optimistic-repeat-install", SettingArity::Boolean),
    ("package-import-method", SettingArity::Parsed(is_enum::<PackageImportMethod>)),
    ("pm-on-fail", SettingArity::Parsed(is_enum::<PmOnFail>)),
    ("prefer-frozen-lockfile", SettingArity::Boolean),
    ("prefer-offline", SettingArity::Boolean),
    ("public-hoist-pattern", SettingArity::Text),
    ("runtime-on-fail", SettingArity::Parsed(is_enum::<RuntimeOnFail>)),
    (
        "save-workspace-protocol",
        SettingArity::BooleanOr { takes: is_enum::<SaveWorkspaceProtocol>, bare_keyword: false },
    ),
    ("shamefully-hoist", SettingArity::Boolean),
    ("shared-workspace-lockfile", SettingArity::Boolean),
    ("side-effects-cache", SettingArity::Boolean),
    ("side-effects-cache-readonly", SettingArity::Boolean),
    ("strict-peer-dependencies", SettingArity::Boolean),
    ("trust-lockfile", SettingArity::Boolean),
    ("trust-policy", SettingArity::Parsed(is_enum::<TrustPolicy>)),
    ("trust-policy-exclude", SettingArity::Text),
    ("trust-policy-ignore-after", SettingArity::Parsed(is_u64)),
    ("unsafe-perm", SettingArity::Boolean),
    ("verify-store-integrity", SettingArity::Boolean),
    ("virtual-store-dir", SettingArity::Text),
    ("virtual-store-only", SettingArity::Boolean),
];

fn is_i32(value: &str) -> bool {
    value.parse::<i32>().is_ok()
}

fn is_u64(value: &str) -> bool {
    value.parse::<u64>().is_ok()
}

fn is_enum<Value: serde::de::DeserializeOwned>(value: &str) -> bool {
    parse_enum::<Value>(value).is_some()
}

fn named_bare_setting_flag(key: &str) -> Option<(&'static str, SettingArity)> {
    BARE_SETTING_FLAGS.into_iter().find(|&(name, _)| name == key)
}

/// Whether `value` is a spelling the `key` setting takes. `true` for a
/// key outside [`BARE_SETTING_FLAGS`], which keeps the `--config.<key>`
/// tolerance for the settings pacquet has not ported.
fn setting_takes(key: &str, value: &str) -> bool {
    match named_bare_setting_flag(key) {
        Some((_, SettingArity::Boolean)) => parse_bool(value).is_some(),
        Some((_, SettingArity::BooleanOr { takes, .. })) => {
            parse_bool(value).is_some() || takes(value)
        }
        Some((_, SettingArity::Text)) => true,
        Some((_, SettingArity::Parsed(takes))) => takes(value),
        None => true,
    }
}

/// Whether the `key` setting claims `token` — the argv token after its
/// flag — as its value.
///
/// A free-form setting refuses one that opens with `-`: that token is the
/// `--` separator, another flag, or a short option, and claiming it would
/// drop the separator or point a path setting at a directory named `--`.
/// A parsed setting decides on its own terms instead, which is what lets
/// `--child-concurrency -1` mean "every core but one".
pub(super) fn claims_as_value(key: &str, token: &str) -> bool {
    match named_bare_setting_flag(key) {
        Some((_, SettingArity::Boolean | SettingArity::BooleanOr { .. })) => {
            is_boolean_value(token)
        }
        Some((_, SettingArity::Text)) => !token.starts_with('-'),
        Some((_, SettingArity::Parsed(takes))) => takes(token),
        None => false,
    }
}

/// Whether a token spells a boolean a bare `--<setting>` flag claims as
/// its value — the same spellings the `--<setting>=<bool>` form takes,
/// so the two agree. Only a boolean is claimed, which is what leaves
/// `pnpm --shamefully-hoist install` its command.
fn is_boolean_value(token: &str) -> bool {
    parse_bool(token).is_some()
}

/// Whether a bare boolean setting flag claims `next` as its value.
///
/// The scan that has to find the subcommand applies this ahead of clap's
/// own arity: nothing else on a command line is spelled `true` /
/// `false`, so stepping over that token is right whichever reading of
/// the flag ends up applying — a command's option of the same name, or
/// the setting. Without it the two readings disagree on width for a name
/// that is both, and `pnpm --lockfile true --config.registry=… install`
/// loses everything past the boolean to the script fallback.
pub(crate) fn bare_boolean_setting_claims(flag: &str, next: Option<&str>) -> bool {
    matches!(
        named_bare_setting_flag(flag),
        Some((_, SettingArity::Boolean | SettingArity::BooleanOr { .. })),
    ) && next.is_some_and(is_boolean_value)
}

/// How many argv slots a bare `--<setting>` flag occupies, given the
/// token after it — for the scan that has to find the subcommand before
/// the settings are stripped. `1` for a token that is not one of the
/// [`BARE_SETTING_FLAGS`], or one whose value is missing, which
/// [`ConfigOverrides::extract`](super::ConfigOverrides::extract) hands to clap intact.
pub(crate) fn bare_setting_flag_width(flag: &str, next: Option<&str>) -> usize {
    1 + usize::from(next.is_some_and(|next| claims_as_value(flag, next)))
}

pub(super) fn scoped_registry_key(key: &str) -> Option<&str> {
    key.strip_suffix(":registry")
        .filter(|scope| scope.starts_with('@') && scope.len() > 1 && !scope.contains('/'))
}

pub(super) fn parse_enum<Value: serde::de::DeserializeOwned>(value: &str) -> Option<Value> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).ok()
}

/// A setting whose type is a boolean or a keyword, from either spelling.
pub(super) fn parse_bool_or_enum<Value: serde::de::DeserializeOwned>(value: &str) -> Option<Value> {
    match parse_bool(value) {
        Some(boolean) => serde_json::from_value(serde_json::Value::Bool(boolean)).ok(),
        None => parse_enum(value),
    }
}

/// An enum setting's kebab-case spelling, for [`Config::explicit_settings`](pnpm_config::Config::explicit_settings)
/// — the form the config files and `pnpm config get` use.
pub(super) fn setting_value<Value: serde::Serialize>(value: Value) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

pub(crate) fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}
