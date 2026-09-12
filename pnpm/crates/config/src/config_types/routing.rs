use super::{HashSet, OnceLock, npm_config_type_keys};

/// Keys from `pnpmTypes` that are valid in a global config file
/// (`pnpmConfigFileKeys` in `configFileKey.ts`).
const PNPM_CONFIG_FILE_KEYS: &[&str] = &[
    "bail",
    "ci",
    "color",
    "cache-dir",
    "child-concurrency",
    "dangerously-allow-all-builds",
    "enable-modules-dir",
    "enable-global-virtual-store",
    "exclude-links-from-lockfile",
    "extend-node-path",
    "fetch-timeout",
    "fetch-warn-timeout-ms",
    "fetch-min-speed-ki-bps",
    "fetching-concurrency",
    "frozen-store",
    "git-checks",
    "git-shallow-hosts",
    "global-bin-dir",
    "global-shims",
    "global-dir",
    "global-path",
    "global-pnpmfile",
    "global-virtual-store-dir",
    "http-proxy",
    "init-package-manager",
    "init-type",
    "optimistic-repeat-install",
    "loglevel",
    "maxsockets",
    "modules-cache-max-age",
    "dlx-cache-max-age",
    "minimum-release-age",
    "minimum-release-age-exclude",
    "minimum-release-age-ignore-missing-time",
    "minimum-release-age-strict",
    "network-concurrency",
    "node-download-mirrors",
    "node-experimental-package-map",
    "node-package-map-type",
    "noproxy",
    "npm-path",
    "npmrc-auth-file",
    "package-import-method",
    "pnpr-server",
    "prefer-frozen-lockfile",
    "prefer-offline",
    "prefer-symlinked-executables",
    "block-exotic-subdeps",
    "registry-supports-time-field",
    "reporter",
    "resolution-mode",
    "script-shell",
    "shell-emulator",
    "side-effects-cache",
    "side-effects-cache-readonly",
    "state-dir",
    "store-dir",
    "strict-dep-builds",
    "trust-lockfile",
    "trust-policy",
    "trust-policy-exclude",
    "trust-policy-ignore-after",
    "update-notifier",
    "use-beta-cli",
    "use-stderr",
    "verify-deps-before-run",
    "verify-store-integrity",
    "virtual-store-dir",
    "virtual-store-dir-max-length",
    "virtual-store-type",
];

/// Structured YAML settings parsed from `pnpm-workspace.yaml` / global
/// `config.yaml` that have no scalar CLI config type
/// (`structuredConfigFileKeys` in `configFileKey.ts`).
const STRUCTURED_CONFIG_FILE_KEYS: &[&str] = &["named-registries", "registries"];

/// Keys present in `pnpmTypes` but excluded from the global config file
/// (`excludedPnpmKeys` in `configFileKey.ts`) — CLI flags and workspace-only
/// settings.
const EXCLUDED_PNPM_KEYS: &[&str] = &[
    "auto-install-peers",
    "catalog-mode",
    "config-dir",
    "merge-git-branch-lockfiles",
    "merge-git-branch-lockfiles-branch-pattern",
    "deploy-all-files",
    "dedupe-peer-dependents",
    "dedupe-peers",
    "dedupe-direct-deps",
    "dedupe-injected-deps",
    "dev",
    "dir",
    "disallow-workspace-cycles",
    "enable-pre-post-scripts",
    "filter",
    "filter-prod",
    "force-legacy-deploy",
    "frozen-lockfile",
    "git-branch-lockfile",
    "hoist",
    "hoist-pattern",
    "hoist-workspace-packages",
    "hoisting-limits",
    "ignore-compatibility-db",
    "ignore-pnpmfile",
    "ignore-workspace",
    "ignore-workspace-cycles",
    "ignore-workspace-root-check",
    "include-workspace-root",
    "inject-workspace-packages",
    "legacy-dir-filtering",
    "link-workspace-packages",
    "lockfile",
    "lockfile-dir",
    "lockfile-include-tarball-url",
    "lockfile-only",
    "modules-dir",
    "node-linker",
    "offline",
    "pack-destination",
    "pack-gzip-level",
    "patches-dir",
    "pnpmfile",
    "pm-on-fail",
    "prefer-workspace-packages",
    "preserve-absolute-paths",
    "production",
    "public-hoist-pattern",
    "publish-branch",
    "recursive-install",
    "resolve-peers-from-workspace-root",
    "runtime",
    "runtime-on-fail",
    "aggregate-output",
    "reporter-hide-prefix",
    "save-catalog-name",
    "save-peer",
    "save-workspace-protocol",
    "shamefully-hoist",
    "shared-workspace-lockfile",
    "symlink",
    "sort",
    "stream",
    "strict-store-pkg-content-check",
    "strict-peer-dependencies",
    "virtual-store-only",
    "peers-suffix-max-length",
    "workspace-concurrency",
    "workspace-packages",
    "workspace-root",
    "test-pattern",
    "changed-files-ignore-pattern",
    "embed-readme",
    "skip-manifest-obfuscation",
    "fail-if-no-match",
    "sync-injected-deps-after-scripts",
    "cpu",
    "libc",
    "os",
    "audit-level",
    "yes",
];

/// The npm auth settings recognized by [`is_ini_config_key`]
/// ([`NPM_AUTH_SETTINGS`] in `localConfig.ts`).
const NPM_AUTH_SETTINGS: &[&str] = &[
    "ca",
    "cafile",
    "cert",
    "key",
    "registry",
    "_auth",
    "_authToken",
    "_password",
    "email",
    "username",
];

fn pnpm_config_file_keys() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| PNPM_CONFIG_FILE_KEYS.iter().copied().collect())
}

fn structured_config_file_keys() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| STRUCTURED_CONFIG_FILE_KEYS.iter().copied().collect())
}

fn excluded_pnpm_keys() -> &'static HashSet<&'static str> {
    static SET: OnceLock<HashSet<&'static str>> = OnceLock::new();
    SET.get_or_init(|| EXCLUDED_PNPM_KEYS.iter().copied().collect())
}

/// Whether `key` would be read from an INI config file (auth / scoped /
/// per-registry). Mirrors `isIniConfigKey`.
#[must_use]
pub fn is_ini_config_key(key: &str) -> bool {
    key.starts_with('@') || key.starts_with("//") || NPM_AUTH_SETTINGS.contains(&key)
}

/// Whether `kebab_key` is valid in a global config file. Mirrors
/// `isConfigFileKey`: a pnpm config-file key, or an npm-compatible key that
/// is not in the excluded list.
#[must_use]
pub fn is_config_file_key(kebab_key: &str) -> bool {
    pnpm_config_file_keys().contains(kebab_key)
        || structured_config_file_keys().contains(kebab_key)
        || (npm_config_type_keys().contains(kebab_key) && !excluded_pnpm_keys().contains(kebab_key))
}
