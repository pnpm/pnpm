import { type Config } from './Config.js'
import { type InheritableConfig, inheritPickedConfig } from './inheritPickedConfig.js'
import { type types } from './types.js'

const RAW_AUTH_CFG_KEYS = [
  'ca',
  'cafile',
  'cert',
  'key',
  'local-address',
  'git-shallow-hosts',
  'https-proxy',
  'proxy',
  'no-proxy',
  'registry',
  'strict-ssl',
] satisfies Array<keyof typeof types>

const RAW_AUTH_CFG_KEY_SUFFIXES = [
  ':cafile',
  ':certfile',
  ':keyfile',
  ':registry',
  ':tokenHelper',
  ':_auth',
  ':_authToken',
]

const AUTH_CFG_KEYS = [
  'ca',
  'cert',
  'key',
  'localAddress',
  'gitShallowHosts',
  'httpsProxy',
  'httpProxy',
  'noProxy',
  'registry',
  'registries',
  'strictSsl',
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

export function inheritAuthConfig (targetCfg: InheritableConfig, authSrcCfg: InheritableConfig): void {
  inheritPickedConfig(targetCfg, authSrcCfg, pickAuthConfig, pickRawAuthConfig)
}

/**
 * Whether the config key would be read from an INI config file.
 */
export const isIniConfigKey = (key: string): boolean =>
  key.startsWith('@') || key.startsWith('//') || NPM_AUTH_SETTINGS.includes(key)

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
