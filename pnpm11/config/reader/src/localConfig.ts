import type { Config } from './Config.js'
import { type InheritableConfigPair, inheritPickedConfig } from './inheritPickedConfig.js'
import type { types } from './types.js'

const RAW_AUTH_CFG_KEYS = [
  'ca',
  'cafile',
  'cert',
  'key',
  'registry',
] satisfies Array<keyof typeof types>

/**
 * Network-related keys that should be readable from .npmrc (for migration from npm)
 * but written to YAML config files (config.yaml / pnpm-workspace.yaml).
 */
const NETWORK_INI_KEYS = [
  'https-proxy',
  'proxy',
  'no-proxy',
  'http-proxy',
  'local-address',
  'strict-ssl',
]

const RAW_AUTH_CFG_KEY_SUFFIXES = [
  ':ca',
  ':cafile',
  ':cert',
  ':certfile',
  ':key',
  ':keyfile',
  ':registry',
  ':tokenHelper',
  ':_auth',
  ':_authToken',
]

const AUTH_CFG_KEYS = [
  'ca',
  'cert',
  'configByUri',
  'key',
  'registry',
  'registriesByScope',
] satisfies Array<keyof Config>

/**
 * Config key categories inherited by `pnpm dlx`.
 *
 * ## Principle
 *
 * `pnpm dlx` runs packages in isolation from the current project. It must not
 * read project-structural settings (hoisting, linking, workspace layout, etc.)
 * from local config. However, several categories of local settings DO apply:
 *
 * 1. **Registry & auth:** needed to reach the same package sources
 *    (registries, tokens, certificates).
 * 2. **Node.js download mirrors:** same idea for Node.js runtime tarballs when
 *    those URIs are declared in `pnpm-workspace.yaml` — but only the `release`
 *    channel. Node publishes a signed `SHASUMS256.txt` for `release` alone, and
 *    the resolver verifies it (`verifySignature: releaseChannel === 'release'`).
 *    A workspace-supplied `rc`/`nightly` mirror would hand a repository both the
 *    archive and its checksum for a runtime that `dlx` then executes.
 * 3. **Security & trust policy:** these reflect the user's or organization's
 *    security posture and must apply regardless of how a package is installed.
 *    A setting that answers "what am I allowed to download?" belongs here.
 * 4. **Catalogs:** the `catalog:` protocol resolves package versions through
 *    workspace catalog entries; without them, `pnpm dlx pkg@catalog:...` cannot
 *    look up the requested version.
 * 5. **Fetch retry/timeout:** governs how the client talks to the registry.
 *    These reflect the same network environment as a regular install.
 *
 * ## Rules
 *
 * | Category                       | Inherited by dlx? | Examples                                         |
 * |--------------------------------|--------------------|--------------------------------------------------|
 * | Registry & auth                | Yes                | registry, _authToken, ca                         |
 * | Node.js download mirrors       | `release` only     | nodeDownloadMirrors.release (signed SHASUMS)      |
 * | Security & trust policy        | Yes                | minimumReleaseAge, trustPolicy                   |
 * | Catalogs                       | Yes                | catalogs                                         |
 * | Fetch retry/timeout            | Yes                | fetchRetries, fetchTimeout                       |
 * | Installation structure         | No                 | shamefully-hoist, node-linker, hoist-pattern      |
 * | Workspace settings             | No                 | link-workspace-packages, shared-workspace-lockfile|
 * | Resolution strategy            | No                 | resolution-mode, dedupe-peers                     |
 */
const SECURITY_POLICY_CFG_KEYS = [
  'minimumReleaseAge',
  'minimumReleaseAgeExclude',
  'minimumReleaseAgeIgnoreMissingTime',
  'minimumReleaseAgeStrict',
  'trustLockfile',
  'trustPolicy',
  'trustPolicyExclude',
  'trustPolicyIgnoreAfter',
] satisfies Array<keyof Config>

const CATALOGS_CFG_KEYS = [
  'catalogs',
] satisfies Array<keyof Config>

const FETCH_CFG_KEYS = [
  'fetchRetryFactor',
  'fetchRetryMaxtimeout',
  'fetchRetryMintimeout',
  'fetchRetries',
  'fetchTimeout',
] satisfies Array<keyof Config>

const NPM_AUTH_SETTINGS = [
  ...RAW_AUTH_CFG_KEYS,
  '_auth',
  '_authToken',
  '_password',
  'email',
  'username',
]

function isRawAuthCfgKey (rawCfgKey: string): boolean {
  if ((RAW_AUTH_CFG_KEYS as string[]).includes(rawCfgKey)) return true
  if (RAW_AUTH_CFG_KEY_SUFFIXES.some(suffix => rawCfgKey.endsWith(suffix))) return true
  return false
}

