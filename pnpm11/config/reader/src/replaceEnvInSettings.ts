import { envReplace } from '@pnpm/config.env-replace'
import { DEFAULT_REGISTRY_SCOPE, type PnpmSettings } from '@pnpm/types'

import { hasEnvPlaceholder, RegistryEnvNotExpandedError } from './envPlaceholders.js'
import { redactRegistryUrl } from './registryUrlUserinfo.js'

export interface ReplaceEnvInSettingsOptions {
  expandRequestDestinationEnv: boolean
  /** Receives a warning for each setting dropped for its placeholder. */
  warnings: string[]
}

/**
 * Scalar settings that pick where a request goes or what it carries. A
 * repo-controlled file may not resolve an environment variable into either,
 * since the one lets it choose the host and the other lets it send that host
 * the variable's value.
 */
const REQUEST_SCALAR_KEYS = new Set(['pnprServer', 'registry', 'httpProxy', 'httpsProxy', 'noProxy', 'proxy', 'noproxy', 'userAgent'])

export function replaceEnvInSettings (
  settings: PnpmSettings,
  opts: ReplaceEnvInSettingsOptions
): PnpmSettings {
  const newSettings: PnpmSettings = {}
  for (const [key, value] of Object.entries(settings)) {
    const newKey = envReplace(key, process.env)
    if (typeof value === 'string' && isGatedRequestScalar(newKey, value, opts)) continue
    newSettings[newKey as keyof PnpmSettings] = replaceEnvInSettingValue(newKey, value, opts) as never
  }
  return newSettings
}

function isGatedRequestScalar (key: string, value: string, opts: ReplaceEnvInSettingsOptions): boolean {
  if (!REQUEST_SCALAR_KEYS.has(key) || opts.expandRequestDestinationEnv || !hasEnvPlaceholder(value)) return false
  opts.warnings.push(ignoredSettingWarning(key))
  return true
}

function replaceEnvInSettingValue (key: string, value: unknown, opts: ReplaceEnvInSettingsOptions): unknown {
  if (typeof value === 'string') return envReplace(value, process.env)
  if (key === 'namedRegistries' || (key === 'registries' && isScopeRouteMap(value))) {
    return opts.expandRequestDestinationEnv
      ? replaceEnvInStringValues(value)
      : copyStringValuesWithoutEnvPlaceholders(key, value, opts.warnings)
  }
  if (key === 'registries') {
    // A declaration map is keyed by registry URL rather than valued by one,
    // so the request destination is the key and the gate has to apply there.
    return opts.expandRequestDestinationEnv
      ? replaceEnvInKeys(value)
      : copyEntriesWithoutEnvPlaceholderKeys(value, opts.warnings)
  }
  return value
}

/**
 * Whether a `registries` value is the older `<scope>: <url>` shape rather than
 * a map of declarations. The registry URL is the value in the one and the key
 * in the other, so which one carries the request destination — and so which
 * one the env-placeholder gate applies to — follows from this.
 *
 * A map that mixes the two is a declaration map here; {@link
 * translateRegistrySettings} rejects it with a message about the mixing.
 */
function isScopeRouteMap (value: unknown): boolean {
  if (value == null || typeof value !== 'object' || Array.isArray(value)) return false
  const values = Object.values(value as Record<string, unknown>)
  return values.length > 0 && values.every((entry) => typeof entry === 'string')
}

function replaceEnvInStringValues (value: unknown): unknown {
  if (value == null || typeof value !== 'object' || Array.isArray(value)) return value
  const out: Record<string, unknown> = {}
  for (const [entryKey, entryValue] of Object.entries(value as Record<string, unknown>)) {
    out[entryKey] = typeof entryValue === 'string' ? envReplace(entryValue, process.env) : entryValue
  }
  return out
}

/**
 * Drops the routes whose URL carries a placeholder. Only the default registry
 * may be dropped: every other route names packages that would otherwise
 * resolve from the default registry.
 */
function copyStringValuesWithoutEnvPlaceholders (key: string, value: unknown, warnings: string[]): unknown {
  if (value == null || typeof value !== 'object' || Array.isArray(value)) return value
  const out: Record<string, unknown> = {}
  for (const [entryKey, entryValue] of Object.entries(value as Record<string, unknown>)) {
    if (typeof entryValue === 'string' && hasEnvPlaceholder(entryValue)) {
      const setting = `${key}['${redactRegistryUrl(entryKey)}']`
      if (key === 'namedRegistries' || entryKey !== 'default') throw new RegistryEnvNotExpandedError(setting, WORKSPACE_MANIFEST)
      warnings.push(ignoredSettingWarning(setting))
      continue
    }
    out[entryKey] = entryValue
  }
  return out
}

function replaceEnvInKeys (value: unknown): unknown {
  if (value == null || typeof value !== 'object' || Array.isArray(value)) return value
  const out: Record<string, unknown> = {}
  for (const [entryKey, entryValue] of Object.entries(value as Record<string, unknown>)) {
    out[envReplace(entryKey, process.env)] = entryValue
  }
  return out
}

/**
 * Drops the entries whose URL carries a placeholder. The URL is the key of a
 * declaration and the value of a scope route, which a mixed map holds too. An
 * entry that routes a scope or a prefix may not be dropped, for the reason
 * {@link copyStringValuesWithoutEnvPlaceholders} gives.
 */
function copyEntriesWithoutEnvPlaceholderKeys (value: unknown, warnings: string[]): unknown {
  if (value == null || typeof value !== 'object' || Array.isArray(value)) return value
  const out: Record<string, unknown> = {}
  for (const [entryKey, entryValue] of Object.entries(value as Record<string, unknown>)) {
    if (hasEnvPlaceholder(typeof entryValue === 'string' ? entryValue : entryKey)) {
      warnings.push(ignoredSettingWarning(droppableEntrySetting(entryKey, entryValue)))
      continue
    }
    out[entryKey] = entryValue
  }
  return out
}

function droppableEntrySetting (entryKey: string, entryValue: unknown): string {
  const setting = `registries['${redactRegistryUrl(entryKey)}']`
  if (routesNonDefaultPackages(entryKey, entryValue)) throw new RegistryEnvNotExpandedError(setting, WORKSPACE_MANIFEST)
  return setting
}

function routesNonDefaultPackages (entryKey: string, entry: unknown): boolean {
  if (typeof entry === 'string') return entryKey !== 'default'
  if (entry == null || typeof entry !== 'object') return false
  const { scopes, prefix } = entry as { scopes?: unknown, prefix?: unknown }
  if (prefix != null) return true
  return Array.isArray(scopes) && scopes.some((scope) => scope !== DEFAULT_REGISTRY_SCOPE)
}

const WORKSPACE_MANIFEST = 'pnpm-workspace.yaml'

function ignoredSettingWarning (setting: string): string {
  return `Ignored "${setting}" in ${WORKSPACE_MANIFEST}: environment variables are not expanded in the registry, proxy, and user agent settings of a file committed to the repository. ` +
    'Write the value literally, or move the setting to the global config.yaml.'
}
