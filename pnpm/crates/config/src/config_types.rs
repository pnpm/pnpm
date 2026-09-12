//! The config-key `types` registry and the file-routing predicates the
//! `pnpm config` command builds on.
//!
//! Contents:
//! - the merged `types` table (`pnpmTypes` ∪ `npmConfigTypes`),
//!   reduced to the only fact the config command reads off a type: whether the
//!   type list includes `Number` (used by `castField` to coerce a value).
//! - [`is_ini_config_key`] / [`is_config_file_key`].

pub use routing::{is_config_file_key, is_ini_config_key};

use std::{collections::HashSet, sync::OnceLock};

/// `(kebab-key, type-includes-Number)` for pnpm's own settings (`pnpmTypes`).
const PNPM_TYPES: &[(&str, bool)] = &[
    ("auto-install-peers", false),
    ("bail", false),
    ("ci", false),
    ("cache-dir", false),
    ("catalog-mode", false),
    ("child-concurrency", true),
    ("merge-git-branch-lockfiles", false),
    ("merge-git-branch-lockfiles-branch-pattern", false),
    ("color", false),
    ("config-dir", false),
    ("dangerously-allow-all-builds", false),
    ("deploy-all-files", false),
    ("dedupe-peer-dependents", false),
    ("dedupe-peers", false),
    ("dedupe-direct-deps", false),
    ("dedupe-injected-deps", false),
    ("dev", false),
    ("dir", false),
    ("disallow-workspace-cycles", false),
    ("enable-modules-dir", false),
    ("enable-pre-post-scripts", false),
    ("enable-global-virtual-store", false),
    ("exclude-links-from-lockfile", false),
    ("extend-node-path", false),
    ("fetch-timeout", true),
    ("fetch-warn-timeout-ms", true),
    ("fetch-min-speed-ki-bps", true),
    ("fetching-concurrency", true),
    ("filter", false),
    ("filter-prod", false),
    ("force-legacy-deploy", false),
    ("frozen-lockfile", false),
    ("git-checks", false),
    ("git-shallow-hosts", false),
    ("global-bin-dir", false),
    ("global-dir", false),
    ("global-path", false),
    ("global-pnpmfile", false),
    ("global-shims", false),
    ("git-branch-lockfile", false),
    ("hoist", false),
    ("http-proxy", false),
    ("hoist-pattern", false),
    ("hoist-workspace-packages", false),
    ("hoisting-limits", false),
    ("ignore-compatibility-db", false),
    ("ignore-pnpmfile", false),
    ("ignore-workspace", false),
    ("ignore-workspace-cycles", false),
    ("ignore-workspace-root-check", false),
    ("optimistic-repeat-install", false),
    ("include-workspace-root", false),
    ("init-package-manager", false),
    ("init-type", false),
    ("inject-workspace-packages", false),
    ("legacy-dir-filtering", false),
    ("link-workspace-packages", false),
    ("lockfile", false),
    ("lockfile-dir", false),
    ("lockfile-include-tarball-url", false),
    ("lockfile-only", false),
    ("loglevel", false),
    ("maxsockets", true),
    ("modules-cache-max-age", true),
    ("dlx-cache-max-age", true),
    ("minimum-release-age", true),
    ("minimum-release-age-exclude", false),
    ("minimum-release-age-ignore-missing-time", false),
    ("minimum-release-age-strict", false),
    ("modules-dir", false),
    ("network-concurrency", true),
    ("node-experimental-package-map", false),
    ("node-package-map-type", false),
    ("node-linker", false),
    ("noproxy", false),
    ("npm-path", false),
    ("npmrc-auth-file", false),
    ("offline", false),
    ("pack-destination", false),
    ("pack-gzip-level", true),
    ("package-import-method", false),
    ("patches-dir", false),
    ("pnpmfile", false),
    ("pm-on-fail", false),
    ("prefer-frozen-lockfile", false),
    ("prefer-offline", false),
    ("prefer-symlinked-executables", false),
    ("prefer-workspace-packages", false),
    ("preserve-absolute-paths", false),
    ("production", false),
    ("public-hoist-pattern", false),
    ("publish-branch", false),
    ("recursive-install", false),
    ("block-exotic-subdeps", false),
    ("reporter", false),
    ("resolution-mode", false),
    ("resolve-peers-from-workspace-root", false),
    ("runtime", false),
    ("runtime-on-fail", false),
    ("aggregate-output", false),
    ("reporter-hide-prefix", false),
    ("save-peer", false),
    ("save-catalog-name", false),
    ("save-workspace-protocol", false),
    ("script-shell", false),
    ("shamefully-hoist", false),
    ("shared-workspace-lockfile", false),
    ("shell-emulator", false),
    ("side-effects-cache", false),
    ("side-effects-cache-readonly", false),
    ("symlink", false),
    ("sort", false),
    ("state-dir", false),
    ("store-dir", false),
    ("stream", false),
    ("strict-dep-builds", false),
    ("strict-store-pkg-content-check", false),
    ("strict-peer-dependencies", false),
    ("trust-lockfile", false),
    ("trust-policy", false),
    ("trust-policy-exclude", false),
    ("trust-policy-ignore-after", true),
    ("use-beta-cli", false),
    ("use-stderr", false),
    ("verify-deps-before-run", false),
    ("verify-store-integrity", false),
    ("frozen-store", false),
    ("global-virtual-store-dir", false),
    ("virtual-store-dir", false),
    ("virtual-store-only", false),
    ("virtual-store-type", false),
    ("virtual-store-dir-max-length", true),
    ("peers-suffix-max-length", true),
    ("workspace-concurrency", true),
    ("workspace-packages", false),
    ("workspace-root", false),
    ("yes", false),
    ("test-pattern", false),
    ("changed-files-ignore-pattern", false),
    ("embed-readme", false),
    ("skip-manifest-obfuscation", false),
    ("update-notifier", false),
    ("pnpr-server", false),
    ("registry-supports-time-field", false),
    ("fail-if-no-match", false),
    ("sync-injected-deps-after-scripts", false),
    ("cpu", false),
    ("libc", false),
    ("os", false),
    ("audit-level", false),
];

