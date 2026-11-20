import { normalizeRegistryKey } from '@pnpm/config.pick-registry-for-package'
import type { RegistriesByScope, RegistryContext } from '@pnpm/types'

import type { WorkspaceStateSettings } from './types.js'

/**
 * The `registryPrefixes` a workspace state records, keyed in sorted order:
 * `undefined` when no prefix is declared. See
 * `WorkspaceStateSettings.registryPrefixes`.
 */
export function recordedRegistryPrefixes (
  { registriesByPrefix }: Partial<Pick<RegistryContext, 'registriesByPrefix'>>
): Record<string, string> | undefined {
  if (registriesByPrefix == null || Object.keys(registriesByPrefix).length === 0) return undefined
  return sortedEntries(registriesByPrefix)
}

export type RegistryKeySettings = Pick<WorkspaceStateSettings, 'defaultRegistry' | 'registryPrefixes'>

/**
 * What a workspace state records about the rule that keys registry packages:
 * the declared prefixes and the default registry, or nothing when no prefix
 * is declared. See `WorkspaceStateSettings.registryPrefixes` and
 * `WorkspaceStateSettings.defaultRegistry`.
 */
export function recordedRegistryKeySettings (
  opts: Partial<Pick<RegistryContext, 'registriesByPrefix' | 'registriesByScope'>>
): RegistryKeySettings {
  const registryPrefixes = recordedRegistryPrefixes(opts)
  if (registryPrefixes == null) return {}
  const defaultRegistry = opts.registriesByScope?.default
  return { registryPrefixes, defaultRegistry: defaultRegistry == null ? undefined : normalizeRegistryKey(defaultRegistry) }
}

/**
 * The scope routes an install resolves plain keys through, keyed in sorted
 * order, as `WorkspaceStateSettings.registryRoutes` records them.
 */
export function recordedRegistryRoutes (registriesByScope: RegistriesByScope): Record<string, string> {
  const { default: _default, ...scopes } = registriesByScope
  return sortedEntries(scopes)
}

function sortedEntries (record: Record<string, string>): Record<string, string> {
  return Object.fromEntries(Object.keys(record).sort().map((key) => [key, record[key]]))
}
