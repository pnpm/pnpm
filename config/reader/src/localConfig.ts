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
  'registries',
] satisfies Array<keyof Config>

/**
 * Security policy config keys.
 *
 * ## Principle
 *
 * `pnpm dlx` runs packages in isolation from the current project. It must not
 * read project-structural settings (hoisting, linking, workspace layout, etc.)
 * from local config. However, two categories of local settings DO apply:
 *
 * 1. **Registry & auth:** needed to reach the same package sources
 *    (registries, tokens, certificates).
 * 2. **Security & trust policy:** these reflect the user's or organization's
 *    security posture and must apply regardless of how a package is installed.
 *    A setting that answers "what am I allowed to download?" belongs here.
 *
 * Other settings are intentionally excluded. These are the ones that control
 * how downloaded packages are arranged in `node_modules` (hoisting, linking,
 * workspace layout, etc.).
 *
 * ## Rules
 *
 * | Category                       | Inherited by dlx? | Examples                                         |
 * |--------------------------------|--------------------|--------------------------------------------------|
 * | Registry & auth                | Yes                | registry, _authToken, ca                         |
 * | Security & trust policy        | Yes                | minimumReleaseAge, trustPolicy                   |
 * | Installation structure         | No                 | shamefully-hoist, node-linker, hoist-pattern      |
 * | Workspace settings             | No                 | link-workspace-packages, shared-workspace-lockfile|
 * | Resolution strategy            | No                 | resolution-mode, dedupe-peers                     |
 */
const SECURITY_POLICY_CFG_KEYS = [
  'minimumReleaseAge',
  'minimumReleaseAgeExclude',
  'minimumReleaseAgeIgnoreMissingTime',
  'minimumReleaseAgeStrict',
  'trustPolicy',
  'trustPolicyExclude',
  'trustPolicyIgnoreAfter',
] satisfies Array<keyof Config>

const NPM_AUTH_SETTINGS = [
  ...RAW_AUTH_CFG_KEYS,
  '_auth',
  '_authToken',
  '_password',
  'email',
  'keyfile',
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
    if (isAuthCfgKey(key as keyof Config) || isSecurityPolicyCfgKey(key as keyof Config)) {
      result[key] = localCfg[key as keyof Config]
    }
  }
  return result as Partial<Config>
}

export function inheritAuthConfig (target: InheritableConfigPair, src: InheritableConfigPair): void {
  inheritPickedConfig(target, src, pickAuthConfig, pickRawAuthConfig)
}

/**
 * Inherits both auth/registry settings and security/trust policy settings
 * from a local config source into the target config.
 *
 * Used by `pnpm dlx` and `pnpm create` so that these commands respect
 * the local project's registry authentication and security policies
 * while ignoring project-structural settings.
 */
export function inheritDlxConfig (target: InheritableConfigPair, src: InheritableConfigPair): void {
  inheritPickedConfig(target, src, pickDlxConfig, pickRawAuthConfig)
}

/**
 * Whether the config key would be read from an INI config file.
 */
export const isIniConfigKey = (key: string): boolean =>
  key.startsWith('@') || key.startsWith('//') || NPM_AUTH_SETTINGS.includes(key)

/**
 * Whether the config key should be read from .npmrc files.
 * This includes auth keys and proxy keys (proxy keys are readable from .npmrc
 * for easier migration from npm, but are written to YAML config files).
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
