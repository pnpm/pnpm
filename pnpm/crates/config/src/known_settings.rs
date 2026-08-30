//! Which keys name a setting some supported pnpm reads, and the suggestion
//! for a key that names none.
//!
//! Mirrors `unknownSettings.ts` in pnpm's `config.reader`: the lists below
//! are copies of its lists, so the two stacks agree on which keys get the
//! "not recognized by this version of pnpm" treatment. The pacquet-only
//! settings (e.g. `globalShims`) enter through [`WorkspaceSettings`]'s own
//! field names instead.

use crate::{
    WorkspaceSettings,
    config_types::is_type_key,
    naming_cases::{to_camel_case, to_kebab_case},
};
use std::{collections::HashSet, sync::OnceLock};

/// A YAML-language-server schema association, not a setting; tools put it in
/// config files pnpm reads, so it must not trip the unknown-setting warnings.
pub const SCHEMA_DIRECTIVE_KEY: &str = "$schema";

/// Mirror of the same-named list in `unknownSettings.ts`.
const TYPED_WORKSPACE_MANIFEST_KEYS: &[&str] = &[
    "allowBuilds",
    "allowUnusedPatches",
    "allowedDeprecatedVersions",
    "audit",
    "auditConfig",
    "catalog",
    "catalogs",
    "configDependencies",
    "enableGlobalVirtualStore",
    "httpProxy",
    "httpsProxy",
    "ignoredOptionalDependencies",
    "namedRegistries",
    "nodeDownloadMirrors",
    "noProxy",
    "npmrcAuthFile",
    "overrides",
    "packageExtensions",
    "packages",
    "patchedDependencies",
    "peerDependencyRules",
    "pnprServer",
    "registries",
    "remoteSideEffectsCache",
    "requiredScripts",
    "sideEffectsCache",
    "supportedArchitectures",
    "update",
    "updateConfig",
    "versioning",
    "virtualStoreType",
];

/// Mirror of the same-named list in `unknownSettings.ts`: settings whose only
/// spelling in pnpm is a camelCase config field, plus pnpm's reader-derived
/// fields.
const CONFIG_ONLY_SETTING_KEYS: &[&str] = &[
    "allowNew",
    "auditIgnorePrune",
    "authConfig",
    "autoConfirmAllPrompts",
    "bin",
    "catalogPrune",
    "cleanupUnusedCatalogs",
    "configByUri",
    "enablePnp",
    "extraBinPaths",
    "extraEnv",
    "globalPkgDir",
    "globalPrefix",
    "ignoreCurrentSpecifiers",
    "maxSockets",
    "minimumReleaseAgeExcludePrune",
    "packageConfigs",
    "packageManagerNetworkConfig",
    "packageManagerRegistries",
    "pending",
    "pnpmExecPath",
    "pnpmHomeDir",
    "recursive",
    "registriesByPrefix",
    "registriesByScope",
    "registryOptionsByUrl",
    "reverse",
    "sideEffectsCacheRead",
    "sideEffectsCacheWrite",
    "tryLoadDefaultPnpmfile",
    "useGitBranchLockfile",
    "useLockfile",
    "useRunningStoreServer",
    "useStoreServer",
    "userConfig",
    "workspaceDir",
    "workspacePackagePatterns",
    "workspacePrefix",
];

/// Mirror of the same-named list in `unknownSettings.ts`.
const UNTYPED_WORKSPACE_SETTING_KEYS: &[&str] = &[
    "executionEnv",
    "ignoredBuiltDependencies",
    "neverBuiltDependencies",
    "onlyBuiltDependencies",
    "onlyBuiltDependenciesFile",
];

const SETTINGS_OF_OTHER_PNPM_VERSIONS: &[(&str, &str)] = &[("confirmModulesPurge", "pnpm v11")];

/// The camelCase field names of [`WorkspaceSettings`], read off its serde
/// serialization so the set cannot drift from the struct.
fn settings_field_keys() -> &'static HashSet<String> {
    static SET: OnceLock<HashSet<String>> = OnceLock::new();
    SET.get_or_init(|| match serde_json::to_value(WorkspaceSettings::default()) {
        Ok(serde_json::Value::Object(fields)) => fields.into_iter().map(|(key, _)| key).collect(),
        _ => HashSet::new(),
    })
}