function isAuthCfgKey (cfgKey: keyof Config): cfgKey is typeof AUTH_CFG_KEYS[number] {
  return (AUTH_CFG_KEYS as Array<keyof Config>).includes(cfgKey)
}

function isSecurityPolicyCfgKey (cfgKey: keyof Config): cfgKey is typeof SECURITY_POLICY_CFG_KEYS[number] {
  return (SECURITY_POLICY_CFG_KEYS as Array<keyof Config>).includes(cfgKey)
}

function isCatalogsCfgKey (cfgKey: keyof Config): cfgKey is typeof CATALOGS_CFG_KEYS[number] {
  return (CATALOGS_CFG_KEYS as Array<keyof Config>).includes(cfgKey)
}

function isFetchCfgKey (cfgKey: keyof Config): cfgKey is typeof FETCH_CFG_KEYS[number] {
  return (FETCH_CFG_KEYS as Array<keyof Config>).includes(cfgKey)
}

function pickRawAuthConfig<RawLocalCfg extends Record<string, unknown>> (rawLocalCfg: RawLocalCfg): Partial<RawLocalCfg> {
  const result: Partial<RawLocalCfg> = {}
  for (const key in rawLocalCfg) {
    if (isRawAuthCfgKey(key)) {
      result[key] = rawLocalCfg[key]
    }
  }
  return result
}

function pickAuthConfig (localCfg: Partial<Config>): Partial<Config> {
  const result: Record<string, unknown> = {}
  for (const key in localCfg) {
    if (isAuthCfgKey(key as keyof Config)) {
      result[key] = localCfg[key as keyof Config]
    }
  }
  return result as Partial<Config>
}

function pickDlxConfig (localCfg: Partial<Config>): Partial<Config> {
  const result: Record<string, unknown> = {}
  for (const key in localCfg) {
    if (
      isAuthCfgKey(key as keyof Config) ||
      isSecurityPolicyCfgKey(key as keyof Config) ||
      isCatalogsCfgKey(key as keyof Config) ||
      isFetchCfgKey(key as keyof Config)
    ) {
      result[key] = localCfg[key as keyof Config]
    }
  }
  return result as Partial<Config>
}

export function inheritAuthConfig (target: InheritableConfigPair, src: InheritableConfigPair): void {
  inheritPickedConfig(target, src, pickAuthConfig, pickRawAuthConfig)
}

/**
 * Inherits the categories listed above (auth/registry, security & trust
 * policy, catalogs, and fetch retry/timeout) from a local config source
 * into the target config.
 */
export function inheritDlxConfig (target: InheritableConfigPair, src: InheritableConfigPair): void {
  inheritPickedConfig(target, src, pickDlxConfig, pickRawAuthConfig)
  inheritReleaseNodeDownloadMirror(target, src)
}

/**
 * Inherits only `nodeDownloadMirrors.release` from the local config.
 *
 * Node publishes a signed `SHASUMS256.txt` for the `release` channel only, and
 * the runtime resolver verifies that signature for `release` alone. A mirror
 * inherited for `rc` or `nightly` would let the local project choose both the
 * archive and the checksum for a runtime that `dlx` executes, so those entries
 * stay with whatever the user configured globally.
 *
 * This cannot live in `pickDlxConfig`: `inheritPickedConfig` shallow-assigns the
 * picked keys, so returning a one-entry map there would discard the user's own
 * mirrors for the other channels instead of leaving them alone.
 */
function inheritReleaseNodeDownloadMirror (target: InheritableConfigPair, src: InheritableConfigPair): void {
  const release = src.config.nodeDownloadMirrors?.release
  if (release == null) return
  target.config.nodeDownloadMirrors = {
    ...target.config.nodeDownloadMirrors,
    release,
  }
}

/**
 * Whether the config key would be read from an INI config file.
 */
export const isIniConfigKey = (key: string): boolean =>
  key.startsWith('@') || key.startsWith('//') || NPM_AUTH_SETTINGS.includes(key)

/**
 * Whether the config key should be read from .npmrc files.
 */
export const isNpmrcReadableKey = (key: string): boolean =>
  isIniConfigKey(key) || NETWORK_INI_KEYS.includes(key)

/**
 * Filter keys that are allowed to be read from an INI config file.
 */
export function pickIniConfig<RawConfig extends Record<string, unknown>> (rawConfig: RawConfig): Partial<RawConfig> {
  const result: Partial<RawConfig> = {}

  for (const key in rawConfig) {
    if (isIniConfigKey(key)) {
      result[key] = rawConfig[key]
    }
  }

  return result
}
