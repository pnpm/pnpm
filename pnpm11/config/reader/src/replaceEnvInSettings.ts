import { envReplace } from '@pnpm/config.env-replace'
import type { PnpmSettings } from '@pnpm/types'

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
  if (key === 'registry') throw new RegistryEnvNotExpandedError(key, WORKSPACE_MANIFEST)
  opts.warnings.push(ignoredSettingWarning(key))
  return true
}

function replaceEnvInSettingValue (key: string, value: unknown, opts: ReplaceEnvInSettingsOptions): unknown {
  if (typeof value === 'string') return envReplace(value, process.env)
  if (key === 'namedRegistries' || (key === 'registries' && isScopeRouteMap(value))) {
    return opts.expandRequestDestinationEnv
      ? replaceEnvInStringValues(value)
      : assertNoEnvPlaceholderValues(key, value)
  }
  if (key === 'registries') {
    // A declaration map is keyed by registry URL rather than valued by one,
    // so the request destination is the key and the gate has to apply there.
    return opts.expandRequestDestinationEnv
      ? replaceEnvInKeys(value)
      : assertNoEnvPlaceholderEntries(value)
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
 * Dropping a registry URL would resolve the packages it serves from another
 * registry, so a placeholder in one fails instead of being ignored.
 */
function assertNoEnvPlaceholderValues (key: string, value: unknown): unknown {
  if (value == null || typeof value !== 'object' || Array.isArray(value)) return value
  for (const [entryKey, entryValue] of Object.entries(value as Record<string, unknown>)) {
    if (typeof entryValue === 'string' && hasEnvPlaceholder(entryValue)) {
      throw new RegistryEnvNotExpandedError(`${key}['${redactRegistryUrl(entryKey)}']`, WORKSPACE_MANIFEST)
    }
  }
  return value
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
 * {@link assertNoEnvPlaceholderValues} for a declaration map, whose URL is the
 * key of a declaration and the value of a scope route, which a mixed map holds
 * too.
 */
function assertNoEnvPlaceholderEntries (value: unknown): unknown {
  if (value == null || typeof value !== 'object' || Array.isArray(value)) return value
  for (const [entryKey, entryValue] of Object.entries(value as Record<string, unknown>)) {
    if (hasEnvPlaceholder(typeof entryValue === 'string' ? entryValue : entryKey)) {
      throw new RegistryEnvNotExpandedError(`registries['${redactRegistryUrl(entryKey)}']`, WORKSPACE_MANIFEST)
    }
  }
  return value
}

const WORKSPACE_MANIFEST = 'pnpm-workspace.yaml'

function ignoredSettingWarning (setting: string): string {
  return `Ignored "${setting}" in ${WORKSPACE_MANIFEST}: environment variables are not expanded in the proxy, pnprServer, and userAgent settings of a file committed to the repository. ` +
    'Write the value literally, or move the setting to the global config.yaml.'
}