fn known_setting_keys() -> &'static HashSet<String> {
    static SET: OnceLock<HashSet<String>> = OnceLock::new();
    SET.get_or_init(|| {
        TYPED_WORKSPACE_MANIFEST_KEYS
            .iter()
            .chain(CONFIG_ONLY_SETTING_KEYS)
            .chain(UNTYPED_WORKSPACE_SETTING_KEYS)
            .map(|key| (*key).to_string())
            .chain(settings_field_keys().iter().cloned())
            .collect()
    })
}

/// The same corpus in a stable order, so the suggestion picked among
/// equally-close candidates does not vary run to run.
fn known_setting_keys_sorted() -> &'static Vec<String> {
    static LIST: OnceLock<Vec<String>> = OnceLock::new();
    LIST.get_or_init(|| {
        let mut keys: Vec<String> = known_setting_keys().iter().cloned().collect();
        keys.sort_unstable();
        keys
    })
}

/// Whether `key`, given in either camelCase or kebab-case, names a setting
/// some supported pnpm reads from at least one config source. A key failing
/// this check is a typo or belongs to a pnpm version this repository does not
/// know, so it gets the "not recognized by this version of pnpm" warning
/// instead of advice to move it to another config file.
#[must_use]
pub fn is_known_setting_key(key: &str) -> bool {
    known_setting_keys().contains(to_camel_case(key).as_str()) || is_type_key(&to_kebab_case(key))
}

/// Render an unrecognized key for a warning or error, appending the closest
/// known setting name when one is close enough to look like a typo.
///
/// `key` must already be sanitized for display: it comes from a config file a
/// repository may control, and the rendering reaches a terminal or a CI log.
#[must_use]
pub fn annotate_unknown_setting(key: &str) -> String {
    let camel = to_camel_case(key);
    if let Some((_, version)) =
        SETTINGS_OF_OTHER_PNPM_VERSIONS.iter().find(|(setting, _)| *setting == camel)
    {
        return format!(r#""{key}" (a {version} setting)"#);
    }
    match did_you_mean(&camel, known_setting_keys_sorted().iter().map(String::as_str)) {
        Some(suggestion) => format!(r#""{key}" (did you mean "{suggestion}"?)"#),
        None => format!(r#""{key}""#),
    }
}

/// The candidate most similar to `input`, if any clears the similarity
/// threshold `didyoumean2` (pnpm's suggester) applies by default:
/// `1 - distance / max_len >= 0.4`, compared case-insensitively.
fn did_you_mean<'a>(input: &str, candidates: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let input_chars: Vec<char> = input.to_lowercase().chars().collect();
    let mut candidate_chars = Vec::new();
    let mut rows = LevenshteinRows::default();
    let mut best: Option<(&str, f64)> = None;
    for candidate in candidates {
        candidate_chars.clear();
        candidate_chars.extend(candidate.chars().flat_map(char::to_lowercase));
        let max_len = input_chars.len().max(candidate_chars.len());
        if max_len == 0 {
            continue;
        }
        // The distance is at least the length difference, so a candidate whose
        // length alone puts it past the threshold cannot clear it, and the
        // matrix does not have to be filled to find that out.
        if input_chars.len().abs_diff(candidate_chars.len()) > max_len * 3 / 5 {
            continue;
        }
        let distance = rows.levenshtein(&input_chars, &candidate_chars);
        #[expect(clippy::cast_precision_loss, reason = "setting names are far shorter than 2^52")]
        let similarity = 1.0 - distance as f64 / max_len as f64;
        if similarity >= 0.4 && best.is_none_or(|(_, best_similarity)| similarity > best_similarity)
        {
            best = Some((candidate, similarity));
        }
    }
    best.map(|(candidate, _)| candidate)
}

/// The two rows a Levenshtein pass needs, kept across the candidates so the
/// suggestion for one key allocates a bounded number of times rather than
/// twice per candidate.
#[derive(Default)]
struct LevenshteinRows {
    previous: Vec<usize>,
    current: Vec<usize>,
}

impl LevenshteinRows {
    fn levenshtein(&mut self, left: &[char], right: &[char]) -> usize {
        self.previous.clear();
        self.previous.extend(0..=right.len());
        for (row, left_char) in left.iter().enumerate() {
            self.current.clear();
            self.current.push(row + 1);
            for (column, right_char) in right.iter().enumerate() {
                let substitution = self.previous[column] + usize::from(left_char != right_char);
                let insertion = self.current[column] + 1;
                let deletion = self.previous[column + 1] + 1;
                self.current.push(substitution.min(insertion).min(deletion));
            }
            std::mem::swap(&mut self.previous, &mut self.current);
        }
        self.previous[right.len()]
    }
}

#[cfg(test)]
mod tests;