/// `(kebab-key, type-includes-Number)` for the npm-compatible settings
/// (`npmConfigTypes`). Applied after [`PNPM_TYPES`], so on the few
/// overlapping keys these definitions win — but none of the overlaps change
/// number-ness, so the merged "includes Number" answer is the union.
const NPM_CONFIG_TYPES: &[(&str, bool)] = &[
    ("access", false),
    ("allow-same-version", false),
    ("bin-links", false),
    ("ca", false),
    ("cafile", false),
    ("cert", false),
    ("commit-hooks", false),
    ("depth", true),
    ("description", false),
    ("dev", false),
    ("dry-run", false),
    ("engine-strict", false),
    ("fetch-retries", true),
    ("fetch-retry-factor", true),
    ("fetch-retry-mintimeout", true),
    ("fetch-retry-maxtimeout", true),
    ("force", false),
    ("git", false),
    ("git-tag-version", false),
    ("global", false),
    ("https-proxy", false),
    ("ignore-scripts", false),
    ("init-author-name", false),
    ("init-author-email", false),
    ("init-author-url", false),
    ("init-license", false),
    ("init-version", false),
    ("json", false),
    ("key", false),
    ("local-address", false),
    ("long", false),
    ("maxsockets", true),
    ("message", false),
    ("node-options", false),
    ("node-version", false),
    ("no-proxy", false),
    ("offline", false),
    ("only", false),
    ("optional", false),
    ("otp", false),
    ("package-lock", false),
    ("parseable", false),
    ("prefer-offline", false),
    ("prefix", false),
    ("production", false),
    ("progress", false),
    ("provenance", false),
    ("proxy", false),
    ("registry", false),
    ("save", false),
    ("save-dev", false),
    ("save-exact", false),
    ("save-optional", false),
    ("save-prefix", false),
    ("save-prod", false),
    ("scope", false),
    ("script-shell", false),
    ("scripts-prepend-node-path", false),
    ("sign-git-tag", false),
    ("strict-ssl", false),
    ("tag", false),
    ("tag-version-prefix", false),
    ("unsafe-perm", false),
    ("user-agent", false),
    ("userconfig", false),
    ("umask", true),
    ("version", false),
];

fn numeric_type_keys() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| {
        let typed = PNPM_TYPES.iter().chain(NPM_CONFIG_TYPES);
        typed.filter(|(_, is_number)| *is_number).map(|(key, _)| *key).collect()
    })
}

fn all_type_keys() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| PNPM_TYPES.iter().chain(NPM_CONFIG_TYPES).map(|(key, _)| *key).collect())
}

fn npm_config_type_keys() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| NPM_CONFIG_TYPES.iter().map(|(key, _)| *key).collect())
}

/// Whether `kebab_key` is a known config key (an own key of pnpm's merged
/// `types` table). Mirrors `Object.hasOwn(types, kebabKey)`.
#[must_use]
pub fn is_type_key(kebab_key: &str) -> bool {
    all_type_keys().contains(kebab_key)
}

/// Whether the type of `kebab_key` includes `Number`, i.e. a string value
/// should be coerced to a number. Mirrors `castField`'s `typeList.includes(Number)`.
#[must_use]
pub fn type_includes_number(kebab_key: &str) -> bool {
    numeric_type_keys().contains(kebab_key)
}

#[cfg(test)]
mod tests;

mod routing;
